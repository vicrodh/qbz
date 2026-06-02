//! Frontend-agnostic renderer-side pure helpers (slice 6).
//!
//! Pure protocol/format math used by the renderer orchestration (queue
//! materialize / cursor-align). No engine, no I/O, no Tauri. Relocated here so
//! both the Tauri adapter and the Slint adapter share one definition; the
//! src-tauri side re-exports these. The load-dedup predicates and the
//! audio-quality report helpers move here alongside their orchestration /
//! report consumers in the later slice-6 steps.

use qbz_models::{QueueTrack, RepeatMode, Track};

/// Source tag stamped on remote queue tracks materialized from a QConnect cloud
/// queue. Matches the Tauri adapter's prior `QCONNECT_REMOTE_QUEUE_SOURCE`.
pub const QCONNECT_REMOTE_QUEUE_SOURCE: &str = "qobuz_connect_remote";

pub fn qconnect_repeat_mode_from_loop_mode(loop_mode: i32) -> Option<RepeatMode> {
    // QConnect protocol loop mode values: 1 = off, 2 = repeat one, 3 = repeat all.
    match loop_mode {
        0 | 1 => Some(RepeatMode::Off),
        2 => Some(RepeatMode::One),
        3 => Some(RepeatMode::All),
        _ => None,
    }
}

pub fn normalize_volume_to_fraction(volume: i32) -> f32 {
    volume.clamp(0, 100) as f32 / 100.0
}

pub fn model_track_to_core_queue_track(track: &Track) -> QueueTrack {
    let artwork_url = track
        .album
        .as_ref()
        .and_then(|album| album.image.best().cloned());
    let artist = track
        .performer
        .as_ref()
        .map(|performer| performer.name.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let album = track
        .album
        .as_ref()
        .map(|album| album.title.clone())
        .unwrap_or_else(|| "Unknown Album".to_string());
    let album_id = track.album.as_ref().and_then(|album| {
        let trimmed = album.id.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let artist_id = track.performer.as_ref().map(|performer| performer.id);

    QueueTrack {
        id: track.id,
        title: track.title.clone(),
        version: track.version.clone(),
        artist,
        album,
        duration_secs: track.duration as u64,
        artwork_url,
        hires: track.hires,
        bit_depth: track.maximum_bit_depth,
        sample_rate: track.maximum_sampling_rate,
        is_local: false,
        album_id: album_id.clone(),
        artist_id,
        streamable: track.streamable,
        source: Some(QCONNECT_REMOTE_QUEUE_SOURCE.to_string()),
        parental_warning: track.parental_warning,
        source_item_id_hint: album_id,
    }
}
