//! Discord Rich Presence controller (Slint side).
//!
//! Owns a process-global [`DiscordRpc`] (the frontend-agnostic core in
//! `qbz-integrations`) and wires it to:
//!   - the opt-in toggle (Settings › Integrations), persisted in `ui_prefs`;
//!   - playback track-change + play/pause transitions, which push the
//!     "now listening" activity;
//!   - the shell session lifecycle: [`init`] runs AFTER the session is active
//!     (the PR #477 fix — never at early boot), [`clear`] on logout / exit.
//!
//! The IPC is blocking, so every Discord call runs inside `spawn_blocking`.
//! All calls are no-ops when the user has not opted in.

use std::sync::{Arc, LazyLock};

use qbz_app::shell::AppRuntime;
use qbz_integrations::{DiscordRpc, NowListening};

use crate::adapter::SlintAdapter;

type Runtime = Arc<AppRuntime<SlintAdapter>>;

/// Process-global Discord client. Holds the lazy IPC connection + opt-in flag;
/// `set_enabled(false)` tears the connection down so the presence disappears.
static DISCORD: LazyLock<DiscordRpc> = LazyLock::new(DiscordRpc::default);

/// Apply the persisted opt-in. Called from `init_shell_for_user` — i.e. AFTER
/// the session is active, for BOTH the online and offline entry paths. This is
/// the persistence fix from PR #477: initializing here (not at early boot,
/// before the session/prefs existed) makes "enabled on restart" actually launch.
pub fn init(runtime: &Runtime, handle: &tokio::runtime::Handle) {
    let enabled = crate::ui_prefs::load().discord_rpc_enabled;
    DISCORD.set_enabled(enabled);
    if enabled {
        push(runtime, handle);
    }
}

/// Settings toggle: persist the opt-in, apply it, and push the current track
/// (enable) or let `set_enabled(false)` clear the activity (disable).
pub fn set_enabled(enabled: bool, runtime: &Runtime, handle: &tokio::runtime::Handle) {
    let mut prefs = crate::ui_prefs::load();
    prefs.discord_rpc_enabled = enabled;
    crate::ui_prefs::save(&prefs);
    DISCORD.set_enabled(enabled);
    if enabled {
        push(runtime, handle);
    }
}

/// Tear down the live activity + IPC connection (logout / app exit). No-op when
/// not connected.
pub fn clear(handle: &tokio::runtime::Handle) {
    handle.spawn(async {
        let _ = tokio::task::spawn_blocking(|| DISCORD.clear()).await;
    });
}

/// Build the "now listening" snapshot from the live queue + playback state and
/// push it to Discord. No-op when disabled (cheap early return — nothing is
/// fetched). Called on track change and play/pause, mirroring the Tauri
/// service's (track_id, is_playing) transition pushes.
pub fn push(runtime: &Runtime, handle: &tokio::runtime::Handle) {
    if !DISCORD.is_enabled() {
        return;
    }
    let runtime = runtime.clone();
    handle.spawn(async move {
        let state = runtime.core().get_queue_state().await;
        let Some(track) = state.current_track else {
            // Nothing playing — drop the activity.
            let _ = tokio::task::spawn_blocking(|| DISCORD.clear()).await;
            return;
        };
        let pb = runtime.core().get_playback_state();
        let title = match track.version.as_deref().filter(|v| !v.is_empty()) {
            Some(version) => format!("{} ({version})", track.title),
            None => track.title.clone(),
        };
        // Discord's large_image needs an http(s) URL or an asset key; local /
        // Plex covers are filesystem paths Discord can't fetch, so drop them
        // (the core falls back to the "cover" asset key).
        let cover_url = track
            .artwork_url
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"));
        let meta = NowListening {
            title,
            artist: track.artist,
            album: track.album,
            is_playing: pb.is_playing,
            current_time: pb.position as f64,
            duration: track.duration_secs as f64,
            cover_url,
        };
        let _ = tokio::task::spawn_blocking(move || DISCORD.update(&meta)).await;
    });
}
