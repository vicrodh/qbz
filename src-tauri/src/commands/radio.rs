//! Radio Engine Tauri commands
//!
//! Creates and manages radio sessions for infinite playback discovery

use tauri::State;
use tokio::task;

use crate::api::{QobuzClient, Track};
use crate::playback_context::{ContentSource, ContextType, PlaybackContext};
use crate::queue::QueueTrack;
use crate::radio_engine::{BuildRadioOptions, RadioEngine, RadioPoolBuilder};
use crate::AppState;

/// Create an artist radio session
///
/// This creates a new radio session based on an artist and populates the queue
/// with initial tracks from the radio engine.
#[tauri::command]
pub async fn create_artist_radio(
    artist_id: u64,
    artist_name: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    log::info!("[Radio] Creating artist radio for: {} (ID: {})", artist_name, artist_id);

    // Clone client for use in builder
    let client = state.client.lock().await.clone();

    // Build radio pool in async context (RadioPoolBuilder needs client for API calls)
    let session_id = task::spawn_blocking(move || -> Result<String, String> {
        let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
        let builder = RadioPoolBuilder::new(&radio_db, &client, BuildRadioOptions::default());

        // This is async but we're in spawn_blocking, so we need to use tokio runtime
        let rt = tokio::runtime::Handle::current();
        let session = rt.block_on(builder.create_artist_radio(artist_id))?;
        Ok(session.id)
    })
    .await
    .map_err(|e| format!("Radio task failed: {}", e))??;

    log::info!("[Radio] Artist radio session created: {}", session_id);

    // Get client again for fetching tracks
    let client = state.client.lock().await;

    // Generate track IDs from radio engine
    let track_ids = task::spawn_blocking({
        let session_id = session_id.clone();
        move || -> Result<Vec<u64>, String> {
            let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
            let radio_engine = RadioEngine::new(radio_db);

            let mut tracks_with_distance = Vec::new();

            // Generate more tracks to increase chance of finding seed artist track
            for _ in 0..20 {
                match radio_engine.next_track(&session_id) {
                    Ok(radio_track) => {
                        tracks_with_distance.push((radio_track.track_id, radio_track.distance));
                    }
                    Err(e) => {
                        log::warn!("[Radio] Failed to get next radio track: {}", e);
                        break;
                    }
                }
            }

            // Find first track from seed artist (distance 0) and move to front
            if let Some(seed_idx) = tracks_with_distance.iter().position(|(_, dist)| *dist == 0) {
                if seed_idx != 0 {
                    tracks_with_distance.swap(0, seed_idx);
                    log::info!("[Radio] Moved seed artist track to position 0 (was at position {})", seed_idx);
                }
            } else {
                log::warn!("[Radio] No seed artist track found in first 20 tracks");
            }

            // Take first 15 tracks
            Ok(tracks_with_distance.into_iter().take(15).map(|(id, _)| id).collect())
        }
    })
    .await
    .map_err(|e| format!("Track generation task failed: {}", e))??;

    // Fetch full track details from Qobuz
    let mut tracks = Vec::new();
    for track_id in track_ids {
        match client.get_track(track_id).await {
            Ok(track) => {
                tracks.push(track);
            }
            Err(e) => {
                log::warn!("[Radio] Failed to fetch track {}: {}", track_id, e);
            }
        }
    }

    if tracks.is_empty() {
        return Err("Failed to generate any radio tracks".to_string());
    }

    log::info!("[Radio] Generated {} initial tracks", tracks.len());

    // Convert to QueueTrack format
    let queue_tracks: Vec<QueueTrack> = tracks.iter().map(track_to_queue_track).collect();

    // Set the queue
    state.queue.set_queue(queue_tracks, Some(0));

    // Set playback context to radio
    let track_ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
    let context = PlaybackContext::new(
        ContextType::Radio,
        session_id.clone(),
        artist_name,
        ContentSource::Qobuz,
        track_ids,
        0,
    );
    state.context.set_context(context);

    log::info!("[Radio] Artist radio ready: {}", session_id);

    Ok(session_id)
}

/// Create a track radio session
///
/// This creates a new radio session based on a specific track and populates the queue
/// with initial tracks from the radio engine.
#[tauri::command]
pub async fn create_track_radio(
    track_id: u64,
    track_name: String,
    artist_id: u64,
    state: State<'_, AppState>,
) -> Result<String, String> {
    log::info!(
        "[Radio] Creating track radio for: {} (Track ID: {}, Artist ID: {})",
        track_name,
        track_id,
        artist_id
    );

    // Clone client for use in builder
    let client = state.client.lock().await.clone();

    // Build radio pool in async context (RadioPoolBuilder needs client for API calls)
    let session_id = task::spawn_blocking(move || -> Result<String, String> {
        let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
        let builder = RadioPoolBuilder::new(&radio_db, &client, BuildRadioOptions::default());

        // This is async but we're in spawn_blocking, so we need to use tokio runtime
        let rt = tokio::runtime::Handle::current();
        let session = rt.block_on(builder.create_track_radio(track_id, artist_id))?;
        Ok(session.id)
    })
    .await
    .map_err(|e| format!("Radio task failed: {}", e))??;

    log::info!("[Radio] Track radio session created: {}", session_id);

    // Get client again for fetching tracks
    let client = state.client.lock().await;

    // Generate track IDs from radio engine
    let track_ids = task::spawn_blocking({
        let session_id = session_id.clone();
        let seed_track_id = track_id;
        move || -> Result<Vec<u64>, String> {
            let radio_db = crate::radio_engine::db::RadioDb::open_default()?;
            let radio_engine = RadioEngine::new(radio_db);

            let mut tracks_with_source = Vec::new();

            // Generate more tracks to find the seed track
            for _ in 0..20 {
                match radio_engine.next_track(&session_id) {
                    Ok(radio_track) => {
                        tracks_with_source.push((radio_track.track_id, radio_track.source.clone()));
                    }
                    Err(e) => {
                        log::warn!("[Radio] Failed to get next radio track: {}", e);
                        break;
                    }
                }
            }

            // Find the seed track itself and move to front
            if let Some(seed_idx) = tracks_with_source.iter().position(|(id, source)| *id == seed_track_id && source == "seed_track") {
                if seed_idx != 0 {
                    tracks_with_source.swap(0, seed_idx);
                    log::info!("[Radio] Moved seed track to position 0 (was at position {})", seed_idx);
                }
            } else {
                log::warn!("[Radio] Seed track {} not found in first 20 tracks", seed_track_id);
            }

            // Take first 15 tracks
            Ok(tracks_with_source.into_iter().take(15).map(|(id, _)| id).collect())
        }
    })
    .await
    .map_err(|e| format!("Track generation task failed: {}", e))??;

    // Fetch full track details from Qobuz
    let mut tracks = Vec::new();
    for track_id in track_ids {
        match client.get_track(track_id).await {
            Ok(track) => {
                tracks.push(track);
            }
            Err(e) => {
                log::warn!("[Radio] Failed to fetch track {}: {}", track_id, e);
            }
        }
    }

    if tracks.is_empty() {
        return Err("Failed to generate any radio tracks".to_string());
    }

    log::info!("[Radio] Generated {} initial tracks", tracks.len());

    // Convert to QueueTrack format
    let queue_tracks: Vec<QueueTrack> = tracks.iter().map(track_to_queue_track).collect();

    // Set the queue
    state.queue.set_queue(queue_tracks, Some(0));

    // Set playback context to radio
    let track_ids: Vec<u64> = tracks.iter().map(|t| t.id).collect();
    let context = PlaybackContext::new(
        ContextType::Radio,
        session_id.clone(),
        track_name,
        ContentSource::Qobuz,
        track_ids,
        0,
    );
    state.context.set_context(context);

    log::info!("[Radio] Track radio ready: {}", session_id);

    Ok(session_id)
}

/// Convert API Track to QueueTrack
fn track_to_queue_track(track: &Track) -> QueueTrack {
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|a| a.image.large.clone());

    let artist = track
        .performer
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());

    let album = track
        .album
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_else(|| "Unknown Album".to_string());

    let album_id = track.album.as_ref().map(|a| a.id.clone());
    let artist_id = track.performer.as_ref().map(|p| p.id);

    QueueTrack {
        id: track.id,
        title: track.title.clone(),
        artist,
        album,
        duration_secs: track.duration as u64,
        artwork_url,
        hires: track.hires,
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
        is_local: false,
        album_id,
        artist_id,
    }
}
