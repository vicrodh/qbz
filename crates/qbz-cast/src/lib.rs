//! qbz-cast - Casting support for QBZ
//!
//! Provides casting capabilities for Chromecast and DLNA devices.
//! This crate is Tauri-agnostic and can be used by any Rust application.
//!
//! # Architecture
//!
//! - **Chromecast**: Full implementation using rust-cast. Uses a dedicated thread
//!   for connection management due to rust-cast's use of Rc (not thread-safe).
//!
//! - **DLNA/UPnP**: Full implementation using rupnp for AVTransport/RenderingControl.
//!
//! - **MediaServer**: Local HTTP server for streaming audio to cast devices.
//!   Supports byte-range requests for seeking.

pub mod chromecast;
pub mod dlna;
pub mod errors;
pub mod media_server;

// Re-export error types at root
pub use errors::{CastError, DlnaError};

// Re-export media server
pub use media_server::MediaServer;

// Re-export Chromecast types
pub use chromecast::{
    CastApplication, CastCommand, CastDeviceConnection, CastPositionInfo, CastStatus,
    ChromecastHandle, DeviceDiscovery, DiscoveredDevice, MediaMetadata,
};

// Re-export DLNA types
pub use dlna::{
    DiscoveredDlnaDevice, DlnaConnection, DlnaDiscovery, DlnaMetadata, DlnaPositionInfo, DlnaStatus,
};

/// Cast device type alias for backwards compatibility
pub type CastDevice = CastDeviceConnection;

/// Install the rustls process-level `CryptoProvider` (aws-lc-rs) exactly once.
///
/// `rust_cast` opens a TLS control channel to the Chromecast via rustls. In the
/// full QBZ binary BOTH the `aws-lc-rs` and `ring` providers are enabled (reqwest
/// pulls `ring`, rust_cast pulls `aws-lc-rs`), so rustls can't auto-select one
/// and panics with "Could not automatically determine the process-level
/// CryptoProvider". Installing one explicitly before any TLS use fixes it.
///
/// Idempotent (a second call is a no-op); harmless if some other component
/// already installed a default (`install_default` then returns Err, ignored).
pub fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        if rustls::crypto::aws_lc_rs::default_provider()
            .install_default()
            .is_err()
        {
            log::debug!("[cast] rustls CryptoProvider already installed");
        }
    });
}
