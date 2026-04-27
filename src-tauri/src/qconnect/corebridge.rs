//! Bridge between cloud-emitted QConnect events and the local CoreBridge
//! audio engine. Owns the application of renderer commands (play/pause/
//! seek), queue materialization (replicating the cloud's queue locally),
//! shuffle projection (without inventing a local order), and cursor
//! alignment.

use std::collections::HashMap;
use std::sync::Arc;

use qconnect_app::{QConnectQueueState, QConnectRendererState, RendererCommand};
use tokio::sync::{Mutex, RwLock};

use crate::core_bridge::CoreBridge;

use super::queue_resolution::{
    dedupe_track_ids, resolve_core_shuffle_order, resolve_remote_start_index,
};
use super::session::is_peer_renderer_active;
use super::{
    ensure_remote_track_loaded, model_track_to_core_queue_track, normalize_volume_to_fraction,
    qconnect_repeat_mode_from_loop_mode, QconnectRemoteSyncState, PLAYING_STATE_PAUSED,
    PLAYING_STATE_PLAYING, PLAYING_STATE_STOPPED, PLAYING_STATE_UNKNOWN,
};

pub(super) fn queue_state_needs_materialization(
    previous: Option<&QConnectQueueState>,
    next: &QConnectQueueState,
) -> bool {
    let Some(previous) = previous else {
        return true;
    };

    previous.version != next.version
        || previous.queue_items != next.queue_items
        || previous.shuffle_mode != next.shuffle_mode
        || previous.shuffle_order != next.shuffle_order
        || previous.autoplay_mode != next.autoplay_mode
        || previous.autoplay_loading != next.autoplay_loading
        || previous.autoplay_items != next.autoplay_items
}

pub(super) async fn apply_remote_loop_mode_to_corebridge(
    core_bridge: &Arc<RwLock<Option<CoreBridge>>>,
    loop_mode: i32,
) -> Result<(), String> {
    let repeat_mode = qconnect_repeat_mode_from_loop_mode(loop_mode)
        .ok_or_else(|| format!("unsupported qconnect loop mode: {loop_mode}"))?;

    let bridge_guard = core_bridge.read().await;
    let Some(bridge) = bridge_guard.as_ref() else {
        return Err("core bridge is not initialized yet".to_string());
    };

    bridge.set_repeat_mode(repeat_mode).await;
    Ok(())
}

pub(super) async fn apply_renderer_command_to_corebridge(
    core_bridge: &Arc<RwLock<Option<CoreBridge>>>,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    command: &RendererCommand,
    renderer_state: &QConnectRendererState,
) -> Result<(), String> {
    let bridge_guard = core_bridge.read().await;
    let Some(bridge) = bridge_guard.as_ref() else {
        return Err("core bridge is not initialized yet".to_string());
    };

    match command {
        RendererCommand::SetState {
            playing_state,
            current_position_ms,
            current_track,
            next_track,
            ..
        } => {
            let resolved_playing_state = renderer_state.playing_state.or(*playing_state);
            let mut projection_renderer_state = renderer_state.clone();
            if projection_renderer_state.current_track.is_none() {
                projection_renderer_state.current_track = current_track.clone();
            }
            if projection_renderer_state.next_track.is_none() {
                projection_renderer_state.next_track = next_track.clone();
            }
            let resolved_current_track = projection_renderer_state.current_track.as_ref();
            let peer_renderer_active = {
                let state = sync_state.lock().await;
                is_peer_renderer_active(&state.session)
            };
            if let Some(projected_track) = resolved_current_track {
                let queue_state = {
                    let state = sync_state.lock().await;
                    state.last_remote_queue_state.clone()
                };
                let projection_applied = if let Some(queue_state) = queue_state.as_ref() {
                    sync_corebridge_remote_shuffle_projection(
                        bridge,
                        sync_state,
                        queue_state,
                        &projection_renderer_state,
                    )
                    .await?
                } else {
                    false
                };

                // Track-manipulation operations (cursor align, force-restart,
                // ensure_remote_track_loaded) only run when the COMMAND
                // explicitly specifies a current_track. The projection's
                // resolved_current_track can be stale: when the cloud sends
                // a state-only update (pause/resume) with command.current_track=null,
                // the projection falls back to renderer_state.current_track,
                // which is the cloud's last-known view of qbz's playback —
                // potentially behind qbz's actual local advance. Using that
                // stale value to align/load causes spurious track switches
                // (e.g., pause from iOS made qbz jump back to a previous
                // track). The outer renderer_state-based projection is still
                // used for shuffle sync above and downstream playing_state /
                // seek operations, which remain safe because they don't
                // change the queue cursor or load tracks.
                let _ = projected_track; // retained for shuffle projection above
                if let Some(command_track) = current_track.as_ref() {
                    if !projection_applied {
                        if let Err(err) =
                            align_corebridge_queue_cursor(bridge, command_track.track_id).await
                        {
                            log::warn!(
                                "[QConnect] Failed to align CoreBridge queue cursor: {err}"
                            );
                        }
                    }

                    if matches!(
                        resolved_playing_state,
                        Some(PLAYING_STATE_PLAYING | PLAYING_STATE_PAUSED)
                    ) {
                        // Force-restart removed: the cloud routinely re-emits
                        // SetState with current_position_ms=0 for the same
                        // track when only secondary fields change (e.g.,
                        // next_track corrections, queue_item_id refreshes).
                        // Reloading the stream on every echo caused first-
                        // track hiccup on album change and "needs several
                        // taps" on prev/next. Track-change cases are handled
                        // by align_corebridge_queue_cursor + ensure_remote_
                        // track_loaded below; legitimate seek-to-start from
                        // a peer controller can use the seek path with
                        // target>1s if needed.
                        if let Err(err) =
                            ensure_remote_track_loaded(bridge, sync_state, command_track.track_id)
                                .await
                        {
                            log::warn!(
                                "[QConnect] Failed to load remote track {}: {err}",
                                command_track.track_id
                            );
                        }
                    }
                }
            }

            if let Some(value) = resolved_playing_state {
                match value {
                    PLAYING_STATE_PLAYING => {
                        bridge.resume()?;
                    }
                    PLAYING_STATE_PAUSED => {
                        bridge.pause()?;
                    }
                    PLAYING_STATE_STOPPED => {
                        bridge.stop()?;
                    }
                    PLAYING_STATE_UNKNOWN => {}
                    _ => {
                        log::debug!("[QConnect] Unknown playing state received: {value}");
                    }
                }
            }

            if let Some(position_ms) =
                renderer_state.current_position_ms.or(*current_position_ms)
            {
                let playback_state = bridge.get_playback_state();
                let current_pos_secs = playback_state.position;
                let target_secs = position_ms / 1000;
                // Reject echo seeks: when the command targets the same track
                // qbz is already playing AND target<=1s while local is well
                // ahead, this is the cloud re-emitting a stale SetState
                // (frequently fires on next_track corrections and queue_
                // item_id refreshes). A real peer "go to start" intent
                // would target the same track as the local one but the
                // round-trip to qbz is already a few seconds, making this
                // case indistinguishable from echo — favor stability.
                let is_echo_reset = current_track
                    .as_ref()
                    .map(|cmd_track| cmd_track.track_id == playback_state.track_id)
                    .unwrap_or(false)
                    && target_secs <= 1
                    && current_pos_secs > 2;
                if peer_renderer_active
                    && !is_echo_reset
                    && current_pos_secs.abs_diff(target_secs) > 2
                {
                    log::info!(
                        "[QConnect] SetState seek: current={}s target={}s",
                        current_pos_secs,
                        target_secs
                    );
                    bridge.seek(target_secs)?;
                }
            }
        }
        RendererCommand::SetVolume { volume, .. } => {
            if let Some(resolved) = renderer_state.volume.or(*volume) {
                bridge.set_volume(normalize_volume_to_fraction(resolved))?;
            }
        }
        RendererCommand::MuteVolume { value } => {
            if *value {
                bridge.set_volume(0.0)?;
            } else if let Some(resolved) = renderer_state.volume {
                bridge.set_volume(normalize_volume_to_fraction(resolved))?;
            }
        }
        RendererCommand::SetLoopMode { loop_mode } => {
            let resolved_loop_mode = renderer_state.loop_mode.unwrap_or(*loop_mode);
            let repeat_mode = qconnect_repeat_mode_from_loop_mode(resolved_loop_mode)
                .ok_or_else(|| format!("unsupported qconnect loop mode: {resolved_loop_mode}"))?;
            bridge.set_repeat_mode(repeat_mode).await;
        }
        RendererCommand::SetActive { active } => {
            if !*active {
                bridge.stop()?;
            }
        }
        RendererCommand::SetMaxAudioQuality { .. } | RendererCommand::SetShuffleMode { .. } => {}
    }

    Ok(())
}

async fn sync_corebridge_remote_shuffle_projection(
    bridge: &CoreBridge,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    queue_state: &QConnectQueueState,
    renderer_state: &QConnectRendererState,
) -> Result<bool, String> {
    if !queue_state.shuffle_mode || queue_state.queue_items.is_empty() {
        return Ok(false);
    }

    let start_index = resolve_remote_start_index(
        queue_state,
        renderer_state
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id),
        renderer_state
            .current_track
            .as_ref()
            .map(|item| item.track_id),
    );
    let Some(start_index) = start_index else {
        return Ok(false);
    };

    let core_shuffle_order = resolve_core_shuffle_order(
        queue_state,
        renderer_state
            .current_track
            .as_ref()
            .map(|item| item.queue_item_id),
        renderer_state
            .current_track
            .as_ref()
            .map(|item| item.track_id),
        renderer_state
            .next_track
            .as_ref()
            .map(|item| item.queue_item_id),
        renderer_state.next_track.as_ref().map(|item| item.track_id),
    );

    // Same deferral rule as materialize_remote_queue_to_corebridge: do
    // not invent an identity shuffle when the cloud hasn't yet sent the
    // authoritative shuffle_order. Wait for the second QueueUpdated.
    if core_shuffle_order.is_none() {
        return Ok(false);
    }

    let should_apply = {
        let state = sync_state.lock().await;
        state.last_materialized_start_index != Some(start_index)
            || state.last_materialized_core_shuffle_order != core_shuffle_order
    };
    if !should_apply {
        return Ok(false);
    }

    let (tracks, _) = bridge.get_all_queue_tracks().await;
    if tracks.len() != queue_state.queue_items.len() || tracks.is_empty() {
        return Ok(false);
    }

    bridge
        .set_queue_with_order(
            tracks,
            Some(start_index),
            queue_state.shuffle_mode,
            core_shuffle_order.clone(),
        )
        .await;

    let mut state = sync_state.lock().await;
    state.last_materialized_start_index = Some(start_index);
    state.last_materialized_core_shuffle_order = core_shuffle_order;
    Ok(true)
}

pub(super) async fn materialize_remote_queue_to_corebridge(
    core_bridge: &Arc<RwLock<Option<CoreBridge>>>,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    queue_state: &QConnectQueueState,
) -> Result<(), String> {
    let (
        renderer_queue_item_id,
        renderer_track_id,
        renderer_next_queue_item_id,
        renderer_next_track_id,
        renderer_playing_state,
        should_skip,
    ) = {
        let mut state = sync_state.lock().await;
        if !queue_state_needs_materialization(state.last_applied_queue_state.as_ref(), queue_state)
        {
            (
                state.last_renderer_queue_item_id,
                state.last_renderer_track_id,
                state.last_renderer_next_queue_item_id,
                state.last_renderer_next_track_id,
                state.last_renderer_playing_state,
                true,
            )
        } else {
            state.last_applied_queue_state = Some(queue_state.clone());
            (
                state.last_renderer_queue_item_id,
                state.last_renderer_track_id,
                state.last_renderer_next_queue_item_id,
                state.last_renderer_next_track_id,
                state.last_renderer_playing_state,
                false,
            )
        }
    };

    if should_skip {
        log::debug!(
            "[QConnect] materialize_remote_queue: skipped (identical snapshot {}.{})",
            queue_state.version.major,
            queue_state.version.minor
        );
        return Ok(());
    }

    log::info!(
        "[QConnect] materialize_remote_queue: version={}.{} items={} renderer_qid={:?} renderer_tid={:?} renderer_next_qid={:?} renderer_next_tid={:?} playing_state={:?}",
        queue_state.version.major,
        queue_state.version.minor,
        queue_state.queue_items.len(),
        renderer_queue_item_id,
        renderer_track_id,
        renderer_next_queue_item_id,
        renderer_next_track_id,
        renderer_playing_state
    );

    let bridge_guard = core_bridge.read().await;
    let Some(bridge) = bridge_guard.as_ref() else {
        return Err("core bridge is not initialized yet".to_string());
    };

    if queue_state.queue_items.is_empty() {
        // Preserve legacy behavior: keep current track on qconnect sync clears.
        bridge.clear_queue(true).await;
        bridge.set_shuffle(false).await;
        let mut state = sync_state.lock().await;
        state.last_materialized_start_index = None;
        state.last_materialized_core_shuffle_order = None;
        return Ok(());
    }

    let unique_track_ids = dedupe_track_ids(queue_state);
    let fetched_tracks = bridge
        .get_tracks_batch(&unique_track_ids)
        .await
        .map_err(|err| format!("fetch tracks batch for remote queue: {err}"))?;

    let mut tracks_by_id = HashMap::with_capacity(fetched_tracks.len());
    for track in fetched_tracks {
        tracks_by_id.insert(track.id, model_track_to_core_queue_track(&track));
    }

    let mut queue_tracks = Vec::with_capacity(queue_state.queue_items.len());
    for item in &queue_state.queue_items {
        if let Some(queue_track) = tracks_by_id.get(&item.track_id) {
            queue_tracks.push(queue_track.clone());
            continue;
        }

        match bridge.get_track(item.track_id).await {
            Ok(track) => {
                let mapped = model_track_to_core_queue_track(&track);
                tracks_by_id.insert(item.track_id, mapped.clone());
                queue_tracks.push(mapped);
            }
            Err(err) => {
                log::warn!(
                    "[QConnect] Unable to hydrate remote queue track {}: {}",
                    item.track_id,
                    err
                );
            }
        }
    }

    if queue_tracks.is_empty() {
        return Err("remote queue materialization produced zero playable tracks".to_string());
    }

    // Resolve start index from remote state first, then from the local playback
    // cursor only if that track is still part of the remote queue.
    let current_playback_track_id = match bridge.get_playback_state().track_id {
        0 => None,
        track_id => Some(track_id),
    };
    let mut start_index =
        resolve_remote_start_index(queue_state, renderer_queue_item_id, renderer_track_id);
    if start_index.is_none() {
        start_index = resolve_remote_start_index(
            queue_state,
            renderer_next_queue_item_id,
            renderer_next_track_id,
        )
        .map(|index| index.saturating_sub(1));
    }
    if start_index.is_none() {
        start_index = current_playback_track_id.and_then(|track_id| {
            queue_state
                .queue_items
                .iter()
                .position(|item| item.track_id == track_id)
        });
    }
    if start_index.is_none() && !queue_tracks.is_empty() {
        start_index = Some(0);
    }
    let core_shuffle_order = resolve_core_shuffle_order(
        queue_state,
        renderer_queue_item_id,
        renderer_track_id,
        renderer_next_queue_item_id,
        renderer_next_track_id,
    );
    // The cloud sends two QueueUpdated events during a shuffle toggle:
    // first with shuffle_mode=true and shuffle_order=null (the flag
    // broadcasts immediately), then ~400ms later with the computed
    // shuffle_order. If we mark shuffle_enabled=true on the first event
    // with an absent order, set_queue_with_order falls into its identity
    // path (0,1,2,...) — that is qbz inventing a sequence that diverges
    // from the order the cloud is about to authorize. Defer the engine
    // shuffle activation until the authoritative order is present.
    let effective_shuffle_enabled = queue_state.shuffle_mode && core_shuffle_order.is_some();
    log::info!(
        "[QConnect] materialize_remote_queue: setting queue with {} tracks, start_index={:?}, local_track_id={:?}, remote_shuffle_mode={}, shuffle_order_present={}, engine_shuffle_enabled={}",
        queue_tracks.len(),
        start_index,
        current_playback_track_id,
        queue_state.shuffle_mode,
        core_shuffle_order.is_some(),
        effective_shuffle_enabled,
    );
    bridge
        .set_queue_with_order(
            queue_tracks,
            start_index,
            effective_shuffle_enabled,
            core_shuffle_order.clone(),
        )
        .await;

    {
        let mut state = sync_state.lock().await;
        state.last_materialized_start_index = start_index;
        state.last_materialized_core_shuffle_order = core_shuffle_order;
    }

    let local_track_missing_from_remote = current_playback_track_id
        .map(|track_id| {
            !queue_state
                .queue_items
                .iter()
                .any(|item| item.track_id == track_id)
        })
        .unwrap_or(true);

    if let Some(index) = start_index {
        if local_track_missing_from_remote {
            log::info!(
                "[QConnect] materialize_remote_queue: aligning queue cursor to remote index {}",
                index
            );
            let _ = bridge.play_index(index).await;
        }
    }

    if current_playback_track_id.is_some()
        && current_playback_track_id != renderer_track_id
        && local_track_missing_from_remote
        && matches!(
            renderer_playing_state,
            Some(PLAYING_STATE_STOPPED | PLAYING_STATE_UNKNOWN)
        )
    {
        log::info!(
            "[QConnect] materialize_remote_queue: stopping stale local playback track {:?} after remote queue replacement",
            current_playback_track_id
        );
        let _ = bridge.stop();
    }

    Ok(())
}

pub(super) async fn align_corebridge_queue_cursor(
    bridge: &CoreBridge,
    track_id: u64,
) -> Result<(), String> {
    let (tracks, current_index) = bridge.get_all_queue_tracks().await;
    log::info!(
        "[QConnect] align_corebridge_queue_cursor: track_id={track_id} queue_len={} current_index={:?}",
        tracks.len(),
        current_index
    );
    if let Some(target_index) = tracks.iter().position(|track| track.id == track_id) {
        if current_index != Some(target_index) {
            log::info!(
                "[QConnect] align_corebridge_queue_cursor: moving cursor from {:?} to {target_index}",
                current_index
            );
            let _ = bridge.play_index(target_index).await;
        }
        return Ok(());
    }

    log::info!(
        "[QConnect] align_corebridge_queue_cursor: track {track_id} not in queue, fetching and creating single-track queue"
    );
    let track = bridge
        .get_track(track_id)
        .await
        .map_err(|err| format!("fetch current remote track {track_id}: {err}"))?;
    let queue_track = model_track_to_core_queue_track(&track);
    bridge.set_queue(vec![queue_track], Some(0)).await;
    Ok(())
}
