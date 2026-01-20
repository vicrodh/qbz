/**
 * Offline Mode State Store
 *
 * Manages offline mode detection and settings.
 * Polls backend periodically and listens for manual toggle events.
 * Handles pending playlist sync queue for playlists created offline.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { showToast } from './toastStore';

// Types matching Rust backend
export type OfflineReason = 'no_network' | 'not_logged_in' | 'manual_override';

export interface PendingPlaylist {
  id: number;
  name: string;
  description: string | null;
  isPublic: boolean;
  trackIds: number[];
  localTrackIds: number[];
  createdAt: number;
  synced: boolean;
  qobuzPlaylistId: number | null;
}

export interface OfflineStatus {
  isOffline: boolean;
  reason: OfflineReason | null;
  manualModeEnabled: boolean;
}

export interface OfflineSettings {
  manualOfflineMode: boolean;
  showPartialPlaylists: boolean;
  allowCastWhileOffline: boolean;
  allowImmediateScrobbling: boolean;
  allowAccumulatedScrobbling: boolean;
  showNetworkFoldersInManualOffline: boolean;
}

// Store state
let status: OfflineStatus = {
  isOffline: false,
  reason: null,
  manualModeEnabled: false,
};

let settings: OfflineSettings = {
  manualOfflineMode: false,
  showPartialPlaylists: true,
  allowCastWhileOffline: false,
  allowImmediateScrobbling: false,
  allowAccumulatedScrobbling: true,
  showNetworkFoldersInManualOffline: false,
};

let initialized = false;
let pollInterval: ReturnType<typeof setInterval> | null = null;
let eventUnlisten: UnlistenFn | null = null;

// Listeners
const listeners = new Set<() => void>();

// Callback for online transition (to flush scrobble queue, etc.)
let onOnlineTransitionCallback: (() => void) | null = null;

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Register a callback to be called when transitioning from offline to online
 */
export function onOnlineTransition(callback: () => void): void {
  onOnlineTransitionCallback = callback;
}

/**
 * Subscribe to offline state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener(); // Immediately notify with current state
  return () => listeners.delete(listener);
}

// ============ Getters ============

export function getStatus(): OfflineStatus {
  return status;
}

export function getSettings(): OfflineSettings {
  return settings;
}

export function isOffline(): boolean {
  return status.isOffline;
}

export function getOfflineReason(): OfflineReason | null {
  return status.reason;
}

// ============ Actions ============

/**
 * Fetch current offline status from backend
 */
async function fetchStatus(): Promise<void> {
  try {
    const wasOffline = status.isOffline;
    const newStatus = await invoke<OfflineStatus>('get_offline_status');
    const changed = JSON.stringify(status) !== JSON.stringify(newStatus);
    status = newStatus;
    if (changed) {
      notifyListeners();
      // Show toast for status change (skip on initial load)
      if (initialized) {
        if (!wasOffline && newStatus.isOffline) {
          showToast('Offline mode activated', 'info');
        } else if (wasOffline && !newStatus.isOffline) {
          showToast('Back online', 'success');
        }
      }
      // Check for offline -> online transition
      if (wasOffline && !newStatus.isOffline && onOnlineTransitionCallback) {
        console.log('[Offline] Transitioning to online - triggering callback');
        onOnlineTransitionCallback();
      }
    }
  } catch (error) {
    console.error('Failed to fetch offline status:', error);
  }
}

/**
 * Fetch offline settings from backend
 */
async function fetchSettings(): Promise<void> {
  try {
    settings = await invoke<OfflineSettings>('get_offline_settings');
    notifyListeners();
  } catch (error) {
    console.error('Failed to fetch offline settings:', error);
  }
}

/**
 * Check network connectivity
 */
export async function checkNetwork(): Promise<boolean> {
  try {
    return await invoke<boolean>('check_network');
  } catch (error) {
    console.error('Failed to check network:', error);
    return false;
  }
}

/**
 * Toggle manual offline mode
 */
export async function setManualOffline(enabled: boolean): Promise<void> {
  try {
    const wasOffline = status.isOffline;
    const newStatus = await invoke<OfflineStatus>('set_manual_offline', { enabled });
    status = newStatus;
    settings.manualOfflineMode = enabled;
    notifyListeners();
    // Check for offline -> online transition when disabling manual mode
    if (wasOffline && !newStatus.isOffline && onOnlineTransitionCallback) {
      console.log('[Offline] Transitioning to online after disabling manual mode');
      onOnlineTransitionCallback();
    }
  } catch (error) {
    console.error('Failed to set manual offline mode:', error);
    throw error;
  }
}

/**
 * Set whether to show playlists with partial local content
 */
export async function setShowPartialPlaylists(enabled: boolean): Promise<void> {
  try {
    await invoke('set_show_partial_playlists', { enabled });
    settings.showPartialPlaylists = enabled;
    notifyListeners();
  } catch (error) {
    console.error('Failed to set show partial playlists:', error);
    throw error;
  }
}

/**
 * Set whether to allow Chromecast while in manual offline mode
 */
export async function setAllowCastWhileOffline(enabled: boolean): Promise<void> {
  try {
    await invoke('set_allow_cast_while_offline', { enabled });
    settings.allowCastWhileOffline = enabled;
    notifyListeners();
  } catch (error) {
    console.error('Failed to set allow cast while offline:', error);
    throw error;
  }
}

/**
 * Set whether to allow immediate scrobbling to Last.fm in manual offline mode
 */
export async function setAllowImmediateScrobbling(enabled: boolean): Promise<void> {
  try {
    await invoke('set_allow_immediate_scrobbling', { enabled });
    settings.allowImmediateScrobbling = enabled;
    notifyListeners();
  } catch (error) {
    console.error('Failed to set allow immediate scrobbling:', error);
    throw error;
  }
}

/**
 * Set whether to queue scrobbles for later submission when back online
 */
export async function setAllowAccumulatedScrobbling(enabled: boolean): Promise<void> {
  try {
    await invoke('set_allow_accumulated_scrobbling', { enabled });
    settings.allowAccumulatedScrobbling = enabled;
    notifyListeners();
  } catch (error) {
    console.error('Failed to set allow accumulated scrobbling:', error);
    throw error;
  }
}

/**
 * Set whether to show network folder content in manual offline mode
 */
export async function setShowNetworkFoldersInManualOffline(enabled: boolean): Promise<void> {
  try {
    await invoke('set_show_network_folders_in_manual_offline', { enabled });
    settings.showNetworkFoldersInManualOffline = enabled;
    notifyListeners();
  } catch (error) {
    console.error('Failed to set show network folders in manual offline:', error);
    throw error;
  }
}

/**
 * Force refresh of offline status
 */
export async function refreshStatus(): Promise<void> {
  await fetchStatus();
}

// ============ Initialization ============

/**
 * Initialize the offline store
 * - Fetches initial status and settings
 * - Sets up polling interval (30 seconds)
 * - Listens for backend events
 */
export async function initOfflineStore(): Promise<void> {
  if (initialized) return;

  // Fetch initial state
  await Promise.all([fetchStatus(), fetchSettings()]);

  // Poll every 30 seconds
  pollInterval = setInterval(fetchStatus, 30000);

  // Listen for backend events (from manual toggle)
  eventUnlisten = await listen<OfflineStatus>('offline-status-changed', (event) => {
    const wasOffline = status.isOffline;
    status = event.payload;
    notifyListeners();
    // Show toast for status change
    if (!wasOffline && event.payload.isOffline) {
      showToast('Offline mode activated', 'info');
    } else if (wasOffline && !event.payload.isOffline) {
      showToast('Back online', 'success');
    }
    // Check for offline -> online transition
    if (wasOffline && !event.payload.isOffline && onOnlineTransitionCallback) {
      console.log('[Offline] Transitioning to online via event - triggering callback');
      onOnlineTransitionCallback();
    }
  });

  initialized = true;
}

/**
 * Cleanup the offline store (for app shutdown)
 */
export function cleanupOfflineStore(): void {
  if (pollInterval) {
    clearInterval(pollInterval);
    pollInterval = null;
  }
  if (eventUnlisten) {
    eventUnlisten();
    eventUnlisten = null;
  }
  initialized = false;
}

// ============ Pending Playlist Sync ============

/**
 * Create a playlist while offline (queued for sync when back online)
 */
export async function createPendingPlaylist(
  name: string,
  description: string | null,
  isPublic: boolean,
  trackIds: number[],
  localTrackIds: number[]
): Promise<number> {
  return invoke<number>('create_pending_playlist', {
    name,
    description,
    isPublic,
    trackIds,
    localTrackIds,
  });
}

/**
 * Get all playlists pending sync
 */
export async function getPendingPlaylists(): Promise<PendingPlaylist[]> {
  return invoke<PendingPlaylist[]>('get_pending_playlists');
}

/**
 * Get count of pending playlists
 */
export async function getPendingPlaylistCount(): Promise<number> {
  return invoke<number>('get_pending_playlist_count');
}

/**
 * Mark a pending playlist as synced after successful Qobuz creation
 */
export async function markPendingPlaylistSynced(
  pendingId: number,
  qobuzPlaylistId: number
): Promise<void> {
  await invoke('mark_pending_playlist_synced', { pendingId, qobuzPlaylistId });
}

/**
 * Delete a pending playlist
 */
export async function deletePendingPlaylist(pendingId: number): Promise<void> {
  await invoke('delete_pending_playlist', { pendingId });
}

// ============ Scrobble Queue ============

export interface QueuedScrobble {
  id: number;
  artist: string;
  track: string;
  album: string | null;
  timestamp: number;
  createdAt: number;
  sent: boolean;
}

/**
 * Queue a scrobble for later submission to Last.fm
 */
export async function queueScrobble(
  artist: string,
  track: string,
  album: string | null,
  timestamp: number
): Promise<number> {
  return invoke<number>('queue_scrobble', {
    artist,
    track,
    album,
    timestamp,
  });
}

/**
 * Get queued scrobbles (up to limit)
 */
export async function getQueuedScrobbles(limit?: number): Promise<QueuedScrobble[]> {
  return invoke<QueuedScrobble[]>('get_queued_scrobbles', { limit });
}

/**
 * Mark scrobbles as sent after successful Last.fm submission
 */
export async function markScrobblesSent(ids: number[]): Promise<void> {
  await invoke('mark_scrobbles_sent', { ids });
}

/**
 * Get count of queued (unsent) scrobbles
 */
export async function getQueuedScrobbleCount(): Promise<number> {
  return invoke<number>('get_queued_scrobble_count');
}

/**
 * Cleanup old sent scrobbles
 */
export async function cleanupSentScrobbles(olderThanDays?: number): Promise<number> {
  return invoke<number>('cleanup_sent_scrobbles', { olderThanDays });
}

/**
 * Sync all pending playlists to Qobuz when back online
 */
export async function syncPendingPlaylists(): Promise<void> {
  try {
    console.log('[Offline] Syncing pending playlists...');
    const pending = await getPendingPlaylists();

    if (pending.length === 0) {
      console.log('[Offline] No pending playlists to sync');
      return;
    }

    console.log(`[Offline] Found ${pending.length} pending playlists to sync`);

    for (const playlist of pending) {
      try {
        console.log(`[Offline] Syncing playlist: ${playlist.name}`);

        // Create playlist on Qobuz
        const createdPlaylist = await invoke<{ id: number }>('create_playlist', {
          name: playlist.name,
          description: playlist.description,
          isPublic: playlist.isPublic
        });

        const qobuzPlaylistId = createdPlaylist.id;
        console.log(`[Offline] Created playlist on Qobuz with ID: ${qobuzPlaylistId}`);

        // Add Qobuz tracks if any
        if (playlist.trackIds.length > 0) {
          console.log(`[Offline] Adding ${playlist.trackIds.length} Qobuz tracks`);
          await invoke('add_tracks_to_playlist', {
            playlistId: qobuzPlaylistId,
            trackIds: playlist.trackIds
          });
        }

        // Add local tracks if any
        if (playlist.localTrackIds.length > 0) {
          console.log(`[Offline] Adding ${playlist.localTrackIds.length} local tracks`);
          for (let i = 0; i < playlist.localTrackIds.length; i++) {
            console.log(`[Offline] Adding local track ${playlist.localTrackIds[i]} at position ${playlist.trackIds.length + i}`);
            try {
              await invoke('playlist_add_local_track', {
                playlistId: qobuzPlaylistId,
                localTrackId: playlist.localTrackIds[i],
                position: playlist.trackIds.length + i // Position after Qobuz tracks
              });
              console.log(`[Offline] Successfully added local track ${playlist.localTrackIds[i]}`);
            } catch (localTrackErr) {
              console.error(`[Offline] Failed to add local track ${playlist.localTrackIds[i]}:`, localTrackErr);
              // Re-throw to prevent marking as synced
              throw new Error(`Failed to add local track: ${localTrackErr}`);
            }
          }
        }

        // Mark as synced ONLY if everything succeeded
        await markPendingPlaylistSynced(playlist.id, qobuzPlaylistId);
        console.log(`[Offline] Successfully synced playlist: ${playlist.name}`);

        showToast(`Synced playlist "${playlist.name}"`, 'success');
      } catch (err) {
        console.error(`[Offline] Failed to sync playlist "${playlist.name}":`, err);
        showToast(`Failed to sync playlist "${playlist.name}": ${err}`, 'error');
        // Don't mark as synced if there was an error - will retry on next online transition
      }
    }

    console.log('[Offline] Finished syncing pending playlists');
  } catch (err) {
    console.error('[Offline] Error syncing pending playlists:', err);
  }
}

// ============ State Getter ============

export interface OfflineState {
  status: OfflineStatus;
  settings: OfflineSettings;
}

export function getOfflineState(): OfflineState {
  return {
    status,
    settings,
  };
}
