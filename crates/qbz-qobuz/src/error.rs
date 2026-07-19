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

    /// Qobuz answered 403 Forbidden on an authenticated request. The account is
    /// authenticated but not currently allowed to perform the action (entitlement
    /// not restored after an outage, geo/concurrency limit, or an edge/WAF block).
    /// The string is a short body preview for diagnostics. Terminal, never a
    /// per-quality restriction — abort the fallback loop instead of retrying.
    #[error("Access forbidden by Qobuz (HTTP 403){0}")]
    Forbidden(String),

    /// The 403 circuit breaker is open after repeated forbidden responses; the
    /// request was short-circuited WITHOUT touching the network so we don't get
    /// the IP edge-blocked. Clears itself after a cooldown. See [`crate::forbidden_breaker`].
    #[error("Temporarily backing off after repeated 403s ({0}s remaining)")]
    ForbiddenCircuitOpen(u64),

    #[error("Offline mode is active - Qobuz services are disabled")]
    OfflineMode,

    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("JSON parsing error: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("API error: {0}")]
    ApiResponse(String),

    #[error("Rate limited, retry after {0} seconds")]
    RateLimited(u64),

    #[error("Server error (HTTP {0})")]
    ServerError(u16),
}

impl ApiError {
    /// True for errors worth retrying with backoff (issue #467): transport
    /// problems (timeout/connect/reset), 5xx server errors, and 429 rate
    /// limiting. Terminal errors — a real 404 `TrackUnavailable`, auth or
    /// parse failures — return false and should propagate to the (bounded)
    /// skip path instead of being retried.
    pub fn is_transient(&self) -> bool {
        match self {
            ApiError::NetworkError(e) => crate::retry::reqwest_is_transient(e),
            ApiError::RateLimited(_) | ApiError::ServerError(_) => true,
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;
