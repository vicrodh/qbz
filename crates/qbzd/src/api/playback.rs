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
