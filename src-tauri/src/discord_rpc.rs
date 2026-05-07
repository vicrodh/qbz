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
        if let Ok(mut client) = DiscordIpcClient::new(DISCORD_APP_ID) {
            if client.connect().is_ok() {
                *guard = Some(client);
            }
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
