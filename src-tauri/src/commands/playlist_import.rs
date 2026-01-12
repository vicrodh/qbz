//! Playlist import commands

use tauri::State;

use crate::api_keys::ApiKeysState;
use crate::playlist_import::{
    import_public_playlist, preview_public_playlist, ImportPlaylist, ImportSummary,
    ProviderCredentials,
};
use crate::AppState;

#[tauri::command]
pub async fn playlist_import_preview(
    url: String,
    api_keys: State<'_, ApiKeysState>,
) -> Result<ImportPlaylist, String> {
    log::info!("Command: playlist_import_preview {}", url);

    // Get user-provided credentials from state
    let keys = api_keys.lock().await;
    let spotify_creds = if keys.spotify.is_set() {
        Some(ProviderCredentials {
            client_id: keys.spotify.client_id.clone(),
            client_secret: keys.spotify.client_secret.clone(),
        })
    } else {
        None
    };
    let tidal_creds = if keys.tidal.is_set() {
        Some(ProviderCredentials {
            client_id: keys.tidal.client_id.clone(),
            client_secret: keys.tidal.client_secret.clone(),
        })
    } else {
        None
    };
    drop(keys);

    preview_public_playlist(&url, spotify_creds, tidal_creds)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn playlist_import_execute(
    url: String,
    name_override: Option<String>,
    is_public: Option<bool>,
    state: State<'_, AppState>,
    api_keys: State<'_, ApiKeysState>,
) -> Result<ImportSummary, String> {
    log::info!("Command: playlist_import_execute {}", url);

    // Get user-provided credentials from state
    let keys = api_keys.lock().await;
    let spotify_creds = if keys.spotify.is_set() {
        Some(ProviderCredentials {
            client_id: keys.spotify.client_id.clone(),
            client_secret: keys.spotify.client_secret.clone(),
        })
    } else {
        None
    };
    let tidal_creds = if keys.tidal.is_set() {
        Some(ProviderCredentials {
            client_id: keys.tidal.client_id.clone(),
            client_secret: keys.tidal.client_secret.clone(),
        })
    } else {
        None
    };
    drop(keys);

    let client = state.client.lock().await;
    import_public_playlist(
        &url,
        &client,
        name_override.as_deref(),
        is_public.unwrap_or(false),
        spotify_creds,
        tidal_creds,
    )
    .await
    .map_err(|e| e.to_string())
}
