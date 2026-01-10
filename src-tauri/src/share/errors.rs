//! Error types for the share module

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ShareError {
    #[error("Missing ISRC for track")]
    MissingIsrc,

    #[error("Missing ISRC or URL for track")]
    MissingIdentifier,

    #[error("Missing UPC for album")]
    MissingUpc,

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("Odesli API error: {0}")]
    OdesliError(String),

    #[error("No matches found on Odesli")]
    NoMatches,

    #[error("Invalid content type: {0}")]
    InvalidContentType(String),

    #[error("Request timeout")]
    Timeout,
}

impl serde::Serialize for ShareError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
