//! Orchestrates playlist import

use crate::api::QobuzClient;
use crate::playlist_import::errors::PlaylistImportError;
use crate::playlist_import::match_qobuz::match_tracks;
use crate::playlist_import::models::{ImportPlaylist, ImportSummary};
use crate::playlist_import::providers::{detect_provider, fetch_playlist, ProviderCredentials};

const ADD_CHUNK_SIZE: usize = 50;

pub async fn preview_public_playlist(
    url: &str,
    spotify_creds: Option<ProviderCredentials>,
    tidal_creds: Option<ProviderCredentials>,
) -> Result<ImportPlaylist, PlaylistImportError> {
    let provider = detect_provider(url)?;
    fetch_playlist(provider, spotify_creds, tidal_creds).await
}

pub async fn import_public_playlist(
    url: &str,
    client: &QobuzClient,
    name_override: Option<&str>,
    is_public: bool,
    spotify_creds: Option<ProviderCredentials>,
    tidal_creds: Option<ProviderCredentials>,
) -> Result<ImportSummary, PlaylistImportError> {
    let playlist = preview_public_playlist(url, spotify_creds, tidal_creds).await?;
    let matches = match_tracks(client, &playlist.tracks).await?;

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

        for chunk in matched_track_ids.chunks(ADD_CHUNK_SIZE) {
            client
                .add_tracks_to_playlist(created.id, chunk)
                .await
                .map_err(|e| PlaylistImportError::Qobuz(e.to_string()))?;
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
