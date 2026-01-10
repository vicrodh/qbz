//! Secure credential storage with fallback
//!
//! Tries system keyring first, falls back to file-based storage:
//! - Linux: Secret Service (GNOME Keyring, KWallet via D-Bus)
//! - macOS: Keychain
//! - Windows: Credential Manager
//! - Fallback: Obfuscated file in config directory

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const SERVICE_NAME: &str = "qbz-nix";
const QOBUZ_CREDENTIALS_KEY: &str = "qobuz-credentials";
const FALLBACK_FILE_NAME: &str = ".qbz-auth";

// Simple XOR key for obfuscation (not encryption, just to avoid plain text)
const OBFUSCATION_KEY: &[u8] = b"QbzNixAudiophile2024";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QobuzCredentials {
    pub email: String,
    pub password: String,
}

/// Get the fallback credentials file path
fn get_fallback_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("qbz-nix").join(FALLBACK_FILE_NAME))
}

/// Simple XOR obfuscation (not secure, but avoids plain text)
fn obfuscate(data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ OBFUSCATION_KEY[i % OBFUSCATION_KEY.len()])
        .collect()
}

/// Save credentials to fallback file
fn save_to_fallback(credentials: &QobuzCredentials) -> Result<(), String> {
    let path = get_fallback_path().ok_or("Could not determine config directory")?;

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let json = serde_json::to_string(credentials)
        .map_err(|e| format!("Failed to serialize credentials: {}", e))?;

    let obfuscated = obfuscate(json.as_bytes());
    let encoded = BASE64.encode(&obfuscated);

    fs::write(&path, encoded)
        .map_err(|e| format!("Failed to write credentials file: {}", e))?;

    log::info!("Credentials saved to fallback file");
    Ok(())
}

/// Load credentials from fallback file
fn load_from_fallback() -> Result<Option<QobuzCredentials>, String> {
    let path = match get_fallback_path() {
        Some(p) => p,
        None => return Ok(None),
    };

    if !path.exists() {
        return Ok(None);
    }

    let encoded = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read credentials file: {}", e))?;

    let obfuscated = BASE64.decode(encoded.trim())
        .map_err(|e| format!("Failed to decode credentials: {}", e))?;

    let json_bytes = obfuscate(&obfuscated);
    let json = String::from_utf8(json_bytes)
        .map_err(|e| format!("Failed to decode credentials: {}", e))?;

    let credentials: QobuzCredentials = serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse credentials: {}", e))?;

    log::info!("Credentials loaded from fallback file for: {}", credentials.email);
    Ok(Some(credentials))
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
    Ok(())
}

/// Check if fallback file exists
fn has_fallback_credentials() -> bool {
    get_fallback_path().map(|p| p.exists()).unwrap_or(false)
}

/// Save Qobuz credentials - tries keyring first, then fallback
pub fn save_qobuz_credentials(email: &str, password: &str) -> Result<(), String> {
    log::info!("Attempting to save credentials for: {}", email);

    let credentials = QobuzCredentials {
        email: email.to_string(),
        password: password.to_string(),
    };

    // Try keyring first
    match Entry::new(SERVICE_NAME, QOBUZ_CREDENTIALS_KEY) {
        Ok(entry) => {
            let json = serde_json::to_string(&credentials)
                .map_err(|e| format!("Failed to serialize credentials: {}", e))?;

            match entry.set_password(&json) {
                Ok(()) => {
                    log::info!("Qobuz credentials saved to system keyring successfully");
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("Keyring save failed ({}), trying fallback...", e);
                }
            }
        }
        Err(e) => {
            log::warn!("Keyring not available ({}), using fallback...", e);
        }
    }

    // Fallback to file
    save_to_fallback(&credentials)
}

/// Load Qobuz credentials - tries keyring first, then fallback
pub fn load_qobuz_credentials() -> Result<Option<QobuzCredentials>, String> {
    log::info!("Attempting to load credentials");

    // Try keyring first
    if let Ok(entry) = Entry::new(SERVICE_NAME, QOBUZ_CREDENTIALS_KEY) {
        match entry.get_password() {
            Ok(json) => {
                if let Ok(credentials) = serde_json::from_str::<QobuzCredentials>(&json) {
                    log::info!("Successfully loaded credentials from keyring for: {}", credentials.email);
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
        Ok(entry) => {
            match entry.get_password() {
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
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_roundtrip() {
        // Note: This test requires a working keyring service
        // Skip in CI environments
        if std::env::var("CI").is_ok() {
            return;
        }

        let email = "test@example.com";
        let password = "testpass123";

        // Save
        save_qobuz_credentials(email, password).expect("Failed to save");

        // Load
        let loaded = load_qobuz_credentials()
            .expect("Failed to load")
            .expect("No credentials found");

        assert_eq!(loaded.email, email);
        assert_eq!(loaded.password, password);

        // Clear
        clear_qobuz_credentials().expect("Failed to clear");

        // Verify cleared
        let after_clear = load_qobuz_credentials().expect("Failed to check");
        assert!(after_clear.is_none());
    }
}
