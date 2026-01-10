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

// ============ Types ============

export interface PlayTrackOptions {
  isLocal?: boolean;
  showLoadingToast?: boolean;
}

export interface MediaMetadata {
  title: string;
  artist: string;
  album: string;
  durationSecs: number;
  coverUrl: string | null;
}

// ============ Callbacks ============
// These are set by the consumer (e.g., +page.svelte) to handle UI feedback

let toastCallback: ((message: string, type: 'success' | 'error' | 'info') => void) | null = null;

export function setToastCallback(callback: (message: string, type: 'success' | 'error' | 'info') => void): void {
  toastCallback = callback;
}

function showToast(message: string, type: 'success' | 'error' | 'info'): void {
  if (toastCallback) {
    toastCallback(message, type);
  }
}

// ============ Core Playback ============

/**
 * Play a track and handle all related side effects
 */
export async function playTrack(
  track: PlayingTrack,
  options: PlayTrackOptions = {}
): Promise<boolean> {
  const { isLocal = false, showLoadingToast = true } = options;

  // Set current track in store
  setCurrentTrack(track);

  try {
    if (showLoadingToast) {
      showToast(`Loading: ${track.title}`, 'info');
    }

    // Use appropriate playback command
    if (isLocal) {
      await invoke('library_play_track', { trackId: track.id });
    } else {
      await invoke('play_track', { trackId: track.id });
    }

    setIsPlaying(true);
    showToast(`Playing: ${track.title}`, 'success');

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

    // Show system notification
    await showTrackNotification(track.title, track.artist, track.album, track.artwork);

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
    await invoke('set_media_metadata', {
      title: metadata.title,
      artist: metadata.artist,
      album: metadata.album,
      durationSecs: metadata.durationSecs,
      coverUrl: metadata.coverUrl
    });
  } catch (err) {
    console.error('Failed to update media metadata:', err);
  }
}

// ============ System Notifications ============

/**
 * Show system notification for track change
 */
export async function showTrackNotification(
  title: string,
  artist: string,
  album: string,
  artworkUrl?: string
): Promise<void> {
  try {
    await invoke('show_track_notification', {
      title,
      artist,
      album,
      artworkUrl: artworkUrl || null
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

  try {
    // Update "now playing"
    await invoke('lastfm_now_playing', {
      artist,
      track: title,
      album: album || null
    });
    console.log('Last.fm: Updated now playing');

    // Schedule scrobble after 50% of track or 4 minutes (whichever is shorter)
    if (scrobbleTimeout) {
      clearTimeout(scrobbleTimeout);
    }

    const scrobbleDelay = Math.min(durationSecs * 0.5, 240) * 1000; // in ms

    scrobbleTimeout = setTimeout(async () => {
      if (lastScrobbledTrackId !== trackId) {
        try {
          const timestamp = Math.floor(Date.now() / 1000);
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
    }, scrobbleDelay);
  } catch (err) {
    console.error('Last.fm now playing failed:', err);
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
