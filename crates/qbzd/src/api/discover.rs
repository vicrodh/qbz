use std::sync::Arc;
use axum::extract::Query;
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct DiscoverQuery {
    pub genre: Option<String>,
}

#[derive(Deserialize)]
pub struct DiscoverAlbumsQuery {
    pub r#type: Option<String>,
    pub genre: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

#[derive(Deserialize)]
pub struct DiscoverPlaylistsQuery {
    pub tag: Option<String>,
    pub genre: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

#[derive(Deserialize)]
pub struct FeaturedQuery {
    pub r#type: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    pub genre: Option<String>,
}

fn default_limit() -> u32 { 20 }

fn parse_genre_ids(genre: &Option<String>) -> Option<Vec<u64>> {
    genre.as_ref().map(|g| {
        g.split(',').filter_map(|s| s.trim().parse().ok()).collect()
    })
}

pub async fn get_discover_index(
    daemon: Arc<DaemonCore>,
    Query(q): Query<DiscoverQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let genres = parse_genre_ids(&q.genre);
    let result = daemon.core.get_discover_index(genres)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn get_discover_playlists(
    daemon: Arc<DaemonCore>,
    Query(q): Query<DiscoverPlaylistsQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let genres = parse_genre_ids(&q.genre);
    let result = daemon.core.get_discover_playlists(
        q.tag,
        genres,
        Some(q.limit),
        Some(q.offset),
    ).await.map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn get_featured(
    daemon: Arc<DaemonCore>,
    Query(q): Query<FeaturedQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let genre_id = q.genre.as_ref().and_then(|g| g.parse().ok());
    let featured_type = q.r#type.as_deref().unwrap_or("new-releases");
    let result = daemon.core.get_featured_albums(featured_type, q.limit, q.offset, genre_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn get_genres(daemon: Arc<DaemonCore>) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.get_genres(None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
