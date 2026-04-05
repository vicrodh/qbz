//! Sanitization helpers for sensitive values in log output.
//!
//! Masks UUIDs and numeric IDs so logs remain useful for debugging
//! without exposing full identifiers to security scanners or uploaded
//! log files.

use std::fmt::Display;

/// Mask a UUID string, preserving first and last 8 characters.
///
/// `"123e4567-e89b-12d3-a456-426614174000"` → `"123e4567-****-****-****-****14174000"`
///
/// Non-UUID strings are returned with middle characters masked.
pub fn mask_uuid(uuid: &str) -> String {
    // Standard UUID: 8-4-4-4-12 = 36 chars
    if uuid.len() == 36 && uuid.chars().filter(|c| *c == '-').count() == 4 {
        let first = &uuid[..8];
        let last = &uuid[28..];
        format!("{first}-****-****-****-****{last}")
    } else if uuid.len() > 8 {
        let quarter = uuid.len() / 4;
        let first = &uuid[..quarter];
        let last = &uuid[uuid.len() - quarter..];
        format!("{first}****{last}")
    } else {
        "****".to_string()
    }
}

/// Mask a numeric or string ID, preserving at most the first 4 characters.
///
/// `12345678` → `"1234****"`
/// `42` → `"****"`
pub fn mask_id(id: impl Display) -> String {
    let s = id.to_string();
    if s.len() > 4 {
        format!("{}****", &s[..4])
    } else {
        "****".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_uuid_standard() {
        assert_eq!(
            mask_uuid("123e4567-e89b-12d3-a456-426614174000"),
            "123e4567-****-****-****-****14174000"
        );
    }

    #[test]
    fn test_mask_uuid_short() {
        assert_eq!(mask_uuid("abcd"), "****");
    }

    #[test]
    fn test_mask_uuid_non_standard() {
        assert_eq!(mask_uuid("abcdef1234567890"), "abcd****7890");
    }

    #[test]
    fn test_mask_id_long() {
        assert_eq!(mask_id(12345678), "1234****");
    }

    #[test]
    fn test_mask_id_short() {
        assert_eq!(mask_id(42), "****");
    }

    #[test]
    fn test_mask_id_zero() {
        assert_eq!(mask_id(0), "****");
    }

    #[test]
    fn test_mask_id_five_digits() {
        assert_eq!(mask_id(10001), "1000****");
    }
}
