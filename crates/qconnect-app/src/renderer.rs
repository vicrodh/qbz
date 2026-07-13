//! Frontend-agnostic renderer-side pure helpers (slice 6).
//!
//! Pure protocol/format math used by the renderer orchestration (queue
//! materialize / cursor-align). No engine, no I/O, no Tauri. Relocated here so
//! both the Tauri adapter and the Slint adapter share one definition; the
//! src-tauri side re-exports these. The load-dedup predicates and the
//! audio-quality report helpers move here alongside their orchestration /
//! report consumers in the later slice-6 steps.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use qbz_models::{QueueTrack, RepeatMode, Track};
use qbz_player::PlaybackState;
use tokio::sync::Mutex;

use crate::queue_resolution::{
    dedupe_track_ids, resolve_core_shuffle_order, resolve_remote_start_index,
};
use crate::renderer_engine::QconnectRendererEngine;
use crate::session::quality_from_max_audio_quality;
use crate::{QConnectQueueState, QConnectRendererState, QconnectRemoteSyncState, RendererCommand};

/// QConnect protocol `playing_state` wire values. Single source of truth for the
/// renderer orchestration; the Tauri adapter re-exports these from here.
pub const PLAYING_STATE_UNKNOWN: i32 = 0;
pub const PLAYING_STATE_STOPPED: i32 = 1;
pub const PLAYING_STATE_PLAYING: i32 = 2;
pub const PLAYING_STATE_PAUSED: i32 = 3;

/// Dedup window: an echoed SetState for a track whose load was registered within
/// this window does not re-trigger the load. The audio thread updates
/// `playback_state.track_id` only after the engine appends the source, so a bare
/// `track_id` comparison would re-fire during that buffer/decode gap.
const LOAD_ATTEMPT_DEDUP_WINDOW: Duration = Duration::from_secs(5);

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
        album_version: None,
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
        context_kind: None,
        context_id: None,
    }
}

// ===================== Renderer orchestration (slice 6, step 6) =====================
//
// Engine-agnostic: written ONLY against `QconnectRendererEngine` + the shared
// `QconnectRemoteSyncState`. The Tauri/Slint adapters obtain a concrete engine
// (`&CoreBridge` / `&SlintEngine`) — including any "not initialized yet" guard —
// and dispatch here, so the hard-won echo/cursor/materialize/shuffle logic is
// never re-derived per frontend. Ported byte-for-byte from the prior Tauri
// `corebridge.rs` / `track_loading.rs`; only `bridge` -> `engine` and the
// guard-unwrap (which stays adapter-side) changed.

pub fn queue_state_needs_materialization(
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

pub fn should_reload_remote_track(playback_state: &PlaybackState, track_id: u64) -> bool {
    // Only reload when the track ID actually changed. The previous
    // !has_loaded_audio gate fired during the buffering window of an
    // initial load (qbz already started fetching but the audio engine
    // hasn't reported the track as loaded yet) — when the cloud echo
    // SetState arrived for the same track, this caused a redundant
    // load that interrupted the in-progress one. That was the residual
    // first-track hiccup.
    playback_state.track_id != track_id
}

/// Returns true if a load attempt for `track_id` was registered within the
/// dedup window (see `LOAD_ATTEMPT_DEDUP_WINDOW`).
fn is_recent_load_attempt(state: &QconnectRemoteSyncState, track_id: u64) -> bool {
    match state.last_load_attempt {
        Some((tid, ts)) => tid == track_id && ts.elapsed() < LOAD_ATTEMPT_DEDUP_WINDOW,
        None => false,
    }
}

/// Load a remote track into the engine, deduped against echoed SetState frames.
/// Records the attempt BEFORE dispatching the load (the audio thread updates
/// `playback_state.track_id` only after the engine appends the source, so the
/// recording must precede the load to close the echo window).
///
/// `start_position_secs` is the position to resume the stream at. For a normal
/// peer track-change the cloud sends position ~0, so this is 0 (a fresh track).
/// On a TAKEBACK whose first load lands here (SetActive arrived before the cloud
/// knew our current_track, so the force-stream couldn't fire), the SetState
/// carries the peer's real position and we resume there instead of streaming
/// from 0 and then trying to seek forward — a forward seek past the buffered
/// watermark is silently ignored by the audio thread, so streaming from 0 left
/// the first takeback playing from the start (bad for audiobooks). The protected
/// bit-perfect seams + the HTTP feeder live behind `start_track_stream`.
pub async fn ensure_remote_track_loaded(
    engine: &impl QconnectRendererEngine,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    track_id: u64,
    max_audio_quality: Option<i32>,
    start_position_secs: u64,
) -> Result<(), String> {
    {
        let state = sync_state.lock().await;
        if is_recent_load_attempt(&state, track_id) {
            return Ok(());
        }
    }
    let playback_state = engine.get_playback_state();
    if !should_reload_remote_track(&playback_state, track_id) {
        return Ok(());
    }

    {
        let mut state = sync_state.lock().await;
        state.last_load_attempt = Some((track_id, Instant::now()));
    }

    let quality = quality_from_max_audio_quality(max_audio_quality);
    let duration_secs = engine
        .get_track(track_id)
        .await
        .map(|track| u64::from(track.duration))
        .unwrap_or(0);
    engine
        .start_track_stream(track_id, quality, duration_secs, start_position_secs)
        .await
}

/// Force a (re)stream of `track_id` at `start_position_secs` when BECOMING the
/// active renderer (takeback). Unlike [`ensure_remote_track_loaded`], this does
/// NOT short-circuit on a matching `playback_state.track_id`: a prior
/// controller->renderer handoff tore the local stream down via `engine.stop()`
/// (audio buffer cleared, `has_loaded_audio` false) while `current_track_id`
/// still reports the old track, so the plain track-id guard would skip the load
/// and the following `resume()` would fail with "no audio data available".
///
/// It DOES skip when the engine is already streaming this exact track with audio
/// loaded (`track_id` matches AND `has_loaded_audio`), so a spurious SetActive
/// during live renderer playback never restarts the current track; and it keeps
/// the dedup window so the SetActive->SetState echo doesn't double-load.
///
/// `start_position_secs` resumes at the handed-off position (the cloud carries
/// the peer's last position in `renderer_state.current_position_ms`), so a long
/// track / audiobook does not restart from 0. Resume is honored by the protected
/// `play_streaming_dynamic` session-resume path behind `start_track_stream`.
pub async fn force_remote_track_stream(
    engine: &impl QconnectRendererEngine,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    track_id: u64,
    max_audio_quality: Option<i32>,
    start_position_secs: u64,
) -> Result<(), String> {
    let playback_state = engine.get_playback_state();
    if playback_state.track_id == track_id && engine.has_loaded_audio() {
        return Ok(());
    }

    {
        let state = sync_state.lock().await;
        if is_recent_load_attempt(&state, track_id) {
            return Ok(());
        }
    }
    {
        let mut state = sync_state.lock().await;
        state.last_load_attempt = Some((track_id, Instant::now()));
    }

    let quality = quality_from_max_audio_quality(max_audio_quality);
    let duration_secs = engine
        .get_track(track_id)
        .await
        .map(|track| u64::from(track.duration))
        .unwrap_or(0);
    engine
        .start_track_stream(track_id, quality, duration_secs, start_position_secs)
        .await
}

pub async fn apply_remote_loop_mode(
    engine: &impl QconnectRendererEngine,
    loop_mode: i32,
) -> Result<(), String> {
    let repeat_mode = qconnect_repeat_mode_from_loop_mode(loop_mode)
        .ok_or_else(|| format!("unsupported qconnect loop mode: {loop_mode}"))?;
    engine.set_repeat_mode(repeat_mode).await;
    Ok(())
}

pub async fn apply_renderer_command(
    engine: &impl QconnectRendererEngine,
    sync_state: &Arc<Mutex<QconnectRemoteSyncState>>,
    command: &RendererCommand,
    renderer_state: &QConnectRendererState,
) -> Result<(), String> {
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
            if let Some(projected_track) = resolved_current_track {
                let queue_state = {
                    let state = sync_state.lock().await;
                    state.last_remote_queue_state.clone()
                };
                let projection_applied = if let Some(queue_state) = queue_state.as_ref() {
                    sync_remote_shuffle_projection(
                        engine,
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
                            align_queue_cursor(engine, command_track.track_id).await
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
                        // by align_queue_cursor + ensure_remote_track_loaded
                        // below; legitimate seek-to-start from a peer
                        // controller can use the seek path with target>1s.
                        // Resume the load at the cloud's reported position (same
                        // source the seek block below uses). For a normal peer
                        // track-change this is ~0; on a takeback whose first load
                        // lands here it is the peer's position, so we stream from
                        // there instead of from 0 + an ignored forward seek.
                        let start_position_secs = renderer_state
                            .current_position_ms
                            .or(*current_position_ms)
                            .map(|ms| ms / 1000)
                            .unwrap_or(0);
                        if let Err(err) = ensure_remote_track_loaded(
                            engine,
                            sync_state,
                            command_track.track_id,
                            projection_renderer_state.max_audio_quality,
                            start_position_secs,
                        )
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
                        engine.resume()?;
                    }
                    PLAYING_STATE_PAUSED => {
                        engine.pause()?;
                    }
                    PLAYING_STATE_STOPPED => {
                        engine.stop()?;
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
                let playback_state = engine.get_playback_state();
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
                // Issue #387: honor seeks regardless of which device is the
                // active renderer. The previous gate (`peer_renderer_active`)
                // skipped seeks entirely when local was the active renderer,
                // breaking the case where a peer controller (e.g. official
                // Qobuz mobile app) sends a real seek to qbz acting as the
                // renderer — the audio thread never moved while the cloud
                // state advanced, so the controller's progress bar locked.
                // The is_echo_reset + abs_diff > 2 gates already filter the
                // cloud-echo case the peer_renderer_active check was added
                // to defend against in commit 147bcbd7. If hiccups return,
                // revert this change and reintroduce a more targeted echo
                // detector (UUID-based) instead of the all-or-nothing gate.
                if !is_echo_reset && current_pos_secs.abs_diff(target_secs) > 2 {
                    log::info!(
                        "[QConnect] SetState seek: current={}s target={}s",
                        current_pos_secs,
                        target_secs
                    );
                    engine.seek(target_secs)?;
                }
            }
        }
        RendererCommand::SetVolume { volume, .. } => {
            if let Some(resolved) = renderer_state.volume.or(*volume) {
                engine.set_volume(normalize_volume_to_fraction(resolved))?;
            }
        }
        RendererCommand::MuteVolume { value } => {
            if *value {
                engine.set_volume(0.0)?;
            } else if let Some(resolved) = renderer_state.volume {
                engine.set_volume(normalize_volume_to_fraction(resolved))?;
            }
        }
        RendererCommand::SetLoopMode { loop_mode } => {
            let resolved_loop_mode = renderer_state.loop_mode.unwrap_or(*loop_mode);
            let repeat_mode = qconnect_repeat_mode_from_loop_mode(resolved_loop_mode)
                .ok_or_else(|| format!("unsupported qconnect loop mode: {resolved_loop_mode}"))?;
            engine.set_repeat_mode(repeat_mode).await;
        }
        RendererCommand::SetActive { active } => {
            if *active {
                // Becoming the active renderer (takeback). FORCE a stream of the
                // current track instead of a plain ensure-loaded: a prior
                // controller->renderer transition tore the local stream down via
                // engine.stop() (audio buffer cleared, has_loaded_audio=false)
                // while current_track_id still reports the old track, so the
                // track-id guard in ensure_remote_track_loaded would skip the load
                // and the next SetState's resume() would fail with "no audio data
                // available". Resume at the handed-off position so a long
                // track / audiobook does not restart from 0.
                if let Some(current) = renderer_state.current_track.as_ref() {
                    let start_position_secs = renderer_state
                        .current_position_ms
                        .map(|ms| ms / 1000)
                        .unwrap_or(0);
                    if let Err(err) = force_remote_track_stream(
                        engine,
                        sync_state,
                        current.track_id,
                        renderer_state.max_audio_quality,
                        start_position_secs,
                    )
                    .await
                    {
                        log::warn!("[QConnect] SetActive(true) force-stream failed: {err}");
                    }
                }
            } else {
                engine.stop()?;
            }
        }
        RendererCommand::SetMaxAudioQuality { max_audio_quality } => {
            // Applied on the next load via renderer_state.max_audio_quality
            // (recorded by the core reducer). No immediate re-fetch.
            log::info!("[QConnect] SetMaxAudioQuality => {max_audio_quality}");
        }
        RendererCommand::SetShuffleMode { shuffle_mode } => {
            let enabled = renderer_state.shuffle_mode.unwrap_or(*shuffle_mode);
            // WS-authoritative: flip ONLY the flag here. Never generate a local
            // shuffle order — the cloud owns queue order, which arrives separately
            // via `sync_remote_shuffle_projection` / `materialize_remote_queue`.
            // Calling the order-generating `set_shuffle` would produce a divergent
            // local random order ("es un infierno" — the documented failure mode).
            engine.set_shuffle_flag(enabled).await;
        }
    }

    Ok(())
}

async fn sync_remote_shuffle_projection(
    engine: &impl QconnectRendererEngine,
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

    // Same deferral rule as materialize_remote_queue: do not invent an
    // identity shuffle when the cloud hasn't yet sent the authoritative
    // shuffle_order. Wait for the second QueueUpdated.
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

    let (tracks, _) = engine.get_all_queue_tracks().await;
    if tracks.len() != queue_state.queue_items.len() || tracks.is_empty() {
        return Ok(false);
    }

    engine
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

pub async fn materialize_remote_queue(
    engine: &impl QconnectRendererEngine,
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

    if queue_state.queue_items.is_empty() {
        // Preserve legacy behavior: keep current track on qconnect sync clears.
        engine.clear_queue(true).await;
        engine.set_shuffle(false).await;
        let mut state = sync_state.lock().await;
        state.last_materialized_start_index = None;
        state.last_materialized_core_shuffle_order = None;
        return Ok(());
    }

    let unique_track_ids = dedupe_track_ids(queue_state);
    let fetched_tracks = engine
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

        match engine.get_track(item.track_id).await {
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
    let current_playback_track_id = match engine.get_playback_state().track_id {
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
    engine
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
            let _ = engine.play_index(index).await;
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
        let _ = engine.stop();
    }

    Ok(())
}

pub async fn align_queue_cursor(
    engine: &impl QconnectRendererEngine,
    track_id: u64,
) -> Result<(), String> {
    let (tracks, current_index) = engine.get_all_queue_tracks().await;
    log::info!(
        "[QConnect] align_queue_cursor: track_id={track_id} queue_len={} current_index={:?}",
        tracks.len(),
        current_index
    );
    if let Some(target_index) = tracks.iter().position(|track| track.id == track_id) {
        if current_index != Some(target_index) {
            log::info!(
                "[QConnect] align_queue_cursor: moving cursor from {:?} to {target_index}",
                current_index
            );
            let _ = engine.play_index(target_index).await;
        }
        return Ok(());
    }

    log::info!(
        "[QConnect] align_queue_cursor: track {track_id} not in queue, fetching and creating single-track queue"
    );
    let track = engine
        .get_track(track_id)
        .await
        .map_err(|err| format!("fetch current remote track {track_id}: {err}"))?;
    let queue_track = model_track_to_core_queue_track(&track);
    engine.set_queue(vec![queue_track], Some(0)).await;
    Ok(())
}

// ===================== Mock-engine trait tests (slice 6, step 8) =====================
//
// These exercise the renderer orchestration end-to-end against a recording mock
// engine — the hard-won behavior that previously could only be tested through the
// Tauri adapter. A passing test here proves the logic is engine-independent: any
// future Slint regression is a wiring bug in its trait impl, not a re-derivation
// bug in the shared logic.

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    use async_trait::async_trait;
    use qbz_models::{Quality, QueueTrack, RepeatMode, Track};
    use qbz_player::PlaybackState;
    use qconnect_core::{QueueItem, QueueVersion};
    use tokio::sync::Mutex;

    use crate::renderer_engine::QconnectRendererEngine;
    use crate::{QConnectQueueState, QConnectRendererState, QconnectRemoteSyncState, RendererCommand};

    #[derive(Default)]
    struct MockCalls {
        resumes: u32,
        pauses: u32,
        stops: u32,
        seeks: Vec<u64>,
        set_volumes: Vec<f32>,
        set_repeat_modes: u32,
        set_shuffles: Vec<bool>,
        set_shuffle_flags: Vec<bool>,
        set_queue_with_order: Vec<(bool, Option<Vec<usize>>)>,
        set_queues: u32,
        clear_queues: Vec<bool>,
        play_indexes: Vec<usize>,
        get_tracks_batch: u32,
        start_track_streams: Vec<u64>,
        start_positions: Vec<u64>,
    }

    /// Records every engine call; serves canned `PlaybackState` + queue snapshot.
    struct MockEngine {
        calls: Arc<StdMutex<MockCalls>>,
        playback: PlaybackState,
        queue_tracks: Vec<QueueTrack>,
        queue_index: Option<usize>,
        loaded_audio: bool,
    }

    impl MockEngine {
        fn new() -> Self {
            Self {
                calls: Arc::new(StdMutex::new(MockCalls::default())),
                playback: PlaybackState::default(),
                queue_tracks: Vec::new(),
                queue_index: None,
                loaded_audio: false,
            }
        }

        fn calls(&self) -> std::sync::MutexGuard<'_, MockCalls> {
            self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl QconnectRendererEngine for MockEngine {
        fn resume(&self) -> Result<(), String> {
            self.calls().resumes += 1;
            Ok(())
        }
        fn pause(&self) -> Result<(), String> {
            self.calls().pauses += 1;
            Ok(())
        }
        fn stop(&self) -> Result<(), String> {
            self.calls().stops += 1;
            Ok(())
        }
        fn seek(&self, position_secs: u64) -> Result<(), String> {
            self.calls().seeks.push(position_secs);
            Ok(())
        }
        fn set_volume(&self, fraction: f32) -> Result<(), String> {
            self.calls().set_volumes.push(fraction);
            Ok(())
        }
        fn get_playback_state(&self) -> PlaybackState {
            self.playback.clone()
        }
        fn has_loaded_audio(&self) -> bool {
            self.loaded_audio
        }
        async fn set_repeat_mode(&self, _mode: RepeatMode) {
            self.calls().set_repeat_modes += 1;
        }
        async fn set_shuffle(&self, enabled: bool) {
            self.calls().set_shuffles.push(enabled);
        }
        async fn set_shuffle_flag(&self, enabled: bool) {
            self.calls().set_shuffle_flags.push(enabled);
        }
        async fn get_all_queue_tracks(&self) -> (Vec<QueueTrack>, Option<usize>) {
            (self.queue_tracks.clone(), self.queue_index)
        }
        async fn set_queue(&self, _tracks: Vec<QueueTrack>, _start_index: Option<usize>) {
            self.calls().set_queues += 1;
        }
        async fn set_queue_with_order(
            &self,
            _tracks: Vec<QueueTrack>,
            _start_index: Option<usize>,
            shuffle_enabled: bool,
            shuffle_order: Option<Vec<usize>>,
        ) {
            self.calls()
                .set_queue_with_order
                .push((shuffle_enabled, shuffle_order));
        }
        async fn clear_queue(&self, keep_current: bool) {
            self.calls().clear_queues.push(keep_current);
        }
        async fn play_index(&self, index: usize) -> Option<QueueTrack> {
            self.calls().play_indexes.push(index);
            None
        }
        async fn get_track(&self, track_id: u64) -> Result<Track, String> {
            Ok(mock_track(track_id))
        }
        async fn get_tracks_batch(&self, track_ids: &[u64]) -> Result<Vec<Track>, String> {
            self.calls().get_tracks_batch += 1;
            Ok(track_ids.iter().map(|&id| mock_track(id)).collect())
        }
        async fn start_track_stream(
            &self,
            track_id: u64,
            _quality: Quality,
            _duration_secs: u64,
            start_position_secs: u64,
        ) -> Result<(), String> {
            let mut calls = self.calls();
            calls.start_track_streams.push(track_id);
            calls.start_positions.push(start_position_secs);
            Ok(())
        }
        fn current_output_format(&self) -> Option<(u32, u32)> {
            Some((44_100, 16))
        }
    }

    fn qi(track_id: u64, queue_item_id: u64) -> QueueItem {
        QueueItem {
            track_context_uuid: "ctx".to_string(),
            track_id,
            queue_item_id,
        }
    }

    fn mock_track(id: u64) -> Track {
        serde_json::from_value(serde_json::json!({ "id": id, "title": "t", "duration": 100 }))
            .expect("mock track")
    }

    fn mock_queue_track(id: u64) -> QueueTrack {
        model_track_to_core_queue_track(&mock_track(id))
    }

    fn queue_state(
        version: QueueVersion,
        items: Vec<QueueItem>,
        shuffle_mode: bool,
        shuffle_order: Option<Vec<usize>>,
    ) -> QConnectQueueState {
        QConnectQueueState {
            version,
            queue_items: items,
            shuffle_mode,
            shuffle_order,
            autoplay_mode: false,
            autoplay_loading: false,
            autoplay_items: Vec::new(),
            updated_at_ms: 0,
            last_server_queue_hash: None,
        }
    }

    fn sync() -> Arc<Mutex<QconnectRemoteSyncState>> {
        Arc::new(Mutex::new(QconnectRemoteSyncState::default()))
    }

    /// #2 — two loads for the same track within the dedup window trigger exactly
    /// one `start_track_stream`; the second is swallowed by the 5s window even
    /// though the audio thread hasn't reported the track yet.
    #[tokio::test]
    async fn ensure_remote_track_loaded_dedups_within_window() {
        let engine = MockEngine::new(); // playback track_id 0 != 42 → would reload
        let sync = sync();
        ensure_remote_track_loaded(&engine, &sync, 42, None, 0)
            .await
            .unwrap();
        ensure_remote_track_loaded(&engine, &sync, 42, None, 0)
            .await
            .unwrap();
        assert_eq!(engine.calls().start_track_streams, vec![42]);
    }

    /// #2 — no reload when the audio thread already plays the requested track.
    #[tokio::test]
    async fn ensure_remote_track_loaded_skips_when_track_unchanged() {
        let mut engine = MockEngine::new();
        engine.playback = PlaybackState {
            track_id: 42,
            ..Default::default()
        };
        let sync = sync();
        ensure_remote_track_loaded(&engine, &sync, 42, None, 0)
            .await
            .unwrap();
        assert!(engine.calls().start_track_streams.is_empty());
    }

    /// #1 / #387 — a SetState targeting the SAME track at <=1s while local is well
    /// ahead is a cloud echo: the seek is rejected.
    #[tokio::test]
    async fn apply_renderer_command_rejects_echo_seek() {
        let mut engine = MockEngine::new();
        engine.playback = PlaybackState {
            track_id: 7,
            position: 30,
            ..Default::default()
        };
        engine.queue_tracks = vec![mock_queue_track(7)];
        engine.queue_index = Some(0);
        let sync = sync();
        let cmd = RendererCommand::SetState {
            playing_state: None,
            current_position_ms: Some(0),
            current_track: Some(qi(7, 0)),
            next_track: None,
        };
        apply_renderer_command(&engine, &sync, &cmd, &QConnectRendererState::default())
            .await
            .unwrap();
        assert!(
            engine.calls().seeks.is_empty(),
            "echo seek must be rejected (#387 is_echo_reset)"
        );
    }

    /// #1 / #387 — a genuine peer seek (target far from local) IS honored, even
    /// for the same track (the bug the all-or-nothing peer gate caused).
    #[tokio::test]
    async fn apply_renderer_command_honors_genuine_seek() {
        let mut engine = MockEngine::new();
        engine.playback = PlaybackState {
            track_id: 7,
            position: 10,
            ..Default::default()
        };
        engine.queue_tracks = vec![mock_queue_track(7)];
        engine.queue_index = Some(0);
        let sync = sync();
        let cmd = RendererCommand::SetState {
            playing_state: None,
            current_position_ms: Some(40_000),
            current_track: Some(qi(7, 0)),
            next_track: None,
        };
        apply_renderer_command(&engine, &sync, &cmd, &QConnectRendererState::default())
            .await
            .unwrap();
        assert_eq!(engine.calls().seeks, vec![40]);
    }

    /// #4 — WS-authoritative shuffle: a standalone SetShuffleMode must flip the
    /// flag ONLY (set_shuffle_flag), NEVER generate a local order. It must not
    /// call the order-generating set_shuffle, and must not apply any queue order
    /// (set_queue_with_order) — the cloud's order arrives separately.
    #[tokio::test]
    async fn apply_renderer_command_setshufflemode_is_flag_only() {
        let mut engine = MockEngine::new();
        engine.queue_tracks = vec![mock_queue_track(1), mock_queue_track(2), mock_queue_track(3)];
        engine.queue_index = Some(0);
        let sync = sync();
        let cmd = RendererCommand::SetShuffleMode { shuffle_mode: true };
        apply_renderer_command(&engine, &sync, &cmd, &QConnectRendererState::default())
            .await
            .unwrap();
        let calls = engine.calls();
        assert_eq!(
            calls.set_shuffle_flags,
            vec![true],
            "SetShuffleMode must take the flag-only path"
        );
        assert!(
            calls.set_shuffles.is_empty(),
            "SetShuffleMode must NEVER call the order-generating set_shuffle (WS-authoritative rule)"
        );
        assert!(
            calls.set_queue_with_order.is_empty(),
            "SetShuffleMode must not apply any local order; the cloud's order arrives separately"
        );
    }

    /// #3 — a state-only SetState (command.current_track = None) must NOT align the
    /// cursor or load a track from the renderer_state's stale current_track (the
    /// iOS-pause-jumped-back fix); the playing_state is still applied.
    #[tokio::test]
    async fn apply_renderer_command_skips_track_ops_on_state_only_update() {
        let mut engine = MockEngine::new();
        engine.playback = PlaybackState {
            track_id: 7,
            position: 5,
            ..Default::default()
        };
        let sync = sync();
        let cmd = RendererCommand::SetState {
            playing_state: Some(PLAYING_STATE_PAUSED),
            current_position_ms: None,
            current_track: None,
            next_track: None,
        };
        // renderer_state carries a STALE current_track the projection would fall
        // back to — it must not drive a load/align.
        let renderer_state = QConnectRendererState {
            current_track: Some(qi(99, 0)),
            ..Default::default()
        };
        apply_renderer_command(&engine, &sync, &cmd, &renderer_state)
            .await
            .unwrap();
        let calls = engine.calls();
        assert!(
            calls.start_track_streams.is_empty(),
            "no load on state-only update"
        );
        assert!(
            calls.play_indexes.is_empty(),
            "no cursor align on state-only update"
        );
        assert_eq!(calls.pauses, 1, "pause still applied");
    }

    /// #5 — shuffle deferral: the first event (shuffle_mode=true, order=None)
    /// materializes with shuffle_enabled=false (no invented identity order); the
    /// second event (authoritative order present) enables shuffle.
    #[tokio::test]
    async fn materialize_defers_shuffle_until_authoritative_order() {
        let engine = MockEngine::new();
        let sync = sync();
        let items = vec![qi(10, 0), qi(11, 1)];

        let q1 = queue_state(QueueVersion::new(1, 0), items.clone(), true, None);
        materialize_remote_queue(&engine, &sync, &q1).await.unwrap();
        {
            let calls = engine.calls();
            assert_eq!(calls.set_queue_with_order.len(), 1);
            assert!(
                !calls.set_queue_with_order[0].0,
                "shuffle deferred while order absent"
            );
        }

        let q2 = queue_state(QueueVersion::new(1, 1), items, true, Some(vec![1, 0]));
        materialize_remote_queue(&engine, &sync, &q2).await.unwrap();
        {
            let calls = engine.calls();
            assert_eq!(calls.set_queue_with_order.len(), 2);
            assert!(
                calls.set_queue_with_order[1].0,
                "shuffle enabled once authoritative order present"
            );
        }
    }

    /// #1 (takeback) — becoming the active renderer FORCE-streams the current
    /// track even though `playback_state.track_id` still matches: the prior
    /// controller->renderer stop() cleared the audio buffer but left the stale
    /// track id, so the plain track-id guard would skip the load and the next
    /// SetState's resume() would fail with "no audio data available". Also
    /// resumes at the handed-off position, not 0.
    #[tokio::test]
    async fn set_active_force_streams_on_takeback_when_audio_torn_down() {
        let mut engine = MockEngine::new();
        engine.playback = PlaybackState {
            track_id: 7, // stale id left by stop(); audio is gone
            ..Default::default()
        };
        engine.loaded_audio = false;
        let sync = sync();
        let cmd = RendererCommand::SetActive { active: true };
        let renderer_state = QConnectRendererState {
            current_track: Some(qi(7, 0)),
            current_position_ms: Some(45_000),
            ..Default::default()
        };
        apply_renderer_command(&engine, &sync, &cmd, &renderer_state)
            .await
            .unwrap();
        let calls = engine.calls();
        assert_eq!(
            calls.start_track_streams,
            vec![7],
            "takeback must force a stream even when the track id matches"
        );
        assert_eq!(
            calls.start_positions,
            vec![45],
            "takeback must resume at the handed-off position (45s), not 0"
        );
    }

    /// #1 (no-interrupt) — a SetActive(true) while the renderer is ALREADY
    /// streaming this exact track with audio loaded must NOT restart it (guards
    /// against a spurious activation tearing down live playback).
    #[tokio::test]
    async fn set_active_does_not_restart_when_already_streaming() {
        let mut engine = MockEngine::new();
        engine.playback = PlaybackState {
            track_id: 7,
            ..Default::default()
        };
        engine.loaded_audio = true; // live playback in progress
        let sync = sync();
        let cmd = RendererCommand::SetActive { active: true };
        let renderer_state = QConnectRendererState {
            current_track: Some(qi(7, 0)),
            current_position_ms: Some(45_000),
            ..Default::default()
        };
        apply_renderer_command(&engine, &sync, &cmd, &renderer_state)
            .await
            .unwrap();
        assert!(
            engine.calls().start_track_streams.is_empty(),
            "must not restart an already-streaming track on a spurious SetActive"
        );
    }

    /// #1 (takeback first-load via SetState) — when the FIRST load on a takeback
    /// lands in the SetState path (SetActive arrived before current_track was
    /// known, so the force-stream couldn't fire), the load must stream at the
    /// cloud's reported position, not 0 — so a mid-track takeback resumes where
    /// the peer was instead of restarting (a forward seek past the buffered
    /// watermark is silently ignored, so streaming from 0 stuck at the start).
    #[tokio::test]
    async fn apply_renderer_command_setstate_streams_at_reported_position() {
        let engine = MockEngine::new(); // playback track_id 0 → fresh load
        let sync = sync();
        let cmd = RendererCommand::SetState {
            playing_state: Some(PLAYING_STATE_PLAYING),
            current_position_ms: Some(118_000),
            current_track: Some(qi(7, 1)),
            next_track: None,
        };
        apply_renderer_command(&engine, &sync, &cmd, &QConnectRendererState::default())
            .await
            .unwrap();
        let calls = engine.calls();
        assert_eq!(calls.start_track_streams, vec![7], "fresh takeback load");
        assert_eq!(
            calls.start_positions,
            vec![118],
            "takeback load must resume at the cloud position (118s), not 0"
        );
    }
}
