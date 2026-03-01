//! QBZ-NIX: Native high-fidelity streaming client for Linux
//!
//! This application uses the Qobuz API but is not certified by Qobuz.

use std::sync::atomic::AtomicBool;

/// Flag to prevent double-closing secondary windows.
/// Set in CloseRequested, checked in RunEvent::Exit.
/// Without this, calling close() on windows that WebKit is already tearing
/// down causes "free(): corrupted unsorted chunks" heap corruption.
static WINDOWS_CLOSED_BY_USER: AtomicBool = AtomicBool::new(false);

// New multi-crate architecture
pub mod commands_v2;
pub mod core_bridge;
pub mod integrations_v2;
pub mod runtime;
pub mod session_lifecycle;
pub mod tauri_adapter;

pub mod auto_theme;

pub mod api;
pub mod api_cache;
pub mod api_server;
pub mod artist_blacklist;
pub mod artist_vectors;
pub mod audio;
pub mod cache;
pub mod cast;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod discogs;
pub mod flatpak;
pub mod lastfm;
pub mod library;
pub mod listenbrainz;
pub mod logging;
pub mod lyrics;
pub mod media_controls;
pub mod migration;
pub mod musicbrainz;
pub mod network;
pub mod offline;
pub mod offline_cache;
pub mod playback_context;
pub mod player;
pub mod playlist_import;
pub mod plex;
pub mod queue;
pub mod radio_engine;
pub mod reco_store;
pub mod session_store;
pub mod share;
pub mod tray;
pub mod updates;
pub mod user_data;
pub mod visualizer;
pub mod pdf_viewer;

use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::{Mutex, RwLock};

use api::QobuzClient;
use cache::{AudioCache, PlaybackCache};
use lastfm::LastFmClient;
use media_controls::{MediaControlsManager, TrackInfo};
use playback_context::ContextManager;
use player::Player;
use queue::QueueManager;
use share::SongLinkClient;
use visualizer::Visualizer;

/// Application state shared across commands
pub struct AppState {
    pub client: Arc<RwLock<QobuzClient>>,
    pub player: Player,
    pub queue: QueueManager,
    pub context: ContextManager,
    pub media_controls: MediaControlsManager,
    pub audio_cache: Arc<AudioCache>,
    pub lastfm: Arc<Mutex<LastFmClient>>,
    pub songlink: SongLinkClient,
    pub visualizer: Visualizer,
}

impl AppState {
    pub fn new() -> Self {
        Self::with_device_and_settings(None, config::audio_settings::AudioSettings::default())
    }

    pub fn with_device(device_name: Option<String>) -> Self {
        Self::with_device_and_settings(
            device_name,
            config::audio_settings::AudioSettings::default(),
        )
    }

    pub fn with_device_and_settings(
        device_name: Option<String>,
        audio_settings: config::audio_settings::AudioSettings,
    ) -> Self {
        // Create playback cache (L2 - disk, 800MB)
        let playback_cache = match PlaybackCache::new(800 * 1024 * 1024) {
            Ok(cache) => Some(Arc::new(cache)),
            Err(e) => {
                log::warn!(
                    "Failed to create playback cache: {}. Disk spillover disabled.",
                    e
                );
                None
            }
        };

        // Create audio cache (L1 - memory, 400MB) with optional disk spillover
        let audio_cache = if let Some(pc) = playback_cache {
            Arc::new(AudioCache::with_playback_cache(400 * 1024 * 1024, pc))
        } else {
            Arc::new(AudioCache::default())
        };

        // Create visualizer first to get the tap for the player
        let visualizer = Visualizer::new();
        let viz_tap = visualizer.get_tap();

        Self {
            client: Arc::new(RwLock::new(QobuzClient::default())),
            player: Player::new(
                device_name,
                audio_settings,
                Some(viz_tap),
                audio::AudioDiagnostic::new(),
            ),
            queue: QueueManager::new(),
            context: ContextManager::new(),
            media_controls: MediaControlsManager::new(),
            audio_cache,
            lastfm: Arc::new(Mutex::new(LastFmClient::default())),
            songlink: SongLinkClient::new(),
            visualizer,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Update MPRIS metadata when track changes
pub fn update_media_controls_metadata(
    media_controls: &MediaControlsManager,
    title: &str,
    artist: &str,
    album: &str,
    duration_secs: Option<u64>,
    cover_url: Option<String>,
) {
    let track_info = TrackInfo {
        title: title.to_string(),
        artist: artist.to_string(),
        album: album.to_string(),
        duration_secs,
        cover_url,
    };
    media_controls.set_metadata(&track_info);
}

/// Add a KWin window rule that forces server-side decorations (SSD) for QBZ.
///
/// GTK3 on Wayland hardcodes CLIENT_SIDE in the xdg-decoration protocol,
/// so KWin scripting alone can't override it. Window rules operate at a
/// deeper level (applied during window setup, before protocol negotiation)
/// and can force KWin to draw native SSD.
///
/// The rule is written to ~/.config/kwinrulesrc and persists across sessions.
/// It's removed when the user disables system title bar.
fn setup_kwin_window_rule() -> Result<(), String> {
    let config_path = dirs::config_dir()
        .ok_or_else(|| "Could not determine config directory".to_string())?
        .join("kwinrulesrc");

    // Read existing rules to find next available slot and check for existing QBZ rule
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut existing_count: u32 = 0;
    let mut qbz_rule_group: Option<u32> = None;

    for line in existing.lines() {
        // Parse [General] count=N
        if line.starts_with("count=") {
            if let Ok(n) = line.trim_start_matches("count=").parse::<u32>() {
                existing_count = n;
            }
        }
        // Check if we already have a QBZ rule
        if line.contains("Description=QBZ Native Title Bar") {
            // Find the group number from context — we'll just scan backwards
            // Simpler: track current group
        }
    }

    // Parse INI to find existing QBZ rule group
    let mut current_group: Option<u32> = None;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];
            current_group = inner.parse::<u32>().ok();
        }
        if trimmed == "Description=QBZ Native Title Bar" {
            qbz_rule_group = current_group;
        }
    }

    let rule_num = if let Some(num) = qbz_rule_group {
        log::info!("KWin window rule for QBZ already exists (group {}), updating", num);
        num
    } else {
        existing_count + 1
    };

    // Write the rule using kwriteconfig6
    let group = rule_num.to_string();
    let rules: &[(&str, &str)] = &[
        ("Description", "QBZ Native Title Bar"),
        ("noborder", "false"),
        ("noborderrule", "2"),       // 2 = Force
        ("wmclass", "qbz"),
        ("wmclasscomplete", "false"),
        ("wmclassmatch", "1"),       // 1 = Exact match
        ("types", "1"),              // 1 = Normal windows
    ];

    for (key, value) in rules {
        let output = std::process::Command::new("kwriteconfig6")
            .args(["--file", "kwinrulesrc", "--group", &group, "--key", key, value])
            .output()
            .map_err(|e| format!("kwriteconfig6 failed: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "kwriteconfig6 --key {} failed: {}",
                key,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    // Update the count in [General] if we added a new rule
    if qbz_rule_group.is_none() {
        let new_count = (existing_count + 1).to_string();
        let output = std::process::Command::new("kwriteconfig6")
            .args(["--file", "kwinrulesrc", "--group", "General", "--key", "count", &new_count])
            .output()
            .map_err(|e| format!("kwriteconfig6 count update failed: {}", e))?;

        if !output.status.success() {
            return Err(format!(
                "kwriteconfig6 count failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    // Tell KWin to reload rules
    let output = std::process::Command::new("qdbus6")
        .args(["org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"])
        .output()
        .map_err(|e| format!("qdbus6 reconfigure failed: {}", e))?;

    if !output.status.success() {
        log::warn!(
            "KWin reconfigure failed (non-fatal): {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    log::info!("KWin window rule set for native title bar (group {})", rule_num);
    Ok(())
}

/// Remove the KWin window rule for QBZ when system title bar is disabled.
#[allow(dead_code)]
fn remove_kwin_window_rule() {
    // Find and remove QBZ rule from kwinrulesrc
    let config_path = match dirs::config_dir() {
        Some(p) => p.join("kwinrulesrc"),
        None => return,
    };

    let existing = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Find the QBZ rule group number
    let mut current_group: Option<String> = None;
    let mut qbz_group: Option<String> = None;

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_group = Some(trimmed[1..trimmed.len() - 1].to_string());
        }
        if trimmed == "Description=QBZ Native Title Bar" {
            qbz_group = current_group.clone();
        }
    }

    if let Some(group) = qbz_group {
        // Delete the group using kwriteconfig6
        let _ = std::process::Command::new("kwriteconfig6")
            .args(["--file", "kwinrulesrc", "--group", &group, "--delete-group"])
            .output();

        // Decrement count
        let mut count: u32 = 0;
        for line in existing.lines() {
            if line.starts_with("count=") {
                count = line.trim_start_matches("count=").parse().unwrap_or(0);
            }
        }
        if count > 0 {
            let new_count = (count - 1).to_string();
            let _ = std::process::Command::new("kwriteconfig6")
                .args(["--file", "kwinrulesrc", "--group", "General", "--key", "count", &new_count])
                .output();
        }

        // Reconfigure KWin
        let _ = std::process::Command::new("qdbus6")
            .args(["org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"])
            .output();

        log::info!("KWin window rule for QBZ removed");
    }
}

/// Get the primary screen resolution in logical pixels.
/// Tries multiple methods: Wayland (wlr-randr, wayland-info), X11 (xdpyinfo).
/// Returns (width, height) or None if detection fails.
#[cfg(target_os = "linux")]
fn get_screen_resolution() -> Option<(f64, f64)> {
    use std::process::Command;

    // Method 1: Try xdpyinfo (works on X11 and XWayland)
    if let Ok(output) = Command::new("xdpyinfo").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Look for "dimensions:    WIDTHxHEIGHT pixels"
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("dimensions:") {
                    // Parse "dimensions:    3840x2160 pixels (...)"
                    if let Some(dims) = trimmed.split_whitespace().nth(1) {
                        let parts: Vec<&str> = dims.split('x').collect();
                        if parts.len() == 2 {
                            if let (Ok(w), Ok(h)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                                log::info!("Screen resolution detected via xdpyinfo: {}x{}", w as u32, h as u32);
                                return Some((w, h));
                            }
                        }
                    }
                }
            }
        }
    }

    // Method 2: Try xrandr (widely available)
    if let Ok(output) = Command::new("xrandr").arg("--current").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Look for line with " connected " and a resolution like "2560x1440+0+0"
            for line in stdout.lines() {
                if line.contains(" connected ") {
                    // Parse "DP-1 connected primary 2560x1440+0+0 ..."
                    for token in line.split_whitespace() {
                        if token.contains('x') && token.contains('+') {
                            // "2560x1440+0+0"
                            let res_part = token.split('+').next().unwrap_or("");
                            let parts: Vec<&str> = res_part.split('x').collect();
                            if parts.len() == 2 {
                                if let (Ok(w), Ok(h)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                                    log::info!("Screen resolution detected via xrandr: {}x{}", w as u32, h as u32);
                                    return Some((w, h));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    log::warn!("Could not detect screen resolution");
    None
}

pub fn run() {
    // Load .env file if present (for development)
    // Silently ignore if not found (production builds use compile-time env vars)
    dotenvy::dotenv().ok();

    // Initialize logging with TeeWriter (captures to ring buffer + stderr)
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .target(env_logger::Target::Pipe(Box::new(logging::TeeWriter)))
        .init();

    log::info!("QBZ starting...");

    // Migrate data from old App ID if needed
    match flatpak::migrate_app_id_data() {
        Ok(true) => log::info!("App ID migration completed successfully"),
        Ok(false) => log::debug!("No App ID migration needed"),
        Err(e) => log::error!("App ID migration failed: {}", e),
    }

    // ── Phase 1: Device-level init (before login) ─────────────────────
    // Read audio settings from flat path once for player initialization.
    // The managed state starts empty and is populated after login.
    let (saved_device, audio_settings) = config::audio_settings::AudioSettingsStore::new()
        .ok()
        .and_then(|store| {
            store
                .get_settings()
                .ok()
                .map(|settings| (settings.output_device.clone(), settings))
        })
        .unwrap_or_else(|| {
            log::info!("No saved audio settings found, using defaults");
            (None, config::audio_settings::AudioSettings::default())
        });

    if let Some(ref device) = saved_device {
        log::info!("Initializing player with saved device: {}", device);
    }
    log::info!(
        "Audio settings: exclusive_mode={}, dac_passthrough={}, sample_rate={:?}",
        audio_settings.exclusive_mode,
        audio_settings.dac_passthrough,
        audio_settings.preferred_sample_rate
    );

    // Read tray settings for startup tray initialization.
    // Prefer last active user-scoped settings, then fallback to global flat path.
    let tray_settings = if let Some(last_uid) = user_data::UserDataPaths::load_last_user_id() {
        let user_settings = dirs::data_dir()
            .map(|d| d.join("qbz").join("users").join(last_uid.to_string()))
            .and_then(|user_dir| {
                config::tray_settings::TraySettingsStore::new_at(&user_dir)
                    .and_then(|store| store.get_settings())
                    .ok()
            });
        if let Some(settings) = user_settings {
            log::info!("Loaded tray settings from last active user {}", last_uid);
            settings
        } else {
            config::tray_settings::TraySettingsStore::new()
                .and_then(|store| store.get_settings())
                .unwrap_or_default()
        }
    } else {
        config::tray_settings::TraySettingsStore::new()
            .and_then(|store| store.get_settings())
            .unwrap_or_default()
    };
    log::info!(
        "Tray settings: enable={}, minimize_to_tray={}, close_to_tray={}",
        tray_settings.enable_tray,
        tray_settings.minimize_to_tray,
        tray_settings.close_to_tray
    );

    // Read window settings for decoration and size configuration before window creation.
    let window_settings = config::window_settings::WindowSettingsStore::new()
        .and_then(|store| store.get_settings())
        .unwrap_or_default();
    log::info!(
        "Window settings: use_system_titlebar={}, size={}x{}, maximized={}",
        window_settings.use_system_titlebar,
        window_settings.window_width as u32,
        window_settings.window_height as u32,
        window_settings.is_maximized,
    );
    let use_system_titlebar = window_settings.use_system_titlebar;
    let mut saved_win_width = window_settings.window_width;
    let mut saved_win_height = window_settings.window_height;
    let saved_win_maximized = window_settings.is_maximized;

    // Safety: clamp window size to screen resolution.
    // Prevents the window from opening larger than the display (issue #139).
    // This also handles corrupt DB values that pass the 200..32767 validation
    // but exceed the actual monitor dimensions.
    #[cfg(target_os = "linux")]
    {
        // Read current screen resolution from xdpyinfo or Wayland
        if let Some((screen_w, screen_h)) = get_screen_resolution() {
            // Leave room for taskbar/panels (90% of screen)
            let max_w = screen_w * 0.95;
            let max_h = screen_h * 0.95;
            if saved_win_width > max_w || saved_win_height > max_h {
                log::warn!(
                    "Window size {}x{} exceeds screen {}x{}, clamping to {}x{}",
                    saved_win_width as u32, saved_win_height as u32,
                    screen_w as u32, screen_h as u32,
                    max_w as u32, max_h as u32
                );
                saved_win_width = saved_win_width.min(max_w);
                saved_win_height = saved_win_height.min(max_h);
            }
        }
    }

    // One-time cleanup: remove the KWin SSD window rule written by QBZ 1.1.14.
    // That version wrote a kwinrulesrc entry forcing server-side decorations; it
    // was removed in 1.1.15 due to a GTK3/WebKit heap corruption bug. We silently
    // delete the stale rule so KWin stops applying SSD on existing installs.
    // No qdbus6 reconfigure call here — KWin picks it up on next restart, and
    // users affected can run `qdbus6 org.kde.KWin /KWin reconfigure` manually or
    // just restart their session. This block does nothing if the rule is absent.
    {
        if let Some(path) = dirs::config_dir().map(|d| d.join("kwinrulesrc")) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if content.contains("Description=QBZ Native Title Bar") {
                    // Strip the [N] group that contains our rule
                    let mut out = String::with_capacity(content.len());
                    let mut skip = false;
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if trimmed.starts_with('[') && trimmed.ends_with(']') {
                            skip = false;
                        }
                        if trimmed == "Description=QBZ Native Title Bar" {
                            // Remove the header we already wrote + this section
                            // by trimming back to the previous newline
                            if let Some(pos) = out.rfind('[') {
                                out.truncate(pos);
                            }
                            skip = true;
                            continue;
                        }
                        if !skip {
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                    if let Err(e) = std::fs::write(&path, out) {
                        log::warn!("[Cleanup] Failed to remove stale KWin rule: {}", e);
                    } else {
                        log::info!("[Cleanup] Removed stale KWin SSD rule from kwinrulesrc");
                    }
                }
            }
        }
    }

    // Initialize casting state (Chromecast, DLNA) — device-level, not per-user
    let cast_state = cast::CastState::new().expect("Failed to initialize Chromecast state");
    let dlna_state = cast::DlnaState::new(cast_state.media_server.clone())
        .expect("Failed to initialize DLNA state");

    // Initialize API server state for remote control (device-level)
    let api_server_state = api_server::ApiServerState::new();

    // ── Phase 2: Per-user states (empty until activate_user_session) ──
    let library_state = library::init_library_state_empty();
    let offline_cache_state = offline_cache::OfflineCacheState::new_empty();
    let lyrics_state = lyrics::LyricsState::new_empty();
    let reco_state = reco_store::RecoState::new_empty();
    let api_cache_state = api_cache::ApiCacheState::new_empty();
    let session_store_state = session_store::SessionStoreState::new_empty();
    let audio_settings_state = config::audio_settings::AudioSettingsState::new_empty();
    let download_settings_state = config::download_settings::create_empty_download_settings_state();
    let offline_state = offline::OfflineState::new_empty();
    let playback_prefs_state = config::playback_preferences::PlaybackPreferencesState::new_empty();
    let favorites_prefs_state =
        config::favorites_preferences::FavoritesPreferencesState::new_empty();
    let favorites_cache_state = config::favorites_cache::FavoritesCacheState::new_empty();
    let tray_settings_state = config::tray_settings::TraySettingsState::new_empty();
    let remote_control_settings_state =
        config::remote_control_settings::RemoteControlSettingsState::new_empty();
    let allowed_origins_state = config::remote_control_settings::AllowedOriginsState::new_empty();
    // LegalSettings is GLOBAL (not per-user) - must be initialized at startup
    // so ToS acceptance can be checked BEFORE attempting auto-login
    let legal_settings_state = config::legal_settings::create_legal_settings_state()
        .unwrap_or_else(|e| {
            log::warn!(
                "Failed to initialize legal settings: {}. Using empty state.",
                e
            );
            config::legal_settings::create_empty_legal_settings_state()
        });
    let updates_state =
        updates::UpdatesState::new_empty().expect("Failed to initialize empty updates state");
    let subscription_state = config::create_empty_subscription_state();
    let musicbrainz_state = musicbrainz::MusicBrainzSharedState::new_empty();
    let artist_vectors_state = artist_vectors::ArtistVectorStoreState::new_empty();
    let blacklist_state = artist_blacklist::BlacklistState::new_empty();
    let listenbrainz_state = listenbrainz::ListenBrainzSharedState::new_empty();

    // V2 integration states (using qbz-integrations crate)
    let listenbrainz_v2_state = integrations_v2::ListenBrainzV2State::new();
    let musicbrainz_v2_state = integrations_v2::MusicBrainzV2State::new();
    let lastfm_v2_state = integrations_v2::LastFmV2State::new();
    let developer_settings_state = config::developer_settings::DeveloperSettingsState::new()
        .unwrap_or_else(|e| {
            log::warn!(
                "Failed to initialize developer settings: {}. Using empty state.",
                e
            );
            config::developer_settings::DeveloperSettingsState::new_empty()
        });
    let graphics_settings_state = config::graphics_settings::GraphicsSettingsState::new()
        .unwrap_or_else(|e| {
            log::warn!(
                "Failed to initialize graphics settings: {}. Using empty state.",
                e
            );
            config::graphics_settings::GraphicsSettingsState::new_empty()
        });
    let window_settings_state =
        config::window_settings::WindowSettingsState::new().unwrap_or_else(|e| {
            log::warn!(
                "Failed to initialize window settings: {}. Using empty state.",
                e
            );
            config::window_settings::WindowSettingsState::new_empty()
        });

    // Clone settings for use in closures
    let enable_tray = tray_settings.enable_tray;

    // Initialize per-user data paths (no user active yet until login)
    let user_data_paths = user_data::UserDataPaths::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Second instance launched — bring existing window to front
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }

            // Check if second instance was launched with a Qobuz link arg
            for arg in &args {
                if arg.starts_with("qobuzapp://")
                    || arg.contains("play.qobuz.com/")
                    || arg.contains("open.qobuz.com/")
                {
                    if let Ok(resolved) = qbz_qobuz::resolve_link(arg) {
                        log::info!("Single-instance forwarding link: {:?}", resolved);
                        let _ = app.emit("link:resolved", &resolved);
                    }
                    break;
                }
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::with_device_and_settings(
            saved_device,
            audio_settings,
        ))
        .manage(core_bridge::CoreBridgeState::new())
        .manage(runtime::RuntimeManagerState::new())
        .manage(user_data_paths)
        .setup(move |app| {
            // On KDE Plasma + Wayland, GTK3 always uses client-side decorations
            // (CSD) regardless of GTK_CSD env var, because it hardcodes
            // CLIENT_SIDE in the xdg-decoration protocol. This means
            // decorations(true) shows a GTK/Breeze-GTK title bar, not the
            // native KDE Breeze one.
            //
            // Workaround: create the window with decorations=false (no GTK CSD),
            // then load a KWin script via D-Bus that forces KWin to draw its own
            // server-side decorations (SSD) for QBZ. This gives a single, native
            // KDE title bar identical to Dolphin/Konsole.
            let is_kde_wayland = std::env::var("GDK_BACKEND")
                .map(|v| v == "wayland")
                .unwrap_or(false)
                && auto_theme::system::detect_desktop_environment()
                    == auto_theme::system::DesktopEnvironment::KdePlasma;

            let use_kwin_ssd = use_system_titlebar && is_kde_wayland;

            // On KDE Wayland: always decorations=false, KWin script adds SSD
            // On other DEs: use decorations directly (GTK CSD is acceptable)
            let gtk_decorations = if use_kwin_ssd {
                false
            } else {
                use_system_titlebar
            };

            log::info!(
                "Creating main window (decorations={}, kwin_ssd={})",
                gtk_decorations,
                use_kwin_ssd
            );
            let main_window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App(std::path::PathBuf::from("index.html")),
            )
            .title("QBZ")
            .inner_size(saved_win_width, saved_win_height)
            .min_inner_size(800.0, 600.0)
            .decorations(gtk_decorations)
            .transparent(true)
            .resizable(true)
            .zoom_hotkeys_enabled(true)
            .build()
            .map_err(|e| {
                log::error!("Failed to create main window: {}", e);
                e
            })?;

            // Restore maximized state
            if saved_win_maximized {
                let _ = main_window.maximize();
            }

            // Persist window size across sessions.
            // - Resize (non-maximized): saves width/height so last user size is remembered.
            // - CloseRequested: saves the maximized flag.
            {
                let ws_state = app.state::<config::window_settings::WindowSettingsState>();
                let store = std::sync::Arc::clone(&ws_state.store);
                let win_for_events = main_window.clone();
                main_window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::Resized(size) => {
                            if size.width > 0 && size.height > 0 {
                                let maximized = win_for_events.is_maximized().unwrap_or(false);
                                if !maximized {
                                    // Convert physical pixels to logical pixels
                                    // inner_size() expects logical, Resized gives physical
                                    let scale = win_for_events.scale_factor().unwrap_or(1.0);
                                    let logical_w = size.width as f64 / scale;
                                    let logical_h = size.height as f64 / scale;
                                    if let Ok(guard) = store.lock() {
                                        if let Some(s) = guard.as_ref() {
                                            let _ = s.set_window_size(logical_w, logical_h);
                                        }
                                    }
                                }
                            }
                        }
                        tauri::WindowEvent::CloseRequested { .. } => {
                            let maximized = win_for_events.is_maximized().unwrap_or(false);
                            if let Ok(guard) = store.lock() {
                                if let Some(s) = guard.as_ref() {
                                    let _ = s.set_is_maximized(maximized);
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }

            // Add KWin window rule to force server-side decorations for QBZ
            if use_kwin_ssd {
                std::thread::spawn(|| {
                    if let Err(e) = setup_kwin_window_rule() {
                        log::warn!("Failed to set KWin window rule: {}", e);
                    }
                });
            }

            // Initialize system tray icon (only if enabled)
            if enable_tray {
                if let Err(e) = tray::init_tray(app.handle()) {
                    log::error!("Failed to initialize tray icon: {}", e);
                }
            } else {
                log::info!("System tray icon disabled by user setting");
            }

            // Initialize media controls (MPRIS) now that we have an AppHandle
            app.state::<AppState>()
                .media_controls
                .init(app.handle().clone());

            // Initialize CoreBridge (new multi-crate architecture)
            // Store V2 player state for event loop access
            let v2_player_state: Arc<tokio::sync::RwLock<Option<qbz_player::SharedState>>> =
                Arc::new(tokio::sync::RwLock::new(None));
            let v2_player_state_setter = v2_player_state.clone();
            {
                let core_bridge_arc = app.state::<core_bridge::CoreBridgeState>().0.clone();
                let adapter = tauri_adapter::TauriAdapter::new(app.handle().clone());
                let v1_viz_tap = app.state::<AppState>().visualizer.get_tap();
                let v2_viz_tap = qbz_audio::VisualizerTap {
                    ring_buffer: v1_viz_tap.ring_buffer.clone(),
                    enabled: v1_viz_tap.enabled.clone(),
                    sample_rate: v1_viz_tap.sample_rate.clone(),
                };
                tauri::async_runtime::spawn(async move {
                    let bridge = core_bridge::CoreBridge::new(adapter, Some(v2_viz_tap)).await;
                    match bridge {
                        Ok(b) => {
                            // Store V2 player state for event loop
                            let v2_state = b.player().state.clone();
                            *v2_player_state_setter.write().await = Some(v2_state);
                            *core_bridge_arc.write().await = Some(b);
                            log::info!(
                                "CoreBridge initialized successfully (V2 player state captured)"
                            );
                        }
                        Err(e) => {
                            log::error!("Failed to initialize CoreBridge: {}", e);
                        }
                    }
                });
            }

            // NOTE: Visualizer FFT thread and Remote Control API server are started
            // in activate_user_session (post-login), not here. They need per-user
            // state to be initialized first.

            // NOTE: Subscription purge check moved to activate_user_session
            // (runs after login when per-user state is available)

            // Check if app was launched with a Qobuz link argument
            // (first launch, not single-instance — that's handled by the plugin above)
            {
                let launch_handle = app.handle().clone();
                let args: Vec<String> = std::env::args().collect();
                for arg in &args[1..] {
                    // skip binary name
                    if arg.starts_with("qobuzapp://")
                        || arg.contains("play.qobuz.com/")
                        || arg.contains("open.qobuz.com/")
                    {
                        if let Ok(resolved) = qbz_qobuz::resolve_link(arg) {
                            log::info!("Launch arg link resolved: {:?}", resolved);
                            // Delay emission to give frontend time to mount
                            tauri::async_runtime::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                let _ = launch_handle.emit("link:resolved", &resolved);
                            });
                        }
                        break;
                    }
                }
            }

            // Start background task to emit playback events
            let app_handle = app.handle().clone();
            let legacy_player_state = app.state::<AppState>().player.state.clone();

            tauri::async_runtime::spawn(async move {
                let mut last_position: u64 = 0;
                let mut last_is_playing: bool = false;
                let mut last_track_id: u64 = 0;

                loop {
                    // Check V2 player state first (takes priority if active)
                    // V2 player is accessed via async lock, but we only need a clone
                    let v2_state_opt: Option<qbz_player::SharedState> =
                        v2_player_state.read().await.clone();

                    // Determine which player is active (V2 takes priority if it has a track)
                    let (
                        is_playing,
                        track_id,
                        position,
                        duration,
                        volume,
                        sample_rate,
                        bit_depth,
                        normalization_gain,
                        gapless_ready,
                        gapless_next_track_id,
                    ) = if let Some(ref v2_state) = v2_state_opt {
                        let v2_track_id = v2_state.current_track_id();
                        if v2_track_id != 0 {
                            // V2 player is active
                            (
                                v2_state.is_playing(),
                                v2_track_id,
                                v2_state.current_position(),
                                v2_state.duration(),
                                v2_state.volume(),
                                v2_state.get_sample_rate(),
                                v2_state.get_bit_depth(),
                                v2_state.get_normalization_gain(),
                                v2_state.is_gapless_ready(),
                                v2_state.get_gapless_next_track_id(),
                            )
                        } else {
                            // Fallback to legacy player
                            (
                                legacy_player_state.is_playing(),
                                legacy_player_state.current_track_id(),
                                legacy_player_state.current_position(),
                                legacy_player_state.duration(),
                                legacy_player_state.volume(),
                                legacy_player_state.get_sample_rate(),
                                legacy_player_state.get_bit_depth(),
                                legacy_player_state.get_normalization_gain(),
                                legacy_player_state.is_gapless_ready(),
                                legacy_player_state.get_gapless_next_track_id(),
                            )
                        }
                    } else {
                        // V2 not initialized yet, use legacy
                        (
                            legacy_player_state.is_playing(),
                            legacy_player_state.current_track_id(),
                            legacy_player_state.current_position(),
                            legacy_player_state.duration(),
                            legacy_player_state.volume(),
                            legacy_player_state.get_sample_rate(),
                            legacy_player_state.get_bit_depth(),
                            legacy_player_state.get_normalization_gain(),
                            legacy_player_state.is_gapless_ready(),
                            legacy_player_state.get_gapless_next_track_id(),
                        )
                    };

                    // Emit when:
                    // 1) normal in-track state changes/position updates, or
                    // 2) terminal transition to no-track (track_id becomes 0).
                    // Case (2) is required so frontend can run end-of-track fallback
                    // auto-advance paths reliably.
                    let track_cleared = track_id == 0 && last_track_id != 0;
                    let should_emit = (track_id != 0
                        && (is_playing != last_is_playing
                            || track_id != last_track_id
                            || (is_playing && position != last_position)))
                        || track_cleared;

                    let should_update_mpris = should_emit || (track_id == 0 && last_track_id != 0);

                    if should_emit {
                        // Get queue state for shuffle/repeat (still from legacy queue)
                        let queue_state = &app_handle.state::<AppState>().queue;
                        let shuffle = queue_state.is_shuffle();
                        let repeat = match queue_state.get_repeat() {
                            queue::RepeatMode::Off => "off",
                            queue::RepeatMode::All => "all",
                            queue::RepeatMode::One => "one",
                        };
                        // Use values collected from active player (V2 or legacy)
                        let event = player::PlaybackEvent {
                            is_playing,
                            position,
                            duration,
                            track_id,
                            volume,
                            sample_rate: if sample_rate > 0 {
                                Some(sample_rate)
                            } else {
                                None
                            },
                            bit_depth: if bit_depth > 0 { Some(bit_depth) } else { None },
                            shuffle: Some(shuffle),
                            repeat: Some(repeat.to_string()),
                            normalization_gain,
                            gapless_ready,
                            gapless_next_track_id,
                        };
                        let _ = app_handle.emit("playback:state", &event);
                        api_server::broadcast_playback_event(&app_handle, &event);
                        last_position = position;
                        last_is_playing = is_playing;
                        last_track_id = track_id;
                    }

                    if should_update_mpris {
                        let media_controls = &app_handle.state::<AppState>().media_controls;
                        if track_id == 0 {
                            media_controls.set_stopped();
                        } else {
                            media_controls.set_playback_with_progress(is_playing, position);
                        }
                    }

                    // Adaptive polling:
                    // - fast (250ms) when playing - improves seekbar/lyrics sync
                    // - slow (1000ms) when paused/stopped with a track loaded
                    // - very slow (5000ms) when no track is loaded (idle)
                    let sleep_duration = if is_playing {
                        std::time::Duration::from_millis(250)
                    } else if track_id == 0 {
                        std::time::Duration::from_millis(5000)
                    } else {
                        std::time::Duration::from_millis(1000)
                    };
                    tokio::time::sleep(sleep_duration).await;
                }
            });

            Ok(())
        })
        .on_window_event(move |window, event| {
            // Only handle close-to-tray for the main window.
            // Secondary windows (miniplayer, oauth) are managed elsewhere.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" {
                    return;
                }
                let close_to_tray = window
                    .app_handle()
                    .try_state::<config::tray_settings::TraySettingsState>()
                    .and_then(|state| state.get_settings().ok())
                    .map(|s| s.close_to_tray)
                    .unwrap_or(false);
                if close_to_tray {
                    log::info!("Close to tray: hiding window instead of closing");
                    let _ = window.hide();
                    // Also hide the miniplayer if open
                    if let Some(mini) = window.app_handle().webview_windows().get("miniplayer") {
                        let _ = mini.hide();
                    }
                    api.prevent_close();
                } else {
                    // Close secondary windows before the main window closes.
                    // Use close() (not destroy()) so WebKit can clean up gracefully.
                    // Set flag so RunEvent::Exit won't double-close them.
                    WINDOWS_CLOSED_BY_USER.store(true, std::sync::atomic::Ordering::SeqCst);
                    for (label, win) in window.app_handle().webview_windows() {
                        if label != "main" {
                            log::info!("App closing: closing secondary window '{}'", label);
                            let _ = win.close();
                        }
                    }

                    // Cleanup cast devices on actual close
                    log::info!("App closing: cleaning up cast devices");

                    // Disconnect Chromecast if connected (sends message through channel)
                    if let Some(cast_state) = window.app_handle().try_state::<cast::CastState>() {
                        log::info!("Disconnecting Chromecast on app exit");
                        let _ = cast_state.chromecast.disconnect();
                    }

                    log::info!("DLNA connection will be cleaned up on drop");
                }
            }
        })
        .manage(library_state)
        .manage(cast_state)
        .manage(dlna_state)
        // .manage(airplay_state)  // AirPlay DISABLED
        .manage(offline_cache_state)
        .manage(lyrics_state)
        .manage(reco_state)
        .manage(api_cache_state)
        .manage(session_store_state)
        .manage(audio_settings_state)
        .manage(download_settings_state)
        .manage(subscription_state)
        .manage(offline_state)
        .manage(playback_prefs_state)
        .manage(favorites_prefs_state)
        .manage(favorites_cache_state)
        .manage(tray_settings_state)
        .manage(remote_control_settings_state)
        .manage(allowed_origins_state)
        .manage(api_server_state)
        .manage(legal_settings_state)
        .manage(updates_state)
        .manage(musicbrainz_state)
        .manage(listenbrainz_state)
        .manage(artist_vectors_state)
        .manage(blacklist_state)
        .manage(developer_settings_state)
        .manage(graphics_settings_state)
        .manage(window_settings_state)
        .manage(pdf_viewer::BookletState::new())
        // V2 integration states (qbz-integrations crate)
        .manage(listenbrainz_v2_state)
        .manage(musicbrainz_v2_state)
        .manage(lastfm_v2_state)
        .invoke_handler(tauri::generate_handler![
            commands_v2::runtime_get_status,
            commands_v2::runtime_bootstrap,
            commands_v2::v2_is_logged_in,
            commands_v2::v2_login,
            commands_v2::v2_logout,
            commands_v2::v2_activate_offline_session,
            commands_v2::v2_init_client,
            commands_v2::v2_auto_login,
            commands_v2::v2_manual_login,
            commands_v2::v2_start_oauth_login,
            commands_v2::v2_get_user_info,
            commands_v2::v2_save_credentials,
            commands_v2::v2_clear_saved_credentials,
            // Temporary compatibility commands still invoked by frontend during migration
            offline_cache::commands::check_album_fully_cached,
            network::commands::check_network_path,
            offline::commands::create_pending_playlist,
            offline::commands::get_pending_playlist_count,
            offline::commands::queue_scrobble,
            offline::commands::get_queued_scrobbles,
            offline::commands::get_queued_scrobble_count,
            offline::commands::cleanup_sent_scrobbles,
            library::commands::discogs_has_credentials,
            library::commands::discogs_search_artwork,
            library::commands::discogs_download_artwork,
            library::commands::library_update_folder_settings,
            library::commands::get_track_by_path,
            commands::search::get_artist_basic,
            commands::search::get_artist_detail,
            commands::search::get_artist_tracks,
            commands::search::get_artist_albums,
            commands::search::get_releases_grid,
            commands::playback::get_pipewire_sinks,
            commands::playback::get_audio_output_status,
            flatpak::get_flatpak_help_text,
            config::legal_settings::get_qobuz_tos_accepted,
            updates::has_flatpak_welcome_been_shown,
            lyrics::commands::lyrics_get,
            config::favorites_cache::is_track_favorite,
            commands_v2::v2_set_api_locale,
            commands_v2::v2_set_use_system_titlebar,
            commands_v2::v2_set_enable_tray,
            commands_v2::v2_set_minimize_to_tray,
            commands_v2::v2_set_close_to_tray,
            commands_v2::v2_get_tray_settings,
            commands_v2::v2_set_autoplay_mode,
            commands_v2::v2_set_show_context_icon,
            commands_v2::v2_set_persist_session,
            commands_v2::v2_get_playback_preferences,
            commands_v2::v2_get_favorites_preferences,
            commands_v2::v2_save_favorites_preferences,
            commands_v2::v2_get_cache_stats,
            commands_v2::v2_get_available_backends,
            commands_v2::v2_get_devices_for_backend,
            commands_v2::v2_get_hardware_audio_status,
            commands_v2::v2_get_default_device_name,
            commands_v2::v2_query_dac_capabilities,
            commands_v2::v2_get_alsa_plugins,
            commands_v2::v2_plex_ping,
            commands_v2::v2_plex_get_music_sections,
            commands_v2::v2_plex_get_section_tracks,
            commands_v2::v2_plex_get_track_metadata,
            commands_v2::v2_plex_auth_pin_start,
            commands_v2::v2_plex_auth_pin_check,
            commands_v2::v2_plex_play_track,
            commands_v2::v2_set_visualizer_enabled,
            commands_v2::v2_get_developer_settings,
            commands_v2::v2_set_developer_force_dmabuf,
            commands_v2::v2_get_graphics_settings,
            commands_v2::v2_get_graphics_startup_status,
            commands_v2::v2_set_hardware_acceleration,
            commands_v2::v2_set_gdk_scale,
            commands_v2::v2_set_gdk_dpi_scale,
            commands_v2::v2_clear_cache,
            commands_v2::v2_clear_artist_cache,
            commands_v2::v2_get_vector_store_stats,
            commands_v2::v2_get_playlist_suggestions,
            commands_v2::v2_clear_vector_store,
            commands_v2::v2_add_to_artist_blacklist,
            commands_v2::v2_remove_from_artist_blacklist,
            commands_v2::v2_set_blacklist_enabled,
            commands_v2::v2_get_artist_blacklist,
            commands_v2::v2_get_blacklist_settings,
            commands_v2::v2_clear_artist_blacklist,
            commands_v2::v2_plex_open_auth_url,
            commands_v2::v2_plex_cache_get_sections,
            commands_v2::v2_plex_cache_save_sections,
            commands_v2::v2_plex_cache_save_tracks,
            commands_v2::v2_plex_cache_get_tracks,
            commands_v2::v2_plex_cache_get_albums,
            commands_v2::v2_plex_cache_get_album_tracks,
            commands_v2::v2_plex_cache_search_tracks,
            commands_v2::v2_plex_cache_update_track_quality,
            commands_v2::v2_plex_cache_get_tracks_needing_hydration,
            commands_v2::v2_plex_cache_clear,
            commands_v2::v2_cast_start_discovery,
            commands_v2::v2_cast_stop_discovery,
            commands_v2::v2_cast_get_devices,
            commands_v2::v2_cast_connect,
            commands_v2::v2_cast_disconnect,
            commands_v2::v2_cast_play_track,
            commands_v2::v2_cast_play,
            commands_v2::v2_cast_pause,
            commands_v2::v2_cast_stop,
            commands_v2::v2_cast_seek,
            commands_v2::v2_cast_get_position,
            commands_v2::v2_cast_set_volume,
            commands_v2::v2_dlna_start_discovery,
            commands_v2::v2_dlna_stop_discovery,
            commands_v2::v2_dlna_get_devices,
            commands_v2::v2_dlna_connect,
            commands_v2::v2_dlna_disconnect,
            commands_v2::v2_dlna_play_track,
            commands_v2::v2_dlna_play,
            commands_v2::v2_dlna_pause,
            commands_v2::v2_dlna_stop,
            commands_v2::v2_dlna_seek,
            commands_v2::v2_dlna_get_position,
            commands_v2::v2_dlna_set_volume,
            commands_v2::v2_airplay_start_discovery,
            commands_v2::v2_airplay_stop_discovery,
            commands_v2::v2_airplay_get_devices,
            commands_v2::v2_airplay_connect,
            commands_v2::v2_airplay_disconnect,
            commands_v2::v2_airplay_load_media,
            commands_v2::v2_airplay_play,
            commands_v2::v2_airplay_pause,
            commands_v2::v2_airplay_stop,
            commands_v2::v2_airplay_set_volume,
            commands_v2::v2_clear_offline_cache,
            commands_v2::v2_library_remove_folder,
            commands_v2::v2_library_check_folder_accessible,
            commands_v2::v2_library_clear_artwork_cache,
            commands_v2::v2_library_clear_thumbnails_cache,
            commands_v2::v2_library_get_thumbnail,
            commands_v2::v2_library_get_thumbnails_cache_size,
            commands_v2::v2_library_get_scan_progress,
            commands_v2::v2_library_get_tracks_by_ids,
            commands_v2::v2_library_play_track,
            commands_v2::v2_playlist_set_sort,
            commands_v2::v2_playlist_set_artwork,
            commands_v2::v2_playlist_get_all_settings,
            commands_v2::v2_playlist_get_favorites,
            commands_v2::v2_playlist_get_local_tracks_with_position,
            commands_v2::v2_playlist_get_settings,
            commands_v2::v2_playlist_get_stats,
            commands_v2::v2_playlist_increment_play_count,
            commands_v2::v2_playlist_get_all_stats,
            commands_v2::v2_playlist_get_all_local_track_counts,
            commands_v2::v2_playlist_add_local_track,
            commands_v2::v2_playlist_remove_local_track,
            commands_v2::v2_playlist_set_hidden,
            commands_v2::v2_playlist_set_favorite,
            commands_v2::v2_playlist_reorder,
            commands_v2::v2_playlist_init_custom_order,
            commands_v2::v2_playlist_set_custom_order,
            commands_v2::v2_playlist_move_track,
            commands_v2::v2_playlist_get_custom_order,
            commands_v2::v2_playlist_has_custom_order,
            commands_v2::v2_playlist_get_tracks_with_local_copies,
            commands_v2::v2_library_set_album_artwork,
            commands_v2::v2_library_set_album_hidden,
            commands_v2::v2_create_artist_radio,
            commands_v2::v2_create_track_radio,
            commands_v2::v2_create_album_radio,
            commands_v2::v2_create_qobuz_artist_radio,
            commands_v2::v2_create_qobuz_track_radio,
            commands_v2::v2_delete_playlist_folder,
            commands_v2::v2_reorder_playlist_folders,
            commands_v2::v2_move_playlist_to_folder,
            commands_v2::v2_get_playlist_folders,
            commands_v2::v2_create_playlist_folder,
            commands_v2::v2_update_playlist_folder,
            commands_v2::v2_lyrics_clear_cache,
            commands_v2::v2_lyrics_get_cache_stats,
            commands_v2::v2_musicbrainz_get_cache_stats,
            commands_v2::v2_musicbrainz_clear_cache,
            commands_v2::v2_set_show_partial_playlists,
            commands_v2::v2_set_allow_cast_while_offline,
            commands_v2::v2_set_allow_immediate_scrobbling,
            commands_v2::v2_set_allow_accumulated_scrobbling,
            commands_v2::v2_set_show_network_folders_in_manual_offline,
            commands_v2::v2_get_offline_status,
            commands_v2::v2_get_offline_settings,
            commands_v2::v2_set_manual_offline,
            commands_v2::v2_check_network,
            commands_v2::v2_add_tracks_to_pending_playlist,
            commands_v2::v2_get_pending_playlists,
            commands_v2::v2_update_pending_playlist_qobuz_id,
            commands_v2::v2_mark_pending_playlist_synced,
            commands_v2::v2_delete_pending_playlist,
            commands_v2::v2_mark_scrobbles_sent,
            commands_v2::v2_remove_cached_track,
            commands_v2::v2_get_cached_tracks,
            commands_v2::v2_get_offline_cache_stats,
            commands_v2::v2_set_offline_cache_limit,
            commands_v2::v2_open_offline_cache_folder,
            commands_v2::v2_open_album_folder,
            commands_v2::v2_open_track_folder,
            commands_v2::v2_lastfm_open_auth_url,
            commands_v2::v2_lastfm_set_credentials,
            commands_v2::v2_lastfm_has_embedded_credentials,
            commands_v2::v2_remote_control_get_status,
            commands_v2::v2_remote_control_set_enabled,
            commands_v2::v2_remote_control_set_port,
            commands_v2::v2_remote_control_set_secure,
            commands_v2::v2_remote_control_regenerate_token,
            commands_v2::v2_remote_control_get_pairing_qr,
            commands_v2::v2_is_running_in_flatpak,
            commands_v2::v2_detect_legacy_cached_files,
            commands_v2::v2_reco_log_event,
            commands_v2::v2_reco_train_scores,
            commands_v2::v2_reco_get_home,
            commands_v2::v2_reco_get_home_ml,
            commands_v2::v2_reco_get_home_resolved,
            commands_v2::v2_library_get_cache_stats,
            commands_v2::v2_library_get_stats,
            commands_v2::v2_library_get_albums,
            commands_v2::v2_library_get_folders,
            commands_v2::v2_library_get_folders_with_metadata,
            commands_v2::v2_library_add_folder,
            commands_v2::v2_library_cleanup_missing_files,
            commands_v2::v2_library_fetch_missing_artwork,
            commands_v2::v2_library_get_artists,
            commands_v2::v2_library_search,
            commands_v2::v2_library_get_album_tracks,
            commands_v2::v2_library_update_folder_path,
            commands_v2::v2_library_cache_artist_image,
            commands_v2::v2_library_set_custom_artist_image,
            commands_v2::v2_library_remove_custom_artist_image,
            commands_v2::v2_library_get_artist_image,
            commands_v2::v2_library_get_all_custom_artist_images,
            commands_v2::v2_library_set_custom_album_cover,
            commands_v2::v2_library_remove_custom_album_cover,
            commands_v2::v2_library_get_all_custom_album_covers,
            commands_v2::v2_save_image_url_to_file,
            commands_v2::v2_show_track_notification,
            commands_v2::v2_subscribe_playlist,
            commands_v2::v2_cache_track_for_offline,
            commands_v2::v2_cache_tracks_batch_for_offline,
            commands_v2::v2_start_legacy_migration,
            commands_v2::v2_library_scan,
            commands_v2::v2_library_stop_scan,
            commands_v2::v2_library_scan_folder,
            commands_v2::v2_library_clear,
            commands_v2::v2_library_update_album_metadata,
            commands_v2::v2_library_write_album_metadata_to_files,
            commands_v2::v2_library_refresh_album_metadata_from_files,
            commands_v2::v2_factory_reset,
            commands_v2::v2_set_qobuz_tos_accepted,
            commands_v2::v2_get_update_preferences,
            commands_v2::v2_get_current_version,
            commands_v2::v2_check_for_updates,
            commands_v2::v2_fetch_release_for_version,
            commands_v2::v2_set_update_check_on_launch,
            commands_v2::v2_set_show_whats_new_on_launch,
            commands_v2::v2_acknowledge_release,
            commands_v2::v2_ignore_release,
            commands_v2::v2_has_whats_new_been_shown,
            commands_v2::v2_mark_whats_new_shown,
            commands_v2::v2_mark_flatpak_welcome_shown,
            commands_v2::v2_get_backend_logs,
            commands_v2::v2_upload_logs_to_paste,
            commands_v2::v2_get_download_settings,
            commands_v2::v2_set_show_downloads_in_library,
            commands_v2::v2_get_device_sample_rate_limit,
            commands_v2::v2_set_device_sample_rate_limit,
            commands_v2::v2_set_force_x11,
            commands_v2::v2_restart_app,
            commands_v2::v2_get_queue_state,
            commands_v2::v2_get_all_queue_tracks,
            commands_v2::v2_get_current_queue_track,
            commands_v2::v2_set_repeat_mode,
            commands_v2::v2_toggle_shuffle,
            commands_v2::v2_set_shuffle,
            commands_v2::v2_clear_queue,
            commands_v2::v2_add_to_queue,
            commands_v2::v2_add_to_queue_next,
            commands_v2::v2_bulk_add_to_queue,
            commands_v2::v2_bulk_add_to_queue_next,
            commands_v2::v2_set_queue,
            commands_v2::v2_remove_from_queue,
            commands_v2::v2_remove_upcoming_track,
            commands_v2::v2_next_track,
            commands_v2::v2_previous_track,
            commands_v2::v2_play_queue_index,
            commands_v2::v2_move_queue_track,
            commands_v2::v2_add_tracks_to_queue,
            commands_v2::v2_add_tracks_to_queue_next,
            commands_v2::v2_search_albums,
            commands_v2::v2_search_tracks,
            commands_v2::v2_search_artists,
            commands_v2::v2_search_all,
            commands_v2::v2_get_album,
            commands_v2::v2_get_track,
            commands_v2::v2_get_artist,
            commands_v2::v2_get_favorites,
            commands_v2::v2_add_favorite,
            commands_v2::v2_remove_favorite,
            commands_v2::v2_get_user_playlists,
            commands_v2::v2_get_playlist,
            commands_v2::v2_playlist_import_preview,
            commands_v2::v2_playlist_import_execute,
            commands_v2::v2_get_playlist_track_ids,
            commands_v2::v2_check_playlist_duplicates,
            commands_v2::v2_add_tracks_to_playlist,
            commands_v2::v2_remove_tracks_from_playlist,
            commands_v2::v2_create_playlist,
            commands_v2::v2_delete_playlist,
            commands_v2::v2_update_playlist,
            commands_v2::v2_search_playlists,
            commands_v2::v2_get_tracks_batch,
            commands_v2::v2_get_genres,
            commands_v2::v2_get_discover_index,
            commands_v2::v2_get_discover_playlists,
            commands_v2::v2_get_playlist_tags,
            commands_v2::v2_get_discover_albums,
            commands_v2::v2_get_featured_albums,
            commands_v2::v2_get_artist_page,
            commands_v2::v2_get_similar_artists,
            commands_v2::v2_get_artist_with_albums,
            commands_v2::v2_get_label,
            commands_v2::v2_get_label_page,
            commands_v2::v2_get_label_explore,
            commands_v2::v2_pause_playback,
            commands_v2::v2_resume_playback,
            commands_v2::v2_stop_playback,
            commands_v2::v2_seek,
            commands_v2::v2_set_volume,
            commands_v2::v2_get_playback_state,
            commands_v2::v2_play_track,
            commands_v2::v2_set_media_metadata,
            commands_v2::v2_play_next_gapless,
            commands_v2::v2_prefetch_track,
            commands_v2::v2_reinit_audio_device,
            commands_v2::v2_get_audio_settings,
            commands_v2::v2_set_audio_output_device,
            commands_v2::v2_set_audio_exclusive_mode,
            commands_v2::v2_set_audio_dac_passthrough,
            commands_v2::v2_set_audio_pw_force_bitperfect,
            commands_v2::v2_set_sync_audio_on_startup,
            commands_v2::v2_set_audio_sample_rate,
            commands_v2::v2_set_audio_backend_type,
            commands_v2::v2_set_audio_alsa_plugin,
            commands_v2::v2_set_audio_gapless_enabled,
            commands_v2::v2_set_audio_normalization_enabled,
            commands_v2::v2_set_audio_normalization_target,
            commands_v2::v2_set_audio_device_max_sample_rate,
            commands_v2::v2_set_audio_limit_quality_to_device,
            commands_v2::v2_set_audio_streaming_only,
            commands_v2::v2_reset_audio_settings,
            commands_v2::v2_set_audio_stream_first_track,
            commands_v2::v2_set_audio_stream_buffer_seconds,
            commands_v2::v2_set_audio_alsa_hardware_volume,
            commands_v2::v2_listenbrainz_get_status,
            commands_v2::v2_listenbrainz_is_enabled,
            commands_v2::v2_listenbrainz_set_enabled,
            commands_v2::v2_listenbrainz_connect,
            commands_v2::v2_listenbrainz_disconnect,
            commands_v2::v2_listenbrainz_now_playing,
            commands_v2::v2_listenbrainz_scrobble,
            commands_v2::v2_musicbrainz_is_enabled,
            commands_v2::v2_musicbrainz_set_enabled,
            commands_v2::v2_musicbrainz_resolve_track,
            commands_v2::v2_musicbrainz_resolve_artist,
            commands_v2::v2_resolve_musician,
            commands_v2::v2_get_musician_appearances,
            commands_v2::v2_remote_metadata_search,
            commands_v2::v2_remote_metadata_get_album,
            commands_v2::v2_musicbrainz_get_artist_relationships,
            commands_v2::v2_lastfm_get_auth_url,
            commands_v2::v2_lastfm_complete_auth,
            commands_v2::v2_lastfm_is_authenticated,
            commands_v2::v2_lastfm_disconnect,
            commands_v2::v2_lastfm_now_playing,
            commands_v2::v2_lastfm_scrobble,
            commands_v2::v2_lastfm_set_session,
            commands_v2::v2_listenbrainz_queue_listen,
            commands_v2::v2_listenbrainz_flush_queue,
            commands_v2::v2_get_playback_context,
            commands_v2::v2_set_playback_context,
            commands_v2::v2_clear_playback_context,
            commands_v2::v2_has_playback_context,
            commands_v2::v2_save_session_state,
            commands_v2::v2_load_session_state,
            commands_v2::v2_save_session_position,
            commands_v2::v2_save_session_volume,
            commands_v2::v2_save_session_playback_mode,
            commands_v2::v2_clear_session,
            commands_v2::v2_get_cached_favorite_tracks,
            commands_v2::v2_sync_cached_favorite_tracks,
            commands_v2::v2_cache_favorite_track,
            commands_v2::v2_uncache_favorite_track,
            commands_v2::v2_bulk_add_favorites,
            commands_v2::v2_clear_favorites_cache,
            commands_v2::v2_get_cached_favorite_albums,
            commands_v2::v2_sync_cached_favorite_albums,
            commands_v2::v2_cache_favorite_album,
            commands_v2::v2_uncache_favorite_album,
            commands_v2::v2_get_cached_favorite_artists,
            commands_v2::v2_sync_cached_favorite_artists,
            commands_v2::v2_cache_favorite_artist,
            commands_v2::v2_uncache_favorite_artist,
            commands_v2::v2_share_track_songlink,
            commands_v2::v2_share_album_songlink,
            commands_v2::v2_library_backfill_downloads,
            commands_v2::v2_lyrics_get,
            commands_v2::v2_create_pending_playlist,
            commands_v2::v2_get_pending_playlist_count,
            commands_v2::v2_queue_scrobble,
            commands_v2::v2_get_queued_scrobbles,
            commands_v2::v2_get_queued_scrobble_count,
            commands_v2::v2_cleanup_sent_scrobbles,
            commands_v2::v2_get_track_by_path,
            commands_v2::v2_check_network_path,
            commands_v2::v2_library_update_folder_settings,
            commands_v2::v2_discogs_has_credentials,
            commands_v2::v2_discogs_search_artwork,
            commands_v2::v2_discogs_download_artwork,
            commands_v2::v2_check_album_fully_cached,
            commands_v2::v2_purchases_get_all,
            commands_v2::v2_purchases_get_ids,
            commands_v2::v2_purchases_get_by_type,
            commands_v2::v2_purchases_search,
            commands_v2::v2_purchases_get_album,
            commands_v2::v2_purchases_get_formats,
            commands_v2::v2_purchases_download_album,
            commands_v2::v2_purchases_download_track,
            commands_v2::v2_purchases_mark_downloaded,
            commands_v2::v2_purchases_remove_downloaded,
            commands_v2::v2_purchases_get_downloaded_track_ids,
            commands_v2::v2_dynamic_suggest,
            commands_v2::v2_dynamic_suggest_raw,
            commands_v2::v2_resolve_music_link,
            commands_v2::v2_resolve_qobuz_link,
            commands_v2::v2_get_qobuz_track_url,
            commands_v2::v2_check_qobuzapp_handler,
            commands_v2::v2_register_qobuzapp_handler,
            commands_v2::v2_deregister_qobuzapp_handler,
            // Auto-theme
            commands_v2::v2_detect_desktop_environment,
            commands_v2::v2_get_system_wallpaper,
            commands_v2::v2_get_system_accent_color,
            commands_v2::v2_generate_theme_from_image,
            commands_v2::v2_generate_theme_from_wallpaper,
            commands_v2::v2_generate_theme_from_system_colors,
            commands_v2::v2_get_system_color_scheme,
            commands_v2::v2_extract_palette,
            commands_v2::v2_fetch_url_bytes,
            // PDF booklet viewer (MuPDF backend)
            pdf_viewer::v2_booklet_open,
            pdf_viewer::v2_booklet_render_page,
            pdf_viewer::v2_booklet_save,
            pdf_viewer::v2_booklet_close,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Graceful shutdown: stop audio and visualizer BEFORE process
                // teardown. The stop() calls are fire-and-forget via channel,
                // so we must wait for the audio threads to actually drop their
                // CPAL streams.
                log::info!("RunEvent::Exit — stopping audio and visualizer");

                if let Some(state) = app_handle.try_state::<AppState>() {
                    state.visualizer.set_enabled(false);
                    let _ = state.player.stop();
                }

                if let Some(bridge_state) = app_handle.try_state::<core_bridge::CoreBridgeState>() {
                    if let Ok(guard) = bridge_state.0.try_read() {
                        if let Some(bridge) = guard.as_ref() {
                            let _ = bridge.player().stop();
                        }
                    }
                }

                // Wait for audio threads to process stop and drop CPAL streams.
                // Without this, exit() frees heap while CPAL threads run.
                std::thread::sleep(std::time::Duration::from_millis(200));

                // Only close secondary windows if CloseRequested didn't already
                // do it. Double-closing causes WebKit heap corruption because
                // close() on a window already mid-teardown triggers double-free
                // in EGL/TLS cleanup → "free(): corrupted unsorted chunks".
                if !WINDOWS_CLOSED_BY_USER.load(std::sync::atomic::Ordering::SeqCst) {
                    // SIGTERM or other non-user-initiated exit — windows weren't
                    // closed in CloseRequested, so we must close them here.
                    for (label, win) in app_handle.webview_windows() {
                        if label != "main" {
                            log::info!("Exit: closing secondary window '{}'", label);
                            let _ = win.close();
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
                log::info!("RunEvent::Exit — shutdown complete");
            }
        });
}
