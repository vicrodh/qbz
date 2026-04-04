//! Application core: state machine, initialization, and main loop.

pub mod input;
pub mod state;

use std::io;
use std::panic;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    self, DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::Block;
use ratatui::Terminal;
use tokio::sync::mpsc;

use qbz_audio::settings::AudioSettingsStore;
use qbz_audio::AudioDiagnostic;
use qbz_cache::{AudioCache, PlaybackCache};
use qbz_core::QbzCore;
use qbz_models::CoreEvent;
use qbz_player::Player;

use crate::adapter::TuiAdapter;
use crate::credentials;
use crate::theme::{Theme, THEME};
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

    /// Main event loop.
    ///
    /// Sets up the terminal (raw mode, alternate screen, mouse capture),
    /// installs a panic hook for clean restoration, then runs the main loop:
    ///   1. Render frame
    ///   2. Poll crossterm events (100ms timeout)
    ///   3. Dispatch input to `input.rs`
    ///   4. Drain CoreEvent channel -> update state
    ///   5. Poll Player SharedState -> update playback display
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // --- Install panic hook to restore terminal on crash ---
        let original_hook = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let _ = restore_terminal();
            original_hook(info);
        }));

        // --- Setup terminal ---
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            DisableLineWrap,
        )?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        log::info!("[TUI] Terminal initialized, entering main loop");

        // --- Main loop ---
        let result = self.event_loop(&mut terminal).await;

        // --- Cleanup (always runs) ---
        let _ = restore_terminal();
        terminal.show_cursor()?;
        log::info!("[TUI] Terminal restored, exiting");

        result
    }

    /// The actual event loop, separated for clean error handling.
    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let tick_rate = Duration::from_millis(100);

        while !self.should_quit {
            // 1. Render frame
            let base_color = THEME.base();
            terminal.draw(|frame| {
                let area = frame.area();
                let block = Block::default().style(
                    ratatui::style::Style::default().bg(base_color),
                );
                frame.render_widget(block, area);
            })?;

            // 2. Poll crossterm events
            if event::poll(tick_rate)? {
                match event::read()? {
                    Event::Key(key) => {
                        input::handle_key(self, key);
                    }
                    Event::Mouse(mouse) => {
                        input::handle_mouse(self, mouse);
                    }
                    Event::Resize(_, _) => {
                        // Terminal will re-render on next tick
                    }
                    _ => {}
                }
            }

            // 3. Drain CoreEvent channel -> update state
            self.drain_core_events();

            // 4. Poll Player state -> update playback display
            self.poll_player_state();
        }

        Ok(())
    }

    /// Drain all pending CoreEvents and update AppState.
    fn drain_core_events(&mut self) {
        while let Ok(ev) = self.core_event_rx.try_recv() {
            self.handle_core_event(ev);
        }
    }

    /// Process a single CoreEvent and update state accordingly.
    fn handle_core_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::QueueUpdated { state: queue_state } => {
                if let Some(ref track) = queue_state.current_track {
                    self.state.playback.track_title = Some(track.title.clone());
                    self.state.playback.track_artist = Some(track.artist.clone());
                    self.state.playback.track_album = Some(track.album.clone());
                    self.state.playback.duration_secs = track.duration_secs;
                    self.state.playback.track_id = track.id;
                    self.state.playback.artwork_url = track.artwork_url.clone();
                    if let Some(sr) = track.sample_rate {
                        self.state.playback.sample_rate = sr as u32;
                    }
                    if let Some(bd) = track.bit_depth {
                        self.state.playback.bit_depth = bd;
                    }
                }
                log::debug!(
                    "[TUI] Queue updated: {} total, shuffle={}",
                    queue_state.total_tracks,
                    queue_state.shuffle,
                );
            }
            CoreEvent::TrackStarted { track, .. } => {
                self.state.playback.track_title = Some(track.title.clone());
                self.state.playback.track_artist = Some(track.artist.clone());
                self.state.playback.track_album = Some(track.album.clone());
                self.state.playback.duration_secs = track.duration_secs;
                self.state.playback.track_id = track.id;
                self.state.playback.artwork_url = track.artwork_url.clone();
                self.state.playback.is_playing = true;
                self.state.playback.position_secs = 0;
                log::info!("[TUI] Track started: {} - {}", track.artist, track.title);
            }
            CoreEvent::PlaybackStateChanged { state: pb_state } => {
                use qbz_models::playback::PlaybackState;
                self.state.playback.is_playing = pb_state == PlaybackState::Playing;
                self.state.playback.is_buffering = pb_state == PlaybackState::Loading;
                log::debug!("[TUI] Playback state: {:?}", pb_state);
            }
            CoreEvent::PositionUpdated {
                position_secs,
                duration_secs,
            } => {
                self.state.playback.position_secs = position_secs;
                self.state.playback.duration_secs = duration_secs;
            }
            CoreEvent::VolumeChanged { volume } => {
                self.state.playback.volume = volume;
            }
            CoreEvent::RepeatModeChanged { mode } => {
                log::debug!("[TUI] Repeat mode changed: {:?}", mode);
            }
            CoreEvent::ShuffleChanged { enabled } => {
                log::debug!("[TUI] Shuffle changed: {}", enabled);
            }
            CoreEvent::LoggedIn { session } => {
                self.state.authenticated = true;
                self.state.user_email = Some(session.email.clone());
                self.state.subscription = Some(session.subscription_label.clone());
                self.state.active_modal = None; // close login modal
                log::info!("[TUI] Logged in as {}", session.email);
            }
            CoreEvent::LoggedOut => {
                self.state.authenticated = false;
                self.state.user_email = None;
                self.state.subscription = None;
                log::info!("[TUI] Logged out");
            }
            CoreEvent::Error {
                code,
                message,
                recoverable,
            } => {
                let level = if recoverable {
                    state::StatusLevel::Warning
                } else {
                    state::StatusLevel::Error
                };
                self.state.status_message = Some((
                    format!("[{}] {}", code, message),
                    level,
                ));
                log::warn!("[TUI] Core error: [{}] {}", code, message);
            }
            CoreEvent::PlaybackError { track_id, message } => {
                self.state.status_message = Some((
                    format!("Playback error (track {}): {}", track_id, message),
                    state::StatusLevel::Error,
                ));
                log::error!("[TUI] Playback error on track {}: {}", track_id, message);
            }
            CoreEvent::NetworkError { message } => {
                self.state.status_message = Some((
                    format!("Network error: {}", message),
                    state::StatusLevel::Error,
                ));
                log::error!("[TUI] Network error: {}", message);
            }
            other => {
                log::trace!("[TUI] Unhandled CoreEvent: {:?}", other);
            }
        }
    }

    /// Poll the Player's shared state and update the playback display.
    fn poll_player_state(&mut self) {
        let ps = self.core.get_playback_state();
        self.state.playback.is_playing = ps.is_playing;
        self.state.playback.position_secs = ps.position;
        self.state.playback.duration_secs = ps.duration;
        self.state.playback.volume = ps.volume;
        if ps.track_id != 0 {
            self.state.playback.track_id = ps.track_id;
        }
    }
}

/// Restore terminal to normal state (disable raw mode, leave alternate screen).
fn restore_terminal() -> Result<(), Box<dyn std::error::Error>> {
    terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        EnableLineWrap,
    )?;
    Ok(())
}
