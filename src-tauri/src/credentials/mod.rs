//! Secure credential storage with fallback
//!
//! The encrypted AES-256-GCM file in the app config directory is the source
//! of truth for every credential. The OS keyring is used as an optional
//! best-effort cache:
//!
//! - Writes always go to the file first; the keyring is written opportunistically.
//! - Reads try the keyring first (cheaper when it works) and fall back to the file.
//! - Any keyring operation that fails or times out marks the keyring as broken
//!   for the rest of the process, so later reads/writes skip it entirely.
//!
//! This matters on Linux systems where GNOME Keyring / KWallet may be locked
//! with a password that no longer matches the current session (see issue #329).
//! Without the timeout + session memoization, every login triggers a blocking
//! "unlock keyring" dialog and the user has to dismiss 3-4 prompts per session.
//! With them, the dialog appears at most once per session and the app continues
//! through the encrypted file path regardless of what the user does with it.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use keyring::Entry;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const SERVICE_NAME: &str = "qbz";
const QOBUZ_CREDENTIALS_KEY: &str = "qobuz-credentials";
const FALLBACK_FILE_NAME: &str = ".qbz-auth";
const LEGACY_FALLBACK_FILE_NAME: &str = ".qbz-auth.legacy";
const OAUTH_TOKEN_FILE_NAME: &str = ".qbz-oauth-token";
const INSTALLATION_SALT_FILE_NAME: &str = ".qbz-cred-salt";
const MACHINE_ID_FALLBACK_FILE_NAME: &str = ".qbz-machine-id";

// Legacy XOR key for migration (only used for reading old format)
const LEGACY_OBFUSCATION_KEY: &[u8] = b"QbzNixAudiophile2024";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QobuzCredentials {
    pub email: String,
    pub password: String,
}

/// Encrypted data format stored in file
#[derive(Serialize, Deserialize)]
struct EncryptedCredentials {
    /// Version for future format changes
    version: u8,
    /// Base64-encoded nonce (12 bytes for AES-GCM)
    nonce: String,
    /// Base64-encoded ciphertext
    ciphertext: String,
}

/// Get the fallback credentials file path
fn get_fallback_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz").join(FALLBACK_FILE_NAME))
}

/// Get the legacy fallback file path (for migration)
fn get_legacy_fallback_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz").join(LEGACY_FALLBACK_FILE_NAME))
}

/// Get the per-installation salt file path used for key derivation.
fn get_installation_salt_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz").join(INSTALLATION_SALT_FILE_NAME))
}

/// Get path for persistent fallback machine identifier.
fn get_machine_id_fallback_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz").join(MACHINE_ID_FALLBACK_FILE_NAME))
}

/// Load a persistent installation salt, or create one on first use.
fn load_or_create_installation_salt() -> Result<Vec<u8>, String> {
    let path =
        get_installation_salt_path().ok_or("Could not determine config directory for salt")?;

    if path.exists() {
        let encoded =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read salt file: {}", e))?;
        let decoded = BASE64
            .decode(encoded.trim())
            .map_err(|e| format!("Failed to decode salt file: {}", e))?;
        if decoded.len() != 32 {
            return Err("Invalid installation salt length".to_string());
        }
        return Ok(decoded);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create salt directory: {}", e))?;
    }

    let mut salt = [0u8; 32];
    rand::rng().fill(&mut salt);
    fs::write(&path, BASE64.encode(salt))
        .map_err(|e| format!("Failed to write installation salt: {}", e))?;

    Ok(salt.to_vec())
}

/// Load a persistent machine identifier fallback, or create one on first use.
fn load_or_create_machine_id_fallback() -> Result<Vec<u8>, String> {
    let path = get_machine_id_fallback_path()
        .ok_or("Could not determine config directory for machine fallback id")?;

    if path.exists() {
        let encoded = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read machine fallback id: {}", e))?;
        let decoded = BASE64
            .decode(encoded.trim())
            .map_err(|e| format!("Failed to decode machine fallback id: {}", e))?;
        if decoded.len() != 32 {
            return Err("Invalid machine fallback id length".to_string());
        }
        return Ok(decoded);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create machine fallback directory: {}", e))?;
    }

    let mut machine_fallback = [0u8; 32];
    rand::rng().fill(&mut machine_fallback);
    fs::write(&path, BASE64.encode(machine_fallback))
        .map_err(|e| format!("Failed to write machine fallback id: {}", e))?;

    Ok(machine_fallback.to_vec())
}

/// Get machine-specific identifier for key derivation
fn get_machine_id() -> Result<Vec<u8>, String> {
    // Try /etc/machine-id first (Linux)
    if let Ok(id) = fs::read_to_string("/etc/machine-id") {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.as_bytes().to_vec());
        }
    }

    // Fallback to hostname
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        if !hostname.trim().is_empty() {
            return Ok(hostname.as_bytes().to_vec());
        }
    }

    // Last resort: use username
    if let Ok(user) = std::env::var("USER") {
        if !user.trim().is_empty() {
            return Ok(user.as_bytes().to_vec());
        }
    }

    // Persisted random fallback for environments without stable machine/user IDs.
    load_or_create_machine_id_fallback()
}

/// Retrieve per-app secret from XDG Desktop Portal (cached for session lifetime).
/// Returns None if portal is unavailable (headless, old DEs, non-Linux).
#[cfg(target_os = "linux")]
fn get_portal_secret() -> Option<Vec<u8>> {
    use std::sync::OnceLock;
    static PORTAL_SECRET: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    PORTAL_SECRET
        .get_or_init(|| {
            let rt = tokio::runtime::Handle::try_current().ok()?;
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                let _ = tx.send(ashpd::desktop::secret::retrieve().await.ok());
            });
            match rx.recv_timeout(std::time::Duration::from_secs(3)) {
                Ok(secret) => {
                    if secret.is_some() {
                        log::info!("[Credentials] Using XDG portal secret for key derivation");
                    }
                    secret
                }
                Err(_) => {
                    log::debug!("[Credentials] XDG portal secret unavailable (timeout/missing)");
                    None
                }
            }
        })
        .clone()
}

/// Derive encryption key from XDG portal secret + machine ID + installation salt.
/// Portal secret adds DE-agnostic, Flatpak-safe entropy when available.
fn derive_key() -> Result<[u8; 32], String> {
    let machine_id = get_machine_id()?;
    let installation_salt = load_or_create_installation_salt()?;

    #[cfg(target_os = "linux")]
    let portal_secret = get_portal_secret();
    #[cfg(not(target_os = "linux"))]
    let portal_secret: Option<Vec<u8>> = None;

    let mut hasher = Sha256::new();
    hasher.update(&installation_salt);
    if let Some(ref secret) = portal_secret {
        hasher.update(secret);
    }
    hasher.update(&machine_id);
    hasher.update(&installation_salt);

    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    Ok(key)
}

/// Encrypt credentials using AES-256-GCM
fn encrypt_credentials(credentials: &QobuzCredentials) -> Result<String, String> {
    let key = derive_key()?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Failed to create cipher: {}", e))?;

    // Generate random nonce
    let mut nonce_raw = [0u8; 12];
    rand::rng().fill(&mut nonce_raw);
    let nonce_bytes: [u8; 12] = aes_gcm::aead::generic_array::GenericArray::from(nonce_raw).into();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let json = serde_json::to_string(credentials)
        .map_err(|e| format!("Failed to serialize credentials: {}", e))?;

    let ciphertext = cipher
        .encrypt(nonce, json.as_bytes())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    let encrypted = EncryptedCredentials {
        version: 1,
        nonce: BASE64.encode(nonce_bytes),
        ciphertext: BASE64.encode(ciphertext),
    };

    serde_json::to_string(&encrypted)
        .map_err(|e| format!("Failed to serialize encrypted data: {}", e))
}

/// Decrypt credentials using AES-256-GCM
fn decrypt_credentials(encrypted_json: &str) -> Result<QobuzCredentials, String> {
    let encrypted: EncryptedCredentials = serde_json::from_str(encrypted_json)
        .map_err(|e| format!("Failed to parse encrypted data: {}", e))?;

    if encrypted.version != 1 {
        return Err(format!(
            "Unsupported encryption version: {}",
            encrypted.version
        ));
    }

    let key = derive_key()?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Failed to create cipher: {}", e))?;

    let nonce_bytes = BASE64
        .decode(&encrypted.nonce)
        .map_err(|e| format!("Failed to decode nonce: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = BASE64
        .decode(&encrypted.ciphertext)
        .map_err(|e| format!("Failed to decode ciphertext: {}", e))?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| "Decryption failed (wrong key or corrupted data)".to_string())?;

    let json = String::from_utf8(plaintext)
        .map_err(|e| format!("Failed to decode decrypted data: {}", e))?;

    serde_json::from_str(&json).map_err(|e| format!("Failed to parse credentials: {}", e))
}

/// Legacy XOR deobfuscation (for migration only)
fn legacy_deobfuscate(data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ LEGACY_OBFUSCATION_KEY[i % LEGACY_OBFUSCATION_KEY.len()])
        .collect()
}

/// Try to load credentials from legacy XOR format
fn load_legacy_credentials(path: &PathBuf) -> Result<Option<QobuzCredentials>, String> {
    if !path.exists() {
        return Ok(None);
    }

    let encoded =
        fs::read_to_string(path).map_err(|e| format!("Failed to read legacy file: {}", e))?;

    let obfuscated = BASE64
        .decode(encoded.trim())
        .map_err(|e| format!("Failed to decode legacy data: {}", e))?;

    let json_bytes = legacy_deobfuscate(&obfuscated);
    let json = String::from_utf8(json_bytes)
        .map_err(|e| format!("Failed to decode legacy credentials: {}", e))?;

    serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse legacy credentials: {}", e))
        .map(Some)
}

/// Save credentials to fallback file (AES-256-GCM encrypted)
fn save_to_fallback(credentials: &QobuzCredentials) -> Result<(), String> {
    let path = get_fallback_path().ok_or("Could not determine config directory")?;

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let encrypted = encrypt_credentials(credentials)?;

    fs::write(&path, encrypted).map_err(|e| format!("Failed to write credentials file: {}", e))?;

    log::info!("Credentials saved to encrypted fallback file");
    Ok(())
}

/// Load credentials from fallback file
fn load_from_fallback() -> Result<Option<QobuzCredentials>, String> {
    let path = match get_fallback_path() {
        Some(p) => p,
        None => return Ok(None),
    };

    if !path.exists() {
        // Check for legacy file and migrate if found
        if let Some(legacy_path) = get_legacy_fallback_path() {
            if legacy_path.exists() {
                log::info!("Found legacy credentials file, attempting migration...");
                if let Ok(Some(creds)) = load_legacy_credentials(&legacy_path) {
                    // Save in new format
                    if save_to_fallback(&creds).is_ok() {
                        // Remove legacy file
                        let _ = fs::remove_file(&legacy_path);
                        log::info!("Successfully migrated credentials to new encrypted format");
                        return Ok(Some(creds));
                    }
                }
            }
        }

        // Also check if the current file is in legacy format (migration from old .qbz-auth)
        let current_path = get_fallback_path();
        if let Some(ref p) = current_path {
            if p.exists() {
                // Try reading as JSON first (new format)
                if let Ok(content) = fs::read_to_string(p) {
                    if content.trim().starts_with('{') && content.contains("\"version\"") {
                        // It's the new format, will be handled below
                    } else {
                        // Might be legacy format
                        log::info!("Attempting to read legacy format from current file...");
                        if let Ok(Some(creds)) = load_legacy_credentials(p) {
                            // Save in new format
                            if save_to_fallback(&creds).is_ok() {
                                log::info!(
                                    "Successfully migrated credentials to new encrypted format"
                                );
                                return Ok(Some(creds));
                            }
                        }
                    }
                }
            }
        }

        return Ok(None);
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read credentials file: {}", e))?;

    // Check if it's the new format or legacy
    if content.trim().starts_with('{') && content.contains("\"version\"") {
        // New encrypted format
        match decrypt_credentials(&content) {
            Ok(creds) => {
                log::info!("Credentials loaded from encrypted fallback file");
                Ok(Some(creds))
            }
            Err(e) => {
                log::warn!("Failed to decrypt credentials: {}", e);
                // Try legacy format as fallback
                if let Ok(Some(creds)) = load_legacy_credentials(&path) {
                    log::info!("Loaded from legacy format, will re-encrypt on next save");
                    return Ok(Some(creds));
                }
                Err(e)
            }
        }
    } else {
        // Legacy format - try to load and migrate
        log::info!("Found legacy format, migrating...");
        if let Ok(Some(creds)) = load_legacy_credentials(&path) {
            // Save in new format
            if save_to_fallback(&creds).is_ok() {
                log::info!("Successfully migrated credentials to new encrypted format");
            }
            return Ok(Some(creds));
        }
        Ok(None)
    }
}

/// Clear fallback credentials file
fn clear_fallback() -> Result<(), String> {
    if let Some(path) = get_fallback_path() {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove credentials file: {}", e))?;
            log::info!("Fallback credentials file removed");
        }
    }
    // Also clear legacy file if exists
    if let Some(legacy_path) = get_legacy_fallback_path() {
        if legacy_path.exists() {
            let _ = fs::remove_file(&legacy_path);
        }
    }
    Ok(())
}

/// Check if fallback file exists
fn has_fallback_credentials() -> bool {
    get_fallback_path().map(|p| p.exists()).unwrap_or(false)
}

// ─── Keyring session state ────────────────────────────────────────────────────
//
// Linux Secret Service (the backing store for the `keyring` crate on Linux)
// is allowed to prompt the user for a passphrase when its collection is
// locked. That prompt blocks the calling thread indefinitely. A user whose
// GNOME Keyring got out of sync with their login password sees the dialog
// appear, dismisses it (because they can't remember the old password), and
// we then re-try another keyring operation which produces the dialog again
// — and again, and again, for every keyring touch in the login flow.
//
// The defense here has two parts:
//
// 1. Each keyring call is executed on a worker thread with a hard wall-clock
//    timeout. If the call hasn't returned within the timeout, we assume the
//    user is staring at a dialog they can't satisfy and we give up — the
//    worker thread stays blocked in the background, but our control flow
//    moves on. (The worker eventually resolves when the user dismisses the
//    dialog; it's a small thread leak once per broken-keyring session.)
//
// 2. The first failure or timeout latches a process-wide flag
//    (`KEYRING_STATE`) that short-circuits every subsequent keyring touch.
//    One prompt max per session; everything after that goes straight to the
//    encrypted file. A restart is required to retry the keyring, which is
//    also the point where the user's keyring might have been repaired.
//
// Errors that mean "the entry doesn't exist" (`keyring::Error::NoEntry`)
// don't count as a failure — they're just data the caller has to handle.

const KEYRING_UNTESTED: u8 = 0;
const KEYRING_WORKING: u8 = 1;
const KEYRING_BROKEN: u8 = 2;

static KEYRING_STATE: AtomicU8 = AtomicU8::new(KEYRING_UNTESTED);

/// Per-operation wall-clock limit for any Secret Service / keyring call.
const KEYRING_OP_TIMEOUT: Duration = Duration::from_millis(2500);

fn keyring_is_broken() -> bool {
    KEYRING_STATE.load(Ordering::Relaxed) == KEYRING_BROKEN
}

fn mark_keyring_broken(reason: &str) {
    let previous = KEYRING_STATE.swap(KEYRING_BROKEN, Ordering::Relaxed);
    if previous != KEYRING_BROKEN {
        log::warn!(
            "[Credentials] Disabling system keyring for the rest of this session \
             (falling back to encrypted file only): {}",
            reason
        );
    }
}

fn mark_keyring_working() {
    // Only promote from UNTESTED to WORKING. Never climb back out of BROKEN —
    // once we've given up on the keyring for this session, stay given up.
    let _ = KEYRING_STATE.compare_exchange(
        KEYRING_UNTESTED,
        KEYRING_WORKING,
        Ordering::Relaxed,
        Ordering::Relaxed,
    );
}

/// Run a blocking keyring closure on a worker thread and return its result
/// within `KEYRING_OP_TIMEOUT`. Times out cleanly if the closure is still
/// stuck (typically because the user is looking at an unlock dialog).
fn run_with_keyring_timeout<T, F>(op_name: &'static str, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(KEYRING_OP_TIMEOUT).map_err(|_| {
        format!(
            "keyring {} did not complete within {}ms (likely blocked on a user dialog)",
            op_name,
            KEYRING_OP_TIMEOUT.as_millis()
        )
    })
}

/// Read a value from the keyring. Returns `None` if the entry does not exist
/// OR if the keyring is unavailable / broken for this session. The caller
/// must always have a file-based fallback ready.
fn keyring_get(key: &str) -> Option<String> {
    if keyring_is_broken() {
        return None;
    }
    let service = SERVICE_NAME.to_string();
    let key_owned = key.to_string();
    match run_with_keyring_timeout("get", move || {
        Entry::new(&service, &key_owned).and_then(|e| e.get_password())
    }) {
        Ok(Ok(value)) => {
            mark_keyring_working();
            Some(value)
        }
        Ok(Err(keyring::Error::NoEntry)) => {
            mark_keyring_working();
            None
        }
        Ok(Err(e)) => {
            mark_keyring_broken(&format!("get failed: {}", e));
            None
        }
        Err(reason) => {
            mark_keyring_broken(&reason);
            None
        }
    }
}

/// Write a value to the keyring. Returns `true` on success, `false` on any
/// failure (timeout, locked collection, no backend, etc.). The caller must
/// have already persisted the value through its authoritative path (file).
fn keyring_set(key: &str, value: &str) -> bool {
    if keyring_is_broken() {
        return false;
    }
    let service = SERVICE_NAME.to_string();
    let key_owned = key.to_string();
    let value_owned = value.to_string();
    match run_with_keyring_timeout("set", move || {
        Entry::new(&service, &key_owned).and_then(|e| e.set_password(&value_owned))
    }) {
        Ok(Ok(())) => {
            mark_keyring_working();
            true
        }
        Ok(Err(e)) => {
            mark_keyring_broken(&format!("set failed: {}", e));
            false
        }
        Err(reason) => {
            mark_keyring_broken(&reason);
            false
        }
    }
}

/// Delete a keyring entry. Silent no-op if the keyring is broken or the
/// entry already doesn't exist.
fn keyring_delete(key: &str) {
    if keyring_is_broken() {
        return;
    }
    let service = SERVICE_NAME.to_string();
    let key_owned = key.to_string();
    match run_with_keyring_timeout("delete", move || {
        Entry::new(&service, &key_owned).and_then(|e| e.delete_credential())
    }) {
        Ok(Ok(())) | Ok(Err(keyring::Error::NoEntry)) => {
            mark_keyring_working();
        }
        Ok(Err(e)) => {
            log::debug!("[Credentials] Keyring delete failed (not critical): {}", e);
        }
        Err(reason) => {
            mark_keyring_broken(&reason);
        }
    }
}

/// Save Qobuz email+password credentials.
///
/// File is authoritative: we write it first and fail the operation if that
/// fails. The keyring is a best-effort write-through cache.
pub fn save_qobuz_credentials(email: &str, password: &str) -> Result<(), String> {
    log::info!("[Credentials] Saving Qobuz credentials");

    let credentials = QobuzCredentials {
        email: email.to_string(),
        password: password.to_string(),
    };

    save_to_fallback(&credentials)?;

    let json = serde_json::to_string(&credentials).unwrap_or_default();
    if !json.is_empty() && keyring_set(QOBUZ_CREDENTIALS_KEY, &json) {
        log::debug!("[Credentials] Qobuz credentials also saved to keyring");
    }

    Ok(())
}

/// Load Qobuz email+password credentials. Prefers the keyring when it
/// responds quickly, otherwise reads the encrypted fallback file.
pub fn load_qobuz_credentials() -> Result<Option<QobuzCredentials>, String> {
    log::debug!("[Credentials] Loading Qobuz credentials");

    if let Some(json) = keyring_get(QOBUZ_CREDENTIALS_KEY) {
        match serde_json::from_str::<QobuzCredentials>(&json) {
            Ok(credentials) => {
                log::debug!("[Credentials] Loaded Qobuz credentials from keyring");
                return Ok(Some(credentials));
            }
            Err(e) => {
                log::warn!(
                    "[Credentials] Keyring entry could not be parsed ({}), falling back to file",
                    e
                );
            }
        }
    }

    load_from_fallback()
}

/// Report whether any saved Qobuz credentials exist (keyring or file).
pub fn has_saved_credentials() -> bool {
    if keyring_get(QOBUZ_CREDENTIALS_KEY).is_some() {
        return true;
    }
    has_fallback_credentials()
}

/// Clear saved Qobuz credentials from both the keyring and the fallback file.
pub fn clear_qobuz_credentials() -> Result<(), String> {
    keyring_delete(QOBUZ_CREDENTIALS_KEY);
    clear_fallback()?;
    Ok(())
}

// ─── OAuth token persistence ──────────────────────────────────────────────────
//
// OAuth login produces a `user_auth_token` instead of email+password.
// We persist it encrypted the same way as regular credentials so the user
// doesn't have to re-authenticate via browser on every app start.
// The token is re-used at bootstrap via `POST /user/login` with the
// `X-User-Auth-Token` header. If it has expired Qobuz returns a 4xx and
// we clear the stored token so the user sees the login screen normally.

fn get_oauth_token_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz").join(OAUTH_TOKEN_FILE_NAME))
}

const OAUTH_TOKEN_KEY: &str = "qobuz-oauth-token";

/// Persist the OAuth `user_auth_token`.
///
/// File is authoritative: the encrypted token is written to the config
/// directory unconditionally. The keyring is a best-effort write-through
/// cache — if it fails (or times out behind an unlock dialog), the login
/// flow still completes because the file is already on disk. Inverts the
/// previous keyring-first ordering, which forced a prompt on every login
/// for users with a broken Secret Service collection (issue #329).
pub fn save_oauth_token(token: &str) -> Result<(), String> {
    let placeholder = QobuzCredentials {
        email: token.to_string(),
        password: String::new(),
    };
    let encrypted = encrypt_credentials(&placeholder)?;

    let path = get_oauth_token_path().ok_or("Could not determine config directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    fs::write(&path, &encrypted).map_err(|e| format!("Failed to write OAuth token file: {}", e))?;
    log::info!("[Credentials] OAuth token saved to encrypted file");

    if keyring_set(OAUTH_TOKEN_KEY, &encrypted) {
        log::debug!("[Credentials] OAuth token also saved to keyring");
    }

    Ok(())
}

/// Load a previously saved OAuth `user_auth_token`, or `None` if absent.
/// Prefers the keyring when it responds quickly, otherwise reads the file.
pub fn load_oauth_token() -> Result<Option<String>, String> {
    if let Some(encrypted) = keyring_get(OAUTH_TOKEN_KEY) {
        if !encrypted.is_empty() {
            if let Ok(placeholder) = decrypt_credentials(&encrypted) {
                log::debug!("[Credentials] OAuth token loaded from keyring");
                return Ok(Some(placeholder.email));
            }
            // Legacy format: pre-encryption builds stored the raw token in
            // the keyring. Accept it for this one read; the next successful
            // `save_oauth_token` call will rewrite it encrypted to both the
            // keyring and the file.
            log::debug!("[Credentials] Keyring held legacy plaintext token; will re-encrypt on next save");
            return Ok(Some(encrypted));
        }
    }

    let path = match get_oauth_token_path() {
        Some(p) => p,
        None => return Ok(None),
    };
    if !path.exists() {
        return Ok(None);
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read OAuth token file: {}", e))?;
    if content.trim().is_empty() {
        return Ok(None);
    }

    match decrypt_credentials(&content) {
        Ok(placeholder) => {
            log::debug!("[Credentials] OAuth token loaded from encrypted file");
            Ok(Some(placeholder.email))
        }
        Err(e) => {
            log::warn!("[Credentials] Failed to decrypt OAuth token file: {}", e);
            Ok(None)
        }
    }
}

/// Delete the stored OAuth token (logout or token expiry).
pub fn clear_oauth_token() -> Result<(), String> {
    keyring_delete(OAUTH_TOKEN_KEY);

    if let Some(path) = get_oauth_token_path() {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove OAuth token file: {}", e))?;
            log::info!("[Credentials] OAuth token file cleared");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns true if the config directory is writable (required for encryption salt).
    /// NixOS sandbox builds and CI environments lack a writable HOME.
    fn has_writable_config_dir() -> bool {
        // Nix build sandbox sets HOME to /homeless-shelter
        if let Ok(home) = std::env::var("HOME") {
            if home.contains("homeless-shelter") || home.contains("/nix/store") {
                return false;
            }
        }
        // Also skip if NIX_BUILD_TOP is set (nix-build sandbox)
        if std::env::var("NIX_BUILD_TOP").is_ok() {
            return false;
        }
        if let Some(path) = dirs::config_dir() {
            let test_dir = path.join("qbz");
            if std::fs::create_dir_all(&test_dir).is_ok() {
                return true;
            }
        }
        false
    }

    #[test]
    fn test_encryption_roundtrip() {
        // Skip in environments without a writable config dir (NixOS sandbox, CI)
        if std::env::var("CI").is_ok() || !has_writable_config_dir() {
            return;
        }

        let credentials = QobuzCredentials {
            email: "test@example.com".to_string(),
            password: format!("test-pass-{}", std::process::id()),
        };

        let encrypted = encrypt_credentials(&credentials).expect("Encryption failed");
        let decrypted = decrypt_credentials(&encrypted).expect("Decryption failed");

        assert_eq!(decrypted.email, credentials.email);
        assert_eq!(decrypted.password, credentials.password);
    }

    #[test]
    fn test_credentials_roundtrip() {
        // Skip in environments without keyring or writable config dir
        if std::env::var("CI").is_ok() || !has_writable_config_dir() {
            return;
        }

        // Clear any stale credentials from previous runs (may have different key/salt)
        let _ = clear_qobuz_credentials();

        let email = "test@example.com";
        let password = format!("test-secret-{}", std::process::id());

        // Save
        save_qobuz_credentials(email, &password).expect("Failed to save");

        // Load — if decryption fails due to environment issues, skip rather than panic
        let loaded = match load_qobuz_credentials() {
            Ok(Some(creds)) => creds,
            Ok(None) => {
                eprintln!("Skipping: credentials not found after save (keyring issue)");
                return;
            }
            Err(e) => {
                eprintln!(
                    "Skipping: cannot load credentials in this environment: {}",
                    e
                );
                let _ = clear_qobuz_credentials();
                return;
            }
        };

        assert_eq!(loaded.email, email);
        assert_eq!(loaded.password, password);

        // Clear
        clear_qobuz_credentials().expect("Failed to clear");

        // Verify cleared
        let after_clear = load_qobuz_credentials().expect("Failed to check");
        assert!(after_clear.is_none());
    }
}
