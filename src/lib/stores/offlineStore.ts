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
  localTrackIds: number[]; // DEPRECATED: Use localTrackPaths instead
  localTrackPaths: string[]; // File paths - stable across re-scans
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
    const msg = String(error);
    if (msg.includes('No active session')) {
      console.debug('Failed to fetch offline status (no session yet):', error);
    } else {
      console.error('Failed to fetch offline status:', error);
    }
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
    const msg = String(error);
    if (msg.includes('No active session')) {
      console.debug('Failed to fetch offline settings (no session yet):', error);
    } else {
      console.error('Failed to fetch offline settings:', error);
    }
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
    console.log('[Offline] ========================================');
    console.log('[Offline] setManualOffline called with enabled:', enabled);
    console.log('[Offline] wasOffline:', wasOffline);
    console.log('[Offline] ========================================');

    const newStatus = await invoke<OfflineStatus>('set_manual_offline', { enabled });
    status = newStatus;
    settings.manualOfflineMode = enabled;

    console.log('[Offline] After invoke:');
    console.log('[Offline]   newStatus.isOffline:', newStatus.isOffline);
    console.log('[Offline]   wasOffline:', wasOffline);
    console.log('[Offline]   hasCallback:', !!onOnlineTransitionCallback);
    console.log('[Offline]   will trigger?:', wasOffline && !newStatus.isOffline && !!onOnlineTransitionCallback);

    notifyListeners();
    // Check for offline -> online transition when disabling manual mode
    if (wasOffline && !newStatus.isOffline && onOnlineTransitionCallback) {
      console.log('[Offline] ✓✓✓ TRIGGERING ONLINE TRANSITION CALLBACK ✓✓✓');
      onOnlineTransitionCallback();
      console.log('[Offline] ✓✓✓ CALLBACK EXECUTED ✓✓✓');
    } else {
      console.log('[Offline] ✗✗✗ NOT triggering callback');
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
    await invoke('v2_set_show_partial_playlists', { enabled });
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
    await invoke('v2_set_allow_cast_while_offline', { enabled });
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
    await invoke('v2_set_allow_immediate_scrobbling', { enabled });
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
    await invoke('v2_set_allow_accumulated_scrobbling', { enabled });
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
    await invoke('v2_set_show_network_folders_in_manual_offline', { enabled });
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
  localTrackPaths: string[]
): Promise<number> {
  return invoke<number>('create_pending_playlist', {
    name,
    description,
    isPublic,
    trackIds,
    localTrackPaths,
  });
}

/**
 * Get all playlists pending sync
 */
export async function getPendingPlaylists(): Promise<PendingPlaylist[]> {
  return invoke<PendingPlaylist[]>('v2_get_pending_playlists');
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
  await invoke('v2_mark_pending_playlist_synced', { pendingId, qobuzPlaylistId });
}

/**
 * Delete a pending playlist
 */
export async function deletePendingPlaylist(pendingId: number): Promise<void> {
  await invoke('v2_delete_pending_playlist', { pendingId });
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
  await invoke('v2_mark_scrobbles_sent', { ids });
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

        let qobuzPlaylistId: number;

        // Check if this playlist was already partially synced
        if (playlist.qobuzPlaylistId) {
          qobuzPlaylistId = playlist.qobuzPlaylistId;
          console.log(`[Offline] Using existing Qobuz playlist ID: ${qobuzPlaylistId}`);
        } else {
          // Create playlist on Qobuz
          const createdPlaylist = await invoke<{ id: number }>('v2_create_playlist', {
            name: playlist.name,
            description: playlist.description,
            isPublic: playlist.isPublic
          });

          qobuzPlaylistId = createdPlaylist.id;
          console.log(`[Offline] Created playlist on Qobuz with ID: ${qobuzPlaylistId}`);

          // Save the Qobuz ID immediately (but keep synced=0 until fully complete)
          await invoke('v2_update_pending_playlist_qobuz_id', {
            pendingId: playlist.id,
            qobuzPlaylistId
          });
        }

        // Add Qobuz tracks if any
        if (playlist.trackIds.length > 0) {
          console.log(`[Offline] Adding ${playlist.trackIds.length} Qobuz tracks`);
          await invoke('v2_add_tracks_to_playlist', {
            playlistId: qobuzPlaylistId,
            trackIds: playlist.trackIds
          });
        }

        // Add local tracks if any
        // Prefer localTrackPaths (new system), fall back to localTrackIds (migration)
        const localTracks = playlist.localTrackPaths?.length > 0
          ? playlist.localTrackPaths
          : playlist.localTrackIds;

        if (localTracks && localTracks.length > 0) {
          const usePathSystem = playlist.localTrackPaths?.length > 0;
          console.log(`[Offline] ======================================`);
          console.log(`[Offline] Adding ${localTracks.length} local tracks to playlist ${qobuzPlaylistId}`);
          console.log(`[Offline] Using ${usePathSystem ? 'PATH-based' : 'ID-based (legacy)'} system`);
          console.log(`[Offline] Local tracks:`, localTracks);
          console.log(`[Offline] Qobuz tracks count:`, playlist.trackIds.length);
          console.log(`[Offline] ======================================`);

          for (let i = 0; i < localTracks.length; i++) {
            const trackIdentifier = localTracks[i];
            const position = playlist.trackIds.length + i;

            console.log(`[Offline] ------ Track ${i + 1}/${localTracks.length} ------`);

            try {
              let localTrackId: number;

              if (usePathSystem) {
                // NEW SYSTEM: Resolve file path to current track ID
                const filePath = trackIdentifier as string;
                console.log(`[Offline] Resolving path: ${filePath}`);

                // Get track ID from file path
                const trackInfo = await invoke<{ id: number } | null>('get_track_by_path', {
                  filePath
                });

                if (!trackInfo) {
                  console.warn(`[Offline] ⚠️ File not found in library: ${filePath}`);
                  console.warn(`[Offline] Skipping - file may have been moved or deleted`);
                  continue;
                }

                localTrackId = trackInfo.id;
                console.log(`[Offline] Resolved to track ID: ${localTrackId}`);
              } else {
                // OLD SYSTEM: Use ID directly (for migration)
                localTrackId = trackIdentifier as number;
                console.log(`[Offline] Using legacy track ID: ${localTrackId}`);
              }

              console.log(`[Offline] Position: ${position}`);
              console.log(`[Offline] Calling playlist_add_local_track with params:`, {
                playlistId: qobuzPlaylistId,
                localTrackId: localTrackId,
                position: position
              });

              await invoke('v2_playlist_add_local_track', {
                playlistId: qobuzPlaylistId,
                localTrackId: localTrackId,
                position: position
              });
              console.log(`[Offline] ✓✓✓ SUCCESS: Local track ${localTrackId} added at position ${position}`);
            } catch (localTrackErr) {
              console.error(`[Offline] ✗✗✗ ERROR: Failed to add track`);
              console.error(`[Offline] Error:`, localTrackErr);

              // Check if it's a FOREIGN KEY error (track doesn't exist in library)
              const errorStr = String(localTrackErr);
              if (errorStr.includes('FOREIGN KEY constraint')) {
                console.warn(`[Offline] ⚠️ Track no longer exists in library - skipping`);
                // Don't throw - continue with next track
              } else {
                // Other errors should fail the sync
                throw new Error(`Failed to add local track: ${localTrackErr}`);
              }
            }
          }

          console.log(`[Offline] ======================================`);
          console.log(`[Offline] All local tracks processed`);
          console.log(`[Offline] ======================================`);
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
