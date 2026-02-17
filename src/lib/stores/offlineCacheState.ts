import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type OfflineCacheStatus = 'none' | 'queued' | 'downloading' | 'ready' | 'failed';

export interface OfflineCacheInfo {
  status: OfflineCacheStatus;
  progress: number;
  error?: string;
}

export interface CachedTrackInfo {
  trackId: number;
  title: string;
  artist: string;
  album?: string;
  albumId?: string;
  durationSecs: number;
  fileSizeBytes: number;
  quality: string;
  bitDepth?: number;
  sampleRate?: number;
  status: OfflineCacheStatus;
  progressPercent: number;
  errorMessage?: string;
  createdAt: string;
  lastAccessedAt: string;
}

export interface OfflineCacheStats {
  totalTracks: number;
  readyTracks: number;
  downloadingTracks: number;
  failedTracks: number;
  totalSizeBytes: number;
  limitBytes?: number;
  cachePath: string;
}

// Track offline cache states by track ID
const offlineCacheStates = new Map<number, OfflineCacheInfo>();

// Listeners for state changes
const listeners = new Set<() => void>();

// Event unsubscribe functions
let unlisteners: UnlistenFn[] = [];
// Flag to prevent listener leaks on fast stop/start cycles
let listenersDisposed = false;

export function getOfflineCacheState(trackId: number): OfflineCacheInfo {
  return offlineCacheStates.get(trackId) || { status: 'none', progress: 0 };
}

export function setOfflineCacheState(trackId: number, info: OfflineCacheInfo): void {
  offlineCacheStates.set(trackId, info);
  notifyListeners();
}

// Check if all tracks of an album are cached for offline
export function isAlbumFullyCached(trackIds: number[]): boolean {
  if (trackIds.length === 0) return false;

  return trackIds.every(trackId => {
    const state = getOfflineCacheState(trackId);
    return state.status === 'ready';
  });
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

let rafScheduled = false;

function notifyListeners(): void {
  if (rafScheduled) return;
  rafScheduled = true;
  requestAnimationFrame(() => {
    rafScheduled = false;
    for (const listener of listeners) {
      listener();
    }
  });
}

// Initialize offline cache states from backend
export async function initOfflineCacheStates(): Promise<void> {
  try {
    const tracks = await invoke<CachedTrackInfo[]>('v2_get_cached_tracks');
    for (const track of tracks) {
      offlineCacheStates.set(track.trackId, {
        status: track.status,
        progress: track.progressPercent,
        error: track.errorMessage,
      });
    }
    notifyListeners();
  } catch (err) {
    console.error('Failed to init offline cache states:', err);
  }
}

// Start listening for offline cache events
export async function startOfflineCacheEventListeners(): Promise<void> {
  // Idempotency guard: prevent duplicate listeners on HMR/remount
  if (unlisteners.length > 0) {
    return;
  }

  // Reset disposed flag when starting
  listenersDisposed = false;

  try {
    const unlistenStarted = await listen<{ trackId: number }>('offline:caching_started', (event) => {
      console.log('Offline caching started:', event.payload.trackId);
      setOfflineCacheState(event.payload.trackId, { status: 'downloading', progress: 0 });
    });
    // Check if stop was called while we were awaiting
    if (listenersDisposed) { unlistenStarted(); return; }

    const unlistenProgress = await listen<{
      trackId: number;
      progressPercent: number;
      bytesDownloaded: number;
      totalBytes?: number;
      status: string;
    }>('offline:caching_progress', (event) => {
      const { trackId, progressPercent } = event.payload;
      setOfflineCacheState(trackId, { status: 'downloading', progress: progressPercent });
    });
    if (listenersDisposed) { unlistenStarted(); unlistenProgress(); return; }

    const unlistenCompleted = await listen<{ trackId: number; size: number }>('offline:caching_completed', (event) => {
      console.log('Offline caching completed:', event.payload.trackId);
      setOfflineCacheState(event.payload.trackId, { status: 'ready', progress: 100 });
    });
    if (listenersDisposed) { unlistenStarted(); unlistenProgress(); unlistenCompleted(); return; }

    const unlistenFailed = await listen<{ trackId: number; error: string }>('offline:caching_failed', (event) => {
      console.error('Offline caching failed:', event.payload.trackId, event.payload.error);
      setOfflineCacheState(event.payload.trackId, {
        status: 'failed',
        progress: 0,
        error: event.payload.error,
      });
    });
    if (listenersDisposed) { unlistenStarted(); unlistenProgress(); unlistenCompleted(); unlistenFailed(); return; }

    unlisteners = [unlistenStarted, unlistenProgress, unlistenCompleted, unlistenFailed];
  } catch (err) {
    console.error('Failed to setup offline cache event listeners:', err);
  }
}

// Stop listening for events
export function stopOfflineCacheEventListeners(): void {
  // Set disposed flag to prevent leaks from pending async registrations
  listenersDisposed = true;
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners = [];
}

// Cache a track for offline listening
export async function cacheTrackForOffline(track: {
  id: number;
  title: string;
  artist: string;
  album?: string;
  albumId?: string;
  durationSecs: number;
  quality: string;
  bitDepth?: number;
  sampleRate?: number;
}): Promise<void> {
  try {
    setOfflineCacheState(track.id, { status: 'queued', progress: 0 });
    await invoke('v2_cache_track_for_offline', {
      trackId: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      albumId: track.albumId,
      durationSecs: track.durationSecs,
      quality: track.quality,
      bitDepth: track.bitDepth,
      sampleRate: track.sampleRate,
    });
  } catch (err) {
    console.error('Failed to cache track for offline:', err);
    setOfflineCacheState(track.id, { status: 'failed', progress: 0, error: String(err) });
    throw err;
  }
}

// Remove a track from offline cache
export async function removeCachedTrack(trackId: number): Promise<void> {
  try {
    await invoke('v2_remove_cached_track', { trackId });
    offlineCacheStates.delete(trackId);
    notifyListeners();
  } catch (err) {
    console.error('Failed to remove cached track:', err);
    throw err;
  }
}

// Get offline cache stats
export async function getOfflineCacheStats(): Promise<OfflineCacheStats> {
  return invoke<OfflineCacheStats>('v2_get_offline_cache_stats');
}

// Clear all offline cache
export async function clearOfflineCache(): Promise<void> {
  await invoke('v2_clear_offline_cache');
  offlineCacheStates.clear();
  notifyListeners();
}

// Open offline cache folder
export async function openOfflineCacheFolder(): Promise<void> {
  await invoke('v2_open_offline_cache_folder');
}

// Set offline cache limit
export async function setOfflineCacheLimit(limitMb: number | null): Promise<void> {
  await invoke('v2_set_offline_cache_limit', { limitMb });
}

// Open containing folder for a specific album
export async function openAlbumFolder(albumId: string): Promise<void> {
  await invoke('v2_open_album_folder', { albumId });
}

// Open containing folder for a specific track
export async function openTrackFolder(trackId: number): Promise<void> {
  await invoke('v2_open_track_folder', { trackId });
}

// Refresh a cached track (re-cache, overwriting if exists)
export async function refreshCachedTrack(track: {
  id: number;
  title: string;
  artist: string;
  album?: string;
  albumId?: string;
  durationSecs: number;
  quality: string;
  bitDepth?: number;
  sampleRate?: number;
}): Promise<void> {
  // Just call cacheTrackForOffline - backend handles overwriting
  await cacheTrackForOffline(track);
}

// ============================================================================
// COMPATIBILITY ALIASES - These maintain backward compatibility during refactor
// Will be removed once all references are updated
// ============================================================================

/** @deprecated Use OfflineCacheStatus instead */
export type DownloadStatus = OfflineCacheStatus;

/** @deprecated Use OfflineCacheInfo instead */
export type DownloadInfo = OfflineCacheInfo;

/** @deprecated Use OfflineCacheStats instead */
export type DownloadCacheStats = OfflineCacheStats;

/** @deprecated Use getOfflineCacheState instead */
export const getDownloadState = getOfflineCacheState;

/** @deprecated Use setOfflineCacheState instead */
export const setDownloadState = setOfflineCacheState;

/** @deprecated Use isAlbumFullyCached instead */
export const isAlbumFullyDownloaded = isAlbumFullyCached;

/** @deprecated Use initOfflineCacheStates instead */
export const initDownloadStates = initOfflineCacheStates;

/** @deprecated Use startOfflineCacheEventListeners instead */
export const startDownloadEventListeners = startOfflineCacheEventListeners;

/** @deprecated Use stopOfflineCacheEventListeners instead */
export const stopDownloadEventListeners = stopOfflineCacheEventListeners;

/** @deprecated Use cacheTrackForOffline instead */
export const downloadTrack = cacheTrackForOffline;

/** @deprecated Use removeCachedTrack instead */
export const removeDownload = removeCachedTrack;

/** @deprecated Use getOfflineCacheStats instead */
export const getDownloadCacheStats = getOfflineCacheStats;

/** @deprecated Use clearOfflineCache instead */
export const clearDownloadCache = clearOfflineCache;

/** @deprecated Use openOfflineCacheFolder instead */
export const openDownloadCacheFolder = openOfflineCacheFolder;

/** @deprecated Use setOfflineCacheLimit instead */
export const setDownloadCacheLimit = setOfflineCacheLimit;

/** @deprecated Use refreshCachedTrack instead */
export const reDownloadTrack = refreshCachedTrack;
