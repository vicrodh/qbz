use std::sync::Arc;
use axum::extract::Query;
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct FavoritesQuery {
    pub r#type: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 { 50 }

#[derive(Deserialize)]
pub struct FavoriteAction {
    pub r#type: String,
    pub item_id: String,
}

pub async fn get_favorites(
    daemon: Arc<DaemonCore>,
    Query(q): Query<FavoritesQuery>,
) -> Result<Json<serde_json::Value>, String> {
    let client = daemon.core.client();
    let guard = client.read().await;
    let client = guard.as_ref().ok_or("Not initialized")?;
    let result = client.get_favorites(&q.r#type, q.limit, q.offset)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Json(result))
}

pub async fn add_favorite(
    daemon: Arc<DaemonCore>,
    Json(req): Json<FavoriteAction>,
) -> Result<&'static str, String> {
    let client = daemon.core.client();
    let guard = client.read().await;
    let client = guard.as_ref().ok_or("Not initialized")?;
    client.add_favorite(&req.r#type, &req.item_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok("ok")
}

pub async fn remove_favorite(
    daemon: Arc<DaemonCore>,
    Json(req): Json<FavoriteAction>,
) -> Result<&'static str, String> {
    let client = daemon.core.client();
    let guard = client.read().await;
    let client = guard.as_ref().ok_or("Not initialized")?;
    client.remove_favorite(&req.r#type, &req.item_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok("ok")
}
