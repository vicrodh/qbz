/**
 * Queue State Store
 *
 * Manages playback queue, shuffle, repeat, and local track tracking.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// ============ Types ============

export interface QueueTrack {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  duration: string;
  available?: boolean; // Whether track is available (false when offline without local copy)
  trackId?: number; // For favorite checking
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
  is_local?: boolean;
  album_id?: string | null;
  artist_id?: number | null;
  /** Whether the track is streamable on Qobuz (false = removed/unavailable) */
  streamable?: boolean;
  /** Track source: qobuz | local | plex */
  source?: string;
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

// ============ Offline Mode State ============

let isOfflineMode = false;
let tracksWithLocalCopies = new Set<number>();

/**
 * Set offline mode state for queue availability checking
 */
export function setOfflineMode(offline: boolean): void {
  isOfflineMode = offline;
  // Refresh queue to update availability
  if (queue.length > 0) {
    syncQueueState();
  }
}

/**
 * Update tracks with local copies (called from offline store)
 */
export async function updateLocalCopiesSet(): Promise<void> {
  if (!isOfflineMode || queue.length === 0) {
    tracksWithLocalCopies = new Set();
    return;
  }

  try {
    const trackIds = queue.map(t => parseInt(t.id)).filter(id => !isNaN(id));
    if (trackIds.length === 0) {
      tracksWithLocalCopies = new Set();
      return;
    }

    const localIds = await invoke<number[]>('playlist_get_tracks_with_local_copies', {
      trackIds
    });
    tracksWithLocalCopies = new Set(localIds);

    // Update queue availability
    queue = queue.map(t => {
      const numId = parseInt(t.id);
      return {
        ...t,
        available: isNaN(numId) || localTrackIds.has(numId) || tracksWithLocalCopies.has(numId)
      };
    });
    notifyListeners();
  } catch (err) {
    console.error('Failed to check local copies:', err);
    tracksWithLocalCopies = new Set();
  }
}

// ============ Queue Actions ============

/**
 * Sync queue state from backend (V2)
 */
export async function syncQueueState(): Promise<void> {
  try {
    const queueState = await invoke<BackendQueueState>('v2_get_queue_state');

    // Get track IDs for local copy check
    const trackIds = queueState.upcoming.map(t => t.id);

    // Check local copies if in offline mode
    let localCopies = new Set<number>();
    if (isOfflineMode && trackIds.length > 0) {
      try {
        const localIds = await invoke<number[]>('playlist_get_tracks_with_local_copies', {
          trackIds
        });
        localCopies = new Set(localIds);
        tracksWithLocalCopies = localCopies;
      } catch {
        // Ignore errors, assume all available
      }
    }

    // Convert backend queue tracks to frontend format
    queue = queueState.upcoming.map(t => ({
      id: String(t.id),
      artwork: t.artwork_url || '',
      title: t.title,
      artist: t.artist,
      duration: formatDuration(t.duration_secs),
      available: !isOfflineMode || localTrackIds.has(t.id) || localCopies.has(t.id)
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
 * Toggle shuffle mode (V2)
 */
export async function toggleShuffle(): Promise<{ success: boolean; enabled: boolean }> {
  const newState = !isShuffle;
  isShuffle = newState;
  notifyListeners();

  try {
    await invoke('v2_toggle_shuffle');
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
 * Toggle repeat mode (off -> all -> one -> off) (V2)
 */
export async function toggleRepeat(): Promise<{ success: boolean; mode: RepeatMode }> {
  const nextMode: RepeatMode = repeatMode === 'off' ? 'all' : repeatMode === 'all' ? 'one' : 'off';
  // V2 expects capitalized repeat mode: 'Off' | 'All' | 'One'
  const v2Mode = nextMode.charAt(0).toUpperCase() + nextMode.slice(1);

  try {
    await invoke('v2_set_repeat_mode', { mode: v2Mode });
    repeatMode = nextMode;
    notifyListeners();
    return { success: true, mode: nextMode };
  } catch (err) {
    console.error('Failed to set repeat:', err);
    return { success: false, mode: repeatMode };
  }
}

/**
 * Add track to play next in queue (V2)
 */
export async function addToQueueNext(track: BackendQueueTrack, isLocal = false): Promise<boolean> {
  try {
    await invoke('v2_add_to_queue_next', { track });
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
 * Add track to end of queue (V2)
 */
export async function addToQueue(track: BackendQueueTrack, isLocal = false): Promise<boolean> {
  try {
    await invoke('v2_add_to_queue', { track });
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
 * Add multiple tracks to queue (V2)
 */
export async function addTracksToQueue(tracks: BackendQueueTrack[]): Promise<boolean> {
  try {
    await invoke('v2_add_tracks_to_queue', { tracks });
    await syncQueueState();
    return true;
  } catch (err) {
    console.error('Failed to add tracks to queue:', err);
    return false;
  }
}

/**
 * Set queue with new tracks (V2)
 */
export async function setQueue(tracks: BackendQueueTrack[], startIndex: number, clearLocal = true): Promise<boolean> {
  try {
    await invoke('v2_set_queue', { tracks, startIndex });
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
 * Clear the queue (V2)
 */
export async function clearQueue(): Promise<boolean> {
  try {
    await invoke('v2_clear_queue');
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
 * Play track at specific index in queue (V2)
 */
export async function playQueueIndex(index: number): Promise<BackendQueueTrack | null> {
  try {
    return await invoke<BackendQueueTrack | null>('v2_play_queue_index', { index });
  } catch (err) {
    console.error('Failed to play queue index:', err);
    return null;
  }
}

/**
 * Get next track from queue (V2)
 */
export async function nextTrack(): Promise<BackendQueueTrack | null> {
  try {
    return await invoke<BackendQueueTrack | null>('v2_next_track');
  } catch (err) {
    console.error('Failed to get next track:', err);
    return null;
  }
}

/**
 * Get previous track from queue (V2)
 */
export async function previousTrack(): Promise<BackendQueueTrack | null> {
  try {
    return await invoke<BackendQueueTrack | null>('v2_previous_track');
  } catch (err) {
    console.error('Failed to get previous track:', err);
    return null;
  }
}

/**
 * Move a track from one position to another in the queue (V2)
 */
export async function moveQueueTrack(fromIndex: number, toIndex: number): Promise<boolean> {
  try {
    const success = await invoke<boolean>('v2_move_queue_track', { fromIndex, toIndex });
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
 * Get the backend queue state (for advanced queue operations) (V2)
 */
export async function getBackendQueueState(): Promise<BackendQueueState | null> {
  try {
    return await invoke<BackendQueueState>('v2_get_queue_state');
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

// ============ Event Listeners ============

let queueEventUnlisten: UnlistenFn | null = null;

interface QueueStateEvent {
  shuffle: boolean;
  repeat: string;
}

/**
 * Start listening for queue state events from backend (e.g., shuffle/repeat changes from remote control)
 */
export async function startQueueEventListener(): Promise<void> {
  if (queueEventUnlisten) return;

  try {
    queueEventUnlisten = await listen<QueueStateEvent>('queue:state', (event) => {
      console.log('[Queue] Received queue state event:', event.payload);
      // Update local state directly from event
      isShuffle = event.payload.shuffle;
      repeatMode = event.payload.repeat.toLowerCase() as RepeatMode;
      notifyListeners();
    });
    console.log('[Queue] Started listening for queue state events');
  } catch (err) {
    console.error('[Queue] Failed to start queue event listener:', err);
  }
}

/**
 * Stop listening for queue state events
 */
export function stopQueueEventListener(): void {
  if (queueEventUnlisten) {
    queueEventUnlisten();
    queueEventUnlisten = null;
    console.log('[Queue] Stopped listening for queue state events');
  }
}
