/**
 * Queue State Store
 *
 * Manages playback queue, shuffle, repeat, and local track tracking.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { cmdToggleShuffle, cmdSetRepeatMode, cmdAddToQueue, cmdAddToQueueNext, cmdAddTracksToQueue, cmdAddTracksToQueueNext, cmdSetQueue, cmdClearQueue } from '$lib/services/commandRouter';

// ============ Types ============

export interface QueueTrack {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  duration: string;
  available?: boolean; // Whether track is available (false when offline without local copy)
  trackId?: number; // For favorite checking
  parental_warning?: boolean;
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
  parental_warning?: boolean;
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
let pendingRepeatMode: RepeatMode | null = null;
let hasAuthoritativeRepeatSnapshot = false;

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

function normalizeRepeatMode(value: string): RepeatMode {
  const normalized = value.toLowerCase();
  if (normalized === 'all' || normalized === 'one') {
    return normalized;
  }
  return 'off';
}

async function resolveAuthoritativeRepeatMode(): Promise<RepeatMode> {
  if (pendingRepeatMode) {
    return pendingRepeatMode;
  }

  if (hasAuthoritativeRepeatSnapshot) {
    return repeatMode;
  }

  try {
    const queueState = await invoke<BackendQueueState>('v2_get_queue_state');
    const authoritativeRepeatMode = normalizeRepeatMode(queueState.repeat);

    if (
      authoritativeRepeatMode !== repeatMode ||
      queueState.shuffle !== isShuffle ||
      queueState.total_tracks !== queueTotalTracks ||
      queueState.upcoming.length !== queue.length
    ) {
      await applyBackendQueueState(queueState);
    }

    hasAuthoritativeRepeatSnapshot = true;
    return authoritativeRepeatMode;
  } catch (err) {
    console.warn('[Queue] Failed to fetch authoritative repeat mode before toggle:', err);
    return repeatMode;
  }
}

async function applyBackendQueueState(queueState: BackendQueueState): Promise<void> {
  const trackIds = queueState.upcoming.map(track => track.id);

  let localCopies = new Set<number>();
  if (isOfflineMode && trackIds.length > 0) {
    try {
      const localIds = await invoke<number[]>('v2_playlist_get_tracks_with_local_copies', {
        trackIds
      });
      localCopies = new Set(localIds);
      tracksWithLocalCopies = localCopies;
    } catch {
      // Ignore errors, assume all available
    }
  }

  queue = queueState.upcoming.map(track => ({
    id: String(track.id),
    artwork: track.artwork_url || '',
    title: track.title,
    artist: track.artist,
    duration: formatDuration(track.duration_secs),
    available: !isOfflineMode || localTrackIds.has(track.id) || localCopies.has(track.id),
    parental_warning: track.parental_warning ?? false
  }));

  queueTotalTracks = queueState.total_tracks;
  isShuffle = queueState.shuffle;
  repeatMode = normalizeRepeatMode(queueState.repeat);
  hasAuthoritativeRepeatSnapshot = true;
  if (pendingRepeatMode === repeatMode) {
    pendingRepeatMode = null;
  }
  notifyListeners();
}

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
    const trackIds = queue.map(track => Number.parseInt(track.id)).filter(id => !Number.isNaN(id));
    if (trackIds.length === 0) {
      tracksWithLocalCopies = new Set();
      return;
    }

    const localIds = await invoke<number[]>('v2_playlist_get_tracks_with_local_copies', {
      trackIds
    });
    tracksWithLocalCopies = new Set(localIds);

    // Update queue availability
    queue = queue.map(track => {
      const numId = Number.parseInt(track.id);
      return {
        ...track,
        available: Number.isNaN(numId) || localTrackIds.has(numId) || tracksWithLocalCopies.has(numId)
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
    await applyBackendQueueState(queueState);
  } catch (err) {
    console.error('Failed to sync queue state:', err);
  }
}

/**
 * Toggle shuffle mode (V2)
 */
export async function toggleShuffle(): Promise<{ success: boolean; enabled: boolean }> {
  const newState = !isShuffle;

  try {
    await cmdToggleShuffle();
    return { success: true, enabled: newState };
  } catch (err) {
    console.error('Failed to set shuffle:', err);
    return { success: false, enabled: !newState };
  }
}

/**
 * Toggle repeat mode (off -> all -> one -> off) (V2)
 */
export async function toggleRepeat(): Promise<{ success: boolean; mode: RepeatMode }> {
  const currentMode = await resolveAuthoritativeRepeatMode();
  const nextMode: RepeatMode = currentMode === 'off' ? 'all' : currentMode === 'all' ? 'one' : 'off';

  try {
    await cmdSetRepeatMode(nextMode);
    pendingRepeatMode = nextMode;
    return { success: true, mode: nextMode };
  } catch (err) {
    console.error('Failed to set repeat:', err);
    pendingRepeatMode = null;
    return { success: false, mode: repeatMode };
  }
}

/**
 * Add track to play next in queue (V2)
 */
export async function addToQueueNext(track: BackendQueueTrack, isLocal = false): Promise<boolean> {
  try {
    await cmdAddToQueueNext(track);
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
    await cmdAddToQueue(track);
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
    await cmdAddTracksToQueue(tracks);
    await syncQueueState();
    return true;
  } catch (err) {
    console.error('Failed to add tracks to queue:', err);
    return false;
  }
}

/**
 * Add multiple tracks to play next in queue (V2)
 * Backend reverses the order so they play in the correct sequence.
 */
export async function addTracksToQueueNext(tracks: BackendQueueTrack[]): Promise<boolean> {
  try {
    await cmdAddTracksToQueueNext(tracks);
    await syncQueueState();
    return true;
  } catch (err) {
    console.error('Failed to add tracks to queue next:', err);
    return false;
  }
}

/**
 * Queue epoch counter — incremented on every setQueue/clearQueue call.
 * Used by nextTrackGuarded() to discard stale auto-advance results
 * that arrive after a context switch (album/playlist change).
 */
let queueEpoch = 0;

/** Current queue epoch (read-only for callers). */
export function getQueueEpoch(): number {
  return queueEpoch;
}

/**
 * Set queue with new tracks (V2)
 */
export async function setQueue(tracks: BackendQueueTrack[], startIndex: number, clearLocal = true): Promise<boolean> {
  try {
    queueEpoch++;
    await cmdSetQueue(tracks, startIndex);
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
    queueEpoch++;
    await cmdClearQueue();
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
 * Get next track, but discard the result if the queue changed while
 * the invoke was in-flight (prevents ghost auto-advance after context switch).
 */
export async function nextTrackGuarded(): Promise<BackendQueueTrack | null> {
  const epochBefore = queueEpoch;
  const result = await nextTrack();
  if (queueEpoch !== epochBefore) {
    console.warn('[Queue] Discarding stale next_track result (queue changed during invoke)');
    return null;
  }
  return result;
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
  pendingRepeatMode = null;
  hasAuthoritativeRepeatSnapshot = false;
  localTrackIds = new Set();
  tracksWithLocalCopies = new Set();
  notifyListeners();
}

// ============ Event Listeners ============

let queueEventUnlisteners: UnlistenFn[] = [];

interface QueueStateEvent {
  shuffle: boolean;
  repeat: string;
}

/**
 * Start listening for queue state events from backend (e.g., shuffle/repeat changes from remote control)
 */
export async function startQueueEventListener(): Promise<void> {
  if (queueEventUnlisteners.length > 0) return;

  try {
    const queueUpdatedUnlisten = await listen<BackendQueueState>('queue:updated', (event) => {
      console.log('[Queue] Received queue:updated event:', event.payload);
      applyBackendQueueState(event.payload).catch(err =>
        console.error('[Queue] Failed to apply queue:updated event:', err)
      );
    });

    const shuffleChangedUnlisten = await listen<boolean>('queue:shuffle-changed', (event) => {
      console.log('[Queue] Received queue:shuffle-changed event:', event.payload);
      // Shuffle changes must be reflected by the authoritative queue:updated
      // payload, not by forcing a local resync that can capture an
      // intermediate non-authoritative order.
    });

    const repeatChangedUnlisten = await listen<string>('queue:repeat-changed', (event) => {
      console.log('[Queue] Received queue:repeat-changed event:', event.payload);
      repeatMode = normalizeRepeatMode(event.payload);
      hasAuthoritativeRepeatSnapshot = true;
      if (pendingRepeatMode === repeatMode) {
        pendingRepeatMode = null;
      }
      notifyListeners();
    });

    const queueStateUnlisten = await listen<QueueStateEvent>('queue:state', (event) => {
      console.log('[Queue] Received queue:state event:', event.payload);
      isShuffle = event.payload.shuffle;
      repeatMode = normalizeRepeatMode(event.payload.repeat);
      hasAuthoritativeRepeatSnapshot = true;
      if (pendingRepeatMode === repeatMode) {
        pendingRepeatMode = null;
      }
      notifyListeners();
      syncQueueState().catch(err =>
        console.error('[Queue] Failed to sync queue after queue:state event:', err)
      );
    });

    queueEventUnlisteners = [
      queueUpdatedUnlisten,
      shuffleChangedUnlisten,
      repeatChangedUnlisten,
      queueStateUnlisten
    ];
    console.log('[Queue] Started listening for queue events');
  } catch (err) {
    console.error('[Queue] Failed to start queue event listener:', err);
  }
}

/**
 * Stop listening for queue state events
 */
export function stopQueueEventListener(): void {
  if (queueEventUnlisteners.length > 0) {
    for (const unlisten of queueEventUnlisteners) {
      unlisten();
    }
    queueEventUnlisteners = [];
    console.log('[Queue] Stopped listening for queue events');
  }
}
