/**
 * Queue State Store
 *
 * Manages playback queue, shuffle, repeat, and local track tracking.
 */

import { invoke } from '@tauri-apps/api/core';

// ============ Types ============

export interface QueueTrack {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  duration: string;
}

export interface BackendQueueTrack {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration_secs: number;
  artwork_url: string | null;
  hires: boolean;
  bit_depth: number | null;
  sample_rate: number | null;
}

interface BackendQueueState {
  current_track: BackendQueueTrack | null;
  current_index: number | null;
  upcoming: BackendQueueTrack[];
  history: BackendQueueTrack[];
  shuffle: boolean;
  repeat: 'Off' | 'All' | 'One';
  total_tracks: number;
}

export type RepeatMode = 'off' | 'all' | 'one';

// ============ State ============

let queue: QueueTrack[] = [];
let queueTotalTracks = 0;
let isShuffle = false;
let repeatMode: RepeatMode = 'off';

// Local library track IDs in current queue (for distinguishing from Qobuz tracks)
let localTrackIds = new Set<number>();

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Subscribe to queue state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener(); // Immediately notify with current state
  return () => listeners.delete(listener);
}

// ============ Getters ============

export function getQueue(): QueueTrack[] {
  return queue;
}

export function getQueueTotalTracks(): number {
  return queueTotalTracks;
}

export function getIsShuffle(): boolean {
  return isShuffle;
}

export function getRepeatMode(): RepeatMode {
  return repeatMode;
}

export function isLocalTrack(trackId: number): boolean {
  return localTrackIds.has(trackId);
}

// ============ State Getters ============

export interface QueueState {
  queue: QueueTrack[];
  queueTotalTracks: number;
  isShuffle: boolean;
  repeatMode: RepeatMode;
}

export function getQueueState(): QueueState {
  return {
    queue: [...queue],
    queueTotalTracks,
    isShuffle,
    repeatMode
  };
}

// ============ Internal Helpers ============

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, '0')}`;
}

// ============ Queue Actions ============

/**
 * Sync queue state from backend
 */
export async function syncQueueState(): Promise<void> {
  try {
    const queueState = await invoke<BackendQueueState>('get_queue_state');

    // Convert backend queue tracks to frontend format
    queue = queueState.upcoming.map(t => ({
      id: String(t.id),
      artwork: t.artwork_url || '',
      title: t.title,
      artist: t.artist,
      duration: formatDuration(t.duration_secs)
    }));

    queueTotalTracks = queueState.total_tracks;
    isShuffle = queueState.shuffle;
    repeatMode = queueState.repeat.toLowerCase() as RepeatMode;
    notifyListeners();
  } catch (err) {
    console.error('Failed to sync queue state:', err);
  }
}

/**
 * Toggle shuffle mode
 */
export async function toggleShuffle(): Promise<{ success: boolean; enabled: boolean }> {
  const newState = !isShuffle;
  isShuffle = newState;
  notifyListeners();

  try {
    await invoke('set_shuffle', { enabled: newState });
    return { success: true, enabled: newState };
  } catch (err) {
    console.error('Failed to set shuffle:', err);
    // Revert on error
    isShuffle = !newState;
    notifyListeners();
    return { success: false, enabled: !newState };
  }
}

/**
 * Toggle repeat mode (off -> all -> one -> off)
 */
export async function toggleRepeat(): Promise<{ success: boolean; mode: RepeatMode }> {
  const nextMode: RepeatMode = repeatMode === 'off' ? 'all' : repeatMode === 'all' ? 'one' : 'off';

  try {
    await invoke('set_repeat', { mode: nextMode });
    repeatMode = nextMode;
    notifyListeners();
    return { success: true, mode: nextMode };
  } catch (err) {
    console.error('Failed to set repeat:', err);
    return { success: false, mode: repeatMode };
  }
}

/**
 * Add track to play next in queue
 */
export async function addToQueueNext(track: BackendQueueTrack, isLocal = false): Promise<boolean> {
  try {
    await invoke('add_to_queue_next', { track });
    if (isLocal) {
      localTrackIds = new Set([...localTrackIds, track.id]);
    }
    await syncQueueState();
    return true;
  } catch (err) {
    console.error('Failed to queue track next:', err);
    return false;
  }
}

/**
 * Add track to end of queue
 */
export async function addToQueue(track: BackendQueueTrack, isLocal = false): Promise<boolean> {
  try {
    await invoke('add_to_queue', { track });
    if (isLocal) {
      localTrackIds = new Set([...localTrackIds, track.id]);
    }
    await syncQueueState();
    return true;
  } catch (err) {
    console.error('Failed to add to queue:', err);
    return false;
  }
}

/**
 * Add multiple tracks to queue
 */
export async function addTracksToQueue(tracks: BackendQueueTrack[]): Promise<boolean> {
  try {
    await invoke('add_tracks_to_queue', { tracks });
    await syncQueueState();
    return true;
  } catch (err) {
    console.error('Failed to add tracks to queue:', err);
    return false;
  }
}

/**
 * Set queue with new tracks
 */
export async function setQueue(tracks: BackendQueueTrack[], startIndex: number, clearLocal = true): Promise<boolean> {
  try {
    await invoke('set_queue', { tracks, startIndex });
    if (clearLocal) {
      localTrackIds = new Set();
    }
    await syncQueueState();
    return true;
  } catch (err) {
    console.error('Failed to set queue:', err);
    return false;
  }
}

/**
 * Clear the queue
 */
export async function clearQueue(): Promise<boolean> {
  try {
    await invoke('clear_queue');
    queue = [];
    queueTotalTracks = 0;
    notifyListeners();
    return true;
  } catch (err) {
    console.error('Failed to clear queue:', err);
    return false;
  }
}

/**
 * Play track at specific index in queue
 */
export async function playQueueIndex(index: number): Promise<BackendQueueTrack | null> {
  try {
    return await invoke<BackendQueueTrack | null>('play_queue_index', { index });
  } catch (err) {
    console.error('Failed to play queue index:', err);
    return null;
  }
}

/**
 * Get next track from queue
 */
export async function nextTrack(): Promise<BackendQueueTrack | null> {
  try {
    return await invoke<BackendQueueTrack | null>('next_track');
  } catch (err) {
    console.error('Failed to get next track:', err);
    return null;
  }
}

/**
 * Get previous track from queue
 */
export async function previousTrack(): Promise<BackendQueueTrack | null> {
  try {
    return await invoke<BackendQueueTrack | null>('previous_track');
  } catch (err) {
    console.error('Failed to get previous track:', err);
    return null;
  }
}

/**
 * Move a track from one position to another in the queue
 */
export async function moveQueueTrack(fromIndex: number, toIndex: number): Promise<boolean> {
  try {
    const success = await invoke<boolean>('move_queue_track', { fromIndex, toIndex });
    if (success) {
      await syncQueueState();
    }
    return success;
  } catch (err) {
    console.error('Failed to move queue track:', err);
    return false;
  }
}

// ============ Local Track Management ============

/**
 * Set local track IDs (when playing from local library)
 */
export function setLocalTrackIds(trackIds: number[]): void {
  localTrackIds = new Set(trackIds);
  console.log(`Set ${trackIds.length} local track IDs in queue`);
}

/**
 * Clear local track IDs
 */
export function clearLocalTrackIds(): void {
  localTrackIds = new Set();
}

/**
 * Get the backend queue state (for advanced queue operations)
 */
export async function getBackendQueueState(): Promise<BackendQueueState | null> {
  try {
    return await invoke<BackendQueueState>('get_queue_state');
  } catch (err) {
    console.error('Failed to get backend queue state:', err);
    return null;
  }
}

// ============ Cleanup ============

/**
 * Reset queue state
 */
export function reset(): void {
  queue = [];
  queueTotalTracks = 0;
  isShuffle = false;
  repeatMode = 'off';
  localTrackIds = new Set();
  notifyListeners();
}
