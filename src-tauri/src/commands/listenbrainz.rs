//! ListenBrainz Tauri commands
//!
//! Exposes ListenBrainz scrobbling to the frontend
//!
//! **DEPRECATED**: Use V2 commands in `commands_v2.rs` instead.
//! These legacy commands will be removed in a future version.

#![allow(deprecated)]

use tauri::State;

use crate::listenbrainz::{
    AdditionalInfo, ListenBrainzSharedState, ListenBrainzStatus, QueuedListen, UserInfo,
};

/// Get ListenBrainz connection status
#[deprecated(since = "1.2.0", note = "Use v2_listenbrainz_get_status instead")]
#[tauri::command]
pub async fn listenbrainz_get_status(
    state: State<'_, ListenBrainzSharedState>,
) -> Result<ListenBrainzStatus, String> {
    let client = state.client.lock().await;
    Ok(client.get_status().await)
}

/// Check if ListenBrainz is enabled
#[deprecated(since = "1.2.0", note = "Use v2_listenbrainz_is_enabled instead")]
#[tauri::command]
pub async fn listenbrainz_is_enabled(
    state: State<'_, ListenBrainzSharedState>,
) -> Result<bool, String> {
    let client = state.client.lock().await;
    Ok(client.is_enabled().await)
}

/// Enable or disable ListenBrainz integration
#[deprecated(since = "1.2.0", note = "Use v2_listenbrainz_set_enabled instead")]
#[tauri::command]
pub async fn listenbrainz_set_enabled(
    enabled: bool,
    state: State<'_, ListenBrainzSharedState>,
) -> Result<(), String> {
    // Update client
    {
        let client = state.client.lock().await;
        client.set_enabled(enabled).await;
    }

    // Persist setting
    {
        let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        cache.set_enabled(enabled)?;
    }

    log::info!(
        "ListenBrainz integration {}",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

/// Connect to ListenBrainz with a user token
#[deprecated(since = "1.2.0", note = "Use v2_listenbrainz_connect instead")]
#[tauri::command]
pub async fn listenbrainz_connect(
    token: String,
    state: State<'_, ListenBrainzSharedState>,
) -> Result<UserInfo, String> {
    log::info!("Command: listenbrainz_connect");

    // Validate token and get user info
    let user_info = {
        let client = state.client.lock().await;
        client.set_token(&token).await?
    };

    // Persist credentials
    {
        let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        cache.set_credentials(Some(&token), Some(&user_info.user_name))?;
    }

    Ok(user_info)
}

/// Disconnect from ListenBrainz
#[deprecated(since = "1.2.0", note = "Use v2_listenbrainz_disconnect instead")]
#[tauri::command]
pub async fn listenbrainz_disconnect(
    state: State<'_, ListenBrainzSharedState>,
) -> Result<(), String> {
    log::info!("Command: listenbrainz_disconnect");

    // Clear client state
    {
        let client = state.client.lock().await;
        client.disconnect().await;
    }

    // Clear persisted credentials
    {
        let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        cache.clear_credentials()?;
    }

    Ok(())
}

/// Submit "now playing" to ListenBrainz
#[deprecated(since = "1.2.0", note = "Use v2_listenbrainz_now_playing instead")]
#[tauri::command]
pub async fn listenbrainz_now_playing(
    artist: String,
    track: String,
    album: Option<String>,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    state: State<'_, ListenBrainzSharedState>,
) -> Result<(), String> {
    log::debug!("Command: listenbrainz_now_playing - {} - {}", artist, track);

    let client = state.client.lock().await;

    // Build additional info with MusicBrainz data
    let mut info = AdditionalInfo::new();
    info.recording_mbid = recording_mbid;
    info.release_mbid = release_mbid;
    info.artist_mbids = artist_mbids;
    info.isrc = isrc;
    info.duration_ms = duration_ms;

    client
        .submit_playing_now(&artist, &track, album.as_deref(), Some(info))
        .await
}

/// Submit a scrobble to ListenBrainz
#[deprecated(since = "1.2.0", note = "Use v2_listenbrainz_scrobble instead")]
#[tauri::command]
pub async fn listenbrainz_scrobble(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    state: State<'_, ListenBrainzSharedState>,
) -> Result<(), String> {
    log::info!("Command: listenbrainz_scrobble - {} - {}", artist, track);

    let client = state.client.lock().await;

    // Build additional info with MusicBrainz data
    let mut info = AdditionalInfo::new();
    info.recording_mbid = recording_mbid;
    info.release_mbid = release_mbid;
    info.artist_mbids = artist_mbids;
    info.isrc = isrc;
    info.duration_ms = duration_ms;

    client
        .submit_listen(&artist, &track, album.as_deref(), timestamp, Some(info))
        .await
}

/// Queue a listen for offline submission
#[deprecated(since = "1.2.0", note = "Use v2_listenbrainz_queue_listen instead")]
#[tauri::command]
pub async fn listenbrainz_queue_listen(
    artist: String,
    track: String,
    album: Option<String>,
    timestamp: i64,
    recording_mbid: Option<String>,
    release_mbid: Option<String>,
    artist_mbids: Option<Vec<String>>,
    isrc: Option<String>,
    duration_ms: Option<u64>,
    state: State<'_, ListenBrainzSharedState>,
) -> Result<i64, String> {
    log::info!("Command: listenbrainz_queue_listen - {} - {}", artist, track);

    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.queue_listen(
        timestamp,
        &artist,
        &track,
        album.as_deref(),
        recording_mbid.as_deref(),
        release_mbid.as_deref(),
        artist_mbids.as_deref(),
        isrc.as_deref(),
        duration_ms,
    )
}

/// Get queued listens (for sync)
#[tauri::command]
pub async fn listenbrainz_get_queue(
    limit: Option<u32>,
    state: State<'_, ListenBrainzSharedState>,
) -> Result<Vec<QueuedListen>, String> {
    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.get_queued_listens(limit.unwrap_or(50))
}

/// Get queue count
#[tauri::command]
pub async fn listenbrainz_get_queue_count(
    state: State<'_, ListenBrainzSharedState>,
) -> Result<u32, String> {
    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.get_queue_count()
}

/// Mark queued listens as sent
#[tauri::command]
pub async fn listenbrainz_mark_sent(
    ids: Vec<i64>,
    state: State<'_, ListenBrainzSharedState>,
) -> Result<(), String> {
    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.mark_listens_sent(&ids)
}

/// Flush queue (submit all pending listens)
#[tauri::command]
pub async fn listenbrainz_flush_queue(
    state: State<'_, ListenBrainzSharedState>,
) -> Result<u32, String> {
    log::info!("Command: listenbrainz_flush_queue");

    // Get pending listens
    let pending = {
        let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        cache.get_queued_listens(50)?
    };

    if pending.is_empty() {
        return Ok(0);
    }

    let client = state.client.lock().await;
    let mut sent_ids = Vec::new();

    for listen in &pending {
        // Build additional info
        let mut info = AdditionalInfo::new();
        info.recording_mbid = listen.recording_mbid.clone();
        info.release_mbid = listen.release_mbid.clone();
        info.artist_mbids = listen.artist_mbids.clone();
        info.isrc = listen.isrc.clone();
        info.duration_ms = listen.duration_ms;

        match client
            .submit_listen(
                &listen.artist_name,
                &listen.track_name,
                listen.release_name.as_deref(),
                listen.listened_at,
                Some(info),
            )
            .await
        {
            Ok(()) => {
                sent_ids.push(listen.id);
            }
            Err(e) => {
                log::warn!("Failed to submit queued listen {}: {}", listen.id, e);
                // Increment attempt count
                let cache_opt__ = state.cache.lock().await;
                let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
                let _ = cache.increment_attempts(listen.id);
            }
        }
    }

    // Mark successful ones as sent
    if !sent_ids.is_empty() {
        let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
        cache.mark_listens_sent(&sent_ids)?;
    }

    let sent_count = sent_ids.len() as u32;
    log::info!("ListenBrainz queue flush: sent {}/{} listens", sent_count, pending.len());

    Ok(sent_count)
}

/// Clear the offline queue
#[tauri::command]
pub async fn listenbrainz_clear_queue(
    state: State<'_, ListenBrainzSharedState>,
) -> Result<(), String> {
    log::info!("Command: listenbrainz_clear_queue");
    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.clear_queue()
}

/// Cleanup old sent listens
#[tauri::command]
pub async fn listenbrainz_cleanup_queue(
    older_than_days: Option<u32>,
    state: State<'_, ListenBrainzSharedState>,
) -> Result<u32, String> {
    let cache_opt__ = state.cache.lock().await;
    let cache = cache_opt__.as_ref().ok_or("No active session - please log in")?;
    cache.cleanup_sent_listens(older_than_days.unwrap_or(7))
}
