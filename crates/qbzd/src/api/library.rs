use std::sync::Arc;
use axum::extract::{Path, Query};
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 { 50 }

pub async fn get_albums(daemon: Arc<DaemonCore>) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let db_path = session.data_dir.join("library.db");

    let db = qbz_library::LibraryDatabase::open(&db_path)
        .map_err(|e| format!("Library DB error: {}", e))?;
    let albums = db.get_albums(false)
        .map_err(|e| format!("Failed to get albums: {}", e))?;
    Ok(Json(serde_json::to_value(albums).unwrap_or_default()))
}

pub async fn get_artists(daemon: Arc<DaemonCore>) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let db_path = session.data_dir.join("library.db");

    let db = qbz_library::LibraryDatabase::open(&db_path)
        .map_err(|e| format!("Library DB error: {}", e))?;
    let artists = db.get_artists()
        .map_err(|e| format!("Failed to get artists: {}", e))?;
    Ok(Json(serde_json::to_value(artists).unwrap_or_default()))
}

pub async fn get_album_tracks(
    daemon: Arc<DaemonCore>,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let db_path = session.data_dir.join("library.db");

    let db = qbz_library::LibraryDatabase::open(&db_path)
        .map_err(|e| format!("Library DB error: {}", e))?;
    let tracks = db.get_album_tracks(&key)
        .map_err(|e| format!("Failed to get tracks: {}", e))?;
    Ok(Json(serde_json::to_value(tracks).unwrap_or_default()))
}

pub async fn search_library(
    daemon: Arc<DaemonCore>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let db_path = session.data_dir.join("library.db");

    let db = qbz_library::LibraryDatabase::open(&db_path)
        .map_err(|e| format!("Library DB error: {}", e))?;
    let tracks = db.search(&q.q, q.limit)
        .map_err(|e| format!("Search failed: {}", e))?;
    Ok(Json(serde_json::to_value(tracks).unwrap_or_default()))
}

pub async fn get_stats(daemon: Arc<DaemonCore>) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let db_path = session.data_dir.join("library.db");

    let db = qbz_library::LibraryDatabase::open(&db_path)
        .map_err(|e| format!("Library DB error: {}", e))?;
    let stats = db.get_stats(true)
        .map_err(|e| format!("Stats failed: {}", e))?;
    Ok(Json(serde_json::to_value(stats).unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct AddFolderRequest {
    pub path: String,
}

pub async fn get_folders(daemon: Arc<DaemonCore>) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let db_path = session.data_dir.join("library.db");

    let db = qbz_library::LibraryDatabase::open(&db_path)
        .map_err(|e| format!("Library DB error: {}", e))?;
    let folders = db.get_folders()
        .map_err(|e| format!("Failed to get folders: {}", e))?;
    Ok(Json(serde_json::to_value(folders).unwrap_or_default()))
}

pub async fn add_folder(
    daemon: Arc<DaemonCore>,
    Json(req): Json<AddFolderRequest>,
) -> Result<Json<serde_json::Value>, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let db_path = session.data_dir.join("library.db");

    let db = qbz_library::LibraryDatabase::open(&db_path)
        .map_err(|e| format!("Library DB error: {}", e))?;
    db.add_folder(&req.path)
        .map_err(|e| format!("Failed to add folder: {}", e))?;
    Ok(Json(serde_json::json!({"path": req.path, "status": "added"})))
}

#[derive(Deserialize)]
pub struct RemoveFolderRequest {
    pub path: String,
}

pub async fn remove_folder(
    daemon: Arc<DaemonCore>,
    Json(req): Json<RemoveFolderRequest>,
) -> Result<&'static str, String> {
    let user = daemon.user.read().await;
    let session = user.as_ref().ok_or("No active session")?;
    let db_path = session.data_dir.join("library.db");

    let db = qbz_library::LibraryDatabase::open(&db_path)
        .map_err(|e| format!("Library DB error: {}", e))?;
    db.remove_folder(&req.path)
        .map_err(|e| format!("Failed to remove folder: {}", e))?;
    Ok("ok")
}
