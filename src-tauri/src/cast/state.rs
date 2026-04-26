//! Shared state types for the cast (Chromecast) module.
//!
//! Originally co-located with the Tauri commands in `cast/commands.rs`.
//! Relocated here so the type surface remains available after the
//! legacy `#[tauri::command]` definitions in `commands.rs` are
//! eventually removed.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::cast::chromecast_thread::ChromecastHandle;
use crate::cast::{CastError, DeviceDiscovery, MediaServer};

/// Cast state shared across commands.
///
/// Uses a dedicated thread for Chromecast operations since rust_cast
/// is not thread-safe.
pub struct CastState {
    pub discovery: Arc<Mutex<DeviceDiscovery>>,
    pub chromecast: ChromecastHandle,
    /// Media server is lazily initialized on first cast operation to save CPU when not casting
    pub media_server: Arc<Mutex<Option<MediaServer>>>,
    pub connected_device_ip: Arc<Mutex<Option<String>>>,
}

impl CastState {
    pub fn new() -> Result<Self, CastError> {
        Ok(Self {
            discovery: Arc::new(Mutex::new(DeviceDiscovery::new())),
            chromecast: ChromecastHandle::new(),
            // Don't start media server until needed - saves CPU when not casting
            media_server: Arc::new(Mutex::new(None)),
            connected_device_ip: Arc::new(Mutex::new(None)),
        })
    }

    /// Get or create the media server (lazy initialization)
    pub async fn get_or_create_media_server(&self) -> Result<(), CastError> {
        let mut server_guard = self.media_server.lock().await;
        if server_guard.is_none() {
            log::info!("Starting media server on demand (lazy init)");
            *server_guard = Some(MediaServer::start()?);
        }
        Ok(())
    }
}
