//! Backend selection and runtime dispatch.
//!
//! At open time we try to use the OS keyring first. Success = we store
//! (or rotate-in) a 32-byte master key inside the keyring and use it for
//! AES-256-GCM wraps. Failure (for any reason) = we fall back to HKDF
//! over device identifiers.
//!
//! The backend discriminator is baked into every wrapped blob so a blob
//! produced by one backend can't be silently decrypted by another (and
//! so that if a user later gains keyring access, we don't accidentally
//! ignore their existing KDF-wrapped data).

use std::path::Path;

use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

use crate::cipher::{unwrap_with_key, wrap_with_key};
use crate::error::SecretError;
use crate::install_id;

const MASTER_KEY_LEN: usize = 32;
const KEYRING_ENTRY_NAME: &str = "master-key-v1";
const HKDF_INFO: &[u8] = b"qbz-secrets master-key derivation v1";

const BACKEND_MARKER_KEYRING: u8 = 0;
const BACKEND_MARKER_KDF: u8 = 1;

/// Which backend is active at runtime. Useful for diagnostics / UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// Master key lives in the OS keyring. Moving the offline cache to
    /// another machine makes it unreadable. Gold-standard device binding.
    Keyring,
    /// Master key is derived on the fly from `machine-id` + a persistent
    /// per-install UUID. Still device-bound, but reversible by anyone
    /// with filesystem access to both sources. Used when the OS keyring
    /// is unavailable (headless daemon, Pi-like setups).
    KdfFallback,
}

pub struct Backend {
    kind: BackendKind,
    master_key: [u8; MASTER_KEY_LEN],
}

impl Backend {
    pub fn new(service_name: &str, storage_dir: &Path) -> Result<Self, SecretError> {
        // Try keyring first.
        match try_open_keyring(service_name) {
            Ok(master_key) => {
                log::info!("[qbz-secrets] Using OS keyring backend");
                return Ok(Self {
                    kind: BackendKind::Keyring,
                    master_key,
                });
            }
            Err(e) => {
                log::warn!(
                    "[qbz-secrets] OS keyring unavailable ({}) — falling back to KDF-derived key",
                    e
                );
            }
        }

        // Fallback: derive from device identifiers.
        let master_key = derive_fallback_key(service_name, storage_dir)?;
        Ok(Self {
            kind: BackendKind::KdfFallback,
            master_key,
        })
    }

    pub fn kind(&self) -> BackendKind {
        self.kind
    }

    pub fn wrap(&self, plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
        let marker = match self.kind {
            BackendKind::Keyring => BACKEND_MARKER_KEYRING,
            BackendKind::KdfFallback => BACKEND_MARKER_KDF,
        };
        wrap_with_key(&self.master_key, marker, plaintext)
    }

    pub fn unwrap(&self, wrapped: &[u8]) -> Result<Vec<u8>, SecretError> {
        let expected = match self.kind {
            BackendKind::Keyring => BACKEND_MARKER_KEYRING,
            BackendKind::KdfFallback => BACKEND_MARKER_KDF,
        };
        unwrap_with_key(&self.master_key, expected, wrapped)
    }
}

/// Read (or create and store) the master key from the OS keyring.
fn try_open_keyring(service_name: &str) -> Result<[u8; MASTER_KEY_LEN], SecretError> {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use keyring::Entry;

    let entry = Entry::new(service_name, KEYRING_ENTRY_NAME)
        .map_err(|e| SecretError::Keyring(format!("Entry::new: {}", e)))?;

    match entry.get_password() {
        Ok(existing_b64) => {
            let bytes = B64
                .decode(existing_b64.trim())
                .map_err(|e| SecretError::Keyring(format!("base64 decode: {}", e)))?;
            if bytes.len() != MASTER_KEY_LEN {
                return Err(SecretError::Keyring(format!(
                    "keyring entry has wrong length ({} bytes, expected {})",
                    bytes.len(),
                    MASTER_KEY_LEN
                )));
            }
            let mut out = [0u8; MASTER_KEY_LEN];
            out.copy_from_slice(&bytes);
            Ok(out)
        }
        Err(keyring::Error::NoEntry) => {
            let mut key = [0u8; MASTER_KEY_LEN];
            rand::rng().fill_bytes(&mut key);
            let encoded = B64.encode(key);
            entry
                .set_password(&encoded)
                .map_err(|e| SecretError::Keyring(format!("set_password: {}", e)))?;
            log::info!(
                "[qbz-secrets] Generated fresh 256-bit master key and stored in OS keyring"
            );
            Ok(key)
        }
        Err(e) => Err(SecretError::Keyring(format!("get_password: {}", e))),
    }
}

fn derive_fallback_key(
    service_name: &str,
    storage_dir: &Path,
) -> Result<[u8; MASTER_KEY_LEN], SecretError> {
    // Assemble salt inputs. Any component can be missing; we still
    // derive a usable key as long as at least one is present (the
    // install UUID is guaranteed after first run).
    let install_uuid = install_id::load_or_create(storage_dir).map_err(SecretError::Io)?;
    let machine = install_id::machine_id().unwrap_or_default();

    // The "IKM" is the concatenation of service name + machine id +
    // install uuid. None of these are secrets; the security comes from
    // the fact that all three need to be present on the same filesystem
    // to reconstruct them.
    let mut ikm: Vec<u8> = Vec::with_capacity(service_name.len() + machine.len() + 64);
    ikm.extend_from_slice(service_name.as_bytes());
    ikm.push(0);
    ikm.extend_from_slice(machine.as_bytes());
    ikm.push(0);
    ikm.extend_from_slice(install_uuid.as_bytes());

    // HKDF-SHA256 with a fixed salt (info carries the version).
    let hk = Hkdf::<Sha256>::new(None, &ikm);
    let mut out = [0u8; MASTER_KEY_LEN];
    hk.expand(HKDF_INFO, &mut out)
        .map_err(|e| SecretError::Other(format!("HKDF expand: {}", e)))?;

    log::info!(
        "[qbz-secrets] Derived 256-bit master key via HKDF (machine-id-present={})",
        !machine.is_empty()
    );
    Ok(out)
}
