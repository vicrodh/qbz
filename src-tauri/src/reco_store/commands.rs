//! Tauri commands for recommendation store

use tauri::State;

use crate::reco_store::{HomeSeeds, RecoEventInput, RecoState};

#[tauri::command]
pub async fn reco_log_event(
    event: RecoEventInput,
    state: State<'_, RecoState>,
) -> Result<(), String> {
    log::info!(
        "Command: reco_log_event type={} item={}",
        event.event_type.as_str(),
        event.item_type.as_str()
    );

    let db = state.db.lock().await;
    db.insert_event(&event)
}

#[tauri::command]
pub async fn reco_get_home(
    limit_recent_albums: Option<u32>,
    limit_continue_tracks: Option<u32>,
    limit_top_artists: Option<u32>,
    limit_favorites: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<HomeSeeds, String> {
    let limit_recent_albums = limit_recent_albums.unwrap_or(12);
    let limit_continue_tracks = limit_continue_tracks.unwrap_or(10);
    let limit_top_artists = limit_top_artists.unwrap_or(10);
    let limit_favorites = limit_favorites.unwrap_or(12);

    let db = state.db.lock().await;

    let recently_played_album_ids = db.get_recent_album_ids(limit_recent_albums)?;
    let continue_listening_track_ids = db.get_recent_track_ids(limit_continue_tracks)?;
    let top_artist_ids = db.get_top_artist_ids(limit_top_artists)?;
    let favorite_album_ids = db.get_favorite_album_ids(limit_favorites)?;
    let favorite_track_ids = db.get_favorite_track_ids(limit_favorites)?;

    Ok(HomeSeeds {
        recently_played_album_ids,
        continue_listening_track_ids,
        top_artist_ids,
        favorite_album_ids,
        favorite_track_ids,
    })
}
