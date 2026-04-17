//! QBZ Qobuz - Qobuz API client
//!
//! This crate provides the Qobuz API client:
//! - QobuzClient: Main API interface
//! - Authentication and token management
//! - Search, catalog browsing, streaming URLs
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     qbz-qobuz (Tier 1)                      │
//! │  Qobuz API client, authentication, streaming URLs          │
//! └─────────────────────────────────────────────────────────────┘
//!                              ↑
//!                      ┌───────┴───────┐
//!                      │  qbz-models   │
//!                      │   (Tier 0)    │
//!                      └───────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use qbz_qobuz::QobuzClient;
//!
//! let client = QobuzClient::new()?;
//! client.init().await?;
//! let session = client.login("email", "password").await?;
//! let stream_url = client.get_stream_url(track_id, quality).await?;
//! ```

pub mod auth;
pub mod bundle;
pub mod client;
pub mod cmaf;
pub mod endpoints;
pub mod error;
pub mod link_resolver;
pub mod performers;

// Re-export main types
pub use bundle::BundleTokens;
pub use client::QobuzClient;
pub use cmaf::{
    decrypt_segments_into, download_full as cmaf_download_full,
    download_raw as cmaf_download_raw, setup_streaming as cmaf_setup_streaming, CmafRawBundle,
    CmafStreamingInfo, CMAF_PREFETCH_CONCURRENCY,
};
pub use error::{ApiError, Result};
pub use link_resolver::{resolve_link, LinkResolverError, ResolvedLink};
