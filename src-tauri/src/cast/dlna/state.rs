//! Shared state types for the DLNA/UPnP cast module.
//!
//! Originally co-located with the Tauri commands in
//! `cast/dlna/commands.rs`. Relocated here so the type surface
//! remains available after the legacy `#[tauri::command]` definitions
//! in `commands.rs` are eventually removed.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::cast::dlna::{DlnaConnection, DlnaDiscovery, DlnaError};
use crate::cast::MediaServer;

/// DLNA state shared across commands
pub struct DlnaState {
    pub discovery: Arc<Mutex<DlnaDiscovery>>,
    pub connection: Arc<Mutex<Option<DlnaConnection>>>,
    /// Shared media server (lazily initialized)
    pub media_server: Arc<Mutex<Option<MediaServer>>>,
}

impl DlnaState {
    pub fn new(media_server: Arc<Mutex<Option<MediaServer>>>) -> Result<Self, DlnaError> {
        Ok(Self {
            discovery: Arc::new(Mutex::new(DlnaDiscovery::new())),
            connection: Arc::new(Mutex::new(None)),
            media_server,
        })
    }

    /// Ensure media server is started (lazy initialization)
    pub async fn ensure_media_server(&self) -> Result<(), DlnaError> {
        let mut server_guard = self.media_server.lock().await;
        if server_guard.is_none() {
            log::info!("Starting media server on demand for DLNA");
            *server_guard = Some(MediaServer::start().map_err(|e| {
                DlnaError::Connection(format!("Failed to start media server: {}", e))
            })?);
        }
        Ok(())
    }
}
