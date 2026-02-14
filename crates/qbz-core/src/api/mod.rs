//! Qobuz API client module
//!
//! This module handles all communication with the Qobuz API, including:
//! - Bundle token extraction (app_id, secrets)
//! - User authentication
//! - Request signing (MD5 signatures)
//! - All API endpoints (search, albums, tracks, playlists, etc.)

pub mod auth;
pub mod bundle;
pub mod client;
pub mod endpoints;
pub mod error;
pub mod models;
pub mod performers;

pub use client::QobuzClient;
pub use error::ApiError;
pub use models::*;
