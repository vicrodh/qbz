//! Application core: state machine, initialization, and main loop.

pub mod state;

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use tokio::sync::mpsc;

use qbz_audio::settings::AudioSettingsStore;
use qbz_audio::AudioDiagnostic;
use qbz_cache::{AudioCache, PlaybackCache};
use qbz_core::QbzCore;
use qbz_models::CoreEvent;
use qbz_player::Player;

use crate::adapter::TuiAdapter;
use crate::credentials;
use state::AppState;

/// Main TUI application.
///
/// Owns the core orchestrator, event channel, audio cache, and all UI state.
pub struct App {
    pub state: AppState,
    pub core: Arc<QbzCore<TuiAdapter>>,
    pub core_event_rx: mpsc::UnboundedReceiver<CoreEvent>,
    pub should_quit: bool,
    pub playback_generation: Arc<AtomicU64>,
    pub audio_cache: Arc<AudioCache>,
}

impl App {
    /// Build the application: create core, player, cache, authenticate.
    ///
    /// Mirrors the desktop CoreBridge initialization sequence:
    /// 1. Load audio settings from SQLite (AudioSettingsStore)
    /// 2. Create Player with device name + settings
    /// 3. Create QbzCore with TuiAdapter + Player
    /// 4. Call core.init() to extract Qobuz bundle tokens
    /// 5. Authenticate (email/password -> OAuth token fallback)
    /// 6. Initialize L1+L2 cache cascade
    /// 7. Populate initial state
    pub async fn new(no_images: bool) -> Result<Self, Box<dyn std::error::Error>> {
        // --- Audio settings from database ---
        let settings_store = AudioSettingsStore::new()
            .map_err(|e| format!("Failed to open audio settings: {}", e))?;
        let audio_settings = settings_store.get_settings()
            .map_err(|e| format!("Failed to load audio settings: {}", e))?;

        let device_name = audio_settings.output_device.clone();
        log::info!(
            "[TUI] Audio settings loaded: backend={:?}, device={:?}",
            audio_settings.backend_type,
            device_name,
        );

        // --- Create Player (spawns audio thread) ---
        let player = Player::new(
            device_name,
            audio_settings.clone(),
            None, // no visualizer tap in TUI
            AudioDiagnostic::new(),
        );

        // --- Create TuiAdapter + event channel ---
        let (event_tx, event_rx) = mpsc::unbounded_channel::<CoreEvent>();
        let adapter = TuiAdapter::new(event_tx);

        // --- Create QbzCore ---
        let core = Arc::new(QbzCore::new(adapter, player));

        // --- Initialize (extract Qobuz bundle tokens) ---
        core.init().await.map_err(|e| {
            format!("Core init failed: {}", e)
        })?;
        log::info!("[TUI] Core initialized, bundle tokens extracted");

        // --- Authenticate ---
        let mut authenticated = false;
        let mut user_email: Option<String> = None;
        let mut subscription: Option<String> = None;

        // Try email/password credentials first
        match credentials::load_qobuz_credentials() {
            Ok(Some(creds)) => {
                log::info!("[TUI] Credentials found for {}, attempting login...", creds.email);
                match core.login(&creds.email, &creds.password).await {
                    Ok(session) => {
                        log::info!("[TUI] Logged in as {}", session.email);
                        authenticated = true;
                        user_email = Some(session.email);
                        subscription = Some(session.subscription_label);
                    }
                    Err(e) => {
                        log::warn!("[TUI] Password login failed: {}", e);
                    }
                }
            }
            Ok(None) => {
                log::info!("[TUI] No saved credentials found");
            }
            Err(e) => {
                log::warn!("[TUI] Error loading credentials: {}", e);
            }
        }

        // Fallback to OAuth token if password auth failed
        if !authenticated {
            match credentials::load_oauth_token() {
                Ok(Some(token)) => {
                    log::info!("[TUI] OAuth token found, attempting token login...");
                    match core.login_with_token(&token).await {
                        Ok(session) => {
                            log::info!("[TUI] Logged in via OAuth as {}", session.email);
                            authenticated = true;
                            user_email = Some(session.email);
                            subscription = Some(session.subscription_label);
                        }
                        Err(e) => {
                            log::warn!("[TUI] OAuth token login failed: {}", e);
                        }
                    }
                }
                Ok(None) => {
                    log::info!("[TUI] No OAuth token found");
                }
                Err(e) => {
                    log::warn!("[TUI] Error loading OAuth token: {}", e);
                }
            }
        }

        if !authenticated {
            log::info!("[TUI] No valid credentials; starting in unauthenticated mode (login modal will be shown)");
        }

        // --- Initialize L1+L2 cache ---
        let l2_cache = match PlaybackCache::new(800 * 1024 * 1024) {
            Ok(cache) => {
                log::info!("[TUI] L2 disk cache initialized (800 MB)");
                Some(Arc::new(cache))
            }
            Err(e) => {
                log::warn!("[TUI] Failed to create L2 disk cache: {}. Running memory-only.", e);
                None
            }
        };

        let audio_cache = if let Some(l2) = l2_cache {
            Arc::new(AudioCache::with_playback_cache(400 * 1024 * 1024, l2))
        } else {
            Arc::new(AudioCache::new(400 * 1024 * 1024))
        };
        log::info!("[TUI] L1 memory cache initialized (400 MB)");

        // --- Build initial state ---
        let mut app_state = AppState::new(no_images);
        app_state.authenticated = authenticated;
        app_state.user_email = user_email;
        app_state.subscription = subscription;
        app_state.settings.audio_settings = audio_settings;
        app_state.settings.loaded = true;

        // Show login modal if not authenticated
        if !app_state.authenticated {
            app_state.active_modal = Some(state::ModalType::Login);
        }

        Ok(Self {
            state: app_state,
            core,
            core_event_rx: event_rx,
            should_quit: false,
            playback_generation: Arc::new(AtomicU64::new(0)),
            audio_cache,
        })
    }

    /// Main event loop. Stub -- will be implemented in Task 5.
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        log::info!("[TUI] App::run() stub -- event loop not yet implemented");
        Ok(())
    }
}
