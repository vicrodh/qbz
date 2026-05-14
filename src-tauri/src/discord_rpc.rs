//! Discord Rich Presence integration.
//!
//! Wraps `discord-rich-presence` behind three Tauri V2 commands. Connection
//! is lazy: the IPC client is created on the first `update` call when the
//! feature is enabled, and dropped if any operation fails. Failure is silent
//! — when Discord is not running, when the sandbox blocks the IPC socket,
//! or when the user has not opted in, the activity simply does not appear.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use tauri::State;

/// Bridge our Flatpak sandbox to the Discord-Flatpak IPC socket.
///
/// The `discord-rich-presence` crate searches `$XDG_RUNTIME_DIR/discord-ipc-N`
/// for `N=0..9`. Native Discord (deb / rpm / AUR) writes its socket directly
/// to that path, so the search succeeds out of the box. Discord installed via
/// Flathub writes to `$XDG_RUNTIME_DIR/app/com.discordapp.Discord/discord-ipc-N`
/// because each Flatpak app gets its own runtime subdirectory; the crate's
/// default search misses it, and Rich Presence appears broken even though
/// Discord is running.
///
/// Other Flathub apps work around this with a startup wrapper script that
/// unconditionally symlinks every iteration into `$XDG_RUNTIME_DIR` for every
/// user. That conflicts with QBZ's external-service-integration posture: all
/// optional integrations are opt-in (Last.fm, ListenBrainz, MusicBrainz, Plex,
/// Discord here). Running the wrapper for every Flatpak instance — including
/// users who never enabled Discord RPC — silently turns the integration into
/// opt-out at the sandbox layer.
///
/// We make the symlinks here instead, lazily, the first time the user
/// actually connects to Discord. If the user never toggles Discord RPC on,
/// these links are never created and the sandbox stays untouched. The links
/// live for the lifetime of the Flatpak instance (`$XDG_RUNTIME_DIR` is
/// scoped per-instance) so there's no cross-session pollution either.
///
/// Returns silently on every failure path — missing env var, non-Flatpak
/// process, ENOENT on the source dir, EEXIST on the link target, etc. Any
/// caller mistake fails over to the crate's default behavior, which is the
/// same fallback the non-Flatpak code path already accepts.
fn prepare_flatpak_discord_socket_links() {
    if std::env::var_os("FLATPAK_ID").is_none() {
        return;
    }
    let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return;
    };
    let runtime_dir = std::path::PathBuf::from(runtime_dir);

    // discord-rich-presence iterates 0..=9; only bother creating a link when
    // the slot in $XDG_RUNTIME_DIR is currently empty AND the corresponding
    // path inside the Discord-Flatpak app dir exists. Either condition
    // failing is normal (Discord not running, slot already taken by native
    // Discord, etc.) and silently skipped.
    for i in 0..=9 {
        let link_target = runtime_dir.join(format!("discord-ipc-{}", i));
        if link_target.exists() || link_target.symlink_metadata().is_ok() {
            continue;
        }
        let source = runtime_dir
            .join("app")
            .join("com.discordapp.Discord")
            .join(format!("discord-ipc-{}", i));
        if !source.exists() {
            continue;
        }
        let _ = std::os::unix::fs::symlink(&source, &link_target);
    }
}

/// Public Discord Application ID for QBZ. Public identifier, not a secret.
const DISCORD_APP_ID: &str = "1501835855587708988";
const QBZ_HOMEPAGE: &str = "https://qbz.lol";

#[derive(Default)]
pub struct DiscordRpcState {
    client: Mutex<Option<DiscordIpcClient>>,
    enabled: AtomicBool,
}

#[tauri::command]
pub fn v2_discord_rpc_set_enabled(
    state: State<'_, DiscordRpcState>,
    enabled: bool,
) -> Result<(), String> {
    state.enabled.store(enabled, Ordering::SeqCst);
    if !enabled {
        let mut guard = state.client.lock().map_err(|e| e.to_string())?;
        if let Some(client) = guard.as_mut() {
            let _ = client.clear_activity();
            let _ = client.close();
        }
        *guard = None;
    }
    Ok(())
}

#[tauri::command]
pub fn v2_discord_rpc_clear(state: State<'_, DiscordRpcState>) -> Result<(), String> {
    let mut guard = state.client.lock().map_err(|e| e.to_string())?;
    if let Some(client) = guard.as_mut() {
        let _ = client.clear_activity();
        let _ = client.close();
    }
    *guard = None;
    Ok(())
}

#[tauri::command]
pub fn v2_discord_rpc_update(
    state: State<'_, DiscordRpcState>,
    title: String,
    artist: String,
    album: String,
    is_playing: bool,
    current_time: f64,
    duration: f64,
    cover_url: Option<String>,
) -> Result<(), String> {
    if !state.enabled.load(Ordering::SeqCst) {
        return Ok(());
    }

    let mut guard = state.client.lock().map_err(|e| e.to_string())?;

    if guard.is_none() {
        // Bridge the Flatpak sandbox to Discord's IPC socket if we're
        // running under Flatpak. No-op outside that context.
        prepare_flatpak_discord_socket_links();
        // discord-rich-presence 1.0+: DiscordIpcClient::new is infallible
        // (returns Self directly); failures show up at connect() time.
        let mut client = DiscordIpcClient::new(DISCORD_APP_ID);
        if client.connect().is_ok() {
            *guard = Some(client);
        }
    }

    let Some(client) = guard.as_mut() else {
        return Ok(());
    };

    let state_str = format!("by {}", artist);
    let small_text = if is_playing {
        "Playing".to_string()
    } else {
        let mins = (current_time / 60.0).floor() as u32;
        let secs = (current_time % 60.0).floor() as u32;
        format!("Paused at {:02}:{:02}", mins, secs)
    };
    let large_image = cover_url.unwrap_or_else(|| "cover".to_string());

    let mut act = activity::Activity::new()
        .details(&title)
        .state(&state_str)
        .activity_type(activity::ActivityType::Listening)
        .assets(
            activity::Assets::new()
                .large_image(&large_image)
                .small_image("icon")
                .large_text(&album)
                .small_text(&small_text),
        )
        .buttons(vec![activity::Button::new("Get QBZ", QBZ_HOMEPAGE)]);

    if is_playing {
        if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let start_time = now.as_secs() as i64 - current_time as i64;
            let mut ts = activity::Timestamps::new().start(start_time);
            if duration > 0.0 {
                ts = ts.end(start_time + duration as i64);
            }
            act = act.timestamps(ts);
        }
    }

    if client.set_activity(act).is_err() {
        *guard = None;
    }

    Ok(())
}
