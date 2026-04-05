//! Idle/sleep inhibitor via XDG Desktop Portal.
//!
//! Prevents screen blanking and system suspend while audio is playing.
//! Works on GNOME, KDE, Sway, XFCE — and inside Flatpak/Snap sandboxes.

use ashpd::desktop::inhibit::{InhibitFlags, InhibitOptions, InhibitProxy};
use ashpd::desktop::Request;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Manages the XDG portal idle inhibit lifecycle.
pub struct IdleInhibitor {
    handle: Arc<Mutex<Option<Request<()>>>>,
}

impl IdleInhibitor {
    pub fn new() -> Self {
        Self {
            handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Acquire idle+suspend inhibit. No-op if already held.
    pub async fn inhibit(&self) {
        let mut guard = self.handle.lock().await;
        if guard.is_some() {
            return;
        }

        match try_inhibit().await {
            Ok(request) => {
                log::info!("[IdleInhibit] Acquired idle+suspend inhibit via XDG portal");
                *guard = Some(request);
            }
            Err(e) => {
                log::debug!("[IdleInhibit] Could not acquire inhibit: {e}");
            }
        }
    }

    /// Release the inhibit. No-op if not held.
    pub async fn uninhibit(&self) {
        let mut guard = self.handle.lock().await;
        if guard.take().is_some() {
            log::info!("[IdleInhibit] Released idle+suspend inhibit");
        }
    }
}

async fn try_inhibit() -> Result<Request<()>, ashpd::Error> {
    let proxy = InhibitProxy::new().await?;
    let request = proxy
        .inhibit(
            None,
            InhibitFlags::Idle | InhibitFlags::Suspend,
            InhibitOptions::default(),
        )
        .await?;
    Ok(request)
}

impl Default for IdleInhibitor {
    fn default() -> Self {
        Self::new()
    }
}
