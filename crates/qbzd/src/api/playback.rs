use std::sync::Arc;
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct SeekRequest {
    pub position_secs: u64,
}

#[derive(Deserialize)]
pub struct VolumeRequest {
    pub volume: f32,
}

pub async fn get_playback(daemon: Arc<DaemonCore>) -> Json<serde_json::Value> {
    let player = daemon.core.player();
    let state = &player.state;
    Json(serde_json::json!({
        "state": if state.is_playing() { "Playing" } else if state.current_track_id() != 0 { "Paused" } else { "Stopped" },
        "track_id": state.current_track_id(),
        "position_secs": state.current_position(),
        "duration_secs": state.duration(),
        "volume": state.volume(),
        "sample_rate": state.get_sample_rate(),
        "bit_depth": state.get_bit_depth(),
    }))
}

#[derive(Deserialize)]
pub struct PlayTrackRequest {
    pub track_id: u64,
    pub quality: Option<String>,
}

/// Play a specific track by ID. Downloads audio from Qobuz and feeds to player.
pub async fn play_track(
    daemon: Arc<DaemonCore>,
    Json(req): Json<PlayTrackRequest>,
) -> Result<Json<serde_json::Value>, String> {
    let quality = match req.quality.as_deref() {
        Some("Hi-Res+") | Some("UltraHiRes") => qbz_models::Quality::UltraHiRes,
        Some("Hi-Res") | Some("HiRes") => qbz_models::Quality::HiRes,
        Some("Lossless") => qbz_models::Quality::Lossless,
        _ => qbz_models::Quality::HiRes, // Default to HiRes
    };

    log::info!("[qbzd/play] Playing track {} (quality: {:?})", req.track_id, quality);

    // Signal orchestrator to not auto-advance during download
    daemon.skip_auto_advance.store(true, std::sync::atomic::Ordering::Release);

    // Stop current playback
    let _ = daemon.core.stop();

    // Download audio
    let audio_data = crate::daemon::download_track(&daemon, req.track_id).await?;

    // Clear the skip flag
    daemon.skip_auto_advance.store(false, std::sync::atomic::Ordering::Release);

    // Feed to player
    daemon.core.player()
        .play_data(audio_data, req.track_id)
        .map_err(|e| format!("Player error: {}", e))?;

    Ok(Json(serde_json::json!({
        "playing": true,
        "track_id": req.track_id,
    })))
}

/// Play an album by ID. Sets the queue with all album tracks and starts playing.
#[derive(Deserialize)]
pub struct PlayAlbumRequest {
    pub album_id: String,
    pub start_index: Option<usize>,
    pub quality: Option<String>,
}

pub async fn play_album(
    daemon: Arc<DaemonCore>,
    Json(req): Json<PlayAlbumRequest>,
) -> Result<Json<serde_json::Value>, String> {
    // Fetch album with tracks
    let album = daemon.core.get_album(&req.album_id).await
        .map_err(|e| format!("Album fetch failed: {}", e))?;

    let album_tracks = album.tracks.as_ref()
        .map(|tc| &tc.items)
        .ok_or("Album has no tracks")?;

    let tracks: Vec<qbz_models::QueueTrack> = album_tracks.iter().map(|track| {
        qbz_models::QueueTrack {
            id: track.id,
            title: track.title.clone(),
            artist: track.performer.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
            album: album.title.clone(),
            duration_secs: track.duration as u64,
            artwork_url: album.image.large.clone(),
            hires: track.hires_streamable,
            bit_depth: track.maximum_bit_depth,
            sample_rate: track.maximum_sampling_rate,
            is_local: false,
            album_id: Some(album.id.clone()),
            artist_id: Some(album.artist.id),
            streamable: true,
            source: Some("qobuz".to_string()),
            parental_warning: track.parental_warning,
        }
    }).collect();

    if tracks.is_empty() {
        return Err("Album has no tracks".to_string());
    }

    let start = req.start_index.unwrap_or(0);
    let first_track_id = tracks[start].id;

    // Set queue
    daemon.core.set_queue(tracks, Some(start)).await;

    // Play the first track
    let quality = match req.quality.as_deref() {
        Some("Hi-Res+") | Some("UltraHiRes") => qbz_models::Quality::UltraHiRes,
        Some("Hi-Res") | Some("HiRes") => qbz_models::Quality::HiRes,
        _ => qbz_models::Quality::HiRes,
    };

    // Signal orchestrator to not auto-advance during download
    daemon.skip_auto_advance.store(true, std::sync::atomic::Ordering::Release);

    // Stop current playback
    let _ = daemon.core.stop();

    let audio_data = crate::daemon::download_track(&daemon, first_track_id).await?;
    daemon.skip_auto_advance.store(false, std::sync::atomic::Ordering::Release);
    daemon.core.player().play_data(audio_data, first_track_id)
        .map_err(|e| format!("Player error: {}", e))?;

    Ok(Json(serde_json::json!({
        "playing": true,
        "track_id": first_track_id,
        "album": album.title,
        "tracks_count": album_tracks.len(),
    })))
}

pub async fn play(daemon: Arc<DaemonCore>) -> Result<&'static str, String> {
    daemon.core.resume().map_err(|e| e.to_string())?;
    Ok("ok")
}

pub async fn pause(daemon: Arc<DaemonCore>) -> Result<&'static str, String> {
    daemon.core.pause().map_err(|e| e.to_string())?;
    Ok("ok")
}

pub async fn stop(daemon: Arc<DaemonCore>) -> Result<&'static str, String> {
    daemon.core.stop().map_err(|e| e.to_string())?;
    Ok("ok")
}

pub async fn next(daemon: Arc<DaemonCore>) -> Json<serde_json::Value> {
    let track = daemon.core.next_track().await;
    Json(serde_json::json!({
        "track": track,
    }))
}

pub async fn previous(daemon: Arc<DaemonCore>) -> Json<serde_json::Value> {
    let track = daemon.core.previous_track().await;
    Json(serde_json::json!({
        "track": track,
    }))
}

pub async fn seek(
    daemon: Arc<DaemonCore>,
    Json(req): Json<SeekRequest>,
) -> Result<&'static str, String> {
    daemon.core.seek(req.position_secs).map_err(|e| e.to_string())?;
    Ok("ok")
}

pub async fn volume(
    daemon: Arc<DaemonCore>,
    Json(req): Json<VolumeRequest>,
) -> Result<&'static str, String> {
    daemon.core.set_volume(req.volume).map_err(|e| e.to_string())?;
    Ok("ok")
}
