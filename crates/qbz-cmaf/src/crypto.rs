use aes::cipher::{BlockDecryptMut, KeyIvInit, StreamCipher};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::error::CmafError;

const RNG_INIT: &str = "abb21364945c0583309667d13ca3d93a";

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;

fn hex_decode(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("RNG_INIT is valid hex"))
        .collect()
}

/// Derive the 16-byte session key from the session/start `infos` field.
///
/// `infos` format: `"salt_b64url.info_b64url"`
/// IKM = hex-decoded RNG_INIT (16 bytes)
pub fn derive_session_key(infos: &str) -> Result<[u8; 16], CmafError> {
    let parts: Vec<&str> = infos.split('.').collect();
    if parts.len() < 2 {
        return Err(CmafError::InvalidInfos(
            "session infos must have at least 2 dot-separated parts".into(),
        ));
    }

    let salt = URL_SAFE_NO_PAD.decode(parts[0])?;
    let info = URL_SAFE_NO_PAD.decode(parts[1])?;

    let ikm = hex_decode(RNG_INIT);

    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut okm = [0u8; 16];
    hk.expand(&info, &mut okm).map_err(|_| CmafError::HkdfExpand)?;

    Ok(okm)
}

/// Unwrap the per-track content key using the session key.
///
/// `key_str` format: `"qbz-1.wrapped_key_b64url.iv_b64url"`
pub fn unwrap_content_key(session_key: &[u8; 16], key_str: &str) -> Result<[u8; 16], CmafError> {
    let parts: Vec<&str> = key_str.split('.').collect();
    if parts.len() < 3 {
        return Err(CmafError::InvalidKey(
            "key string must have at least 3 dot-separated parts".into(),
        ));
    }

    let wrapped = URL_SAFE_NO_PAD.decode(parts[1])?;
    let iv = URL_SAFE_NO_PAD.decode(parts[2])?;

    if iv.len() != 16 {
        return Err(CmafError::InvalidKey(format!(
            "unwrap IV must be 16 bytes, got {}",
            iv.len()
        )));
    }

    let mut buf = wrapped.clone();
    let decrypted =
        Aes128CbcDec::new(session_key.into(), iv.as_slice().into())
            .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buf)
            .map_err(|e| CmafError::AesDecrypt(format!("AES-CBC unwrap failed: {e}")))?;

    if decrypted.len() != 16 {
        return Err(CmafError::InvalidKey(format!(
            "unwrapped key must be 16 bytes, got {}",
            decrypted.len()
        )));
    }

    let mut key = [0u8; 16];
    key.copy_from_slice(decrypted);
    Ok(key)
}

/// Decrypt a FLAC frame in-place using AES-128-CTR.
///
/// `iv_8` = 8-byte IV from the segment UUID box entry, zero-padded to 16 bytes.
pub fn decrypt_frame(content_key: &[u8; 16], iv_8: &[u8; 8], data: &mut [u8]) {
    let mut nonce = [0u8; 16];
    nonce[..8].copy_from_slice(iv_8);
    Aes128Ctr::new(content_key.into(), &nonce.into()).apply_keystream(data);
}

/// Compute the MD5 request signature for Qobuz CMAF API calls.
///
/// Concatenates method + sorted key-value pairs + timestamp + RNG_INIT,
/// then returns the lowercase hex MD5 digest.
pub fn compute_request_sig(
    method: &str,
    args: &std::collections::BTreeMap<&str, String>,
    timestamp: &str,
) -> String {
    use md5::{Digest, Md5};

    let mut hasher = Md5::new();
    hasher.update(method.as_bytes());
    for (k, v) in args {
        hasher.update(k.as_bytes());
        hasher.update(v.as_bytes());
    }
    hasher.update(timestamp.as_bytes());
    hasher.update(RNG_INIT.as_bytes());

    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_request_sig() {
        let mut args = std::collections::BTreeMap::new();
        args.insert("profile", "qbz-1".to_string());
        let sig = compute_request_sig("sessionstart", &args, "1775500000");
        assert_eq!(sig.len(), 32); // MD5 hex = 32 chars
        // Verify deterministic
        let sig2 = compute_request_sig("sessionstart", &args, "1775500000");
        assert_eq!(sig, sig2);
    }

    #[test]
    fn test_decrypt_frame_roundtrip() {
        let key = [0x42u8; 16];
        let iv = [0x01u8; 8];
        let original = b"Hello FLAC frame data here!".to_vec();
        let mut data = original.clone();
        decrypt_frame(&key, &iv, &mut data);
        assert_ne!(data, original);
        decrypt_frame(&key, &iv, &mut data);
        assert_eq!(data, original);
    }

    #[test]
    fn test_derive_session_key_invalid_infos() {
        let result = derive_session_key("no_dot_here");
        assert!(result.is_err());
    }

    #[test]
    fn test_unwrap_content_key_invalid_format() {
        let key = [0u8; 16];
        let result = unwrap_content_key(&key, "only.two");
        assert!(result.is_err());
    }
}
