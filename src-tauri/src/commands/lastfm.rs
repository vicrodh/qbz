//! Last.fm integration commands

use tauri::State;

use crate::lastfm::LastFmSession;
use crate::AppState;

/// Check if Last.fm has API credentials configured
#[tauri::command]
pub async fn lastfm_has_credentials(state: State<'_, AppState>) -> Result<bool, String> {
    let client = state.lastfm.lock().await;
    Ok(client.has_credentials())
}

/// Set Last.fm API credentials
#[tauri::command]
pub async fn lastfm_set_credentials(
    api_key: String,
    api_secret: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: lastfm_set_credentials");
    let mut client = state.lastfm.lock().await;
    client.set_credentials(api_key, api_secret);
    Ok(())
}

/// Check if Last.fm is authenticated
#[tauri::command]
pub async fn lastfm_is_authenticated(state: State<'_, AppState>) -> Result<bool, String> {
    let client = state.lastfm.lock().await;
    Ok(client.is_authenticated())
}

/// Get Last.fm authentication token and URL
#[tauri::command]
pub async fn lastfm_get_auth_url(state: State<'_, AppState>) -> Result<(String, String), String> {
    log::info!("Command: lastfm_get_auth_url");
    let client = state.lastfm.lock().await;

    if !client.has_credentials() {
        return Err("Last.fm API credentials not configured. Please set API key and secret in settings.".to_string());
    }

    let token = client.get_token().await?;
    let url = client.get_auth_url(&token);
    Ok((token, url))
}

/// Complete Last.fm authentication with token
#[tauri::command]
pub async fn lastfm_authenticate(
    token: String,
    state: State<'_, AppState>,
) -> Result<LastFmSession, String> {
    log::info!("Command: lastfm_authenticate");
    let mut client = state.lastfm.lock().await;
    client.get_session(&token).await
}

/// Set Last.fm session key (for restoring saved session)
#[tauri::command]
pub async fn lastfm_set_session(
    session_key: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: lastfm_set_session");
    let mut client = state.lastfm.lock().await;
    client.set_session_key(session_key);
    Ok(())
}

/// Disconnect from Last.fm
#[tauri::command]
pub async fn lastfm_disconnect(state: State<'_, AppState>) -> Result<(), String> {
    log::info!("Command: lastfm_disconnect");
    let mut client = state.lastfm.lock().await;
    // Reset to default (clears session key)
    *client = crate::lastfm::LastFmClient::default();
    Ok(())
}

/// Scrobble a track to Last.fm
#[tauri::command]
pub async fn lastfm_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: u64,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: lastfm_scrobble - {} - {}", artist, track);
    let client = state.lastfm.lock().await;
    client
        .scrobble(&artist, &track, album.as_deref(), timestamp)
        .await
}

/// Update "now playing" on Last.fm
#[tauri::command]
pub async fn lastfm_now_playing(
    artist: String,
    track: String,
    album: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("Command: lastfm_now_playing - {} - {}", artist, track);
    let client = state.lastfm.lock().await;
    client
        .update_now_playing(&artist, &track, album.as_deref())
        .await
}
