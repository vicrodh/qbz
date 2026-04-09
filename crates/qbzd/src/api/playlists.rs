use std::sync::Arc;
use axum::extract::Path;
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct CreatePlaylistRequest {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub is_public: bool,
}

#[derive(Deserialize)]
pub struct UpdatePlaylistRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_public: Option<bool>,
}

#[derive(Deserialize)]
pub struct TrackIdsRequest {
    pub track_ids: Vec<u64>,
}

pub async fn get_playlists(daemon: Arc<DaemonCore>) -> Result<Json<serde_json::Value>, String> {
    let client = daemon.core.client();
    let guard = client.read().await;
    let client = guard.as_ref().ok_or("Not initialized")?;
    let playlists = client.get_user_playlists()
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(playlists).unwrap_or_default()))
}

pub async fn get_playlist(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, String> {
    let client = daemon.core.client();
    let guard = client.read().await;
    let client = guard.as_ref().ok_or("Not initialized")?;
    let playlist = client.get_playlist(id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(playlist).unwrap_or_default()))
}

pub async fn create_playlist(
    daemon: Arc<DaemonCore>,
    Json(req): Json<CreatePlaylistRequest>,
) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.create_playlist(&req.name, req.description.as_deref(), req.is_public)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn update_playlist(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
    Json(req): Json<UpdatePlaylistRequest>,
) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.update_playlist(id, req.name.as_deref(), req.description.as_deref(), req.is_public)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn delete_playlist(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
) -> Result<&'static str, String> {
    daemon.core.delete_playlist(id)
        .await
        .map_err(|e| e.to_string())?;
    Ok("ok")
}

pub async fn add_tracks(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
    Json(req): Json<TrackIdsRequest>,
) -> Result<&'static str, String> {
    let client = daemon.core.client();
    let guard = client.read().await;
    let client = guard.as_ref().ok_or("Not initialized")?;
    client.add_tracks_to_playlist(id, &req.track_ids)
        .await
        .map_err(|e| e.to_string())?;
    Ok("ok")
}

pub async fn remove_tracks(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
    Json(req): Json<TrackIdsRequest>,
) -> Result<&'static str, String> {
    let client = daemon.core.client();
    let guard = client.read().await;
    let client = guard.as_ref().ok_or("Not initialized")?;
    client.remove_tracks_from_playlist(id, &req.track_ids)
        .await
        .map_err(|e| e.to_string())?;
    Ok("ok")
}

pub async fn search_playlists(
    daemon: Arc<DaemonCore>,
    axum::extract::Query(q): axum::extract::Query<super::search::SearchParams>,
) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.search_playlists(&q.q, q.limit, q.offset)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
