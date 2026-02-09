//! Orchestrates playlist import

use tauri::{AppHandle, Emitter};

use crate::api::QobuzClient;
use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::match_qobuz::match_tracks;
use crate::playlist_import::models::{ImportPlaylist, ImportProgress, ImportSummary};
use crate::playlist_import::providers::{detect_provider, fetch_playlist};

const ADD_CHUNK_SIZE: usize = 50;

pub async fn preview_public_playlist(
    url: &str,
) -> Result<ImportPlaylist, PlaylistImportError> {
    let provider = detect_provider(url)?;
    fetch_playlist(provider).await
}

pub async fn import_public_playlist(
    url: &str,
    client: &QobuzClient,
    name_override: Option<&str>,
    is_public: bool,
    app: &AppHandle,
) -> Result<ImportSummary, PlaylistImportError> {
    let playlist = preview_public_playlist(url).await?;

    // Phase: matching
    let _ = app.emit("import:phase", serde_json::json!({ "phase": "matching" }));
    let matches = match_tracks(client, &playlist.tracks, app).await?;

    let mut matched_track_ids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in &matches {
        if let Some(id) = entry.qobuz_track_id {
            if seen.insert(id) {
                matched_track_ids.push(id);
            }
        }
    }

    let matched_count = matched_track_ids.len() as u32;
    let total_tracks = playlist.tracks.len() as u32;
    let skipped_tracks = total_tracks.saturating_sub(matched_count);

    let mut qobuz_playlist_id = None;

    if !matched_track_ids.is_empty() {
        // Phase: creating
        let _ = app.emit("import:phase", serde_json::json!({ "phase": "creating" }));

        let name = name_override.unwrap_or(&playlist.name);
        let description = playlist
            .description
            .clone()
            .or_else(|| Some(format!("Imported from {}", playlist.provider.as_str())));

        let created = client
            .create_playlist(name, description.as_deref(), is_public)
            .await
            .map_err(|e| PlaylistImportError::Qobuz(e.to_string()))?;

        qobuz_playlist_id = Some(created.id);

        // Phase: adding
        let _ = app.emit("import:phase", serde_json::json!({ "phase": "adding" }));

        let chunks: Vec<&[u64]> = matched_track_ids.chunks(ADD_CHUNK_SIZE).collect();
        let total_chunks = chunks.len() as u32;

        for (i, chunk) in chunks.iter().enumerate() {
            client
                .add_tracks_to_playlist(created.id, chunk)
                .await
                .map_err(|e| PlaylistImportError::Qobuz(e.to_string()))?;

            let _ = app.emit(
                "import:progress",
                ImportProgress {
                    phase: "adding".to_string(),
                    current: (i as u32) + 1,
                    total: total_chunks,
                    matched_so_far: matched_count,
                    current_track: None,
                },
            );
        }
    }

    Ok(ImportSummary {
        provider: playlist.provider,
        playlist_name: playlist.name,
        total_tracks,
        matched_tracks: matched_count,
        skipped_tracks,
        qobuz_playlist_id,
        matches,
    })
}
