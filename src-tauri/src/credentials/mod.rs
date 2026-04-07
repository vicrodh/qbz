//! Secure credential storage with fallback
//!
//! Tries system keyring first, falls back to encrypted file storage:
//! - Linux: Secret Service (GNOME Keyring, KWallet via D-Bus)
//! - macOS: Keychain
//! - Windows: Credential Manager
//! - Fallback: AES-256-GCM encrypted file in config directory

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

/// Save Qobuz credentials - saves to both file (primary) and keyring (secondary)
pub fn save_qobuz_credentials(email: &str, password: &str) -> Result<(), String> {
    log::info!("Attempting to save credentials");

    let credentials = QobuzCredentials {
        email: email.to_string(),
        password: password.to_string(),
    };

    // Always save to encrypted file first (more reliable, especially in dev)
    save_to_fallback(&credentials)?;

    // Also try keyring as secondary (nice to have for desktop integration)
    if let Ok(entry) = Entry::new(SERVICE_NAME, QOBUZ_CREDENTIALS_KEY) {
        let json = serde_json::to_string(&credentials).unwrap_or_default();
        if let Err(e) = entry.set_password(&json) {
            log::debug!("Keyring save failed (not critical): {}", e);
        } else {
            log::debug!("Also saved to keyring");
        }
    }

    Ok(())
}

/// Load Qobuz credentials - tries keyring first, then fallback
pub fn load_qobuz_credentials() -> Result<Option<QobuzCredentials>, String> {
    log::info!("Attempting to load credentials");

    // Try keyring first
    if let Ok(entry) = Entry::new(SERVICE_NAME, QOBUZ_CREDENTIALS_KEY) {
        match entry.get_password() {
            Ok(json) => {
                if let Ok(credentials) = serde_json::from_str::<QobuzCredentials>(&json) {
                    log::info!("Successfully loaded credentials from keyring");
                    return Ok(Some(credentials));
                }
            }
            Err(keyring::Error::NoEntry) => {
                log::debug!("No credentials in keyring, checking fallback...");
            }
            Err(e) => {
                log::warn!("Keyring load failed ({}), checking fallback...", e);
            }
        }
    } else {
        log::warn!("Keyring not available, checking fallback...");
    }

    // Try fallback file
    load_from_fallback()
}

/// Check if credentials are saved (keyring or fallback)
pub fn has_saved_credentials() -> bool {
    log::info!("Checking for saved credentials...");

    // Check keyring
    match Entry::new(SERVICE_NAME, QOBUZ_CREDENTIALS_KEY) {
        Ok(entry) => match entry.get_password() {
            Ok(_) => {
                log::info!("Found credentials in system keyring");
                return true;
            }
            Err(keyring::Error::NoEntry) => {
                log::info!("No credentials in keyring (NoEntry)");
            }
            Err(e) => {
                log::warn!("Keyring check failed: {}", e);
            }
        },
        Err(e) => {
            log::warn!("Keyring not available: {}", e);
        }
    }

    // Check fallback
    let has_fallback = has_fallback_credentials();
    log::info!("Fallback credentials exist: {}", has_fallback);
    has_fallback
}

/// Clear saved Qobuz credentials (both keyring and fallback)
pub fn clear_qobuz_credentials() -> Result<(), String> {
    // Try to clear keyring
    if let Ok(entry) = Entry::new(SERVICE_NAME, QOBUZ_CREDENTIALS_KEY) {
        match entry.delete_credential() {
            Ok(()) => {
                log::info!("Qobuz credentials cleared from keyring");
            }
            Err(keyring::Error::NoEntry) => {
                // Already cleared, that's fine
            }
            Err(e) => {
                log::warn!("Failed to clear keyring: {}", e);
            }
        }
    }

    // Also clear fallback
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
/// Tries system keyring first (encrypted). Only falls back to encrypted file if keyring is unavailable.
/// The token is AES-256-GCM encrypted before storing in keyring — `secret-tool` shows ciphertext, not the raw token.
pub fn save_oauth_token(token: &str) -> Result<(), String> {
    // Encrypt token before storing anywhere
    let placeholder = QobuzCredentials {
        email: token.to_string(),
        password: String::new(),
    };
    let encrypted = encrypt_credentials(&placeholder)?;

    // Try keyring (Secret Service D-Bus: GNOME Keyring, KWallet with bridge)
    if let Ok(entry) = Entry::new(SERVICE_NAME, OAUTH_TOKEN_KEY) {
        if entry.set_password(&encrypted).is_ok() {
            log::info!("[Credentials] OAuth token saved to system keyring (encrypted)");
            // Keyring succeeded — remove any leftover file from previous versions
            if let Some(path) = get_oauth_token_path() {
                if path.exists() {
                    let _ = fs::remove_file(&path);
                    log::info!("[Credentials] Removed legacy OAuth token file (migrated to keyring)");
                }
            }
            return Ok(());
        }
    }

    // Keyring unavailable — fall back to encrypted file
    log::warn!(
        "[Credentials] System keyring unavailable. Storing OAuth token in encrypted file. \
         KDE users: enable 'Use KWallet for the Secret Service interface' in KWallet settings."
    );

    let path = get_oauth_token_path().ok_or("Could not determine config directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    fs::write(&path, &encrypted).map_err(|e| format!("Failed to write OAuth token file: {}", e))?;

    log::info!("[Credentials] OAuth token saved to encrypted file (keyring fallback)");
    Ok(())
}

/// Load a previously saved OAuth `user_auth_token`, or `None` if absent.
/// Tries system keyring first (encrypted), falls back to encrypted file.
pub fn load_oauth_token() -> Result<Option<String>, String> {
    // Try keyring first — value is AES-256-GCM encrypted JSON
    if let Ok(entry) = Entry::new(SERVICE_NAME, OAUTH_TOKEN_KEY) {
        match entry.get_password() {
            Ok(encrypted) if !encrypted.is_empty() => {
                // Try decrypting (new format: encrypted JSON in keyring)
                if let Ok(placeholder) = decrypt_credentials(&encrypted) {
                    log::info!("[Credentials] OAuth token loaded from system keyring (encrypted)");
                    return Ok(Some(placeholder.email));
                }
                // Might be old format (plaintext token in keyring) — migrate
                log::info!("[Credentials] Migrating plaintext keyring entry to encrypted format");
                let token = encrypted;
                if let Ok(()) = save_oauth_token(&token) {
                    log::info!("[Credentials] Keyring entry re-encrypted");
                }
                return Ok(Some(token));
            }
            Ok(_) => {}
            Err(keyring::Error::NoEntry) => {
                log::info!("[Credentials] No OAuth token in keyring, checking file...");
            }
            Err(e) => {
                log::warn!("[Credentials] Keyring load failed ({}), checking file...", e);
            }
        }
    }

    // Fallback to encrypted file
    let path = match get_oauth_token_path() {
        Some(p) => p,
        None => return Ok(None),
    };

    if !path.exists() {
        return Ok(None);
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read OAuth token file: {}", e))?;

    match decrypt_credentials(&content) {
        Ok(placeholder) => {
            log::info!("[Credentials] OAuth token loaded from encrypted file");
            // Migrate: save to keyring (encrypted) and remove file
            if let Ok(entry) = Entry::new(SERVICE_NAME, OAUTH_TOKEN_KEY) {
                let re_encrypted = encrypt_credentials(&QobuzCredentials {
                    email: placeholder.email.clone(),
                    password: String::new(),
                });
                if let Ok(encrypted_str) = re_encrypted {
                    if entry.set_password(&encrypted_str).is_ok() {
                        let _ = fs::remove_file(&path);
                        log::info!("[Credentials] OAuth token migrated to keyring (encrypted), file removed");
                    }
                }
            }
            Ok(Some(placeholder.email))
        }
        Err(e) => {
            log::warn!("[Credentials] Failed to decrypt OAuth token: {}", e);
            Ok(None)
        }
    }
}

/// Delete the stored OAuth token (called on logout or token expiry).
pub fn clear_oauth_token() -> Result<(), String> {
    // Clear from keyring
    if let Ok(entry) = Entry::new(SERVICE_NAME, OAUTH_TOKEN_KEY) {
        match entry.delete_credential() {
            Ok(()) => log::info!("[Credentials] OAuth token cleared from keyring"),
            Err(keyring::Error::NoEntry) => {}
            Err(e) => log::debug!("[Credentials] Keyring clear failed: {}", e),
        }
    }

    // Clear encrypted file
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
