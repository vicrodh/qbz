//! Session topology: renderer registry, active/local renderer tracking,
//! per-renderer cached state, and renderer-state snapshot construction
//! used by the rest of the QConnect module to reason about who owns
//! playback and what the cloud's view of each renderer looks like.



use qconnect_app::{QConnectQueueState, QConnectRendererState};
use qconnect_core::QueueItem;
use serde::{Deserialize, Serialize};

use super::queue_resolution::{
    find_cursor_index_by_queue_item_id, normalize_current_queue_item_id_from_queue_state,
    ordered_queue_cursors, QconnectOrderedQueueCursor,
};
use super::{
    default_qconnect_device_info, resolve_qconnect_device_uuid, QconnectQueueVersionPayload,
    QconnectRemoteSyncState, QconnectVisibleQueueProjection,
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

#[derive(Debug, Clone, Default)]
pub(crate) struct QconnectSessionRendererState {
    pub(super) active: Option<bool>,
    pub(super) playing_state: Option<i32>,
    pub(super) current_position_ms: Option<u64>,
    pub(super) current_queue_item_id: Option<u64>,
    pub(super) volume: Option<i32>,
    pub(super) muted: Option<bool>,
    pub(super) max_audio_quality: Option<i32>,
    pub(super) loop_mode: Option<i32>,
    pub(super) shuffle_mode: Option<bool>,
    pub(super) updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QconnectFileAudioQualitySnapshot {
    pub(super) sampling_rate: i32,
    pub(super) bit_depth: i32,
    pub(super) nb_channels: i32,
    pub(super) audio_quality: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QconnectSessionState {
    pub session_uuid: Option<String>,
    pub active_renderer_id: Option<i32>,
    pub local_renderer_id: Option<i32>,
    pub renderers: Vec<QconnectRendererInfo>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QconnectRendererInfo {
    pub renderer_id: i32,
    pub device_uuid: Option<String>,
    pub friendly_name: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub device_type: Option<i32>,
}

pub(super) fn refresh_local_renderer_id(session: &mut QconnectSessionState) {
    let local_device_uuid = resolve_qconnect_device_uuid();
    if let Some(renderer_id) = session
        .renderers
        .iter()
        .find(|renderer| renderer.device_uuid.as_deref() == Some(local_device_uuid.as_str()))
        .map(|renderer| renderer.renderer_id)
    {
        session.local_renderer_id = Some(renderer_id);
        return;
    }

    let local_device_info = default_qconnect_device_info();
    let local_friendly_name = local_device_info.friendly_name.as_deref();
    let local_brand = local_device_info.brand.as_deref();
    let local_model = local_device_info.model.as_deref();
    let local_device_type = local_device_info.device_type;

    // Some server ADD_RENDERER payloads omit device_uuid for the local renderer.
    // Fall back to a unique device fingerprint so controller-side handoff logic
    // can still distinguish local vs peer renderers.
    if let Some(renderer_id) = find_unique_renderer_id(session, |renderer| {
        renderer.friendly_name.as_deref() == local_friendly_name
            && renderer.brand.as_deref() == local_brand
            && renderer.model.as_deref() == local_model
            && renderer.device_type == local_device_type
    }) {
        session.local_renderer_id = Some(renderer_id);
        return;
    }

    if let Some(renderer_id) = find_unique_renderer_id(session, |renderer| {
        renderer.friendly_name.as_deref() == local_friendly_name
            && renderer.device_type == local_device_type
    }) {
        session.local_renderer_id = Some(renderer_id);
        return;
    }

    session.local_renderer_id = None;
}

pub(super) fn normalize_active_renderer_id(value: Option<i64>) -> Option<i32> {
    value
        .filter(|renderer_id| *renderer_id >= 0)
        .and_then(|renderer_id| i32::try_from(renderer_id).ok())
}

pub(super) fn is_peer_renderer_active(session: &QconnectSessionState) -> bool {
    match (session.active_renderer_id, session.local_renderer_id) {
        (Some(active_renderer_id), Some(local_renderer_id)) => {
            active_renderer_id != local_renderer_id
        }
        _ => false,
    }
}

pub(super) fn is_local_renderer_active(session: &QconnectSessionState) -> bool {
    match (session.active_renderer_id, session.local_renderer_id) {
        (Some(active_renderer_id), Some(local_renderer_id)) => {
            active_renderer_id == local_renderer_id
        }
        _ => false,
    }
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

pub(super) fn find_unique_renderer_id(
    session: &QconnectSessionState,
    predicate: impl Fn(&QconnectRendererInfo) -> bool,
) -> Option<i32> {
    let mut matches = session
        .renderers
        .iter()
        .filter(|renderer| predicate(renderer))
        .map(|renderer| renderer.renderer_id);

    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }

    Some(first)
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
    use super::find_cursor_index_by_track_id;

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
