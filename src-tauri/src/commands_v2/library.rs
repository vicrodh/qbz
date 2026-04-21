use tauri::State;

use crate::api_cache::ApiCacheState;
use crate::artist_blacklist::BlacklistState;
use crate::cast::{CastState, DlnaMetadata, DlnaState, MediaMetadata};
use crate::config::download_settings::DownloadSettingsState;
use crate::core_bridge::CoreBridgeState;
use crate::integrations_v2::MusicBrainzV2State;
use crate::library::{
    get_artwork_cache_dir, thumbnails, AudioFormat, LibraryState, LocalAlbum, LocalTrack,
    MetadataExtractor, PlaylistLocalTrack, PlaylistSettings, PlaylistStats, ScanProgress,
};
use crate::lyrics::LyricsState;
use crate::offline::OfflineState;
use crate::offline_cache::OfflineCacheState;
use crate::plex::{PlexMusicSection, PlexPlayResult, PlexTrack};
use crate::reco_store::{HomeResolved, HomeSeeds, RecoEventInput, RecoState};
use crate::runtime::{CommandRequirement, RuntimeManagerState};
use crate::AppState;
use md5::{Digest, Md5};

// ==================== Casting / Local Library Commands (V2 Native) ====================

#[tauri::command]
pub async fn v2_cast_start_discovery(state: State<'_, CastState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.start_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_stop_discovery(state: State<'_, CastState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.stop_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_get_devices(
    state: State<'_, CastState>,
) -> Result<Vec<crate::cast::DiscoveredDevice>, String> {
    let discovery = state.discovery.lock().await;
    Ok(discovery.get_discovered_devices())
}

#[tauri::command]
pub async fn v2_cast_connect(device_id: String, state: State<'_, CastState>) -> Result<(), String> {
    let device = {
        let discovery = state.discovery.lock().await;
        discovery
            .get_device(&device_id)
            .ok_or_else(|| format!("Device not found: {}", device_id))?
    };
    state
        .chromecast
        .connect(device.ip.clone(), device.port)
        .map_err(|e| e.to_string())?;
    let mut connected = state.connected_device_ip.lock().await;
    *connected = Some(device.ip);
    Ok(())
}

#[tauri::command]
pub async fn v2_cast_disconnect(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.disconnect().map_err(|e| e.to_string())?;
    let mut connected = state.connected_device_ip.lock().await;
    *connected = None;
    Ok(())
}

#[tauri::command]
pub async fn v2_cast_play_track(
    track_id: u64,
    metadata: MediaMetadata,
    cast_state: State<'_, CastState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let stream_url = {
        let client = app_state.client.read().await;
        client
            .get_stream_url_with_fallback(track_id, crate::api::models::Quality::HiRes)
            .await
            .map_err(|e| format!("Failed to get stream URL: {}", e))?
    };

    let content_type = stream_url.mime_type.clone();
    let cache = app_state.audio_cache.clone();
    // TODO: Add CMAF fallback when CoreBridge is accessible here
    // (currently uses legacy QobuzClient via AppState; needs CoreBridgeState param)
    let audio_data = if let Some(cached) = cache.get(track_id) {
        cached.data
    } else {
        let data = super::download_audio(&stream_url.url).await?;
        cache.insert(track_id, data.clone());
        data
    };

    let target_ip = {
        let connected = cast_state.connected_device_ip.lock().await;
        connected.clone()
    };

    cast_state
        .get_or_create_media_server()
        .await
        .map_err(|e| e.to_string())?;

    let url = {
        let mut server_guard = cast_state.media_server.lock().await;
        let server = server_guard
            .as_mut()
            .ok_or("Media server not initialized")?;
        server.register_audio(track_id, audio_data, &content_type);
        match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(track_id, ip),
            None => server.get_audio_url(track_id),
        }
        .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    cast_state
        .chromecast
        .load_media(url, content_type, metadata)
        .map_err(|e| e.to_string())
}

/// Play a LOCAL library track on a Chromecast device. Companion to
/// v2_dlna_play_local_track — same routing problem, same solution pattern.
/// Reads the file from disk, infers content_type, registers bytes with the
/// media server, loads the URL on Chromecast.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_cast_play_local_track(
    trackId: i64,
    metadata: MediaMetadata,
    cast_state: State<'_, CastState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("Chromecast: cast_play_local_track called for track_id={}", trackId);

    let track = {
        let guard = library_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session")?;
        db.get_track(trackId)
            .map_err(|e| format!("Library lookup failed: {}", e))?
            .ok_or_else(|| format!("Track {} not found in library", trackId))?
    };

    let audio_data = std::fs::read(&track.file_path)
        .map_err(|e| format!("Failed to read {}: {}", track.file_path, e))?;
    let content_type = local_track_content_type(&track);

    let target_ip = {
        let connected = cast_state.connected_device_ip.lock().await;
        connected.clone()
    };

    cast_state
        .get_or_create_media_server()
        .await
        .map_err(|e| e.to_string())?;

    let media_key = trackId as u64;
    let url = {
        let mut server_guard = cast_state.media_server.lock().await;
        let server = server_guard
            .as_mut()
            .ok_or("Media server not initialized")?;
        server.register_audio(media_key, audio_data, &content_type);
        match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(media_key, ip),
            None => server.get_audio_url(media_key),
        }
        .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    log::info!(
        "Chromecast: Playing local track {} ({}) via MediaServer URL: {}",
        trackId,
        content_type,
        url
    );

    cast_state
        .chromecast
        .load_media(url, content_type, metadata)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_play(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.play().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_pause(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.pause().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_stop(state: State<'_, CastState>) -> Result<(), String> {
    state.chromecast.stop().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_seek(position_secs: f64, state: State<'_, CastState>) -> Result<(), String> {
    state
        .chromecast
        .seek(position_secs)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_get_position(
    state: State<'_, CastState>,
) -> Result<crate::cast::CastPositionInfo, String> {
    state
        .chromecast
        .get_media_position()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_cast_set_volume(volume: f32, state: State<'_, CastState>) -> Result<(), String> {
    state
        .chromecast
        .set_volume(volume)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_start_discovery(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.start_discovery().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_stop_discovery(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut discovery = state.discovery.lock().await;
    discovery.stop_discovery().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_get_devices(
    state: State<'_, DlnaState>,
) -> Result<Vec<crate::cast::DiscoveredDlnaDevice>, String> {
    let discovery = state.discovery.lock().await;
    Ok(discovery.get_discovered_devices())
}

#[tauri::command]
pub async fn v2_dlna_connect(device_id: String, state: State<'_, DlnaState>) -> Result<(), String> {
    let device = {
        let discovery = state.discovery.lock().await;
        discovery
            .get_device(&device_id)
            .ok_or_else(|| format!("Device not found: {}", device_id))?
    };
    let connection = crate::cast::DlnaConnection::connect(device)
        .await
        .map_err(|e| e.to_string())?;
    let mut state_connection = state.connection.lock().await;
    *state_connection = Some(connection);
    Ok(())
}

#[tauri::command]
pub async fn v2_dlna_disconnect(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    if let Some(conn) = connection.as_mut() {
        conn.disconnect().map_err(|e| e.to_string())?;
    }
    *connection = None;
    Ok(())
}

#[tauri::command]
pub async fn v2_dlna_play_track(
    track_id: u64,
    metadata: DlnaMetadata,
    dlna_state: State<'_, DlnaState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let stream_url = {
        let client = app_state.client.read().await;
        client
            .get_stream_url_with_fallback(track_id, crate::api::models::Quality::HiRes)
            .await
            .map_err(|e| format!("Failed to get stream URL: {}", e))?
    };

    let content_type = stream_url.mime_type.clone();
    let cache = app_state.audio_cache.clone();
    // TODO: Add CMAF fallback when CoreBridge is accessible here
    // (currently uses legacy QobuzClient via AppState; needs CoreBridgeState param)
    let audio_data = if let Some(cached) = cache.get(track_id) {
        cached.data
    } else {
        let data = super::download_audio(&stream_url.url).await?;
        cache.insert(track_id, data.clone());
        data
    };

    let target_ip = {
        let connection = dlna_state.connection.lock().await;
        connection.as_ref().map(|conn| conn.device_ip().to_string())
    };

    dlna_state
        .ensure_media_server()
        .await
        .map_err(|e| e.to_string())?;

    let url = {
        let mut server_guard = dlna_state.media_server.lock().await;
        let server = server_guard
            .as_mut()
            .ok_or("Media server not initialized")?;
        server.register_audio(track_id, audio_data, &content_type);
        match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(track_id, ip),
            None => server.get_audio_url(track_id),
        }
        .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or("Not connected")?;
        conn.load_media(&url, &metadata, &content_type)
            .await
            .map_err(|e| e.to_string())?;
    }
    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or("Not connected")?;
        conn.play().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Play a LOCAL library track on a DLNA renderer. Reads the file from disk,
/// infers content_type from the track's format, registers bytes with the
/// media server, and sends AVTransport URI + Play to the renderer.
///
/// Mirrors `v2_dlna_play_track` but for local-source tracks — that command
/// resolves a Qobuz stream URL and would fail for a library row id. Without
/// this command, casting any local-library track silently falls back to the
/// app's local audio backend (issue #332).
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dlna_play_local_track(
    trackId: i64,
    metadata: DlnaMetadata,
    dlna_state: State<'_, DlnaState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    log::info!("DLNA: dlna_play_local_track called for track_id={}", trackId);

    let track = {
        let guard = library_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session")?;
        db.get_track(trackId)
            .map_err(|e| format!("Library lookup failed: {}", e))?
            .ok_or_else(|| format!("Track {} not found in library", trackId))?
    };

    let audio_data = std::fs::read(&track.file_path)
        .map_err(|e| format!("Failed to read {}: {}", track.file_path, e))?;
    let content_type = local_track_content_type(&track);

    let target_ip = {
        let connection = dlna_state.connection.lock().await;
        connection.as_ref().map(|conn| conn.device_ip().to_string())
    };

    dlna_state
        .ensure_media_server()
        .await
        .map_err(|e| e.to_string())?;

    // Library row ids are small positive autoincrement integers; using them
    // directly as MediaServer keys won't collide with Qobuz track ids within
    // a single cast session (the server is ephemeral per cast connection).
    let media_key = trackId as u64;
    let url = {
        let mut server_guard = dlna_state.media_server.lock().await;
        let server = server_guard
            .as_mut()
            .ok_or("Media server not initialized")?;
        server.register_audio(media_key, audio_data, &content_type);
        match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(media_key, ip),
            None => server.get_audio_url(media_key),
        }
        .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    log::info!(
        "DLNA: Playing local track {} ({}) via MediaServer URL: {}",
        trackId,
        content_type,
        url
    );

    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or("Not connected")?;
        conn.load_media(&url, &metadata, &content_type)
            .await
            .map_err(|e| e.to_string())?;
    }
    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or("Not connected")?;
        conn.play().await.map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn local_track_content_type(track: &LocalTrack) -> String {
    match track.format {
        AudioFormat::Flac => "audio/flac".to_string(),
        AudioFormat::Alac => "audio/mp4".to_string(),
        AudioFormat::Wav => "audio/wav".to_string(),
        AudioFormat::Aiff => "audio/aiff".to_string(),
        AudioFormat::Ape => "audio/x-ape".to_string(),
        AudioFormat::Mp3 => "audio/mpeg".to_string(),
        AudioFormat::Unknown => {
            let ext = std::path::Path::new(&track.file_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            match ext.as_str() {
                "flac" => "audio/flac",
                "mp3" => "audio/mpeg",
                "wav" => "audio/wav",
                "m4a" | "mp4" | "aac" => "audio/mp4",
                "aiff" | "aif" => "audio/aiff",
                "ogg" | "oga" => "audio/ogg",
                "opus" => "audio/opus",
                _ => "audio/octet-stream",
            }
            .to_string()
        }
    }
}

/// Play a PLEX track on a DLNA renderer. Plex serves audio via its own HTTP
/// server, but streaming directly from Plex to the DLNA renderer would
/// require the renderer to present the user's Plex token — we proxy the
/// bytes through our local media server instead so auth stays ours.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_dlna_play_plex_track(
    baseUrl: String,
    token: String,
    ratingKey: String,
    metadata: DlnaMetadata,
    dlna_state: State<'_, DlnaState>,
) -> Result<(), String> {
    log::info!("DLNA: dlna_play_plex_track called for rating_key={}", ratingKey);

    let resolved =
        crate::plex::plex_resolve_track_media(baseUrl, token, ratingKey.clone()).await?;
    // Plex sometimes omits content-type on transcoded streams; audio/mpeg is
    // the most common transcode target and a safe DLNA fallback.
    let content_type = resolved
        .content_type
        .clone()
        .unwrap_or_else(|| "audio/mpeg".to_string());

    let target_ip = {
        let connection = dlna_state.connection.lock().await;
        connection.as_ref().map(|conn| conn.device_ip().to_string())
    };

    dlna_state
        .ensure_media_server()
        .await
        .map_err(|e| e.to_string())?;

    let media_key = plex_key_to_u64(&ratingKey);
    let url = {
        let mut server_guard = dlna_state.media_server.lock().await;
        let server = server_guard
            .as_mut()
            .ok_or("Media server not initialized")?;
        server.register_audio(media_key, resolved.bytes.clone(), &content_type);
        match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(media_key, ip),
            None => server.get_audio_url(media_key),
        }
        .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    log::info!(
        "DLNA: Playing plex track {} ({}) via MediaServer URL: {}",
        ratingKey,
        content_type,
        url
    );

    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or("Not connected")?;
        conn.load_media(&url, &metadata, &content_type)
            .await
            .map_err(|e| e.to_string())?;
    }
    {
        let mut connection = dlna_state.connection.lock().await;
        let conn = connection.as_mut().ok_or("Not connected")?;
        conn.play().await.map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Play a PLEX track on a Chromecast device. Companion to v2_dlna_play_plex_track.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_cast_play_plex_track(
    baseUrl: String,
    token: String,
    ratingKey: String,
    metadata: MediaMetadata,
    cast_state: State<'_, CastState>,
) -> Result<(), String> {
    log::info!("Chromecast: cast_play_plex_track called for rating_key={}", ratingKey);

    let resolved =
        crate::plex::plex_resolve_track_media(baseUrl, token, ratingKey.clone()).await?;
    // Plex sometimes omits content-type on transcoded streams; audio/mpeg is
    // the most common transcode target and a safe DLNA fallback.
    let content_type = resolved
        .content_type
        .clone()
        .unwrap_or_else(|| "audio/mpeg".to_string());

    let target_ip = {
        let connected = cast_state.connected_device_ip.lock().await;
        connected.clone()
    };

    cast_state
        .get_or_create_media_server()
        .await
        .map_err(|e| e.to_string())?;

    let media_key = plex_key_to_u64(&ratingKey);
    let url = {
        let mut server_guard = cast_state.media_server.lock().await;
        let server = server_guard
            .as_mut()
            .ok_or("Media server not initialized")?;
        server.register_audio(media_key, resolved.bytes.clone(), &content_type);
        match target_ip.as_deref() {
            Some(ip) => server.get_audio_url_for_target(media_key, ip),
            None => server.get_audio_url(media_key),
        }
        .ok_or_else(|| "Failed to build media URL".to_string())?
    };

    log::info!(
        "Chromecast: Playing plex track {} ({}) via MediaServer URL: {}",
        ratingKey,
        content_type,
        url
    );

    cast_state
        .chromecast
        .load_media(url, content_type, metadata)
        .map_err(|e| e.to_string())
}

/// Namespace a Plex rating key into u64 for the MediaServer. Most Plex
/// ratingKey strings parse as numeric; fall back to a stable hash so
/// non-numeric keys still route correctly and don't collide with library
/// ids in the low u64 range. High bit set to separate from Qobuz/library.
fn plex_key_to_u64(rating_key: &str) -> u64 {
    const PLEX_NAMESPACE: u64 = 0x4000_0000_0000_0000;
    if let Ok(n) = rating_key.parse::<u64>() {
        return PLEX_NAMESPACE | (n & !PLEX_NAMESPACE);
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    rating_key.hash(&mut hasher);
    PLEX_NAMESPACE | (hasher.finish() & !PLEX_NAMESPACE)
}

#[tauri::command]
pub async fn v2_dlna_play(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.play().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_pause(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.pause().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_stop(state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_seek(position_secs: u64, state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.seek(position_secs).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_get_position(
    state: State<'_, DlnaState>,
) -> Result<crate::cast::DlnaPositionInfo, String> {
    let connection = state.connection.lock().await;
    let conn = connection
        .as_ref()
        .ok_or_else(|| "Not connected".to_string())?;
    conn.get_position_info().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_dlna_set_volume(volume: f32, state: State<'_, DlnaState>) -> Result<(), String> {
    let mut connection = state.connection.lock().await;
    let conn = connection.as_mut().ok_or("Not connected")?;
    conn.set_volume(volume).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_clear_offline_cache(
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    let paths = {
        let guard = cache_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        db.clear_all()?
    };
    for path in paths {
        let p = std::path::Path::new(&path);
        if p.exists() {
            let _ = std::fs::remove_file(p);
        }
    }

    let cache_dir = cache_state.cache_dir.read().unwrap().clone();
    let tracks_dir = cache_dir.join("tracks");
    if tracks_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&tracks_dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "tracks" && !name.ends_with(".db") && !name.ends_with(".db-journal") {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
        }
    }

    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_all_qobuz_cached_tracks()
        .map_err(|e| format!("Failed to remove cached tracks from library: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_library_remove_folder(
    path: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_folder(&path).map_err(|e| e.to_string())?;
    db.delete_tracks_in_folder(&path)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn v2_library_check_folder_accessible(path: String) -> Result<bool, String> {
    log::info!("[V2] library_check_folder_accessible {}", path);

    let path_ref = std::path::Path::new(&path);
    if !path_ref.exists() {
        return Ok(false);
    }

    // Avoid UI stalls on slow/unresponsive mounts.
    let path_clone = path.clone();
    let check_result = tokio::time::timeout(
        std::time::Duration::from_secs(6),
        tokio::task::spawn_blocking(move || {
            std::fs::read_dir(std::path::Path::new(&path_clone)).is_ok()
        }),
    )
    .await;

    match check_result {
        Ok(Ok(accessible)) => Ok(accessible),
        Ok(Err(_)) => {
            log::warn!(
                "[V2] Failed to spawn blocking task for folder check: {}",
                path
            );
            Ok(false)
        }
        Err(_) => {
            // Mounted-but-slow network shares can timeout but still be usable.
            let exists = std::path::Path::new(&path).exists();
            log::warn!(
                "[V2] Timeout checking folder accessibility: {} (exists={})",
                path,
                exists
            );
            Ok(exists)
        }
    }
}

#[tauri::command]
pub async fn v2_library_clear_artwork_cache() -> Result<u64, String> {
    let artwork_dir = get_artwork_cache_dir();
    if !artwork_dir.exists() {
        return Ok(0);
    }
    let mut cleared = 0u64;
    if let Ok(entries) = std::fs::read_dir(&artwork_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    cleared += meta.len();
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    Ok(cleared)
}

#[tauri::command]
pub async fn v2_library_clear_thumbnails_cache() -> Result<u64, String> {
    let size_before = thumbnails::get_cache_size().unwrap_or(0);
    thumbnails::clear_thumbnails().map_err(|e| e.to_string())?;
    Ok(size_before)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_library_get_thumbnail(artworkPath: String) -> Result<String, String> {
    crate::library::library_get_thumbnail(artworkPath).await
}

#[tauri::command]
pub async fn v2_library_get_thumbnails_cache_size() -> Result<u64, String> {
    crate::library::library_get_thumbnails_cache_size().await
}

#[tauri::command]
pub async fn v2_library_get_scan_progress(
    library_state: State<'_, LibraryState>,
) -> Result<ScanProgress, String> {
    crate::library::library_get_scan_progress(library_state).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_library_get_tracks_by_ids(
    trackIds: Vec<i64>,
    library_state: State<'_, LibraryState>,
) -> Result<Vec<LocalTrack>, String> {
    crate::library::library_get_tracks_by_ids(trackIds, library_state).await
}

#[tauri::command]
pub async fn v2_library_play_track(
    track_id: i64,
    library_state: State<'_, LibraryState>,
    bridge: State<'_, CoreBridgeState>,
    offline_cache: State<'_, crate::offline_cache::OfflineCacheState>,
    app_state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await
        .map_err(|e| e.to_string())?;

    let track = {
        let guard = library_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        db.get_track(track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?
    };

    // Qobuz-cached offline tracks are authoritative in the offline-cache
    // DB, not the library index. The library row for these tracks carries
    // metadata only (title/artist/album) plus a display path; the library's
    // `file_path` for v2 entries is a track directory that is not playable
    // directly. So for source='qobuz_download' we always resolve through
    // the offline-cache DB — cache_format tells us whether to decrypt the
    // CMAF bundle or to read a plain-FLAC v1 file.
    //
    // NOTE: `track_id` here is the library row id (autoincrement), NOT the
    // Qobuz track id. The offline-cache DB is keyed by Qobuz track id. The
    // library row carries the Qobuz id in `qobuz_track_id`.
    let is_qobuz_cached = track.source.as_deref() == Some("qobuz_download");
    if is_qobuz_cached {
        let qobuz_track_id = match track.qobuz_track_id {
            Some(id) => id,
            None => {
                return Err(format!(
                    "Library row {} is marked qobuz_download but has no qobuz_track_id",
                    track_id
                ));
            }
        };
        let bundle_row = {
            let guard = offline_cache.db.lock().await;
            guard
                .as_ref()
                .and_then(|db| db.get_cmaf_bundle(qobuz_track_id as u64).ok().flatten())
        };
        match bundle_row {
            Some(row) if row.cache_format == 2 => {
                let cache_path = offline_cache.get_cache_path();
                // spawn_blocking via the ui-events helper so the track row
                // shows an "unlocking" animation while decrypt runs.
                // Display id = library row id (what the UI renders by),
                // CMAF id = qobuz track id (what the bundle is keyed by).
                let audio_data =
                    crate::offline_cache::playback::load_cmaf_bundle_with_ui_events(
                        &app_handle,
                        track_id as u64,
                        qobuz_track_id as u64,
                        row.clone(),
                        cache_path,
                    )
                    .await
                    .ok_or_else(|| {
                        format!(
                            "Offline CMAF bundle for Qobuz track {} is present but failed to decrypt",
                            qobuz_track_id
                        )
                    })?;
                // Warm L1 so subsequent access (replay, gapless) is instant.
                // Keyed by the library row id so Library replay hits; the
                // offline cache DB itself is keyed by Qobuz id and is
                // consulted separately up above.
                app_state.audio_cache.insert(track_id as u64, audio_data.clone());
                let bridge = bridge.get().await;
                // IMPORTANT: play_data gets the LIBRARY track_id (the row
                // id the frontend already tracks), NOT the Qobuz track id.
                // Every piece of UI state — currently-playing card,
                // seekbar position updates, queue auto-advance detection —
                // keys off the id the frontend sent in. Using the Qobuz
                // id here silently desynced the player from the UI:
                // backend reports "playing 95787326" while the frontend
                // waits for updates on 542 → seekbar never ticks, track
                // never auto-advances, queue panics and starts calling
                // next_track every second.
                bridge
                    .player()
                    .play_data(audio_data, track_id as u64)
                    .map_err(|e| format!("Failed to play CMAF offline bundle: {}", e))?;
                if let Some(start_secs) = track.cue_start_secs {
                    let start_pos = start_secs as u64;
                    if start_pos > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        bridge
                            .player()
                            .seek(start_pos)
                            .map_err(|e| format!("Failed to seek: {}", e))?;
                    }
                }
                return Ok(());
            }
            Some(row) => {
                // cache_format=1 (legacy plain FLAC). The authoritative path
                // is the offline-cache row's `segments_path` field (v1 stored
                // the FLAC file path there), not the library index file_path
                // (which for qobuz_download entries is now a display-only
                // value that may be a directory).
                let file_path = std::path::Path::new(&row.segments_path);
                if !file_path.exists() {
                    return Err(format!(
                        "Offline cache file missing for Qobuz track {}: {}",
                        qobuz_track_id, row.segments_path
                    ));
                }
                let audio_data = std::fs::read(file_path)
                    .map_err(|e| format!("Failed to read v1 offline file: {}", e))?;
                let bridge = bridge.get().await;
                bridge
                    .player()
                    .play_data(audio_data, track_id as u64)
                    .map_err(|e| format!("Failed to play: {}", e))?;
                return Ok(());
            }
            None => {
                // Orphan: library row says qobuz_download but offline cache
                // has no record. Either the cache was partially wiped or
                // there's a corruption — surface the error instead of
                // trying to read whatever the library file_path says (which
                // for v2 is a directory and blows up with os error 21).
                return Err(format!(
                    "Offline cache entry for Qobuz track {} is missing (library index is stale)",
                    qobuz_track_id
                ));
            }
        }
    }

    // Regular user local library files (FLAC/MP3 owned by the user,
    // not Qobuz-cached). Read the file path from the library row.
    let file_path = std::path::Path::new(&track.file_path);
    if !file_path.exists() {
        return Err(format!("File not found: {}", track.file_path));
    }
    let audio_data = std::fs::read(file_path).map_err(|e| format!("Failed to read file: {}", e))?;
    let bridge = bridge.get().await;
    bridge
        .player()
        .play_data(audio_data, track_id as u64)
        .map_err(|e| format!("Failed to play: {}", e))?;
    if let Some(start_secs) = track.cue_start_secs {
        let start_pos = start_secs as u64;
        if start_pos > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            bridge
                .player()
                .seek(start_pos)
                .map_err(|e| format!("Failed to seek: {}", e))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn v2_playlist_set_sort(
    playlist_id: u64,
    sort_by: String,
    sort_order: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.update_playlist_sort(playlist_id, &sort_by, &sort_order)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_set_artwork(
    playlist_id: u64,
    artwork_path: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let final_path = if let Some(source_path) = artwork_path {
        let artwork_dir = get_artwork_cache_dir();
        let source = std::path::Path::new(&source_path);
        if !source.exists() {
            return Err(format!("Source image does not exist: {}", source_path));
        }
        let extension = source.extension().and_then(|e| e.to_str()).unwrap_or("jpg");
        let filename = format!(
            "playlist_{}_{}.{}",
            playlist_id,
            chrono::Utc::now().timestamp(),
            extension
        );
        let dest_path = artwork_dir.join(filename);
        std::fs::copy(source, &dest_path).map_err(|e| format!("Failed to copy artwork: {}", e))?;
        Some(dest_path.to_string_lossy().to_string())
    } else {
        None
    };
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.update_playlist_artwork(playlist_id, final_path.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_add_local_track(
    playlist_id: u64,
    local_track_id: i64,
    position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.add_local_track_to_playlist(playlist_id, local_track_id, position)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_remove_local_track(
    playlist_id: u64,
    local_track_id: i64,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_local_track_from_playlist(playlist_id, local_track_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_set_hidden(
    playlist_id: u64,
    hidden: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_playlist_hidden(playlist_id, hidden)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_set_favorite(
    playlist_id: u64,
    favorite: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_playlist_favorite(playlist_id, favorite)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_reorder(
    playlist_ids: Vec<u64>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.reorder_playlists(&playlist_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_init_custom_order(
    playlist_id: u64,
    track_ids: Vec<(i64, bool)>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.init_playlist_custom_order(playlist_id, &track_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_set_custom_order(
    playlist_id: u64,
    orders: Vec<(i64, bool, i32)>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_playlist_custom_order(playlist_id, &orders)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_move_track(
    playlist_id: u64,
    track_id: i64,
    is_local: bool,
    new_position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.move_playlist_track(playlist_id, track_id, is_local, new_position)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_library_set_album_artwork(
    album_group_key: String,
    artwork_path: String,
    state: State<'_, LibraryState>,
) -> Result<String, String> {
    if album_group_key.is_empty() {
        return Err("Album group key is required".to_string());
    }
    let source_path = std::path::Path::new(&artwork_path);
    if !source_path.is_file() {
        return Err("Artwork file not found".to_string());
    }
    let artwork_cache = get_artwork_cache_dir();
    let cached_path = MetadataExtractor::cache_artwork_file(source_path, &artwork_cache)
        .ok_or_else(|| "Failed to cache artwork file".to_string())?;
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.update_album_group_artwork(&album_group_key, &cached_path)
        .map_err(|e| e.to_string())?;
    Ok(cached_path)
}

#[tauri::command]
pub async fn v2_library_set_album_hidden(
    album_group_key: String,
    hidden: bool,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.set_album_hidden(&album_group_key, hidden)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_delete_playlist_folder(
    id: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.delete_playlist_folder(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_reorder_playlist_folders(
    folder_ids: Vec<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.reorder_playlist_folders(&folder_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_move_playlist_to_folder(
    playlist_id: u64,
    folder_id: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.move_playlist_to_folder(playlist_id, folder_id.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_lyrics_clear_cache(state: State<'_, LyricsState>) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.clear().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_musicbrainz_get_cache_stats(
    state: State<'_, MusicBrainzV2State>,
) -> Result<qbz_integrations::musicbrainz::cache::CacheStats, String> {
    let cache_opt = state.cache.lock().await;
    let cache = cache_opt
        .as_ref()
        .ok_or("No active session - please log in")?;
    cache.get_stats()
}

#[tauri::command]
pub async fn v2_musicbrainz_clear_cache(
    state: State<'_, MusicBrainzV2State>,
) -> Result<(), String> {
    let cache_opt = state.cache.lock().await;
    let cache = cache_opt
        .as_ref()
        .ok_or("No active session - please log in")?;
    cache.clear_all()
}

#[tauri::command]
pub fn v2_set_show_partial_playlists(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_show_partial_playlists(enabled)
}

#[tauri::command]
pub fn v2_set_allow_cast_while_offline(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_cast_while_offline(enabled)
}

#[tauri::command]
pub fn v2_set_allow_immediate_scrobbling(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_immediate_scrobbling(enabled)
}

#[tauri::command]
pub fn v2_set_allow_accumulated_scrobbling(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_allow_accumulated_scrobbling(enabled)
}

#[tauri::command]
pub fn v2_set_show_network_folders_in_manual_offline(
    enabled: bool,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.set_show_network_folders_in_manual_offline(enabled)
}

#[tauri::command]
pub async fn v2_get_offline_status(
    state: State<'_, OfflineState>,
) -> Result<crate::offline::OfflineStatus, String> {
    crate::offline::commands::get_offline_status(state).await
}

#[tauri::command]
pub fn v2_get_offline_settings(
    state: State<'_, OfflineState>,
) -> Result<crate::offline::OfflineSettings, String> {
    crate::offline::commands::get_offline_settings(state)
}

#[tauri::command]
pub async fn v2_set_manual_offline(
    enabled: bool,
    state: State<'_, OfflineState>,
    audio_state: State<'_, crate::config::audio_settings::AudioSettingsState>,
    app_handle: tauri::AppHandle,
) -> Result<crate::offline::OfflineStatus, String> {
    crate::offline::commands::set_manual_offline(enabled, state, audio_state, app_handle).await
}

#[tauri::command]
pub async fn v2_check_network() -> bool {
    crate::offline::commands::check_network().await
}

#[tauri::command]
pub fn v2_add_tracks_to_pending_playlist(
    pending_id: i64,
    qobuz_track_ids: Vec<u64>,
    local_track_paths: Vec<String>,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.add_tracks_to_pending_playlist(pending_id, &qobuz_track_ids, &local_track_paths)
}

#[tauri::command]
pub fn v2_update_pending_playlist_qobuz_id(
    pending_id: i64,
    qobuz_playlist_id: u64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.update_qobuz_playlist_id(pending_id, qobuz_playlist_id)
}

#[tauri::command]
pub fn v2_mark_pending_playlist_synced(
    pending_id: i64,
    qobuz_playlist_id: u64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.mark_playlist_synced(pending_id, qobuz_playlist_id)
}

#[tauri::command]
pub fn v2_delete_pending_playlist(
    pending_id: i64,
    state: State<'_, OfflineState>,
) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.delete_pending_playlist(pending_id)
}

#[tauri::command]
pub fn v2_mark_scrobbles_sent(ids: Vec<i64>, state: State<'_, OfflineState>) -> Result<(), String> {
    let guard = state
        .store
        .lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_ref().ok_or("No active session - please log in")?;
    store.mark_scrobbles_sent(&ids)
}

#[tauri::command]
pub fn v2_get_pending_playlists(
    state: State<'_, OfflineState>,
) -> Result<Vec<crate::offline::PendingPlaylist>, String> {
    crate::offline::commands::get_pending_playlists(state)
}

#[tauri::command]
pub async fn v2_remove_cached_track(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
    library_state: State<'_, LibraryState>,
) -> Result<(), String> {
    {
        let guard = cache_state.db.lock().await;
        let db = guard.as_ref().ok_or("No active session - please log in")?;
        if let Some(file_path) = db.delete_track(track_id)? {
            let path = std::path::Path::new(&file_path);
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let guard = library_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let _ = db.remove_qobuz_cached_track(track_id);
    Ok(())
}

#[tauri::command]
pub async fn v2_get_cached_tracks(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<Vec<crate::offline_cache::CachedTrackInfo>, String> {
    let guard = cache_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_all_tracks()
}

#[tauri::command]
pub async fn v2_get_offline_cache_stats(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<crate::offline_cache::OfflineCacheStats, String> {
    let limit = *cache_state.limit_bytes.lock().await;
    let guard = cache_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_stats(&cache_state.get_cache_path(), limit)
}

#[tauri::command]
pub async fn v2_set_offline_cache_limit(
    limit_mb: Option<u64>,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    let limit_bytes = limit_mb.map(|mb| mb * 1024 * 1024);
    let mut limit = cache_state.limit_bytes.lock().await;
    *limit = limit_bytes;
    Ok(())
}

#[tauri::command]
pub async fn v2_open_offline_cache_folder(
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    let path = cache_state.cache_dir.read().unwrap().clone();
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("Failed to create cache directory: {}", e))?;
    open::that(&path).map_err(|e| format!("Failed to open folder: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_open_album_folder(
    album_id: String,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    let guard = cache_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let tracks = db.get_all_tracks()?;
    let album_tracks: Vec<_> = tracks
        .into_iter()
        .filter(|t| t.album_id.as_deref() == Some(&album_id))
        .collect();
    if album_tracks.is_empty() {
        return Err("No cached tracks found for this album".to_string());
    }
    let file_path = db
        .get_file_path(album_tracks[0].track_id)?
        .ok_or_else(|| "Track file path not found".to_string())?;
    let album_dir = std::path::Path::new(&file_path)
        .parent()
        .ok_or_else(|| "Could not determine album folder".to_string())?;
    open::that(album_dir).map_err(|e| format!("Failed to open folder: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_open_track_folder(
    track_id: u64,
    cache_state: State<'_, OfflineCacheState>,
) -> Result<(), String> {
    let guard = cache_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let file_path = db
        .get_file_path(track_id)?
        .ok_or_else(|| "Track file path not found - track may not be cached".to_string())?;
    let track_dir = std::path::Path::new(&file_path)
        .parent()
        .ok_or_else(|| "Could not determine track folder".to_string())?;
    open::that(track_dir).map_err(|e| format!("Failed to open folder: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn v2_lastfm_open_auth_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| format!("Failed to open browser: {}", e))
}

#[tauri::command]
pub async fn v2_lastfm_set_credentials(
    api_key: String,
    api_secret: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut client = state.lastfm.lock().await;
    client.set_credentials(api_key, api_secret);
    Ok(())
}

#[tauri::command]
pub async fn v2_reco_log_event(
    event: RecoEventInput,
    state: State<'_, RecoState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.insert_event(&event)
}

#[tauri::command]
pub async fn v2_reco_train_scores(
    lookback_days: Option<i64>,
    half_life_days: Option<f64>,
    max_events: Option<u32>,
    max_per_type: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<(), String> {
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    let lookback_days = lookback_days.unwrap_or(90);
    let half_life_days = half_life_days.unwrap_or(21.0);
    let max_events = max_events.unwrap_or(5000);
    let max_per_type = max_per_type.unwrap_or(200);

    let now_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let since_ts = now_ts.saturating_sub(lookback_days * 86_400);

    let mut guard = state.db.lock().await;
    let db = guard.as_mut().ok_or("No active session - please log in")?;
    let events = db.get_events_since(since_ts, Some(max_events))?;

    let decay_factor = |age_secs: i64| -> f64 {
        if half_life_days <= 0.0 {
            return 1.0;
        }
        let half_life_secs = half_life_days * 86_400.0;
        let exponent = age_secs as f64 / half_life_secs;
        0.5_f64.powf(exponent)
    };

    let event_weight = |event_type: &str| -> f64 {
        match event_type {
            "play" => 1.0,
            "favorite" => 3.0,
            "playlist_add" => 1.2,
            _ => 1.0,
        }
    };

    let item_weight = |item_type: &str, primary: bool| -> f64 {
        if primary {
            return 1.0;
        }
        match item_type {
            "album" => 0.7,
            "artist" => 0.5,
            "track" => 0.85,
            _ => 0.6,
        }
    };

    let build_scores = |favorites_only: bool| {
        let mut tracks: HashMap<u64, f64> = HashMap::new();
        let mut albums: HashMap<String, f64> = HashMap::new();
        let mut artists: HashMap<u64, f64> = HashMap::new();

        for event in &events {
            if favorites_only && event.event_type != "favorite" {
                continue;
            }

            let age_secs = (now_ts - event.created_at).max(0);
            let base_weight = event_weight(&event.event_type) * decay_factor(age_secs);

            if let Some(track_id) = event.track_id {
                let weight = base_weight * item_weight("track", event.item_type == "track");
                *tracks.entry(track_id).or_insert(0.0) += weight;
            }
            if let Some(album_id) = event.album_id.as_ref() {
                let weight = base_weight * item_weight("album", event.item_type == "album");
                *albums.entry(album_id.clone()).or_insert(0.0) += weight;
            }
            if let Some(artist_id) = event.artist_id {
                let weight = base_weight * item_weight("artist", event.item_type == "artist");
                *artists.entry(artist_id).or_insert(0.0) += weight;
            }
        }

        (tracks, albums, artists)
    };

    let build_track_entries = |scores: HashMap<u64, f64>| {
        let mut entries: Vec<(u64, f64)> = scores.into_iter().collect();
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
            .into_iter()
            .take(max_per_type as usize)
            .map(|(track_id, score)| crate::reco_store::db::RecoScoreEntry {
                track_id: Some(track_id),
                album_id: None,
                artist_id: None,
                score,
            })
            .collect::<Vec<_>>()
    };
    let build_album_entries = |scores: HashMap<String, f64>| {
        let mut entries: Vec<(String, f64)> = scores.into_iter().collect();
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
            .into_iter()
            .take(max_per_type as usize)
            .map(|(album_id, score)| crate::reco_store::db::RecoScoreEntry {
                track_id: None,
                album_id: Some(album_id),
                artist_id: None,
                score,
            })
            .collect::<Vec<_>>()
    };
    let build_artist_entries = |scores: HashMap<u64, f64>| {
        let mut entries: Vec<(u64, f64)> = scores.into_iter().collect();
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        entries
            .into_iter()
            .take(max_per_type as usize)
            .map(|(artist_id, score)| crate::reco_store::db::RecoScoreEntry {
                track_id: None,
                album_id: None,
                artist_id: Some(artist_id),
                score,
            })
            .collect::<Vec<_>>()
    };

    let (all_tracks, all_albums, all_artists) = build_scores(false);
    let (fav_tracks, fav_albums, fav_artists) = build_scores(true);

    db.replace_scores("all", "track", &build_track_entries(all_tracks))?;
    db.replace_scores("all", "album", &build_album_entries(all_albums))?;
    db.replace_scores("all", "artist", &build_artist_entries(all_artists))?;
    db.replace_scores("favorite", "track", &build_track_entries(fav_tracks))?;
    db.replace_scores("favorite", "album", &build_album_entries(fav_albums))?;
    db.replace_scores("favorite", "artist", &build_artist_entries(fav_artists))?;

    Ok(())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_reco_get_home(
    limitRecentAlbums: Option<u32>,
    limitContinueTracks: Option<u32>,
    limitTopArtists: Option<u32>,
    limitFavorites: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<HomeSeeds, String> {
    crate::reco_store::commands::reco_get_home(
        limitRecentAlbums,
        limitContinueTracks,
        limitTopArtists,
        limitFavorites,
        state,
    )
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_reco_get_home_ml(
    limitRecentAlbums: Option<u32>,
    limitContinueTracks: Option<u32>,
    limitTopArtists: Option<u32>,
    limitFavorites: Option<u32>,
    state: State<'_, RecoState>,
) -> Result<HomeSeeds, String> {
    crate::reco_store::commands::reco_get_home_ml(
        limitRecentAlbums,
        limitContinueTracks,
        limitTopArtists,
        limitFavorites,
        state,
    )
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_reco_get_home_resolved(
    limitRecentAlbums: Option<u32>,
    limitContinueTracks: Option<u32>,
    limitTopArtists: Option<u32>,
    limitFavorites: Option<u32>,
    reco_state: State<'_, RecoState>,
    app_state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<HomeResolved, String> {
    crate::reco_store::commands::reco_get_home_resolved(
        limitRecentAlbums,
        limitContinueTracks,
        limitTopArtists,
        limitFavorites,
        reco_state,
        app_state,
        cache_state,
    )
    .await
}

/// Get album suggestions (similar albums) from Qobuz /album/suggest API
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_get_album_suggestions(
    albumId: String,
    limit: Option<u32>,
    state: State<'_, AppState>,
    blacklist_state: State<'_, BlacklistState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Vec<crate::api::models::Album>, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresCoreBridgeAuth)
        .await
        .map_err(|e| e.to_string())?;

    let client = state.client.read().await.clone();
    let response = client
        .get_album_suggest(&albumId)
        .await
        .map_err(|e| format!("Failed to get album suggestions: {}", e))?;

    let mut albums = response.albums.map(|page| page.items).unwrap_or_default();

    // Apply blacklist
    albums.retain(|album| !blacklist_state.is_blacklisted(album.artist.id));

    let max = limit.unwrap_or(10) as usize;
    albums.truncate(max);

    Ok(albums)
}

/// Get "forgotten" favorite albums — favorites not played in recent N days
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_reco_get_forgotten_favorites(
    limit: Option<u32>,
    recencyDays: Option<u32>,
    reco_state: State<'_, RecoState>,
    app_state: State<'_, AppState>,
    cache_state: State<'_, ApiCacheState>,
) -> Result<Vec<crate::reco_store::AlbumCardMeta>, String> {
    let guard = reco_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session")?;
    let album_ids =
        db.get_forgotten_favorite_album_ids(limit.unwrap_or(12), recencyDays.unwrap_or(30))?;
    drop(guard);

    if album_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Resolve album IDs to metadata using the same 3-tier cache as home
    crate::reco_store::commands::resolve_albums(&album_ids, &reco_state, &app_state, &cache_state)
        .await
}

/// Get user's top genres by play count
#[tauri::command]
pub async fn v2_reco_get_top_genres(
    limit: Option<u32>,
    reco_state: State<'_, RecoState>,
) -> Result<Vec<TopGenre>, String> {
    let guard = reco_state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session")?;
    let genres = db.get_top_genre_ids(limit.unwrap_or(5))?;
    Ok(genres
        .into_iter()
        .map(|(id, name)| TopGenre { id, name })
        .collect())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopGenre {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct V2LibraryCacheStats {
    pub artwork_cache_bytes: u64,
    pub thumbnails_cache_bytes: u64,
    pub artwork_file_count: usize,
    pub thumbnail_file_count: usize,
}

#[tauri::command]
pub async fn v2_library_get_cache_stats() -> Result<V2LibraryCacheStats, String> {
    let artwork_dir = get_artwork_cache_dir();
    let (artwork_bytes, artwork_count) = if artwork_dir.exists() {
        let mut size = 0u64;
        let mut count = 0usize;
        if let Ok(entries) = std::fs::read_dir(&artwork_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        size += meta.len();
                        count += 1;
                    }
                }
            }
        }
        (size, count)
    } else {
        (0, 0)
    };
    let thumbnails_bytes = thumbnails::get_cache_size().unwrap_or(0);
    let thumbnail_count = if let Ok(dir) = thumbnails::get_thumbnails_dir() {
        std::fs::read_dir(&dir).map(|e| e.count()).unwrap_or(0)
    } else {
        0
    };
    Ok(V2LibraryCacheStats {
        artwork_cache_bytes: artwork_bytes,
        thumbnails_cache_bytes: thumbnails_bytes,
        artwork_file_count: artwork_count,
        thumbnail_file_count: thumbnail_count,
    })
}

#[tauri::command]
pub async fn v2_playlist_get_all_settings(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistSettings>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_favorites(state: State<'_, LibraryState>) -> Result<Vec<u64>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_favorite_playlist_ids().map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_local_tracks_with_position(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistLocalTrack>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_local_tracks_with_position(playlistId)
        .map_err(|e| e.to_string())
}

/// Add a Plex track (identified by its Plex ratingKey string) to a
/// Qobuz playlist. Plex tracks live in a parallel playlist_plex_tracks
/// table and never hit the Qobuz playlist API, so there's no risk of
/// the remote playlist getting polluted with unreachable ids.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_add_plex_track(
    playlistId: u64,
    ratingKey: String,
    position: i32,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.add_plex_track_to_playlist(playlistId, &ratingKey, position)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_remove_plex_track(
    playlistId: u64,
    ratingKey: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.remove_plex_track_from_playlist(playlistId, &ratingKey)
        .map_err(|e| e.to_string())
}

/// Return the (ratingKey, position) pairs for every Plex track in a
/// playlist. Caller hydrates metadata (title, artist, cover) from the
/// Plex cache.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_plex_tracks_with_position(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Vec<(String, i32)>, String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_playlist_plex_tracks_with_position(playlistId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_settings(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistSettings>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_settings(playlistId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_get_stats(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<Option<PlaylistStats>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_playlist_stats(playlistId).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_playlist_increment_play_count(
    playlistId: u64,
    state: State<'_, LibraryState>,
) -> Result<PlaylistStats, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.increment_playlist_play_count(playlistId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_all_stats(
    state: State<'_, LibraryState>,
) -> Result<Vec<PlaylistStats>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_playlist_get_all_local_track_counts(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<u64, u32>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_local_track_counts()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_get_playlist_folders(
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::PlaylistFolder>, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.get_all_playlist_folders().map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_create_playlist_folder(
    name: String,
    iconType: Option<String>,
    iconPreset: Option<String>,
    iconColor: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<crate::library::PlaylistFolder, String> {
    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;
    db.create_playlist_folder(
        &name,
        iconType.as_deref(),
        iconPreset.as_deref(),
        iconColor.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_update_playlist_folder(
    id: String,
    name: Option<String>,
    iconType: Option<String>,
    iconPreset: Option<String>,
    iconColor: Option<String>,
    customImagePath: Option<String>,
    isHidden: Option<bool>,
    state: State<'_, LibraryState>,
) -> Result<crate::library::PlaylistFolder, String> {
    crate::library::update_playlist_folder(
        id,
        name,
        iconType,
        iconPreset,
        iconColor,
        customImagePath,
        isHidden,
        state,
    )
    .await
}

#[tauri::command]
pub async fn v2_library_get_albums(
    include_hidden: Option<bool>,
    exclude_network_folders: Option<bool>,
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, DownloadSettingsState>,
) -> Result<Vec<LocalAlbum>, String> {
    let include_qobuz = download_settings_state
        .lock()
        .map_err(|e| format!("Failed to lock download settings: {}", e))?
        .as_ref()
        .and_then(|s| s.get_settings().ok())
        .map(|s| s.show_in_library)
        .unwrap_or(false);

    let guard__ = state.db.lock().await;
    let db = guard__
        .as_ref()
        .ok_or("No active session - please log in")?;

    db.get_albums_with_full_filter(
        include_hidden.unwrap_or(false),
        include_qobuz,
        exclude_network_folders.unwrap_or(false),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_library_get_stats(
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, DownloadSettingsState>,
) -> Result<crate::library::LibraryStats, String> {
    crate::library::library_get_stats(state, download_settings_state).await
}

#[tauri::command]
pub async fn v2_library_get_folders(state: State<'_, LibraryState>) -> Result<Vec<String>, String> {
    crate::library::library_get_folders(state).await
}

#[tauri::command]
pub async fn v2_library_get_folders_with_metadata(
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::LibraryFolder>, String> {
    crate::library::library_get_folders_with_metadata(state).await
}

#[tauri::command]
pub async fn v2_library_add_folder(
    path: String,
    state: State<'_, LibraryState>,
) -> Result<crate::library::LibraryFolder, String> {
    crate::library::library_add_folder(path, state).await
}

#[tauri::command]
pub async fn v2_library_cleanup_missing_files(
    state: State<'_, LibraryState>,
) -> Result<crate::library::CleanupResult, String> {
    crate::library::library_cleanup_missing_files(state).await
}

#[tauri::command]
pub async fn v2_library_fetch_missing_artwork(
    state: State<'_, LibraryState>,
) -> Result<u32, String> {
    crate::library::library_fetch_missing_artwork(state).await
}

#[tauri::command]
pub async fn v2_library_get_artists(
    exclude_network_folders: Option<bool>,
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, DownloadSettingsState>,
) -> Result<Vec<crate::library::LocalArtist>, String> {
    crate::library::library_get_artists(exclude_network_folders, state, download_settings_state)
        .await
}

#[tauri::command]
pub async fn v2_library_search(
    query: String,
    limit: Option<u32>,
    exclude_network_folders: Option<bool>,
    state: State<'_, LibraryState>,
    download_settings_state: State<'_, DownloadSettingsState>,
) -> Result<Vec<crate::library::LocalTrack>, String> {
    crate::library::library_search(
        query,
        limit,
        exclude_network_folders,
        state,
        download_settings_state,
    )
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_library_get_album_tracks(
    albumGroupKey: String,
    state: State<'_, LibraryState>,
) -> Result<Vec<crate::library::LocalTrack>, String> {
    crate::library::library_get_album_tracks(albumGroupKey, state).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_get_music_sections(
    baseUrl: String,
    token: String,
) -> Result<Vec<PlexMusicSection>, String> {
    crate::plex::plex_get_music_sections(baseUrl, token).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_get_section_tracks(
    baseUrl: String,
    token: String,
    sectionKey: String,
    limit: Option<u32>,
) -> Result<Vec<PlexTrack>, String> {
    crate::plex::plex_get_section_tracks(baseUrl, token, sectionKey, limit).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn v2_plex_play_track(
    baseUrl: String,
    token: String,
    ratingKey: String,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<PlexPlayResult, String> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresClientInit)
        .await
        .map_err(|e| e.to_string())?;

    let resolved = crate::plex::plex_resolve_track_media(baseUrl, token, ratingKey).await?;
    let bridge_guard = bridge.get().await;
    let player = bridge_guard.player();

    player
        .play_data(resolved.bytes.clone(), resolved.playback_id)
        .map_err(|e| format!("Failed to play Plex track via V2 player: {}", e))?;

    Ok(PlexPlayResult {
        rating_key: resolved.rating_key,
        part_key: resolved.part_key,
        part_url: resolved.part_url,
        bytes: resolved.bytes.len(),
        direct_play_confirmed: resolved.direct_play_confirmed,
        content_type: resolved.content_type,
        sampling_rate_hz: resolved.sampling_rate_hz,
        bit_depth: resolved.bit_depth,
    })
}

#[tauri::command]
pub async fn v2_library_update_folder_path(
    id: i64,
    new_path: String,
    state: State<'_, LibraryState>,
) -> Result<crate::library::LibraryFolder, String> {
    let path_ref = std::path::Path::new(&new_path);
    if !path_ref.exists() {
        return Err("The selected folder does not exist".to_string());
    }
    if !path_ref.is_dir() {
        return Err("The selected path is not a folder".to_string());
    }

    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.update_folder_path(id, &new_path)
        .map_err(|e| e.to_string())?;

    let network_info = crate::network::is_network_path(path_ref);
    if network_info.is_network {
        let fs_type = network_info.mount_info.as_ref().and_then(|mi| {
            if let crate::network::MountKind::Network(nfs) = &mi.kind {
                Some(format!("{:?}", nfs).to_lowercase())
            } else {
                None
            }
        });
        if let Some(folder) = db.get_folder_by_id(id).map_err(|e| e.to_string())? {
            db.update_folder_settings(
                id,
                folder.alias.as_deref(),
                folder.enabled,
                true,
                fs_type.as_deref(),
                false,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    db.get_folder_by_id(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Folder not found after update".to_string())
}

#[tauri::command]
pub async fn v2_library_cache_artist_image(
    artist_name: String,
    image_url: String,
    source: String,
    canonical_name: Option<String>,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.cache_artist_image_with_canonical(
        &artist_name,
        Some(&image_url),
        &source,
        None,
        canonical_name.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CustomArtistImageResult {
    pub image_path: String,
    pub thumbnail_path: String,
}

#[tauri::command]
pub async fn v2_library_set_custom_artist_image(
    artist_name: String,
    custom_image_path: String,
    state: State<'_, LibraryState>,
) -> Result<CustomArtistImageResult, String> {
    let artwork_dir = get_artwork_cache_dir();
    let source = std::path::Path::new(&custom_image_path);
    if !source.exists() {
        return Err(format!(
            "Source image does not exist: {}",
            custom_image_path
        ));
    }

    // Validate extension
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !["png", "jpg", "jpeg", "webp"].contains(&extension.as_str()) {
        return Err(format!(
            "Unsupported image format: {}. Use png, jpg, jpeg, or webp.",
            extension
        ));
    }

    // Generate filename using MD5 hash of artist name
    let mut hasher = Md5::new();
    hasher.update(artist_name.as_bytes());
    let artist_hash = format!("{:x}", hasher.finalize());
    let timestamp = chrono::Utc::now().timestamp();
    let filename = format!("artist_custom_{}_{}.jpg", artist_hash, timestamp);
    let dest_path = artwork_dir.join(&filename);

    // Decode, resize to max 1000x1000, save as JPEG
    let img = image::ImageReader::open(source)
        .map_err(|e| format!("Failed to open image: {}", e))?
        .decode()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    let resized = img.resize(1000, 1000, image::imageops::FilterType::Lanczos3);
    resized
        .save(&dest_path)
        .map_err(|e| format!("Failed to save resized image: {}", e))?;

    // Generate 500x500 thumbnail using qbz-library
    let thumbnail_path = thumbnails::generate_thumbnail(&dest_path)
        .map_err(|e| format!("Failed to generate thumbnail: {}", e))?;

    // Update database
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.cache_artist_image(
        &artist_name,
        None,
        "custom",
        Some(&dest_path.to_string_lossy()),
    )
    .map_err(|e| e.to_string())?;

    Ok(CustomArtistImageResult {
        image_path: dest_path.to_string_lossy().into_owned(),
        thumbnail_path: thumbnail_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn v2_library_remove_custom_artist_image(
    artist_name: String,
    state: State<'_, LibraryState>,
) -> Result<(), String> {
    // Get current info to find file paths to delete
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    let info = db
        .get_artist_image(&artist_name)
        .map_err(|e| e.to_string())?;

    if let Some(info) = info {
        // Delete custom image file if it exists
        if let Some(ref path) = info.custom_image_path {
            let p = std::path::Path::new(path);
            if p.exists() {
                // Also remove thumbnail
                if let Ok(thumb) = thumbnails::get_thumbnail_path(p) {
                    let _ = std::fs::remove_file(thumb);
                }
                let _ = std::fs::remove_file(p);
            }
        }

        // Reset to original image (clear custom_image_path, keep image_url)
        db.cache_artist_image(
            &artist_name,
            info.image_url.as_deref(),
            info.source.as_deref().unwrap_or("qobuz"),
            None,
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn v2_library_get_artist_image(
    artist_name: String,
    state: State<'_, LibraryState>,
) -> Result<Option<crate::library::ArtistImageInfo>, String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_artist_image(&artist_name).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn v2_library_get_all_custom_artist_images(
    state: State<'_, LibraryState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let guard = state.db.lock().await;
    let db = guard.as_ref().ok_or("No active session - please log in")?;
    db.get_all_custom_artist_images().map_err(|e| e.to_string())
}
