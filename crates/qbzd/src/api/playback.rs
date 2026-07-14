// crates/qbzd/src/api/playback.rs — routes 4-12 (02-cli-and-api.md §3.3.4-12):
// GET /api/now-playing + the 8 POST /api/playback/* transport routes.
//
// 409 needs_auth (01-architecture.md §6.2) is gated per-route by reading the
// per-route Errors column in 02 §3.3, not blanket-applied: `/api/now-playing`
// gates unconditionally (§3.3.4); `play`/`toggle` gate ONLY the cold-start
// branch (§3.3.5-8 "cold-start needs a session"); `next`/`previous` gate
// unconditionally before running the advance ritual (§3.3.9-10); `pause`/
// `stop`/`seek`/`volume` never cold-start and are NOT listed with needs_auth
// in their own Errors columns (§3.3.11-12), so they act on whatever is
// already loaded regardless of auth state.
//
// DSD-direct guard: `Player::is_dsd_direct_active()` (qbz-player/src/player/
// mod.rs:4893, "True while a DoP stream is active (volume fixed, seek
// unsupported)") is the player's own guard — previously unconsumed anywhere
// in the workspace. `seek`/`volume` (incl. the `mute` body form) check it
// FIRST and refuse 409 rather than silently no-op (the brief's explicit
// requirement — a silent no-op reads as broken, 02 §1.4).
//
// Mute is daemon-owned state in `DaemonShared.{muted, premute_volume}` (T2
// seam), NOT the desktop's process statics (`crates/qbz/src/playback.rs:
// 3907-3930`) — same semantics (stash-then-zero / restore), different owner.
// The reported `playback.volume` is always the NOMINAL (pre-mute) level, both
// muted and unmuted: `premute_volume` when muted, the live player volume
// otherwise. This is what makes `now`'s and `mute`'s human lines ("vol 80%",
// "muted (was 80%)") trivial reads of one JSON field, and mirrors the
// desktop's PREMUTE_VOLUME/MUTED pair exactly, just relocated.
use std::io::Cursor;

use serde_json::Value;
use tiny_http::Response;

use crate::state::AuthState;

use super::{err_json, json, ApiState};

/// `GET /api/now-playing` (02 §3.3.4). `playback` is the serialized
/// `PlaybackEvent` (qbz-player/src/player/mod.rs:925) with `shuffle`/`repeat`
/// filled in from the queue (the player itself leaves them `None` — "Set by
/// caller with access to queue state") plus the daemon-owned `muted` field,
/// plus an ADDITIVE `queue_len` (02 §3.1.4 allows additive fields within
/// api_version 1; needed so `qbzd now`'s stopped-state render, "stopped ·
/// queue 14 tracks", has a count — the documented playing-state example has
/// no queue count because nothing needs one while a track is loaded).
/// `track` is the current `QueueTrack`, or `null` when nothing is loaded.
pub fn now_playing(state: &ApiState) -> Response<Cursor<Vec<u8>>> {
    if let Some(resp) = auth_gate(state) {
        return resp;
    }

    let player = state.runtime.core().player();
    let mut ev = player.get_playback_event();
    let queue = state.rt.block_on(state.runtime.core().get_queue_state());

    ev.shuffle = Some(queue.shuffle);
    ev.repeat = Some(repeat_str(queue.repeat));

    let (muted, nominal_volume) = nominal_volume(state, ev.volume);
    ev.volume = nominal_volume;

    let mut playback = serde_json::to_value(&ev).unwrap_or_else(|_| serde_json::json!({}));
    if let Value::Object(map) = &mut playback {
        map.insert("muted".into(), serde_json::json!(muted));
        map.insert("queue_len".into(), serde_json::json!(queue.total_tracks));
    }

    let track = queue
        .current_track
        .as_ref()
        .map(|t| serde_json::to_value(t).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);

    json(200, serde_json::json!({"playback": playback, "track": track}))
}

/// `POST /api/playback/play` (02 §3.3.5). Resume if paused; cold-start the
/// current queue track when `!has_loaded_audio()` (the desktop's
/// `toggle_play_pause` cold-start branch, `crates/qbz/src/playback.rs:3837-3860`).
pub fn play(state: &ApiState) -> Response<Cursor<Vec<u8>>> {
    let player = state.runtime.core().player();
    if player.has_loaded_audio() {
        return match state.runtime.core().resume() {
            Ok(()) => json(200, serde_json::json!({"state": "playing"})),
            Err(e) => runtime_error(&e.to_string()),
        };
    }
    match cold_start(state) {
        Ok(()) => json(200, serde_json::json!({"state": "playing"})),
        Err(resp) => resp,
    }
}

/// `POST /api/playback/pause` (02 §3.3.6). Never cold-starts; exit set is
/// 0 · 1 · 3 (no 5, §2.2) so a `Player::pause` channel failure is
/// [`runtime_error`], not [`device_error`].
pub fn pause(state: &ApiState) -> Response<Cursor<Vec<u8>>> {
    match state.runtime.core().pause() {
        Ok(()) => json(200, serde_json::json!({"state": "paused"})),
        Err(e) => runtime_error(&e.to_string()),
    }
}

/// `POST /api/playback/toggle` (02 §3.3.7). Mirrors the desktop's
/// `toggle_play_pause`: playing -> pause; paused-with-loaded-audio -> resume;
/// nothing loaded -> cold-start (same gate as `play`). Exit 5 is reserved for
/// the cold-start branch (`cold_start`'s own [`device_error`]) — the
/// pause/resume branches use [`runtime_error`] like plain `pause`/`stop`.
pub fn toggle(state: &ApiState) -> Response<Cursor<Vec<u8>>> {
    let player = state.runtime.core().player();
    let ev = player.get_playback_event();
    if ev.is_playing {
        return match state.runtime.core().pause() {
            Ok(()) => json(200, serde_json::json!({"state": "paused"})),
            Err(e) => runtime_error(&e.to_string()),
        };
    }
    if player.has_loaded_audio() {
        return match state.runtime.core().resume() {
            Ok(()) => json(200, serde_json::json!({"state": "playing"})),
            Err(e) => runtime_error(&e.to_string()),
        };
    }
    match cold_start(state) {
        Ok(()) => json(200, serde_json::json!({"state": "playing"})),
        Err(resp) => resp,
    }
}

/// `POST /api/playback/stop` (02 §3.3.8). Never cold-starts; same exit-set
/// reasoning as `pause`.
pub fn stop(state: &ApiState) -> Response<Cursor<Vec<u8>>> {
    match state.runtime.core().stop() {
        Ok(()) => json(200, serde_json::json!({"state": "stopped"})),
        Err(e) => runtime_error(&e.to_string()),
    }
}

/// `POST /api/playback/next` (02 §3.3.9).
pub fn next(state: &ApiState) -> Response<Cursor<Vec<u8>>> {
    advance(state, true)
}

/// `POST /api/playback/previous` (02 §3.3.10).
pub fn previous(state: &ApiState) -> Response<Cursor<Vec<u8>>> {
    advance(state, false)
}

/// `POST /api/playback/seek` (02 §3.3.11). Body `{"position": N}` (absolute)
/// or `{"delta": N}` (additive seconds). Returns the CLAMPED target — the
/// value `Player::seek` will settle on (`qbz-player/src/player/mod.rs:5134`
/// clamps to duration) — rather than a live re-read, since `seek` only sends
/// an async command to the audio thread; the clamp is deterministic so the
/// "post-state" is knowable synchronously.
pub fn seek(state: &ApiState, body: &Value) -> Response<Cursor<Vec<u8>>> {
    let player = state.runtime.core().player();
    if player.is_dsd_direct_active() {
        return err_json(
            409,
            "seek_unsupported_dsd",
            "seek is unsupported in DSD-direct mode (bit-perfect passthrough)",
            "set DSD mode to \"convert\": qbzd setup (Audio screen)",
        );
    }
    let ev = player.get_playback_event();
    let target: u64 = if let Some(pos) = body.get("position").and_then(|v| v.as_u64()) {
        pos
    } else if let Some(delta) = body.get("delta").and_then(|v| v.as_i64()) {
        (ev.position as i64 + delta).max(0) as u64
    } else {
        return err_json(
            400,
            "bad_request",
            "seek requires a 'position' or 'delta' field",
            "body: {\"position\": 90} or {\"delta\": -10}",
        );
    };
    let clamped = if ev.duration > 0 { target.min(ev.duration) } else { target };
    if let Err(e) = state.runtime.core().seek(clamped) {
        return runtime_error(&e.to_string());
    }
    json(200, serde_json::json!({"position": clamped, "duration": ev.duration}))
}

/// `POST /api/playback/volume` (02 §3.3.12). One of three body forms:
/// `{"volume": F}` (absolute 0.0-1.0), `{"delta": F}` (additive), or
/// `{"mute": "on"|"off"|"toggle"}` (also `qbzd mute`'s route — no dedicated
/// route, §2.2). All three are gated by the same DSD-direct guard as `seek`.
pub fn volume(state: &ApiState, body: &Value) -> Response<Cursor<Vec<u8>>> {
    let player = state.runtime.core().player();
    if player.is_dsd_direct_active() {
        return err_json(
            409,
            "volume_fixed_dsd",
            "volume is fixed in DSD-direct mode (bit-perfect passthrough)",
            "set DSD mode to \"convert\": qbzd setup (Audio screen)",
        );
    }
    let live = player.get_playback_event().volume;

    if let Some(mute_arg) = body.get("mute").and_then(|v| v.as_str()) {
        return apply_mute(state, live, mute_arg);
    }

    let (muted_before, nominal_before) = nominal_volume(state, live);
    let target = if let Some(v) = body.get("volume").and_then(|v| v.as_f64()) {
        (v as f32).clamp(0.0, 1.0)
    } else if let Some(d) = body.get("delta").and_then(|v| v.as_f64()) {
        (nominal_before + d as f32).clamp(0.0, 1.0)
    } else {
        return err_json(
            400,
            "bad_request",
            "volume requires a 'volume', 'delta' or 'mute' field",
            "body: {\"volume\": 0.75}",
        );
    };

    // An explicit non-zero target clears an active mute (desktop parity:
    // `crates/qbz/src/playback.rs:3921-3924` — "a non-zero level clears any
    // active mute").
    let mut muted_after = muted_before;
    if target > 0.0 && muted_before {
        if let Ok(mut s) = state.shared.lock() {
            s.muted = false;
        }
        muted_after = false;
    }
    if let Err(e) = state.runtime.core().set_volume(target) {
        return runtime_error(&e.to_string());
    }
    json(200, serde_json::json!({"volume": target, "muted": muted_after}))
}

// ============================ internals ============================

/// `{"mute": "on"|"off"|"toggle"}` — stash-then-zero / restore, mirroring the
/// desktop's `toggle_mute` (`crates/qbz/src/playback.rs:3936-3961`) but
/// against `DaemonShared` instead of process statics. `live` is the player's
/// volume BEFORE this call (the value to stash on a fresh mute).
fn apply_mute(state: &ApiState, live: f32, arg: &str) -> Response<Cursor<Vec<u8>>> {
    let mute_on = match arg {
        "on" => true,
        "off" => false,
        "toggle" => !state.shared.lock().map(|s| s.muted).unwrap_or(false),
        other => {
            return err_json(
                400,
                "bad_request",
                &format!("invalid mute state '{other}' — use on, off, or toggle"),
                "body: {\"mute\": \"toggle\"}",
            )
        }
    };

    let mut guard = match state.shared.lock() {
        Ok(g) => g,
        Err(_) => {
            return err_json(500, "internal", "daemon state lock poisoned", "restart qbzd")
        }
    };

    // Desktop fallback for a never-set / zero premute level (playback.rs:3944,
    // 3956): 0.7, so a mute taken at volume 0 still restores to something
    // audible on unmute.
    let (nominal, set_result) = if mute_on && !guard.muted {
        let stash = if live > 0.0 { live } else { 0.7 };
        guard.premute_volume = stash;
        guard.muted = true;
        (stash, state.runtime.core().set_volume(0.0))
    } else if !mute_on && guard.muted {
        let restored = if guard.premute_volume > 0.0 { guard.premute_volume } else { 0.7 };
        guard.muted = false;
        (restored, state.runtime.core().set_volume(restored))
    } else {
        // Already in the requested state — a no-op that still reports the
        // current nominal level.
        let nominal = if guard.muted { guard.premute_volume } else { live };
        (nominal, Ok(()))
    };
    let muted_now = guard.muted;
    drop(guard);

    if let Err(e) = set_result {
        return runtime_error(&e.to_string());
    }
    json(200, serde_json::json!({"volume": nominal, "muted": muted_now}))
}

/// `next`/`previous` (02 §3.3.9-10): gate on NeedsAuth BEFORE running the
/// ritual (unconditional per those two rows' Errors column, unlike
/// play/toggle's cold-start-only gate), then run
/// `qbz_app::playback_driver::advance_and_play` — the FULL ritual (skip-walk →
/// play → prefetch → persist), never a bare cursor move (02 §2.2 trap). Exit
/// set is 0 · 1 · 3 · 4 (no 5, §2.2), so a ritual failure is [`runtime_error`].
fn advance(state: &ApiState, forward: bool) -> Response<Cursor<Vec<u8>>> {
    if let Some(resp) = auth_gate(state) {
        return resp;
    }
    let quality = resolve_quality(state);
    let result = state.rt.block_on(qbz_app::playback_driver::advance_and_play(
        state.runtime.as_ref(),
        quality,
        forward,
    ));
    match result {
        Ok(Some(track)) => json(200, serde_json::to_value(&track).unwrap_or(Value::Null)),
        Ok(None) => json(200, Value::Null),
        Err(e) => runtime_error(&e),
    }
}

/// `play`/`toggle`'s cold-start branch: gate on NeedsAuth, resolve the
/// current queue track, resolve+play it, then best-effort persist the
/// session — the same ritual tail `advance_and_play` runs, minus the
/// cursor-move (we're playing the CURRENT track, not advancing to a new one)
/// and the gapless prefetch (the running driver's tick-based `ArmGapless`
/// picks that up on a later tick once playback is underway).
fn cold_start(state: &ApiState) -> Result<(), Response<Cursor<Vec<u8>>>> {
    if let Some(resp) = auth_gate(state) {
        return Err(resp);
    }
    let queue = state.rt.block_on(state.runtime.core().get_queue_state());
    let Some(track) = queue.current_track else {
        // No documented error code fits "empty queue" exactly; audio_unavailable
        // (503, exit 5) is the closest frozen taxonomy match — "can't produce
        // audio because there is nothing queued" — and the hint names the fix.
        return Err(err_json(
            503,
            "audio_unavailable",
            "queue is empty, nothing to play",
            "queue a track first: qbzd queue add <TRACK_ID>",
        ));
    };
    let quality = resolve_quality(state);
    let played = state.rt.block_on(state.runtime.core().play_track_resolved(
        track.id,
        quality,
        None,
        None,
        0,
    ));
    if let Err(e) = played {
        return Err(device_error(&e));
    }
    state
        .rt
        .block_on(qbz_app::playback_driver::save_session_now(state.runtime.as_ref()));
    Ok(())
}

/// Streaming quality for a play-time resolve, from the daemon's persisted
/// prefs — the SAME key contract `daemon.rs` uses to seed the driver's
/// `DriverDeps.quality` closure at boot (01 §10.3), so a cold-start play and
/// the next auto-advance never pick different tiers.
fn resolve_quality(state: &ApiState) -> qbz_models::Quality {
    let prefs = qbz_app::settings::daemon_prefs::load_at(&state.roots.data);
    qbz_app::playback_driver::quality_from_key(&prefs.streaming_quality)
}

/// 409 `needs_auth` (01 §6.2 / 02 §3.1.3 example envelope, verbatim).
fn auth_gate(state: &ApiState) -> Option<Response<Cursor<Vec<u8>>>> {
    let needs_auth = state
        .shared
        .lock()
        .map(|s| s.auth == AuthState::NeedsAuth)
        .unwrap_or(false);
    if needs_auth {
        Some(err_json(409, "needs_auth", "not logged in to Qobuz", "run: qbzd login"))
    } else {
        None
    }
}

/// 503 `audio_unavailable` — the frozen taxonomy's device/audio bucket
/// (02 §3.1.3), exit 5. Reserved for GENUINE audio/device conditions: the
/// DSD-direct guards (handled inline via `err_json`, not this helper) and
/// cold-start's `play_track_resolved` failure (no device / stream resolve
/// failed). Each route's documented exit set (02 §2.2) decides which one
/// applies — `pause`/`stop`/plain `seek`/`volume`/`next`/`prev` never list
/// exit 5, so their `Player`/`QbzCore` command failures use
/// [`runtime_error`] instead.
fn device_error(message: &str) -> Response<Cursor<Vec<u8>>> {
    err_json(503, "audio_unavailable", message, "check: qbzd status")
}

/// A generic runtime failure, exit 1 (02 §1.3's catch-all) — e.g. the
/// player's command channel is dead. `code` "internal" is NOT one of
/// `error_from_envelope`'s special-cased codes, so it falls to
/// `CliError::Runtime` client-side.
fn runtime_error(message: &str) -> Response<Cursor<Vec<u8>>> {
    err_json(500, "internal", message, "check: qbzd status")
}

/// The NOMINAL volume (what `now`/`volume`/`mute` all report): the live
/// player volume, EXCEPT while muted, where it's the stashed `premute_volume`
/// — the player's real output is 0.0 while muted, but the reported level
/// stays at what the user set it to, so `vol 80%` keeps reading `80%` through
/// a mute/unmute cycle. Returns `(muted, nominal)`.
fn nominal_volume(state: &ApiState, live: f32) -> (bool, f32) {
    match state.shared.lock() {
        Ok(s) => (s.muted, if s.muted { s.premute_volume } else { live }),
        Err(_) => (false, live),
    }
}

fn repeat_str(mode: qbz_models::RepeatMode) -> String {
    match mode {
        qbz_models::RepeatMode::Off => "off",
        qbz_models::RepeatMode::All => "all",
        qbz_models::RepeatMode::One => "one",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_str_matches_contract_lowercase() {
        assert_eq!(repeat_str(qbz_models::RepeatMode::Off), "off");
        assert_eq!(repeat_str(qbz_models::RepeatMode::All), "all");
        assert_eq!(repeat_str(qbz_models::RepeatMode::One), "one");
    }
}
