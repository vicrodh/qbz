//! Authentication and request signing

use chrono::{TimeZone, Utc};
use md5::{Md5, Digest};
use std::time::{SystemTime, UNIX_EPOCH};

use super::error::{ApiError, Result};
use super::models::UserSession;

/// Generate MD5 signature for protected API endpoints
///
/// Signature format: MD5(method + params + timestamp + secret)
pub fn generate_signature(method: &str, params: &str, timestamp: u64, secret: &str) -> String {
    let sig_string = format!("{}{}{}{}", method, params, timestamp, secret);
    let mut hasher = Md5::new();
    hasher.update(sig_string.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Generate signature for track/getFileUrl endpoint
pub fn sign_get_file_url(track_id: u64, format_id: u32, timestamp: u64, secret: &str) -> String {
    let params = format!(
        "format_id{}intentstreamtrack_id{}",
        format_id, track_id
    );
    generate_signature("trackgetFileUrl", &params, timestamp, secret)
}

/// Generate signature for favorite/getUserFavorites endpoint
pub fn sign_get_favorites(timestamp: u64, secret: &str) -> String {
    generate_signature("favoritegetUserFavorites", "", timestamp, secret)
}

/// Get current Unix timestamp
pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// Parse user login response
pub fn parse_login_response(response: &serde_json::Value) -> Result<UserSession> {
    let user = response
        .get("user")
        .ok_or_else(|| ApiError::AuthenticationError("No user in response".to_string()))?;

    let user_auth_token = response
        .get("user_auth_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::AuthenticationError("No auth token in response".to_string()))?
        .to_string();

    let user_id = user
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ApiError::AuthenticationError("No user id".to_string()))?;

    let email = user
        .get("email")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let display_name = user
        .get("display_name")
        .and_then(|v| v.as_str())
        .or_else(|| user.get("login").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();

    // Check subscription
    let credential = user.get("credential");
    let subscription_label = credential
        .and_then(|c| c.get("parameters"))
        .and_then(|p| p.get("short_label"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();

    fn parse_subscription_valid_until(parameters: &serde_json::Value) -> Option<String> {
        // Try common string fields first.
        let string_keys = [
            "end_date",
            "expiration_date",
            "valid_until",
            "expires_at",
            "expiry_date",
        ];
        for key in string_keys {
            if let Some(s) = parameters.get(key).and_then(|v| v.as_str()) {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }

        // Try common timestamp fields (seconds).
        let ts_keys = ["end_date_ts", "expires_at_ts", "expiration_ts", "valid_until_ts"];
        for key in ts_keys {
            if let Some(ts) = parameters.get(key).and_then(|v| v.as_i64()) {
                if ts > 0 {
                    return Some(Utc.timestamp_opt(ts, 0).single()?.date_naive().to_string());
                }
            }
        }

        None
    }

    let subscription_valid_until = credential
        .and_then(|c| c.get("parameters"))
        .and_then(parse_subscription_valid_until);

    // Check if user has valid subscription
    let has_subscription = credential
        .and_then(|c| c.get("parameters"))
        .map(|p| !p.is_null() && p.as_object().map(|o| !o.is_empty()).unwrap_or(false))
        .unwrap_or(false);

    if !has_subscription {
        return Err(ApiError::IneligibleUser);
    }

    Ok(UserSession {
        user_auth_token,
        user_id,
        email,
        display_name,
        subscription_label,
        subscription_valid_until,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_signature() {
        let sig = generate_signature("test", "params", 1234567890, "secret");
        assert_eq!(sig.len(), 32); // MD5 hex is 32 chars
    }

    #[test]
    fn test_sign_get_file_url() {
        let sig = sign_get_file_url(123456, 27, 1234567890, "testsecret");
        assert_eq!(sig.len(), 32);
    }
}
