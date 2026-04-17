use std::sync::Arc;
use axum::extract::{Path, Query};
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct PaginationQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 { 20 }

pub async fn get_artist_page(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.get_artist_page(id, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn get_similar_artists(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
    Query(q): Query<PaginationQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.get_similar_artists(id, q.limit, q.offset)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

/// GET /api/labels/{id}
///
/// Backwards-compatible shape — composes /label/page + /label/getAlbums
/// so existing HTTP clients still see `{ id, name, description, image,
/// albums: { items, total, ... } }`. The legacy `/label/get` one-shot
/// is being retired in the v9.7.0.3 Qobuz API.
pub async fn get_label(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
    Query(q): Query<PaginationQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let (page, albums) = tokio::try_join!(
        async {
            daemon.core.get_label_page(id)
                .await
                .map_err(|e| e.to_string())
        },
        async {
            daemon.core.get_label_albums(id, q.limit, q.offset, None, None, None, None, None)
                .await
                .map_err(|e| e.to_string())
        }
    )?;

    let composed = serde_json::json!({
        "id": page.id,
        "name": page.name,
        "description": page.description,
        "image": page.image,
        "albums": {
            "items": albums.items,
            "total": albums.total,
            "offset": albums.offset,
            "limit": albums.limit,
            "has_more": albums.has_more,
        },
    });
    Ok(Json(composed))
}

pub async fn get_label_page(
    daemon: Arc<DaemonCore>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.get_label_page(id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn get_label_explore(
    daemon: Arc<DaemonCore>,
    Query(q): Query<PaginationQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.get_label_explore(q.limit, q.offset)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}

pub async fn get_playlist_tags(
    daemon: Arc<DaemonCore>,
) -> Result<Json<serde_json::Value>, String> {
    let result = daemon.core.get_playlist_tags()
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(serde_json::to_value(result).unwrap_or_default()))
}
