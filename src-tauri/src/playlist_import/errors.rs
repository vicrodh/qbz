//! Errors for playlist import

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PlaylistImportError {
    #[error("Invalid playlist URL: {0}")]
    InvalidUrl(String),
    #[error("Provider not supported: {0}")]
    UnsupportedProvider(String),
    #[error("Missing credentials: {0}")]
    MissingCredentials(String),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Qobuz error: {0}")]
    Qobuz(String),
}
