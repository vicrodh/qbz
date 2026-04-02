//! Credential loading for the TUI.
//!
//! Replicates the same logic as `src-tauri/src/credentials/mod.rs` so the TUI
//! can read Qobuz credentials saved by the desktop app without depending on the
//! Tauri crate.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

const SERVICE_NAME: &str = "qbz";
const QOBUZ_CREDENTIALS_KEY: &str = "qobuz-credentials";
const FALLBACK_FILE_NAME: &str = ".qbz-auth";
const INSTALLATION_SALT_FILE_NAME: &str = ".qbz-cred-salt";
const MACHINE_ID_FALLBACK_FILE_NAME: &str = ".qbz-machine-id";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QobuzCredentials {
    pub email: String,
    pub password: String,
}

/// Encrypted data format (must match desktop app format)
#[derive(Serialize, Deserialize)]
struct EncryptedCredentials {
    version: u8,
    nonce: String,
    ciphertext: String,
}

fn get_fallback_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz").join(FALLBACK_FILE_NAME))
}

fn get_installation_salt_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz").join(INSTALLATION_SALT_FILE_NAME))
}

fn get_machine_id_fallback_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz").join(MACHINE_ID_FALLBACK_FILE_NAME))
}

fn load_installation_salt() -> Result<Vec<u8>, String> {
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

    Err("Installation salt not found (desktop app has not been run yet?)".to_string())
}

fn load_machine_id_fallback() -> Result<Vec<u8>, String> {
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

    Err("Machine fallback id not found".to_string())
}

fn get_machine_id() -> Result<Vec<u8>, String> {
    if let Ok(id) = fs::read_to_string("/etc/machine-id") {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.as_bytes().to_vec());
        }
    }

    if let Ok(hostname) = std::env::var("HOSTNAME") {
        if !hostname.trim().is_empty() {
            return Ok(hostname.as_bytes().to_vec());
        }
    }

    if let Ok(user) = std::env::var("USER") {
        if !user.trim().is_empty() {
            return Ok(user.as_bytes().to_vec());
        }
    }

    load_machine_id_fallback()
}

fn derive_key() -> Result<[u8; 32], String> {
    let machine_id = get_machine_id()?;
    let installation_salt = load_installation_salt()?;

    let mut hasher = Sha256::new();
    hasher.update(&installation_salt);
    hasher.update(&machine_id);
    hasher.update(&installation_salt);

    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    Ok(key)
}

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

fn load_from_fallback() -> Result<Option<QobuzCredentials>, String> {
    let path = match get_fallback_path() {
        Some(p) => p,
        None => return Ok(None),
    };

    if !path.exists() {
        return Ok(None);
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read credentials file: {}", e))?;

    if content.trim().starts_with('{') && content.contains("\"version\"") {
        match decrypt_credentials(&content) {
            Ok(creds) => {
                log::info!("[TUI] Credentials loaded from encrypted fallback file");
                Ok(Some(creds))
            }
            Err(e) => {
                log::warn!("[TUI] Failed to decrypt credentials: {}", e);
                Err(e)
            }
        }
    } else {
        log::warn!("[TUI] Unrecognized credential file format");
        Ok(None)
    }
}

/// Load Qobuz credentials saved by the desktop app.
///
/// Tries keyring first, then the encrypted fallback file.
pub fn load_qobuz_credentials() -> Result<Option<QobuzCredentials>, String> {
    log::info!("[TUI] Attempting to load credentials");

    // Try keyring first
    match Entry::new(SERVICE_NAME, QOBUZ_CREDENTIALS_KEY) {
        Ok(entry) => {
            log::info!("[TUI] Keyring entry created, attempting to read...");
            match entry.get_password() {
                Ok(json) => {
                    log::info!("[TUI] Got keyring data ({} bytes)", json.len());
                    if let Ok(credentials) = serde_json::from_str::<QobuzCredentials>(&json) {
                        log::info!("[TUI] Credentials loaded from keyring for {}", credentials.email);
                        return Ok(Some(credentials));
                    } else {
                        log::warn!("[TUI] Keyring data is not valid QobuzCredentials JSON");
                    }
                }
                Err(keyring::Error::NoEntry) => {
                    log::info!("[TUI] No credentials in keyring (NoEntry), checking fallback...");
                }
                Err(e) => {
                    log::warn!("[TUI] Keyring get_password failed: {:?}, checking fallback...", e);
                }
            }
        }
        Err(e) => {
            log::warn!("[TUI] Keyring Entry::new failed: {:?}, checking fallback...", e);
        }
    }

    load_from_fallback()
}

const OAUTH_TOKEN_FILE_NAME: &str = ".qbz-oauth-token";

/// Load a previously saved OAuth user_auth_token.
///
/// The token is stored encrypted in the same format as credentials
/// (as the `email` field of a QobuzCredentials struct).
pub fn load_oauth_token() -> Result<Option<String>, String> {
    let path = match dirs::config_dir() {
        Some(p) => p.join("qbz").join(OAUTH_TOKEN_FILE_NAME),
        None => return Ok(None),
    };

    if !path.exists() {
        log::info!("[TUI] No OAuth token file found");
        return Ok(None);
    }

    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read OAuth token file: {}", e))?;

    match decrypt_credentials(&content) {
        Ok(placeholder) => {
            log::info!("[TUI] OAuth token loaded from file");
            Ok(Some(placeholder.email))
        }
        Err(e) => {
            log::warn!("[TUI] Failed to decrypt OAuth token: {}", e);
            Err(e)
        }
    }
}
