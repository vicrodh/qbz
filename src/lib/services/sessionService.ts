/**
 * Session Persistence Service
 *
 * Handles saving and restoring playback state between app sessions.
 * Saves: queue, current track, position, volume, shuffle/repeat modes
 */

import { invoke } from '@tauri-apps/api/core';

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
  wasPlaying: boolean
): Promise<void> {
  try {
    await invoke('v2_save_session_state', {
      queueTracks,
      currentIndex,
      currentPositionSecs,
      volume,
      shuffleEnabled,
      repeatMode,
      wasPlaying,
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
  try {
    await invoke('v2_clear_session');
    console.log('[Session] Session cleared');
  } catch (err) {
    console.error('[Session] Failed to clear session:', err);
  }
}

// Debounce helper for position saves
let positionSaveTimeout: ReturnType<typeof setTimeout> | null = null;
const POSITION_SAVE_DEBOUNCE_MS = 5000; // Save position every 5 seconds max

/**
 * Debounced position save - call frequently, saves every 5 seconds
 */
export function debouncedSavePosition(positionSecs: number): void {
  if (positionSaveTimeout) {
    clearTimeout(positionSaveTimeout);
  }
  positionSaveTimeout = setTimeout(() => {
    saveSessionPosition(positionSecs);
    positionSaveTimeout = null;
  }, POSITION_SAVE_DEBOUNCE_MS);
}

/**
 * Force save position immediately (e.g., on pause or app close)
 */
export function flushPositionSave(positionSecs: number): void {
  if (positionSaveTimeout) {
    clearTimeout(positionSaveTimeout);
    positionSaveTimeout = null;
  }
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
