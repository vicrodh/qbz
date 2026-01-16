/**
 * Playback Service
 *
 * Centralizes playback-related operations including:
 * - Playing tracks (Qobuz and local)
 * - MPRIS metadata updates
 * - System notifications
 * - Last.fm scrobbling
 * - Favorite status checking
 */

import { invoke } from '@tauri-apps/api/core';
import {
  setCurrentTrack,
  setIsPlaying,
  setIsFavorite,
  type PlayingTrack
} from '$lib/stores/playerStore';
import { syncQueueState } from '$lib/stores/queueStore';
import { logRecoEvent } from '$lib/services/recoService';
import {
  isCasting,
  castTrack,
  castPlay,
  castPause,
  castStop
} from '$lib/stores/castStore';
import {
  isOffline as checkIsOffline,
  queueScrobble,
  getQueuedScrobbles,
  markScrobblesSent,
  cleanupSentScrobbles
} from '$lib/stores/offlineStore';

// ============ Types ============

export interface PlayTrackOptions {
  isLocal?: boolean;
  showLoadingToast?: boolean;
  showSuccessToast?: boolean;
}

export interface MediaMetadata {
  title: string;
  artist: string;
  album: string;
  durationSecs: number;
  coverUrl: string | null;
}

// ============ Toast Integration ============
// Use the toast store directly for buffering toast support

import {
  showToast as storeShowToast,
  dismissBuffering,
  type ToastType
} from '$lib/stores/toastStore';

function showToast(message: string, type: ToastType): void {
  storeShowToast(message, type);
}

// ============ Core Playback ============

/**
 * Play a track and handle all related side effects
 */
export async function playTrack(
  track: PlayingTrack,
  options: PlayTrackOptions = {}
): Promise<boolean> {
  const {
    isLocal = false,
    showLoadingToast = true,
    showSuccessToast = true
  } = options;

  // Set current track in store
  setCurrentTrack(track);

  try {
    // For Qobuz tracks: stop current playback immediately and show buffering
    // This prevents the previous track from continuing while we download
    // Local tracks load instantly so they don't need this
    if (!isLocal && !isCasting()) {
      // Stop current playback immediately
      try {
        await invoke('stop_playback');
      } catch {
        // Ignore errors - player might not be playing
      }
      // Show buffering indicator
      if (showLoadingToast) {
        showToast(track.title, 'buffering');
      }
    } else if (showLoadingToast && !isLocal) {
      showToast(track.title, 'buffering');
    }

    // Check if we're casting to an external device
    if (isCasting() && !isLocal) {
      // Cast to connected device
      await castTrack(track.id, {
        title: track.title,
        artist: track.artist,
        album: track.album,
        artworkUrl: track.artwork,
        durationSecs: track.duration
      });
    } else {
      // Use appropriate local playback command
      if (isLocal) {
        await invoke('library_play_track', { trackId: track.id });
      } else {
        await invoke('play_track', { trackId: track.id });
      }
    }

    setIsPlaying(true);
    dismissBuffering();
    if (showSuccessToast) {
      showToast(`Playing: ${track.title}`, 'success');
    }

    // Log play event for recommendations (fire-and-forget)
    if (!isLocal) {
      void logRecoEvent({
        eventType: 'play',
        itemType: 'track',
        trackId: track.id,
        albumId: track.albumId,
        artistId: track.artistId
      });
    }

    // Update MPRIS metadata
    await updateMediaMetadata({
      title: track.title,
      artist: track.artist,
      album: track.album,
      durationSecs: track.duration,
      coverUrl: track.artwork || null
    });

    // Show system notification with artwork and quality info
    await showTrackNotification(
      track.title,
      track.artist,
      track.album,
      track.artwork,
      track.bitDepth,
      track.samplingRate
    );

    // Update Last.fm
    await updateLastfmNowPlaying(track.title, track.artist, track.album, track.duration, track.id);

    // Check favorite status (only for Qobuz tracks)
    if (!isLocal) {
      const isFav = await checkTrackFavorite(track.id);
      setIsFavorite(isFav);
    } else {
      setIsFavorite(false);
    }

    // Sync queue state
    await syncQueueState();

    return true;
  } catch (err) {
    console.error('Failed to play track:', err);
    dismissBuffering();
    showToast(`Playback error: ${err}`, 'error');
    setIsPlaying(false);
    return false;
  }
}

// ============ MPRIS Metadata ============

/**
 * Update system media controls metadata
 */
export async function updateMediaMetadata(metadata: MediaMetadata): Promise<void> {
  try {
    const coverUrl = normalizeCoverUrlForMetadata(metadata.coverUrl);
    await invoke('set_media_metadata', {
      title: metadata.title,
      artist: metadata.artist,
      album: metadata.album,
      durationSecs: metadata.durationSecs,
      coverUrl
    });
  } catch (err) {
    console.error('Failed to update media metadata:', err);
  }
}

function normalizeCoverUrlForMetadata(coverUrl?: string | null): string | null {
  if (!coverUrl) return null;
  const assetPrefix = 'asset://localhost/';
  if (coverUrl.startsWith(assetPrefix)) {
    const encodedPath = coverUrl.slice(assetPrefix.length);
    try {
      const decodedPath = decodeURIComponent(encodedPath);
      return `file://${encodeURI(decodedPath)}`;
    } catch {
      return coverUrl;
    }
  }
  if (coverUrl.startsWith('asset://')) {
    const encodedPath = coverUrl.replace(/^asset:\/\/+/, '');
    try {
      const decodedPath = decodeURIComponent(encodedPath);
      return `file://${encodeURI(decodedPath)}`;
    } catch {
      return coverUrl;
    }
  }
  if (coverUrl.startsWith('/')) {
    return `file://${encodeURI(coverUrl)}`;
  }
  return coverUrl;
}

// ============ System Notifications ============

let systemNotificationsEnabled = true;

/**
 * Load system notifications preference from localStorage
 */
export function loadSystemNotificationsPreference(): void {
  const saved = localStorage.getItem('qbz-system-notifications-enabled');
  if (saved !== null) {
    systemNotificationsEnabled = saved === 'true';
  }
}

/**
 * Set system notifications enabled/disabled
 */
export function setSystemNotificationsEnabled(enabled: boolean): void {
  systemNotificationsEnabled = enabled;
  localStorage.setItem('qbz-system-notifications-enabled', String(enabled));
}

/**
 * Get system notifications enabled state
 */
export function getSystemNotificationsEnabled(): boolean {
  return systemNotificationsEnabled;
}

/**
 * Show system notification for track change
 */
export async function showTrackNotification(
  title: string,
  artist: string,
  album: string,
  artworkUrl?: string,
  bitDepth?: number,
  sampleRate?: number
): Promise<void> {
  // Skip if system notifications are disabled
  if (!systemNotificationsEnabled) {
    return;
  }

  try {
    await invoke('show_track_notification', {
      title,
      artist,
      album,
      artworkUrl: artworkUrl || null,
      bitDepth: bitDepth || null,
      sampleRate: sampleRate || null
    });
  } catch (err) {
    console.error('Failed to show track notification:', err);
  }
}

// ============ Last.fm Integration ============

let lastScrobbledTrackId: number | null = null;
let scrobbleTimeout: ReturnType<typeof setTimeout> | null = null;

/**
 * Update Last.fm "now playing" and schedule scrobble
 */
export async function updateLastfmNowPlaying(
  title: string,
  artist: string,
  album: string,
  durationSecs: number,
  trackId: number
): Promise<void> {
  // Check if scrobbling is enabled
  const scrobblingEnabled = localStorage.getItem('qbz-lastfm-scrobbling') !== 'false';
  const sessionKey = localStorage.getItem('qbz-lastfm-session-key');

  if (!scrobblingEnabled || !sessionKey) return;

  const isOffline = checkIsOffline();

  // Skip "now playing" update when offline (requires network)
  if (!isOffline) {
    try {
      await invoke('lastfm_now_playing', {
        artist,
        track: title,
        album: album || null
      });
      console.log('Last.fm: Updated now playing');
    } catch (err) {
      console.error('Last.fm now playing failed:', err);
    }
  }

  // Schedule scrobble after 50% of track or 4 minutes (whichever is shorter)
  if (scrobbleTimeout) {
    clearTimeout(scrobbleTimeout);
  }

  const scrobbleDelay = Math.min(durationSecs * 0.5, 240) * 1000; // in ms

  scrobbleTimeout = setTimeout(async () => {
    if (lastScrobbledTrackId !== trackId) {
      const timestamp = Math.floor(Date.now() / 1000);

      // If offline, queue the scrobble for later
      if (checkIsOffline()) {
        try {
          await queueScrobble(artist, title, album || null, timestamp);
          lastScrobbledTrackId = trackId;
          console.log('Last.fm: Queued scrobble for later (offline)');
        } catch (err) {
          console.error('Failed to queue scrobble:', err);
        }
      } else {
        // Online - scrobble immediately
        try {
          await invoke('lastfm_scrobble', {
            artist,
            track: title,
            album: album || null,
            timestamp
          });
          lastScrobbledTrackId = trackId;
          console.log('Last.fm: Scrobbled track');
        } catch (err) {
          console.error('Last.fm scrobble failed:', err);
        }
      }
    }
  }, scrobbleDelay);
}

/**
 * Flush queued scrobbles to Last.fm
 * Call this when transitioning from offline to online
 */
export async function flushScrobbleQueue(): Promise<{ sent: number; failed: number }> {
  // Check if scrobbling is enabled
  const scrobblingEnabled = localStorage.getItem('qbz-lastfm-scrobbling') !== 'false';
  const sessionKey = localStorage.getItem('qbz-lastfm-session-key');

  if (!scrobblingEnabled || !sessionKey) {
    return { sent: 0, failed: 0 };
  }

  // Don't try to flush if still offline
  if (checkIsOffline()) {
    console.log('Last.fm: Cannot flush queue (still offline)');
    return { sent: 0, failed: 0 };
  }

  try {
    const queued = await getQueuedScrobbles(50); // Last.fm batch limit
    if (queued.length === 0) {
      return { sent: 0, failed: 0 };
    }

    console.log(`Last.fm: Flushing ${queued.length} queued scrobbles`);

    let sent = 0;
    let failed = 0;
    const sentIds: number[] = [];

    // Send scrobbles one by one (could be optimized to batch later)
    for (const scrobble of queued) {
      // Check if timestamp is too old (>14 days)
      const now = Math.floor(Date.now() / 1000);
      const fourteenDaysAgo = now - (14 * 24 * 60 * 60);

      if (scrobble.timestamp < fourteenDaysAgo) {
        console.log(`Last.fm: Skipping old scrobble (>14 days): ${scrobble.artist} - ${scrobble.track}`);
        sentIds.push(scrobble.id); // Mark as sent to remove from queue
        failed++;
        continue;
      }

      try {
        await invoke('lastfm_scrobble', {
          artist: scrobble.artist,
          track: scrobble.track,
          album: scrobble.album,
          timestamp: scrobble.timestamp
        });
        sentIds.push(scrobble.id);
        sent++;
        console.log(`Last.fm: Flushed scrobble: ${scrobble.artist} - ${scrobble.track}`);
      } catch (err) {
        console.error(`Last.fm: Failed to flush scrobble: ${scrobble.artist} - ${scrobble.track}`, err);
        failed++;
      }
    }

    // Mark successfully sent scrobbles
    if (sentIds.length > 0) {
      await markScrobblesSent(sentIds);
    }

    // Cleanup old sent scrobbles
    await cleanupSentScrobbles(7);

    console.log(`Last.fm: Flush complete - sent: ${sent}, failed: ${failed}`);
    return { sent, failed };
  } catch (err) {
    console.error('Last.fm: Failed to flush scrobble queue:', err);
    return { sent: 0, failed: 0 };
  }
}

// ============ Favorites ============

/**
 * Check if a track is in favorites
 */
export async function checkTrackFavorite(trackId: number): Promise<boolean> {
  try {
    const response = await invoke<{ tracks?: { items: Array<{ id: number }> } }>('get_favorites', {
      favType: 'tracks',
      limit: 500
    });
    if (response.tracks?.items) {
      return response.tracks.items.some(item => item.id === trackId);
    }
    return false;
  } catch (err) {
    console.error('Failed to check favorite status:', err);
    return false;
  }
}

/**
 * Toggle favorite status for a track
 */
export async function toggleTrackFavorite(
  trackId: number,
  currentlyFavorite: boolean
): Promise<{ success: boolean; isFavorite: boolean }> {
  const newFavoriteState = !currentlyFavorite;

  try {
    if (newFavoriteState) {
      await invoke('add_favorite', { favType: 'track', itemId: String(trackId) });
      // Log favorite event for recommendations
      void logRecoEvent({
        eventType: 'favorite',
        itemType: 'track',
        trackId
      });
    } else {
      await invoke('remove_favorite', { favType: 'track', itemId: String(trackId) });
    }
    return { success: true, isFavorite: newFavoriteState };
  } catch (err) {
    console.error('Failed to toggle favorite:', err);
    return { success: false, isFavorite: currentlyFavorite };
  }
}

/**
 * Add a track to favorites
 */
export async function addTrackToFavorites(trackId: number): Promise<boolean> {
  try {
    await invoke('add_favorite', { favType: 'track', itemId: String(trackId) });
    // Log favorite event for recommendations
    void logRecoEvent({
      eventType: 'favorite',
      itemType: 'track',
      trackId
    });
    return true;
  } catch (err) {
    console.error('Failed to add to favorites:', err);
    return false;
  }
}

// ============ Cleanup ============

/**
 * Clear any pending scrobble timeouts
 */
export function cleanup(): void {
  if (scrobbleTimeout) {
    clearTimeout(scrobbleTimeout);
    scrobbleTimeout = null;
  }
}
