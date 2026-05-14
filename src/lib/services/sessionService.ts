/**
 * Session Persistence Service
 *
 * Handles saving and restoring playback state between app sessions.
 * Saves: queue, current track, position, volume, shuffle/repeat modes
 */

import { invoke } from '@tauri-apps/api/core';
import { skipIfRemote } from '$lib/services/commandRouter';

export interface PersistedQueueTrack {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration_secs: number;
  artwork_url: string | null;
  hires?: boolean;
  bit_depth?: number | null;
  sample_rate?: number | null;
  is_local?: boolean;
  album_id?: string | null;
  artist_id?: number | null;
  // Must round-trip through save/restore — otherwise a LocalLibrary queue
  // comes back as a Qobuz queue and auto-advance routes track_ids to
  // v2_play_track (Qobuz) instead of v2_library_play_track. Backend also
  // keeps this in session_store; this interface just needs to carry it.
  source?: string | null;
  // Round-tripped fields, previously hardcoded on load. Persisting them
  // means a restored queue matches the live one for explicit-content
  // badges, unstreamable visual state, and Mixtape source association.
  streamable?: boolean;
  parental_warning?: boolean;
  source_item_id_hint?: string | null;
}

export interface PersistedSession {
  queue_tracks: PersistedQueueTrack[];
  current_index: number | null;
  current_position_secs: number;
  volume: number;
  shuffle_enabled: boolean;
  repeat_mode: string; // "off" | "all" | "one"
  was_playing: boolean;
  saved_at: number;
  last_view: string;
  view_context_id: string | null;
  view_context_type: string | null;
}

/**
 * Save the complete session state
 */
export async function saveSessionState(
  queueTracks: PersistedQueueTrack[],
  currentIndex: number | null,
  currentPositionSecs: number,
  volume: number,
  shuffleEnabled: boolean,
  repeatMode: string,
  wasPlaying: boolean,
  lastView?: string,
  viewContextId?: string | null,
  viewContextType?: string | null
): Promise<void> {
  if (skipIfRemote()) return;
  try {
    await invoke('v2_save_session_state', {
      queueTracks,
      currentIndex,
      currentPositionSecs,
      volume,
      shuffleEnabled,
      repeatMode,
      wasPlaying,
      lastView: lastView ?? 'home',
      viewContextId: viewContextId ?? null,
      viewContextType: viewContextType ?? null,
    });
    console.log('[Session] State saved successfully');
  } catch (err) {
    console.error('[Session] Failed to save state:', err);
  }
}

/**
 * Load the persisted session state
 */
export async function loadSessionState(): Promise<PersistedSession | null> {
  if (skipIfRemote()) return null;
  try {
    const session = await invoke<PersistedSession>('v2_load_session_state');
    console.log('[Session] State loaded:', {
      tracks: session.queue_tracks.length,
      currentIndex: session.current_index,
      position: session.current_position_secs,
      volume: session.volume,
    });
    return session;
  } catch (err) {
    console.error('[Session] Failed to load state:', err);
    return null;
  }
}

/**
 * Quick save of just the playback position (debounced during playback)
 */
export async function saveSessionPosition(positionSecs: number): Promise<void> {
  if (skipIfRemote()) return;
  try {
    await invoke('v2_save_session_position', { positionSecs });
  } catch (err) {
    console.error('[Session] Failed to save position:', err);
  }
}

/**
 * Quick save of volume
 */
export async function saveSessionVolume(volume: number): Promise<void> {
  if (skipIfRemote()) return;
  try {
    await invoke('v2_save_session_volume', { volume });
  } catch (err) {
    console.error('[Session] Failed to save volume:', err);
  }
}

/**
 * Save shuffle and repeat mode
 */
export async function saveSessionPlaybackMode(
  shuffle: boolean,
  repeatMode: string
): Promise<void> {
  if (skipIfRemote()) return;
  try {
    await invoke('v2_save_session_playback_mode', { shuffle, repeatMode });
  } catch (err) {
    console.error('[Session] Failed to save playback mode:', err);
  }
}

/**
 * Clear the session (e.g., on logout)
 */
export async function clearSession(): Promise<void> {
  if (skipIfRemote()) return;
  try {
    await invoke('v2_clear_session');
    console.log('[Session] Session cleared');
  } catch (err) {
    console.error('[Session] Failed to clear session:', err);
  }
}

// Throttle helper for position saves. We THROTTLE (not debounce) because
// position updates arrive continuously while a track plays — a true
// debounce would reset the 5s timer on every update and the save would
// never fire mid-playback, leaving the persisted position frozen at
// whatever value got written during the first quiet window after track
// start. (Real-world symptom: resume position always restored at ~2s,
// no matter how far the user played.) Throttling fires the first call
// immediately, then once per 5s window thereafter, with a tail call to
// flush the latest position when the window ends.
let positionSaveTimeout: ReturnType<typeof setTimeout> | null = null;
let lastSavedPositionMs = 0;
let lastQueuedPositionSecs = 0;
const POSITION_SAVE_THROTTLE_MS = 5000;

export function debouncedSavePosition(positionSecs: number): void {
  lastQueuedPositionSecs = positionSecs;
  const now = Date.now();
  const elapsed = now - lastSavedPositionMs;
  if (elapsed >= POSITION_SAVE_THROTTLE_MS) {
    lastSavedPositionMs = now;
    saveSessionPosition(positionSecs);
    if (positionSaveTimeout) {
      clearTimeout(positionSaveTimeout);
      positionSaveTimeout = null;
    }
    return;
  }
  if (positionSaveTimeout) return;
  positionSaveTimeout = setTimeout(() => {
    positionSaveTimeout = null;
    lastSavedPositionMs = Date.now();
    saveSessionPosition(lastQueuedPositionSecs);
  }, POSITION_SAVE_THROTTLE_MS - elapsed);
}

/**
 * Force save position immediately (e.g., on pause or app close).
 * Resets throttle bookkeeping so the next debouncedSavePosition fires
 * fresh.
 */
export function flushPositionSave(positionSecs: number): void {
  if (positionSaveTimeout) {
    clearTimeout(positionSaveTimeout);
    positionSaveTimeout = null;
  }
  lastSavedPositionMs = Date.now();
  lastQueuedPositionSecs = positionSecs;
  saveSessionPosition(positionSecs);
}

/**
 * Build a complete session state from current app state and save it
 */
export async function saveCurrentSession(
  getQueueState: () => { tracks: Array<{ id: number; title: string; artist: string; album: string; duration_secs: number; artwork_url: string | null }>; currentIndex: number | null },
  getPlayerState: () => { currentTime: number; volume: number; isPlaying: boolean },
  getPlaybackMode: () => { shuffle: boolean; repeat: string }
): Promise<void> {
  if (skipIfRemote()) return;
  const queueState = getQueueState();
  const playerState = getPlayerState();
  const playbackMode = getPlaybackMode();

  await saveSessionState(
    queueState.tracks,
    queueState.currentIndex,
    Math.floor(playerState.currentTime),
    playerState.volume / 100, // Convert 0-100 to 0-1
    playbackMode.shuffle,
    playbackMode.repeat,
    playerState.isPlaying
  );
}
