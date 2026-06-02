//! Session topology: renderer registry, active/local renderer tracking,
//! per-renderer cached state, and renderer-state snapshot construction
//! used by the rest of the QConnect module to reason about who owns
//! playback and what the cloud's view of each renderer looks like.



use qconnect_app::{QConnectQueueState, QConnectRendererState};
use qconnect_core::QueueItem;
use serde::Serialize;

use super::queue_resolution::{
    find_cursor_index_by_queue_item_id, normalize_current_queue_item_id_from_queue_state,
    ordered_queue_cursors, QconnectOrderedQueueCursor,
};
use super::transport::default_qconnect_device_info;
use super::{
    QconnectQueueVersionPayload, QconnectRemoteSyncState, QconnectVisibleQueueProjection,
};

/// `ServerActiveState`, `ConnectionDecision`, and `compute_connection_state`
/// now live in the frontend-agnostic `qconnect_app::session` module. Re-exported
/// here so existing `super::session::…` references compile unchanged.
/// (`ConnectionDecision` is the return type — consumed via field access, so it
/// is not named directly on the Tauri side; it lives in `qconnect_app`.)
pub(super) use qconnect_app::{compute_connection_state, ServerActiveState};

/// Session topology types now live in the frontend-agnostic `qconnect_app::session`
/// module (slice 2+4). Re-exported here so existing `super::session::…` /
/// `super::…` references inside this module compile unchanged, and so the Tauri
/// command surface keeps the same serialized shape.
pub use qconnect_app::{QconnectRendererInfo, QconnectSessionRendererState, QconnectSessionState};

/// Pure session mutators + the file-audio-quality snapshot type also moved to
/// qconnect-app (slice 2+4). Re-exported so existing `super::session::…` /
/// `super::…` references compile unchanged.
pub(super) use qconnect_app::{
    find_unique_renderer_id, is_local_renderer_active, is_peer_renderer_active,
    normalize_active_renderer_id, renderer_allows_remote_volume, QconnectFileAudioQualitySnapshot,
};

pub(super) fn queue_item_snapshot_for_cursor(
    queue: &QConnectQueueState,
    cursor: QconnectOrderedQueueCursor,
) -> Option<QueueItem> {
    match cursor {
        QconnectOrderedQueueCursor::Queue(index) => {
            queue.queue_items.get(index).cloned().map(|mut item| {
                item.queue_item_id = normalize_current_queue_item_id_from_queue_state(queue, index);
                item
            })
        }
        QconnectOrderedQueueCursor::Autoplay(index) => queue.autoplay_items.get(index).cloned(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QconnectRendererReportDebugEvent {
    pub(super) requested_current_queue_item_id: Option<i32>,
    pub(super) requested_next_queue_item_id: Option<i32>,
    pub(super) resolved_current_queue_item_id: Option<i32>,
    pub(super) resolved_next_queue_item_id: Option<i32>,
    pub(super) sent_current_queue_item_id: Option<i32>,
    pub(super) sent_next_queue_item_id: Option<i32>,
    pub(super) report_queue_item_ids: bool,
    pub(super) current_track_id: Option<i64>,
    pub(super) playing_state: i32,
    pub(super) current_position: Option<i32>,
    pub(super) duration: Option<i32>,
    pub(super) queue_version: QconnectQueueVersionPayload,
    pub(super) resolution_strategy: String,
}

/// Thin Tauri wrapper: resolves this device's identity (uuid + device-info)
/// adapter-side, then delegates to the frontend-agnostic resolver in
/// qconnect-app. qconnect-app stays identity-free. The eager device-info build
/// is idempotent and side-effect-free (env reads + the cached uuid), so the
/// resolved renderer id is identical to the prior in-place implementation.
pub(super) fn refresh_local_renderer_id(session: &mut QconnectSessionState) {
    let info = default_qconnect_device_info();
    let identity = qconnect_app::LocalIdentity {
        device_uuid: info.device_uuid.unwrap_or_default(),
        friendly_name: info.friendly_name,
        brand: info.brand,
        model: info.model,
        device_type: info.device_type,
    };
    qconnect_app::refresh_local_renderer_id(session, &identity);
}

pub(super) fn sync_session_renderer_active_flags(state: &mut QconnectRemoteSyncState) {
    for (renderer_id, renderer_state) in &mut state.session_renderer_states {
        renderer_state.active = state
            .session
            .active_renderer_id
            .map(|active_renderer_id| active_renderer_id == *renderer_id);
    }
}

pub(super) fn ensure_session_renderer_state(
    state: &mut QconnectRemoteSyncState,
    renderer_id: i32,
) -> &mut QconnectSessionRendererState {
    let active = state
        .session
        .active_renderer_id
        .map(|active_renderer_id| active_renderer_id == renderer_id);
    state
        .session_renderer_states
        .entry(renderer_id)
        .or_insert_with(|| QconnectSessionRendererState {
            active,
            ..Default::default()
        })
}

pub(super) fn build_session_renderer_snapshot(
    queue: &QConnectQueueState,
    renderer_state: Option<&QconnectSessionRendererState>,
    session_loop_mode: Option<i32>,
) -> QConnectRendererState {
    let renderer_state = renderer_state.cloned().unwrap_or_default();
    let cursors = ordered_queue_cursors(queue);
    let current_index =
        find_cursor_index_by_queue_item_id(&cursors, queue, renderer_state.current_queue_item_id);
    let current_track =
        current_index.and_then(|index| queue_item_snapshot_for_cursor(queue, cursors[index]));
    let next_track = current_index
        .and_then(|index| cursors.get(index + 1).copied())
        .and_then(|cursor| queue_item_snapshot_for_cursor(queue, cursor));

    QConnectRendererState {
        active: renderer_state.active,
        playing_state: renderer_state.playing_state,
        current_position_ms: renderer_state.current_position_ms,
        current_track,
        next_track,
        volume: renderer_state.volume,
        volume_delta: None,
        muted: renderer_state.muted,
        max_audio_quality: renderer_state.max_audio_quality,
        loop_mode: renderer_state.loop_mode.or(session_loop_mode),
        shuffle_mode: renderer_state.shuffle_mode,
        updated_at_ms: renderer_state.updated_at_ms,
    }
}

pub(super) fn build_effective_renderer_snapshot(
    queue: &QConnectQueueState,
    base_renderer_state: &QConnectRendererState,
    session_renderer_state: Option<&QconnectSessionRendererState>,
    session_loop_mode: Option<i32>,
) -> QConnectRendererState {
    let mut renderer_snapshot = base_renderer_state.clone();

    if let Some(session_renderer_state) = session_renderer_state {
        if let Some(active) = session_renderer_state.active {
            renderer_snapshot.active = Some(active);
        }
        if let Some(playing_state) = session_renderer_state.playing_state {
            renderer_snapshot.playing_state = Some(playing_state);
        }
        if let Some(current_position_ms) = session_renderer_state.current_position_ms {
            renderer_snapshot.current_position_ms = Some(current_position_ms);
        }
        if let Some(volume) = session_renderer_state.volume {
            renderer_snapshot.volume = Some(volume);
        }
        if let Some(muted) = session_renderer_state.muted {
            renderer_snapshot.muted = Some(muted);
        }
        if let Some(max_audio_quality) = session_renderer_state.max_audio_quality {
            renderer_snapshot.max_audio_quality = Some(max_audio_quality);
        }
        if let Some(loop_mode) = session_renderer_state.loop_mode.or(session_loop_mode) {
            renderer_snapshot.loop_mode = Some(loop_mode);
        }
        if let Some(shuffle_mode) = session_renderer_state.shuffle_mode {
            renderer_snapshot.shuffle_mode = Some(shuffle_mode);
        }
        if session_renderer_state.updated_at_ms > 0 {
            renderer_snapshot.updated_at_ms = session_renderer_state.updated_at_ms;
        }

        if session_renderer_state.current_queue_item_id.is_some() {
            let session_snapshot = build_session_renderer_snapshot(
                queue,
                Some(session_renderer_state),
                session_loop_mode,
            );
            if session_snapshot.current_track.is_some() {
                renderer_snapshot.current_track = session_snapshot.current_track;
                renderer_snapshot.next_track = session_snapshot.next_track;
            }
        }
    } else if let Some(loop_mode) = session_loop_mode {
        renderer_snapshot.loop_mode = Some(loop_mode);
    }

    renderer_snapshot
}

pub(super) fn build_visible_queue_projection(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
) -> QconnectVisibleQueueProjection {
    use super::queue_resolution::find_cursor_index_by_track_id;

    let cursors = ordered_queue_cursors(queue);

    let current_index = find_cursor_index_by_queue_item_id(
        &cursors,
        queue,
        renderer
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id),
    )
    .or_else(|| {
        find_cursor_index_by_track_id(
            &cursors,
            queue,
            renderer.current_track.as_ref().map(|item| item.track_id),
        )
    });

    let next_index = find_cursor_index_by_queue_item_id(
        &cursors,
        queue,
        renderer.next_track.as_ref().map(|item| item.queue_item_id),
    )
    .or_else(|| {
        find_cursor_index_by_track_id(
            &cursors,
            queue,
            renderer.next_track.as_ref().map(|item| item.track_id),
        )
    });

    let (current_track, start_index) = if let Some(index) = current_index {
        (
            queue_item_snapshot_for_cursor(queue, cursors[index]),
            index.saturating_add(1),
        )
    } else if let Some(index) = next_index {
        let inferred_current = index
            .checked_sub(1)
            .and_then(|current_index| cursors.get(current_index).copied())
            .and_then(|cursor| queue_item_snapshot_for_cursor(queue, cursor));
        (inferred_current, index)
    } else {
        (None, 0)
    };

    let upcoming_tracks: Vec<QueueItem> = cursors
        .into_iter()
        .skip(start_index)
        .filter_map(|cursor| queue_item_snapshot_for_cursor(queue, cursor))
        .collect();

    QconnectVisibleQueueProjection {
        current_track,
        upcoming_tracks,
    }
}

pub(super) fn cache_renderer_snapshot(
    sync_state: &mut QconnectRemoteSyncState,
    renderer_snapshot: &QConnectRendererState,
) {
    sync_state.last_renderer_queue_item_id = renderer_snapshot
        .current_track
        .as_ref()
        .map(|item| item.queue_item_id);
    sync_state.last_renderer_next_queue_item_id = renderer_snapshot
        .next_track
        .as_ref()
        .map(|item| item.queue_item_id);
    sync_state.last_renderer_track_id = renderer_snapshot
        .current_track
        .as_ref()
        .map(|item| item.track_id);
    sync_state.last_renderer_next_track_id = renderer_snapshot
        .next_track
        .as_ref()
        .map(|item| item.track_id);
    sync_state.last_renderer_playing_state = renderer_snapshot.playing_state;
}
