//! Playlist import commands

use tauri::State;

use crate::playlist_import::{import_public_playlist, preview_public_playlist, ImportPlaylist, ImportSummary};
use crate::AppState;

#[tauri::command]
pub async fn playlist_import_preview(url: String) -> Result<ImportPlaylist, String> {
    log::info!("Command: playlist_import_preview {}", url);

    preview_public_playlist(&url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn playlist_import_execute(
    url: String,
    name_override: Option<String>,
    is_public: Option<bool>,
    state: State<'_, AppState>,
) -> Result<ImportSummary, String> {
    log::info!("Command: playlist_import_execute {}", url);

    let client = state.client.lock().await;
    import_public_playlist(
        &url,
        &client,
        name_override.as_deref(),
        is_public.unwrap_or(false),
    )
    .await
    .map_err(|e| e.to_string())
}
