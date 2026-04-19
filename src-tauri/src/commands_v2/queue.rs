use tauri::State;

use qbz_models::{QueueState, QueueTrack as CoreQueueTrack, RepeatMode};
use qconnect_app::QueueCommandType;
use qconnect_app::{QConnectQueueState, QConnectRendererState};

use crate::core_bridge::CoreBridgeState;
use crate::qconnect_service::{QconnectServiceState, QconnectVisibleQueueProjection};
use crate::runtime::{CommandRequirement, RuntimeError, RuntimeManagerState};

// ==================== Queue Commands (V2) ====================

/// Get current queue state (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_get_queue_state(
    bridge: State<'_, CoreBridgeState>,
) -> Result<QueueState, RuntimeError> {
    let bridge = bridge.get().await;
    Ok(bridge.get_queue_state().await)
}

/// Full queue snapshot for session persistence (no caps on track count)
#[derive(serde::Serialize)]
pub struct AllQueueTracksResponse {
    pub tracks: Vec<CoreQueueTrack>,
    pub current_index: Option<usize>,
}

/// Get all queue tracks and current index (for session persistence, no caps)
#[tauri::command]
pub async fn v2_get_all_queue_tracks(
    bridge: State<'_, CoreBridgeState>,
) -> Result<AllQueueTracksResponse, RuntimeError> {
    let bridge = bridge.get().await;
    let (tracks, current_index) = bridge.get_all_queue_tracks().await;
    Ok(AllQueueTracksResponse {
        tracks,
        current_index,
    })
}

/// Get currently selected queue track (V2)
#[tauri::command]
pub async fn v2_get_current_queue_track(
    bridge: State<'_, CoreBridgeState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    let bridge = bridge.get().await;
    let state = bridge.get_queue_state().await;
    Ok(state.current_track.map(Into::into))
}

/// Set repeat mode (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_set_repeat_mode(
    mode: RepeatMode,
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    if qconnect.status().await.transport_connected {
        qconnect
            .send_command(
                QueueCommandType::CtrlSrvrSetLoopMode,
                serde_json::json!({
                    "loop_mode": qconnect_loop_mode_from_repeat_mode(mode),
                }),
            )
            .await
            .map_err(RuntimeError::Internal)?;
        return Ok(());
    }

    let bridge = bridge.get().await;
    bridge.set_repeat_mode(mode).await;
    Ok(())
}

/// Toggle shuffle (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_toggle_shuffle(
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<bool, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    if qconnect.status().await.transport_connected {
        let queue = qconnect
            .queue_snapshot()
            .await
            .map_err(RuntimeError::Internal)?;
        let next_enabled = !queue.shuffle_mode;
        apply_qconnect_shuffle_mode(qconnect.inner(), &queue, next_enabled).await?;
        return Ok(next_enabled);
    }

    let bridge = bridge.get().await;
    Ok(bridge.toggle_shuffle().await)
}

/// Set shuffle mode directly (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_set_shuffle(
    enabled: bool,
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] set_shuffle: {}", enabled);

    if qconnect.status().await.transport_connected {
        let queue = qconnect
            .queue_snapshot()
            .await
            .map_err(RuntimeError::Internal)?;
        apply_qconnect_shuffle_mode(qconnect.inner(), &queue, enabled).await?;
        return Ok(());
    }

    let bridge = bridge.get().await;
    bridge.set_shuffle(enabled).await;
    Ok(())
}

/// Clear the queue (V2 - uses QbzCore)
#[tauri::command]
pub async fn v2_clear_queue(
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;

    if qconnect.status().await.transport_connected {
        qconnect
            .send_command(QueueCommandType::CtrlSrvrClearQueue, serde_json::json!({}))
            .await
            .map_err(RuntimeError::Internal)?;
        return Ok(());
    }

    let bridge = bridge.get().await;
    bridge.clear_queue().await;
    // Queue replaced — Mixtape context is no longer valid.
    runtime.manager().set_queue_source_collection(None).await;
    Ok(())
}

async fn apply_qconnect_shuffle_mode(
    qconnect: &QconnectServiceState,
    queue: &QConnectQueueState,
    enabled: bool,
) -> Result<(), RuntimeError> {
    let renderer = qconnect.renderer_snapshot().await.unwrap_or_default();
    let shuffle_seed = enabled.then(|| rand::random::<u32>() & (i32::MAX as u32));
    let pivot_queue_item_id = resolve_qconnect_shuffle_pivot(queue, &renderer);

    qconnect
        .send_command(
            QueueCommandType::CtrlSrvrSetShuffleMode,
            serde_json::json!({
                "shuffle_mode": enabled,
                "shuffle_seed": shuffle_seed.map(i64::from),
                "shuffle_pivot_queue_item_id": pivot_queue_item_id
                    .and_then(|value| i32::try_from(value).ok())
                    .map(i64::from),
                "autoplay_reset": false,
                "autoplay_loading": false,
            }),
        )
        .await
        .map_err(RuntimeError::Internal)?;
    Ok(())
}

fn qconnect_queue_item_id_to_wire_value(queue_item_id: u64) -> Result<i64, RuntimeError> {
    i64::try_from(queue_item_id)
        .map_err(|_| RuntimeError::Internal("queue_item_id out of range".to_string()))
}

fn build_qconnect_remove_upcoming_payload(
    projection: &QconnectVisibleQueueProjection,
    upcoming_index: usize,
) -> Result<Option<serde_json::Value>, RuntimeError> {
    let Some(queue_item) = projection.upcoming_tracks.get(upcoming_index) else {
        return Ok(None);
    };

    Ok(Some(serde_json::json!({
        "queue_item_ids": [qconnect_queue_item_id_to_wire_value(queue_item.queue_item_id)?],
        "autoplay_reset": false,
        "autoplay_loading": false,
    })))
}

fn build_qconnect_reorder_payload(
    projection: &QconnectVisibleQueueProjection,
    from_index: usize,
    to_index: usize,
) -> Result<Option<serde_json::Value>, RuntimeError> {
    let upcoming_len = projection.upcoming_tracks.len();
    if from_index >= upcoming_len || to_index >= upcoming_len {
        return Ok(None);
    }
    if from_index == to_index {
        return Ok(Some(serde_json::json!({})));
    }

    let mut remaining_queue_item_ids: Vec<u64> = projection
        .upcoming_tracks
        .iter()
        .map(|item| item.queue_item_id)
        .collect();
    let moved_queue_item_id = remaining_queue_item_ids.remove(from_index);
    let insert_position = if from_index < to_index {
        to_index.saturating_sub(1)
    } else {
        to_index
    };
    let insert_after = if insert_position == 0 {
        projection
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id)
    } else {
        remaining_queue_item_ids.get(insert_position - 1).copied()
    };

    Ok(Some(serde_json::json!({
        "queue_item_ids": [qconnect_queue_item_id_to_wire_value(moved_queue_item_id)?],
        "insert_after": insert_after
            .map(qconnect_queue_item_id_to_wire_value)
            .transpose()?,
        "autoplay_reset": false,
        "autoplay_loading": false,
    })))
}

fn qconnect_loop_mode_from_repeat_mode(mode: RepeatMode) -> i32 {
    // QConnect protocol loop mode values:
    // 1 = off, 2 = repeat one, 3 = repeat all.
    match mode {
        RepeatMode::Off => 1,
        RepeatMode::All => 3,
        RepeatMode::One => 2,
    }
}

fn resolve_qconnect_shuffle_pivot(
    queue: &QConnectQueueState,
    renderer: &QConnectRendererState,
) -> Option<u64> {
    let Some(current_track) = renderer.current_track.as_ref() else {
        return None;
    };

    if queue
        .queue_items
        .iter()
        .position(|item| item.queue_item_id == current_track.queue_item_id)
        .is_some()
    {
        return Some(current_track.queue_item_id);
    }

    if let Some((_, item)) = queue
        .queue_items
        .iter()
        .enumerate()
        .find(|(_, item)| item.track_id == current_track.track_id)
    {
        return Some(item.queue_item_id);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        build_qconnect_remove_upcoming_payload, build_qconnect_reorder_payload,
        qconnect_loop_mode_from_repeat_mode, resolve_qconnect_shuffle_pivot,
    };
    use crate::qconnect_service::QconnectVisibleQueueProjection;
    use qbz_models::RepeatMode;
    use qconnect_app::{QConnectQueueState, QConnectRendererState};
    use qconnect_core::QueueItem;
    use serde_json::json;

    fn item(queue_item_id: u64, track_id: u64) -> QueueItem {
        QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id,
            queue_item_id,
        }
    }

    #[test]
    fn maps_repeat_mode_to_qconnect_loop_mode() {
        assert_eq!(qconnect_loop_mode_from_repeat_mode(RepeatMode::Off), 1);
        assert_eq!(qconnect_loop_mode_from_repeat_mode(RepeatMode::All), 3);
        assert_eq!(qconnect_loop_mode_from_repeat_mode(RepeatMode::One), 2);
    }

    #[test]
    fn resolves_shuffle_pivot_from_renderer_queue_item_id() {
        let queue = QConnectQueueState {
            queue_items: vec![item(10, 100), item(11, 101), item(12, 102)],
            ..Default::default()
        };
        let renderer = QConnectRendererState {
            current_track: Some(item(11, 101)),
            ..Default::default()
        };

        let queue_item_id = resolve_qconnect_shuffle_pivot(&queue, &renderer);
        assert_eq!(queue_item_id, Some(11));
    }

    #[test]
    fn resolves_shuffle_pivot_by_track_id_when_renderer_qid_is_placeholder() {
        let queue = QConnectQueueState {
            queue_items: vec![item(20, 200), item(21, 201), item(22, 202)],
            ..Default::default()
        };
        let renderer = QConnectRendererState {
            current_track: Some(item(0, 202)),
            ..Default::default()
        };

        let queue_item_id = resolve_qconnect_shuffle_pivot(&queue, &renderer);
        assert_eq!(queue_item_id, Some(22));
    }

    #[test]
    fn remove_upcoming_payload_uses_queue_item_id_from_projection() {
        let projection = QconnectVisibleQueueProjection {
            current_track: Some(item(0, 100)),
            upcoming_tracks: vec![item(7, 107), item(8, 108)],
        };

        let payload =
            build_qconnect_remove_upcoming_payload(&projection, 1).expect("payload build");

        assert_eq!(
            payload,
            Some(json!({
                "queue_item_ids": [8],
                "autoplay_reset": false,
                "autoplay_loading": false,
            })),
        );
    }

    #[test]
    fn reorder_payload_moves_track_before_drop_target_using_current_anchor() {
        let projection = QconnectVisibleQueueProjection {
            current_track: Some(item(0, 100)),
            upcoming_tracks: vec![item(1, 101), item(2, 102), item(3, 103), item(4, 104)],
        };

        let payload = build_qconnect_reorder_payload(&projection, 0, 3).expect("payload build");

        assert_eq!(
            payload,
            Some(json!({
                "queue_item_ids": [1],
                "insert_after": 3,
                "autoplay_reset": false,
                "autoplay_loading": false,
            })),
        );
    }

    #[test]
    fn reorder_payload_can_move_track_to_first_upcoming_slot() {
        let projection = QconnectVisibleQueueProjection {
            current_track: Some(item(0, 100)),
            upcoming_tracks: vec![item(1, 101), item(2, 102), item(3, 103), item(4, 104)],
        };

        let payload = build_qconnect_reorder_payload(&projection, 3, 0).expect("payload build");

        assert_eq!(
            payload,
            Some(json!({
                "queue_item_ids": [4],
                "insert_after": 0,
                "autoplay_reset": false,
                "autoplay_loading": false,
            })),
        );
    }
}

/// Queue track representation for V2 commands
/// Maps to internal QueueTrack format
/// Field names match frontend BackendQueueTrack interface exactly
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct V2QueueTrack {
    pub id: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub hires: bool,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<f64>,
    #[serde(default)]
    pub is_local: bool,
    pub album_id: Option<String>,
    pub artist_id: Option<u64>,
    #[serde(default = "default_streamable")]
    pub streamable: bool,
    /// Source type: "qobuz", "local", "plex"
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub parental_warning: bool,
    /// Opaque identifier of the Mixtape/Collection item that produced this track.
    /// For non-Mixtape paths, set to album_id as fallback. None is safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_item_id_hint: Option<String>,
}

fn default_streamable() -> bool {
    true
}

// V2 queue track <-> qbz_models::QueueTrack (CoreQueueTrack)
impl From<V2QueueTrack> for CoreQueueTrack {
    fn from(t: V2QueueTrack) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_secs: t.duration_secs,
            artwork_url: t.artwork_url,
            hires: t.hires,
            bit_depth: t.bit_depth,
            sample_rate: t.sample_rate,
            is_local: t.is_local,
            album_id: t.album_id.clone(),
            artist_id: t.artist_id,
            streamable: t.streamable,
            source: t.source,
            parental_warning: t.parental_warning,
            source_item_id_hint: t.source_item_id_hint.or(t.album_id),
        }
    }
}

impl From<CoreQueueTrack> for V2QueueTrack {
    fn from(t: CoreQueueTrack) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_secs: t.duration_secs,
            artwork_url: t.artwork_url,
            hires: t.hires,
            bit_depth: t.bit_depth,
            sample_rate: t.sample_rate,
            is_local: t.is_local,
            album_id: t.album_id,
            artist_id: t.artist_id,
            streamable: t.streamable,
            source: t.source,
            parental_warning: t.parental_warning,
            source_item_id_hint: t.source_item_id_hint,
        }
    }
}

/// Add track to the end of the queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_add_to_queue(
    track: V2QueueTrack,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] add_to_queue: {} - {}", track.id, track.title);
    let bridge = bridge.get().await;
    bridge.add_track(track.into()).await;
    Ok(())
}

/// Add track to play next (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_add_to_queue_next(
    track: V2QueueTrack,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] add_to_queue_next: {} - {}", track.id, track.title);
    let bridge = bridge.get().await;
    bridge.add_track_next(track.into()).await;
    Ok(())
}

/// Add multiple tracks to end of queue (V2 - bulk)
#[tauri::command]
pub async fn v2_bulk_add_to_queue(
    tracks: Vec<V2QueueTrack>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] bulk_add_to_queue: {} tracks", tracks.len());
    let bridge = bridge.get().await;
    for track in tracks {
        bridge.add_track(track.into()).await;
    }
    Ok(())
}

/// Add multiple tracks as play next (V2 - bulk, reversed to preserve order)
#[tauri::command]
pub async fn v2_bulk_add_to_queue_next(
    tracks: Vec<V2QueueTrack>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] bulk_add_to_queue_next: {} tracks", tracks.len());
    let bridge = bridge.get().await;
    // Reverse so the first track in the selection ends up as "next"
    for track in tracks.into_iter().rev() {
        bridge.add_track_next(track.into()).await;
    }
    Ok(())
}

/// Set the entire queue and start playing from index (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_set_queue(
    tracks: Vec<V2QueueTrack>,
    start_index: usize,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!(
        "[V2] set_queue: {} tracks, start at {}",
        tracks.len(),
        start_index
    );
    let queue_tracks: Vec<CoreQueueTrack> = tracks.into_iter().map(Into::into).collect();
    let bridge = bridge.get().await;
    bridge.set_queue(queue_tracks, Some(start_index)).await;
    // Queue replaced — Mixtape context is no longer valid.
    runtime.manager().set_queue_source_collection(None).await;
    Ok(())
}

/// Remove a track from the queue by index (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_remove_from_queue(
    index: usize,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] remove_from_queue: index {}", index);
    let bridge = bridge.get().await;
    bridge.remove_track(index).await;
    Ok(())
}

/// Remove a track from the upcoming queue by its position (V2 - uses CoreBridge)
/// (0 = first upcoming track, handles shuffle mode correctly)
#[tauri::command]
pub async fn v2_remove_upcoming_track(
    upcoming_index: usize,
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!(
        "[V2] remove_upcoming_track: upcoming_index {}",
        upcoming_index
    );

    if qconnect.status().await.transport_connected {
        let projection = qconnect
            .visible_queue_projection()
            .await
            .map_err(RuntimeError::Internal)?;
        let Some(payload) = build_qconnect_remove_upcoming_payload(&projection, upcoming_index)?
        else {
            return Ok(None);
        };

        qconnect
            .send_command(QueueCommandType::CtrlSrvrQueueRemoveTracks, payload)
            .await
            .map_err(RuntimeError::Internal)?;
        return Ok(None);
    }

    let bridge = bridge.get().await;
    Ok(bridge
        .remove_upcoming_track(upcoming_index)
        .await
        .map(Into::into))
}

/// Skip to next track in queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_next_track(
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    app_handle: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] next_track");
    if qconnect
        .skip_next_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)?
    {
        return Ok(None);
    }
    let bridge = bridge.get().await;
    let track = bridge.next_track().await;
    Ok(track.map(Into::into))
}

/// Go to previous track in queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_previous_track(
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    app_handle: tauri::AppHandle,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] previous_track");
    if qconnect
        .skip_previous_if_remote(&app_handle)
        .await
        .map_err(RuntimeError::Internal)?
    {
        return Ok(None);
    }
    let bridge = bridge.get().await;
    let track = bridge.previous_track().await;
    Ok(track.map(Into::into))
}

/// Play a specific track in the queue by index (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_play_queue_index(
    index: usize,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] play_queue_index: {}", index);
    let bridge = bridge.get().await;
    let track = bridge.play_index(index).await;
    Ok(track.map(Into::into))
}

/// Play a track at a given position in the upcoming list (V2 - shuffle-aware).
/// `upcoming_index` matches the position shown in the Queue sidebar; the
/// backend resolves it to the canonical track index, handling shuffle mode
/// correctly. Fixes issue #327.
#[tauri::command]
pub async fn v2_play_queue_upcoming_at(
    upcoming_index: usize,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<Option<V2QueueTrack>, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] play_queue_upcoming_at: {}", upcoming_index);
    let bridge = bridge.get().await;
    let track = bridge.play_upcoming_at(upcoming_index).await;
    Ok(track.map(Into::into))
}

/// Move a track within the queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_move_queue_track(
    from_index: usize,
    to_index: usize,
    bridge: State<'_, CoreBridgeState>,
    qconnect: State<'_, QconnectServiceState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<bool, RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] move_queue_track: {} -> {}", from_index, to_index);

    if qconnect.status().await.transport_connected {
        let projection = qconnect
            .visible_queue_projection()
            .await
            .map_err(RuntimeError::Internal)?;
        let Some(payload) = build_qconnect_reorder_payload(&projection, from_index, to_index)?
        else {
            return Ok(false);
        };
        if from_index == to_index {
            return Ok(true);
        }

        qconnect
            .send_command(QueueCommandType::CtrlSrvrQueueReorderTracks, payload)
            .await
            .map_err(RuntimeError::Internal)?;
        return Ok(true);
    }

    let bridge = bridge.get().await;
    Ok(bridge.move_track(from_index, to_index).await)
}

/// Add multiple tracks to queue (V2 - uses CoreBridge)
#[tauri::command]
pub async fn v2_add_tracks_to_queue(
    tracks: Vec<V2QueueTrack>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] add_tracks_to_queue: {} tracks", tracks.len());
    let queue_tracks: Vec<CoreQueueTrack> = tracks.into_iter().map(Into::into).collect();
    let bridge = bridge.get().await;
    bridge.add_tracks(queue_tracks).await;
    Ok(())
}

/// Add multiple tracks to play next (V2 - uses CoreBridge)
/// Tracks are added in reverse order so they play in the order provided
#[tauri::command]
pub async fn v2_add_tracks_to_queue_next(
    tracks: Vec<V2QueueTrack>,
    bridge: State<'_, CoreBridgeState>,
    runtime: State<'_, RuntimeManagerState>,
) -> Result<(), RuntimeError> {
    runtime
        .manager()
        .check_requirements(CommandRequirement::RequiresUserSession)
        .await?;
    log::info!("[V2] add_tracks_to_queue_next: {} tracks", tracks.len());
    let bridge = bridge.get().await;
    // Add in reverse order so they end up in the correct order
    for track in tracks.into_iter().rev() {
        bridge.add_track_next(track.into()).await;
    }
    Ok(())
}
