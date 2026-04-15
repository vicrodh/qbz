use std::sync::Arc;
use axum::extract::{Path, Query};
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct BatchParams {
    pub ids: String,
}

pub async fn get_album(
    daemon: Arc<DaemonCore>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, String> {
    let album = daemon.core.get_album(&id).await.map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(album).unwrap_or_default()))
}

pub async fn get_artist(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, String> {
    let artist = daemon.core.get_artist(id).await.map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(artist).unwrap_or_default()))
}

pub async fn get_track(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, String> {
    let track = daemon.core.get_track(id).await.map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(track).unwrap_or_default()))
}

pub async fn get_tracks_batch(
    daemon: Arc<DaemonCore>,
    Query(params): Query<BatchParams>,
) -> Result<Json<serde_json::Value>, String> {
    let ids: Vec<u64> = params.ids.split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if ids.is_empty() {
        return Ok(Json(serde_json::json!({"tracks": []})));
    }

    let tracks = daemon.core.get_tracks_batch(&ids).await.map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(tracks).unwrap_or_default()))
}
