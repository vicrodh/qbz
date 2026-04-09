use std::sync::Arc;
use axum::Json;
use qbz_models::{QueueTrack, RepeatMode};
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct SetQueueRequest {
    pub tracks: Vec<QueueTrack>,
    pub start_index: Option<usize>,
}

#[derive(Deserialize)]
pub struct AddTracksRequest {
    pub tracks: Vec<QueueTrack>,
}

#[derive(Deserialize)]
pub struct PlayIndexRequest {
    pub index: usize,
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    pub index: usize,
}

#[derive(Deserialize)]
pub struct MoveRequest {
    pub from: usize,
    pub to: usize,
}

#[derive(Deserialize)]
pub struct ShuffleRequest {
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct RepeatRequest {
    pub mode: String,
}

pub async fn get_queue(daemon: Arc<DaemonCore>) -> Json<serde_json::Value> {
    let state = daemon.core.get_queue_state().await;
    Json(serde_json::to_value(state).unwrap_or_default())
}

pub async fn set_queue(
    daemon: Arc<DaemonCore>,
    Json(req): Json<SetQueueRequest>,
) -> &'static str {
    daemon.core.set_queue(req.tracks, req.start_index).await;
    "ok"
}

pub async fn add(
    daemon: Arc<DaemonCore>,
    Json(req): Json<AddTracksRequest>,
) -> &'static str {
    daemon.core.add_tracks(req.tracks).await;
    "ok"
}

pub async fn add_next(
    daemon: Arc<DaemonCore>,
    Json(req): Json<AddTracksRequest>,
) -> &'static str {
    for track in req.tracks {
        daemon.core.add_track_next(track).await;
    }
    "ok"
}

pub async fn play_index(
    daemon: Arc<DaemonCore>,
    Json(req): Json<PlayIndexRequest>,
) -> Json<serde_json::Value> {
    let track = daemon.core.play_index(req.index).await;
    Json(serde_json::json!({ "track": track }))
}

pub async fn remove(
    daemon: Arc<DaemonCore>,
    Json(req): Json<RemoveRequest>,
) -> Json<serde_json::Value> {
    let removed = daemon.core.remove_track(req.index).await;
    Json(serde_json::json!({ "removed": removed }))
}

pub async fn move_track(
    daemon: Arc<DaemonCore>,
    Json(req): Json<MoveRequest>,
) -> Json<serde_json::Value> {
    let ok = daemon.core.move_track(req.from, req.to).await;
    Json(serde_json::json!({ "success": ok }))
}

pub async fn clear(daemon: Arc<DaemonCore>) -> &'static str {
    daemon.core.clear_queue().await;
    "ok"
}

pub async fn shuffle(
    daemon: Arc<DaemonCore>,
    Json(req): Json<ShuffleRequest>,
) -> Json<serde_json::Value> {
    daemon.core.set_shuffle(req.enabled).await;
    Json(serde_json::json!({ "shuffle": req.enabled }))
}

pub async fn repeat(
    daemon: Arc<DaemonCore>,
    Json(req): Json<RepeatRequest>,
) -> Json<serde_json::Value> {
    let mode = match req.mode.as_str() {
        "one" => RepeatMode::One,
        "all" => RepeatMode::All,
        _ => RepeatMode::Off,
    };
    daemon.core.set_repeat_mode(mode.clone()).await;
    Json(serde_json::json!({ "repeat": format!("{:?}", mode) }))
}
