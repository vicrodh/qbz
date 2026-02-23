//! API error types

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Authentication failed: {0}")]
    AuthenticationError(String),

    #[error("Invalid app ID")]
    InvalidAppId,

    #[error("Invalid app secret")]
    InvalidAppSecret,

    #[error("Failed to extract bundle tokens: {0}")]
    BundleExtractionError(String),

    #[error("User is not eligible (no active subscription)")]
    IneligibleUser,

    #[error("Track is not streamable")]
    NonStreamable,

    #[error("Invalid quality format: {0}")]
    InvalidQuality(u32),

    #[error("No valid quality available for this track")]
    NoQualityAvailable,

    #[error("Track {0} is no longer available on Qobuz")]
    TrackUnavailable(u64),

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("JSON parsing error: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("API error: {0}")]
    ApiResponse(String),

    #[error("Rate limited, retry after {0} seconds")]
    RateLimited(u64),
}

pub type Result<T> = std::result::Result<T, ApiError>;
