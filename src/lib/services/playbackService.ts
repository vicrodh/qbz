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
import { getUserItem, setUserItem } from '$lib/utils/userStorage';

/**
 * Get the preferred streaming quality from localStorage
 * Returns: 'MP3', 'CD Quality', 'Hi-Res', or 'Hi-Res+' (default)
 */
function getStreamingQuality(): string {
  if (typeof localStorage === 'undefined') return 'Hi-Res+';
  return getUserItem('qbz-streaming-quality') || 'Hi-Res+';
}
import {
  setCurrentTrack,
  setIsPlaying,
  setIsFavorite,
  type PlayingTrack
} from '$lib/stores/playerStore';
import { syncQueueState, nextTrack } from '$lib/stores/queueStore';
import { markTrackUnavailable } from '$lib/stores/unavailableTracksStore';
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
import {
  toggleTrackFavorite as storeToggleTrackFavorite,
  addTrackFavorite as storeAddTrackFavorite
} from '$lib/stores/favoritesStore';

// ============ Types ============

/** Result from play_track command */
interface PlayTrackResult {
  format_id: number | null;
}

/**
 * Convert Qobuz format_id to format string for QualityBadge
 * 5=MP3, 6=FLAC 16-bit (CD), 7=FLAC 24-bit (Hi-Res), 27=FLAC 24-bit Hi-Res+
 */
function formatIdToString(formatId: number | null): string | undefined {
  if (formatId === null) return undefined;
  switch (formatId) {
    case 5: return 'mp3';
    case 6: return 'flac';
    case 7: return 'flac24';
    case 27: return 'flac24';
    default: return undefined;
  }
}

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
        const result = await invoke<PlayTrackResult>('play_track', {
          trackId: track.id,
          durationSecs: track.duration,
          quality: getStreamingQuality()
        });

        // Update track format based on actual stream format_id from Qobuz
        const actualFormat = formatIdToString(result.format_id);
        if (actualFormat) {
          track.format = actualFormat;
          // Re-set current track to update the UI with actual format
          setCurrentTrack(track);
        }
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

    // Update ListenBrainz (with MusicBrainz enrichment)
    await updateListenBrainzNowPlaying(
      track.title,
      track.artist,
      track.album,
      track.duration,
      track.id,
      track.isrc
    );

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

    // Check if track is unavailable on Qobuz
    const errorStr = String(err);
    if (errorStr.includes('no longer available') || errorStr.includes('TrackUnavailable')) {
      // Mark track as unavailable for future reference
      markTrackUnavailable(track.id);
      showToast(`"${track.title}" is no longer available`, 'error');

      // Auto-skip to next track
      const next = await nextTrack();
      if (next) {
        console.log('Auto-skipping to next track:', next.title);
        // Small delay to let the toast be visible
        setTimeout(() => {
          playTrack({
            id: next.id,
            title: next.title,
            artist: next.artist,
            album: next.album,
            duration: next.duration_secs,
            artwork: next.artwork_url || undefined,
            albumId: next.album_id || undefined,
            artistId: next.artist_id || undefined,
            bitDepth: next.bit_depth || undefined,
            samplingRate: next.sample_rate || undefined
          }, { showLoadingToast: true, showSuccessToast: true });
        }, 500);
      } else {
        setIsPlaying(false);
      }
      return false;
    }

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
  const saved = getUserItem('qbz-system-notifications-enabled');
  if (saved !== null) {
    systemNotificationsEnabled = saved === 'true';
  }
}

/**
 * Set system notifications enabled/disabled
 */
export function setSystemNotificationsEnabled(enabled: boolean): void {
  systemNotificationsEnabled = enabled;
  setUserItem('qbz-system-notifications-enabled', String(enabled));
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

// ============ ListenBrainz Integration ============

let lastListenBrainzScrobbledTrackId: number | null = null;
let listenbrainzScrobbleTimeout: ReturnType<typeof setTimeout> | null = null;

interface ListenBrainzStatus {
  connected: boolean;
  userName: string | null;
  enabled: boolean;
}

interface MusicBrainzTrackData {
  mbid?: string;
  title?: string;
  artistCredit?: string;
  artistMbids?: string[];
  releaseMbid?: string;
  releaseTitle?: string;
  confidence: 'exact' | 'high' | 'medium' | 'low' | 'none';
}

/**
 * Get ListenBrainz connection status
 */
async function getListenBrainzStatus(): Promise<ListenBrainzStatus | null> {
  try {
    return await invoke<ListenBrainzStatus>('listenbrainz_get_status');
  } catch {
    return null;
  }
}

/**
 * Resolve track to MusicBrainz for enhanced scrobbling
 */
async function resolveMusicBrainzTrack(
  title: string,
  artist: string,
  isrc?: string
): Promise<MusicBrainzTrackData | null> {
  try {
    const result = await invoke<MusicBrainzTrackData>('musicbrainz_resolve_track', {
      isrc: isrc || null,
      title,
      artist
    });
    if (result && result.confidence !== 'none') {
      return result;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * Update ListenBrainz "now playing" and schedule scrobble
 */
export async function updateListenBrainzNowPlaying(
  title: string,
  artist: string,
  album: string,
  durationSecs: number,
  trackId: number,
  isrc?: string
): Promise<void> {
  // Check if ListenBrainz is connected and enabled
  const status = await getListenBrainzStatus();
  if (!status?.connected || !status?.enabled) return;

  const isOffline = checkIsOffline();
  const durationMs = durationSecs * 1000;

  // Try to get MusicBrainz data for enrichment
  let mbData: MusicBrainzTrackData | null = null;
  if (!isOffline) {
    mbData = await resolveMusicBrainzTrack(title, artist, isrc);
  }

  // Skip "now playing" update when offline (requires network)
  if (!isOffline) {
    try {
      await invoke('listenbrainz_now_playing', {
        artist,
        track: title,
        album: album || null,
        recordingMbid: mbData?.mbid || null,
        releaseMbid: mbData?.releaseMbid || null,
        artistMbids: mbData?.artistMbids || null,
        isrc: isrc || null,
        durationMs
      });
      console.log('ListenBrainz: Updated now playing');
    } catch (err) {
      console.error('ListenBrainz now playing failed:', err);
    }
  }

  // Schedule scrobble after 50% of track or 4 minutes (whichever is shorter)
  if (listenbrainzScrobbleTimeout) {
    clearTimeout(listenbrainzScrobbleTimeout);
  }

  const scrobbleDelay = Math.min(durationSecs * 0.5, 240) * 1000; // in ms

  listenbrainzScrobbleTimeout = setTimeout(async () => {
    if (lastListenBrainzScrobbledTrackId !== trackId) {
      const timestamp = Math.floor(Date.now() / 1000);

      // If offline, queue the scrobble for later
      if (checkIsOffline()) {
        try {
          await invoke('listenbrainz_queue_listen', {
            artist,
            track: title,
            album: album || null,
            timestamp,
            recordingMbid: mbData?.mbid || null,
            releaseMbid: mbData?.releaseMbid || null,
            artistMbids: mbData?.artistMbids || null,
            isrc: isrc || null,
            durationMs
          });
          lastListenBrainzScrobbledTrackId = trackId;
          console.log('ListenBrainz: Queued scrobble for later (offline)');
        } catch (err) {
          console.error('Failed to queue ListenBrainz scrobble:', err);
        }
      } else {
        // Online - scrobble immediately
        try {
          await invoke('listenbrainz_scrobble', {
            artist,
            track: title,
            album: album || null,
            timestamp,
            recordingMbid: mbData?.mbid || null,
            releaseMbid: mbData?.releaseMbid || null,
            artistMbids: mbData?.artistMbids || null,
            isrc: isrc || null,
            durationMs
          });
          lastListenBrainzScrobbledTrackId = trackId;
          console.log('ListenBrainz: Scrobbled track');
        } catch (err) {
          console.error('ListenBrainz scrobble failed:', err);
        }
      }
    }
  }, scrobbleDelay);
}

/**
 * Flush queued ListenBrainz listens
 * Call this when transitioning from offline to online
 */
export async function flushListenBrainzQueue(): Promise<number> {
  // Check if ListenBrainz is connected and enabled
  const status = await getListenBrainzStatus();
  if (!status?.connected || !status?.enabled) return 0;

  // Don't try to flush if still offline
  if (checkIsOffline()) {
    console.log('ListenBrainz: Cannot flush queue (still offline)');
    return 0;
  }

  try {
    const sent = await invoke<number>('listenbrainz_flush_queue');
    if (sent > 0) {
      console.log(`ListenBrainz: Flushed ${sent} queued listens`);
    }
    return sent;
  } catch (err) {
    console.error('ListenBrainz: Failed to flush queue:', err);
    return 0;
  }
}

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
  const scrobblingEnabled = getUserItem('qbz-lastfm-scrobbling') !== 'false';
  const sessionKey = getUserItem('qbz-lastfm-session-key');

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
  const scrobblingEnabled = getUserItem('qbz-lastfm-scrobbling') !== 'false';
  const sessionKey = getUserItem('qbz-lastfm-session-key');

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
 * Uses the centralized favorites store to ensure UI stays in sync
 */
export async function toggleTrackFavorite(
  trackId: number,
  currentlyFavorite: boolean
): Promise<{ success: boolean; isFavorite: boolean }> {
  try {
    // Use the store's toggle function - it handles API calls, optimistic updates,
    // loading states, and notifies all subscribed components
    const newState = await storeToggleTrackFavorite(trackId);
    return { success: true, isFavorite: newState };
  } catch (err) {
    console.error('Failed to toggle favorite:', err);
    return { success: false, isFavorite: currentlyFavorite };
  }
}

/**
 * Add a track to favorites
 * Uses the centralized favorites store to ensure UI stays in sync
 */
export async function addTrackToFavorites(trackId: number): Promise<boolean> {
  try {
    // Use the store's add function - it handles API calls and UI sync
    return await storeAddTrackFavorite(trackId);
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
  if (listenbrainzScrobbleTimeout) {
    clearTimeout(listenbrainzScrobbleTimeout);
    listenbrainzScrobbleTimeout = null;
  }
}
