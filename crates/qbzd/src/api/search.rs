use std::sync::Arc;
use axum::extract::Query;
use axum::Json;
use serde::Deserialize;

use crate::daemon::DaemonCore;

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: String,
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_type() -> String { "all".to_string() }
fn default_limit() -> u32 { 30 }

pub async fn search(
    daemon: Arc<DaemonCore>,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, String> {
    match params.r#type.as_str() {
        "all" => {
            let result = daemon.core.catalog_search(&params.q, params.limit, params.offset)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Json(result))
        }
        "albums" => {
            let result = daemon.core.search_albums(&params.q, params.limit, params.offset, None)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Json(serde_json::to_value(result).unwrap_or_default()))
        }
        "tracks" => {
            let result = daemon.core.search_tracks(&params.q, params.limit, params.offset, None)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Json(serde_json::to_value(result).unwrap_or_default()))
        }
        "artists" => {
            let result = daemon.core.search_artists(&params.q, params.limit, params.offset, None)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Json(serde_json::to_value(result).unwrap_or_default()))
        }
        _ => Err(format!("Unknown search type: {}", params.r#type)),
    }
}
