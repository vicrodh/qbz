//! Framework-agnostic application runtime facade.
//!
//! [`AppRuntime`] is the composition root that a non-Tauri UI shell (Slint,
//! TUI, headless) builds on. It owns an `Arc<QbzCore<A>>` plus the
//! framework-agnostic runtime state machine, constructed the same way the
//! Tauri `CoreBridge` does but without any Tauri dependency.
//!
//! Scope of this module (Session 21 / Task 1 of the Slint POC readiness
//! audit): composition and accessors only. It deliberately does NOT own the
//! per-user stores or perform session activation — that is Task 2, the
//! framework-agnostic session activation spine. Until then a shell reaches
//! catalog, playback, and queue functionality through [`AppRuntime::core`].
//!
//! The Tauri app does not consume this module; `CoreBridge` keeps its own
//! construction path. `AppRuntime` is purely additive.

use std::sync::Arc;

use qbz_audio::{settings::AudioSettingsStore, AudioDiagnostic, AudioSettings, VisualizerTap};
use qbz_core::{FrontendAdapter, QbzCore};
use qbz_player::Player;

use crate::runtime::RuntimeManager;

/// Composition root for a non-Tauri UI shell.
///
/// Generic over the [`FrontendAdapter`] the shell supplies, so the same
/// facade serves a Slint adapter, a TUI adapter, or a headless one.
pub struct AppRuntime<A: FrontendAdapter + Send + Sync + 'static> {
    core: Arc<QbzCore<A>>,
    runtime: Arc<RuntimeManager>,
}

impl<A: FrontendAdapter + Send + Sync + 'static> AppRuntime<A> {
    /// Build with explicit audio settings.
    ///
    /// Performs no disk or network access — used by tests and by shells that
    /// already have audio settings loaded.
    pub fn with_audio_settings(
        adapter: A,
        device_name: Option<String>,
        audio_settings: AudioSettings,
        visualizer_tap: Option<VisualizerTap>,
    ) -> Self {
        let diagnostic = AudioDiagnostic::new();
        let player = Player::new(device_name, audio_settings, visualizer_tap, diagnostic);
        let core = QbzCore::new(adapter, player);
        Self {
            core: Arc::new(core),
            runtime: Arc::new(RuntimeManager::new()),
        }
    }

    /// Build, loading persisted audio settings from [`AudioSettingsStore`].
    ///
    /// Falls back to defaults when no settings are saved or the store cannot
    /// be opened. This mirrors the recipe in the Tauri `CoreBridge::new`.
    /// It does not touch the network — call [`AppRuntime::init`] for that.
    pub fn new(adapter: A) -> Self {
        let (device_name, audio_settings) = AudioSettingsStore::new()
            .ok()
            .and_then(|store| {
                store
                    .get_settings()
                    .ok()
                    .map(|settings| (settings.output_device.clone(), settings))
            })
            .unwrap_or_else(|| {
                log::info!("[AppRuntime] No saved audio settings, using defaults");
                (None, AudioSettings::default())
            });
        Self::with_audio_settings(adapter, device_name, audio_settings, None)
    }

    /// Initialize the core (extracts Qobuz bundle tokens).
    ///
    /// Best-effort and offline-tolerant: a network failure here leaves the
    /// core usable for local/offline playback, matching [`QbzCore::init`].
    pub async fn init(&self) -> Result<(), String> {
        self.core.init().await.map_err(|e| e.to_string())
    }

    /// The orchestrator. Shells reach catalog, playback, queue, and auth
    /// functionality through this handle.
    pub fn core(&self) -> &Arc<QbzCore<A>> {
        &self.core
    }

    /// The framework-agnostic runtime state machine.
    pub fn runtime(&self) -> &Arc<RuntimeManager> {
        &self.runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeState;
    use qbz_core::NoOpAdapter;

    #[test]
    fn builds_with_explicit_audio_settings() {
        let rt = AppRuntime::with_audio_settings(
            NoOpAdapter,
            None,
            AudioSettings::default(),
            None,
        );
        // The core handle is reachable without panicking.
        let _core = rt.core();
    }

    #[tokio::test]
    async fn runtime_state_machine_starts_uninitialized() {
        let rt = AppRuntime::with_audio_settings(
            NoOpAdapter,
            None,
            AudioSettings::default(),
            None,
        );
        assert_eq!(
            rt.runtime().get_status().await.state,
            RuntimeState::Uninitialized
        );
    }

    #[tokio::test]
    async fn core_reports_no_session_before_login() {
        let rt = AppRuntime::with_audio_settings(
            NoOpAdapter,
            None,
            AudioSettings::default(),
            None,
        );
        assert!(!rt.core().has_session().await);
        assert!(!rt.core().is_api_initialized().await);
    }
}
