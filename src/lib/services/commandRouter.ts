/**
 * Command Router
 *
 * Thin dispatch layer over Tauri invoke() for playback, queue, and audio
 * commands. Stores and services call these functions instead of invoke()
 * directly so the V2 command names live in one place.
 *
 * This used to bifurcate between local invoke() and a remote HTTP daemon
 * (qbzd). The qbzd daemon was a premature experiment and has been removed;
 * the app always controls local playback now. `isRemote()` / `skipIfRemote()`
 * are kept as always-false no-ops so the ~100 `if (skipIfRemote()) return`
 * guards scattered across stores/services keep compiling without edits.
 */
import { invoke } from '@tauri-apps/api/core';

// ==================== Local-only no-ops (kept for call-site compatibility) ====================

/** Remote targeting was removed — the app is always local. */
export function isRemote(): boolean {
  return false;
}

/**
 * Guard kept for call-site compatibility. Always false now (no remote target),
 * so guarded local-only operations always proceed.
 */
export function skipIfRemote(): boolean {
  return false;
}

// ==================== Playback ====================

export async function cmdPause(): Promise<void> {
  await invoke('v2_pause_playback');
}

export async function cmdResume(): Promise<void> {
  await invoke('v2_resume_playback');
}

export async function cmdStop(): Promise<void> {
  await invoke('v2_stop_playback');
}

export async function cmdSeek(position: number): Promise<void> {
  await invoke('v2_seek', { position: Math.floor(position) });
}

export async function cmdSetVolume(volume: number): Promise<void> {
  await invoke('v2_set_volume', { volume });
}

export async function cmdNext(): Promise<unknown> {
  return invoke('v2_next_track');
}

export async function cmdPrevious(): Promise<unknown> {
  return invoke('v2_previous_track');
}

export async function cmdPlayTrack(
  trackId: number,
  quality?: string,
  durationSecs?: number | null,
  forceLowestQuality?: boolean | null,
  startPositionSecs?: number | null,
): Promise<unknown> {
  // Parameter names MUST match Tauri's camelCase mapping of Rust
  // v2_play_track(track_id, quality, force_lowest_quality, duration_secs,
  // start_position_secs). duration_secs is required on the streaming
  // path — without it the backend stores duration=0 and
  // current_position() clamps to 0, freezing the seekbar (seen on
  // session-restore first play). start_position_secs is still accepted
  // by the backend but the frontend no longer uses it: session resume
  // now seeks to the saved offset only on a cache hit, where the audio
  // is fully in memory and Seek lands (see playerStore togglePlay).
  return invoke('v2_play_track', {
    trackId,
    quality: quality ?? null,
    forceLowestQuality: forceLowestQuality ?? null,
    durationSecs: durationSecs ?? null,
    startPositionSecs: startPositionSecs ?? null,
  });
}

// ==================== Queue ====================

export async function cmdSetQueue(tracks: unknown[], startIndex: number): Promise<void> {
  await invoke('v2_set_queue', { tracks, startIndex });
}

export async function cmdAddToQueue(track: unknown): Promise<void> {
  await invoke('v2_add_to_queue', { track });
}

export async function cmdAddToQueueNext(track: unknown): Promise<void> {
  await invoke('v2_add_to_queue_next', { track });
}

export async function cmdAddTracksToQueue(tracks: unknown[]): Promise<void> {
  await invoke('v2_add_tracks_to_queue', { tracks });
}

export async function cmdAddTracksToQueueNext(tracks: unknown[]): Promise<void> {
  await invoke('v2_add_tracks_to_queue_next', { tracks });
}

export async function cmdClearQueue(opts?: { includeCurrent?: boolean }): Promise<void> {
  await invoke('v2_clear_queue', { includeCurrent: opts?.includeCurrent ?? false });
}

export async function cmdToggleShuffle(): Promise<void> {
  await invoke('v2_toggle_shuffle');
}

export async function cmdSetRepeatMode(mode: string): Promise<void> {
  // Map frontend mode names to V2 command format
  const v2Mode = mode === 'one' ? 'One' : mode === 'all' ? 'All' : 'Off';
  await invoke('v2_set_repeat_mode', { mode: v2Mode });
}

// ==================== Audio Settings ====================

export async function cmdGetAudioSettings(): Promise<unknown> {
  return invoke('v2_get_audio_settings');
}

export async function cmdGetAvailableBackends(): Promise<unknown> {
  return invoke('v2_get_available_backends');
}

export async function cmdGetDevicesForBackend(backendType: string): Promise<unknown> {
  return invoke('v2_get_devices_for_backend', { backendType });
}

export async function cmdGetHardwareAudioStatus(): Promise<unknown> {
  return invoke('v2_get_hardware_audio_status');
}
