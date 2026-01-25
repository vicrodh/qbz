//! Track credits and album credits Tauri commands

use serde::Serialize;
use tauri::State;

use crate::api::models::Album;
use crate::api::performers::{parse_performers, Performer};
use crate::AppState;

/// Track credits with parsed performers
#[derive(Debug, Clone, Serialize)]
pub struct TrackCredits {
    pub id: u64,
    pub number: u32,
    pub title: String,
    pub artist: String,
    pub duration: String,
    pub duration_seconds: u32,
    pub performers: Vec<Performer>,
    pub copyright: Option<String>,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
}

/// Album credits response with all tracks
#[derive(Debug, Clone, Serialize)]
pub struct AlbumCredits {
    pub album: AlbumInfo,
    pub tracks: Vec<TrackCredits>,
}

/// Album metadata for credits modal
#[derive(Debug, Clone, Serialize)]
pub struct AlbumInfo {
    pub id: String,
    pub artwork: String,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<u64>,
    pub year: String,
    pub release_date: Option<String>,
    pub label: String,
    pub genre: String,
    pub quality: String,
    pub track_count: u32,
    pub duration: String,
    pub bit_depth: Option<u32>,
    pub sampling_rate: Option<f64>,
    /// Editorial description/review of the album
    pub description: Option<String>,
}

/// Format duration in seconds to "Xm Ys" or "Xh Ym" format
fn format_duration(seconds: u32) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m {}s", minutes, secs)
    }
}

/// Format track duration to "M:SS" format
fn format_track_duration(seconds: u32) -> String {
    let minutes = seconds / 60;
    let secs = seconds % 60;
    format!("{}:{:02}", minutes, secs)
}

/// Build quality string from bit depth and sample rate
fn format_quality(bit_depth: Option<u32>, sampling_rate: Option<f64>) -> String {
    match (bit_depth, sampling_rate) {
        (Some(bd), Some(sr)) => format!("{}-bit / {}kHz", bd, sr),
        (Some(bd), None) => format!("{}-bit", bd),
        (None, Some(sr)) => format!("{}kHz", sr),
        (None, None) => "Lossless".to_string(),
    }
}

/// Convert API Album to AlbumInfo
fn album_to_info(album: &Album) -> AlbumInfo {
    let year = album
        .release_date_original
        .as_ref()
        .and_then(|d| d.split('-').next())
        .unwrap_or("")
        .to_string();

    let total_duration = album.duration.unwrap_or(0);

    AlbumInfo {
        id: album.id.clone(),
        artwork: album.image.large.clone().unwrap_or_default(),
        title: album.title.clone(),
        artist: album.artist.name.clone(),
        artist_id: if album.artist.id > 0 { Some(album.artist.id) } else { None },
        year,
        release_date: album.release_date_original.clone(),
        label: album.label.as_ref().map(|l| l.name.clone()).unwrap_or_default(),
        genre: album.genre.as_ref().map(|g| g.name.clone()).unwrap_or_default(),
        quality: format_quality(album.maximum_bit_depth, album.maximum_sampling_rate),
        track_count: album.tracks_count.unwrap_or(0),
        duration: format_duration(total_duration),
        bit_depth: album.maximum_bit_depth,
        sampling_rate: album.maximum_sampling_rate,
        description: album.description.clone(),
    }
}

/// Get album credits with all tracks and parsed performers
#[tauri::command]
pub async fn get_album_credits(
    album_id: String,
    state: State<'_, AppState>,
) -> Result<AlbumCredits, String> {
    log::info!("Command: get_album_credits {}", album_id);

    let client = state.client.lock().await;

    // Fetch the album with tracks
    let album = client
        .get_album(&album_id)
        .await
        .map_err(|e| format!("Failed to get album: {}", e))?;

    // Convert album to info
    let album_info = album_to_info(&album);

    // Process tracks with performers
    let tracks: Vec<TrackCredits> = album
        .tracks
        .as_ref()
        .map(|tc| {
            tc.items
                .iter()
                .map(|track| {
                    let performers = track
                        .performers
                        .as_ref()
                        .map(|p| parse_performers(p))
                        .unwrap_or_default();

                    TrackCredits {
                        id: track.id,
                        number: track.track_number,
                        title: track.title.clone(),
                        artist: track
                            .performer
                            .as_ref()
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| album.artist.name.clone()),
                        duration: format_track_duration(track.duration),
                        duration_seconds: track.duration,
                        performers,
                        copyright: track.copyright.clone(),
                        album_id: Some(album.id.clone()),
                        artist_id: track.performer.as_ref().and_then(|p| {
                            if p.id > 0 { Some(p.id) } else { None }
                        }),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(AlbumCredits {
        album: album_info,
        tracks,
    })
}
