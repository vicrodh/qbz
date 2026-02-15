/**
 * Player State Store
 *
 * Manages playback state including current track, play/pause, position, volume.
 * Uses Tauri events for real-time updates from the backend.
 */

import { invoke } from '@tauri-apps/api/core';
import { saveSessionVolume } from '$lib/services/sessionService';
import { getUserItem, setUserItem, removeUserItem } from '$lib/utils/userStorage';

/**
 * Get the preferred streaming quality from localStorage
 * Valid values: 'MP3', 'CD Quality', 'Hi-Res', 'Hi-Res+'
 */
function getStreamingQuality(): string {
  if (typeof localStorage === 'undefined') return 'Hi-Res+';
  const saved = getUserItem('qbz-streaming-quality');
  // Log for debugging issue #34
  console.log('[Quality] getStreamingQuality called, localStorage value:', saved);
  return saved || 'Hi-Res+';
}
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  isCasting,
  castPlay,
  castPause,
  castSeek,
  castSetVolume,
  castStop,
  getCastPosition,
  subscribe as subscribeToCast,
  setOnCastTrackEnded,
  setOnCastDisconnected
} from '$lib/stores/castStore';
import { syncQueueState } from '$lib/stores/queueStore';

// ============ Types ============

export interface PlayingTrack {
  id: number;
  title: string;
  artist: string;
  album: string;
  artwork: string;
  duration: number;
  quality: string;
  bitDepth?: number;
  samplingRate?: number;
  format?: string;
  isLocal?: boolean;
  source?: string;
  // Optional IDs for recommendation tracking
  albumId?: string;
  artistId?: number;
  // ISRC for MusicBrainz/ListenBrainz enrichment
  isrc?: string;
  // Original track quality from metadata (for comparison with actual stream)
  originalBitDepth?: number;
  originalSamplingRate?: number;
}

interface BackendPlaybackState {
  is_playing: boolean;
  position: number;
  duration: number;
  track_id: number;
  volume: number;
}

// Event payload from backend
interface PlaybackEvent {
  is_playing: boolean;
  position: number;
  duration: number;
  track_id: number;
  volume: number;
  sample_rate: number | null;  // Actual stream sample rate in Hz
  bit_depth: number | null;    // Actual stream bit depth
  normalization_gain: number | null;  // Active normalization gain factor (null = not applied)
  gapless_ready: boolean;       // Backend wants next track queued for gapless
  gapless_next_track_id: number; // Track ID queued for gapless (0 = none)
}

// Queue track from backend (for external track sync)
interface QueueTrack {
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
  source?: string;
  album_id?: string | null;
  artist_id?: number | null;
}

interface PlexPlayTrackResult {
  sampling_rate_hz?: number | null;
  bit_depth?: number | null;
}

// ============ State ============

/**
 * Load persisted volume from localStorage
 */
function loadPersistedVolume(): number {
  if (typeof localStorage === 'undefined') return 75;
  const stored = getUserItem('qbz-volume');
  if (stored) {
    const parsed = parseFloat(stored);
    if (!isNaN(parsed) && parsed >= 0 && parsed <= 100) {
      return parsed;
    }
  }
  return 75;
}

/**
 * Save volume to localStorage
 */
function persistVolume(vol: number): void {
  if (typeof localStorage === 'undefined') return;
  setUserItem('qbz-volume', String(vol));
}

let currentTrack: PlayingTrack | null = null;
let isPlaying = false;
let currentTime = 0;
let duration = 0;
let volume = loadPersistedVolume();
let preMuteVolume: number | null = null; // Volume before mute, null when not muted
let isFavorite = false;
// Event listener state (replaces polling)
let eventUnlisten: UnlistenFn | null = null;
let castUnsubscribe: (() => void) | null = null;
let isAdvancingTrack = false;
let isSkipping = false;
let queueEnded = false;
let normalizationGain: number | null = null;  // Current normalization gain (null = not active)

// Callbacks for track advancement (set by consumer)
let onTrackEnded: (() => Promise<void>) | null = null;
let onResumeFromStop: (() => Promise<void>) | null = null;

// Gapless: callback to get the next track ID for pre-queuing
let gaplessGetNextTrackId: (() => number | null) | null = null;
// Gapless: callback when backend transitions to next track (update frontend metadata/queue)
let onGaplessTransition: ((trackId: number) => Promise<void>) | null = null;
// Track whether gapless pre-queue request is in flight
let gaplessRequestInFlight = false;

// Session restore state - when set, next play will load the track first
let pendingSessionRestore: { trackId: number; position: number } | null = null;

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Subscribe to player state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener(); // Immediately notify with current state
  return () => listeners.delete(listener);
}

// ============ Getters ============

export function getCurrentTrack(): PlayingTrack | null {
  return currentTrack;
}

export function getIsPlaying(): boolean {
  return isPlaying;
}

export function getCurrentTime(): number {
  return currentTime;
}

export function getDuration(): number {
  return duration;
}

export function getVolume(): number {
  return volume;
}

export function getIsFavorite(): boolean {
  return isFavorite;
}

export function getIsSkipping(): boolean {
  return isSkipping;
}

export function getNormalizationGain(): number | null {
  return normalizationGain;
}

// ============ State Setter ============

export interface PlayerState {
  currentTrack: PlayingTrack | null;
  isPlaying: boolean;
  currentTime: number;
  duration: number;
  volume: number;
  isFavorite: boolean;
  isSkipping: boolean;
  normalizationGain: number | null;
}

export function getPlayerState(): PlayerState {
  return {
    currentTrack,
    isPlaying,
    currentTime,
    duration,
    volume,
    isFavorite,
    isSkipping,
    normalizationGain
  };
}

// ============ Track Actions ============

/**
 * Set the current track (called when starting playback)
 * Preserves original quality values for comparison with actual stream
 */
export function setCurrentTrack(track: PlayingTrack | null): void {
  if (track) {
    // Store original quality values before they might be overwritten by stream events
    currentTrack = {
      ...track,
      originalBitDepth: track.bitDepth,
      originalSamplingRate: track.samplingRate
    };
    duration = track.duration;
    currentTime = 0;
    queueEnded = false;
  } else {
    currentTrack = null;
    duration = 0;
    currentTime = 0;
  }
  notifyListeners();
}

/**
 * Set favorite status
 */
export function setIsFavorite(favorite: boolean): void {
  isFavorite = favorite;
  notifyListeners();
}

/**
 * Set skipping state (prevents concurrent skip operations)
 */
export function setIsSkipping(skipping: boolean): void {
  isSkipping = skipping;
  notifyListeners();
}

/**
 * Mark queue as ended (prevents spam when no more tracks)
 */
export function setQueueEnded(ended: boolean): void {
  queueEnded = ended;
}

// ============ Playback Controls ============

/**
 * Set pending session restore - will load track on next play
 */
export function setPendingSessionRestore(trackId: number, position: number): void {
  pendingSessionRestore = { trackId, position };
  console.log('[Player] Set pending session restore:', { trackId, position });
}

/**
 * Clear pending session restore
 */
export function clearPendingSessionRestore(): void {
  pendingSessionRestore = null;
}

/**
 * Check if there's a pending session restore
 */
export function hasPendingSessionRestore(): boolean {
  return pendingSessionRestore !== null;
}

/**
 * Toggle play/pause
 */
export async function togglePlay(): Promise<void> {
  if (!currentTrack) {
    // After stop, try to resume from the queue's current track
    if (onResumeFromStop) {
      await onResumeFromStop();
    }
    return;
  }

  const newIsPlaying = !isPlaying;
  isPlaying = newIsPlaying;
  notifyListeners();

  try {
    if (isCasting()) {
      if (newIsPlaying) {
        await castPlay();
      } else {
        await castPause();
      }
      return;
    }

    if (newIsPlaying) {
      // Check if we need to load the track first (session restore)
      if (pendingSessionRestore && pendingSessionRestore.trackId === currentTrack.id) {
        console.log('[Player] Loading restored track:', pendingSessionRestore.trackId);
        const savedPosition = pendingSessionRestore.position;
        pendingSessionRestore = null; // Clear before loading

        // Restore source-specific playback
        if (currentTrack.source === 'plex') {
          const plexBaseUrl = getUserItem('qbz-plex-poc-base-url') || '';
          const plexToken = getUserItem('qbz-plex-poc-token') || '';
          if (!plexBaseUrl || !plexToken) {
            throw new Error('Missing Plex configuration for session restore');
          }
          const result = await invoke<PlexPlayTrackResult>('plex_play_track', {
            baseUrl: plexBaseUrl,
            token: plexToken,
            ratingKey: String(currentTrack.id)
          });
          if (result.sampling_rate_hz && result.sampling_rate_hz > 0) {
            currentTrack.samplingRate = result.sampling_rate_hz / 1000;
          }
          if (result.bit_depth && result.bit_depth > 0) {
            currentTrack.bitDepth = result.bit_depth;
          }
        } else if (currentTrack.isLocal || currentTrack.id < 0) {
          // Local filesystem track
          const localTrackId = Math.abs(currentTrack.id);
          await invoke('library_play_track', { trackId: localTrackId });
        } else {
          // Qobuz track - use v2_play_track
          await invoke('v2_play_track', {
            trackId: currentTrack.id,
            quality: getStreamingQuality()
          });
        }

        // Seek to saved position after a short delay to let audio load
        if (savedPosition > 0) {
          setTimeout(async () => {
            try {
              await invoke('v2_seek', { position: savedPosition });
              console.log('[Player] Seeked to restored position:', savedPosition);
            } catch (seekErr) {
              console.error('[Player] Failed to seek to restored position:', seekErr);
            }
          }, 500);
        }

      } else {
        await invoke('v2_resume_playback');
      }
    } else {
      await invoke('v2_pause_playback');
    }
  } catch (err) {
    console.error('Failed to toggle playback:', err);
    // Revert on error
    isPlaying = !newIsPlaying;
    notifyListeners();
  }
}

/**
 * Set playing state directly
 */
export function setIsPlaying(playing: boolean): void {
  isPlaying = playing;
  notifyListeners();
}

/**
 * Seek to position
 */
export async function seek(position: number): Promise<void> {
  const clampedPosition = Math.max(0, Math.min(duration, position));
  currentTime = clampedPosition;
  notifyListeners();

  try {
    if (isCasting()) {
      await castSeek(Math.floor(clampedPosition));
      return;
    }

    await invoke('v2_seek', { position: Math.floor(clampedPosition) });
  } catch (err) {
    console.error('Failed to seek:', err);
  }
}

/**
 * Re-read volume from localStorage after user-scoped storage becomes available.
 * Called after login sets the userId, so getUserItem reads the correct scoped key.
 */
export async function resyncPersistedVolume(): Promise<void> {
  const persistedVolume = loadPersistedVolume();
  volume = persistedVolume;
  notifyListeners();
  try {
    await invoke('v2_set_volume', { volume: persistedVolume / 100 });
    console.log('[Player] Resynced volume after login:', persistedVolume);
  } catch {
    console.debug('[Player] Could not resync volume to backend');
  }
}

/**
 * Set volume (0-100)
 * Always persists the volume, even when nothing is playing.
 * The volume will be applied when playback starts.
 */
export async function setVolume(newVolume: number): Promise<void> {
  const clampedVolume = Math.max(0, Math.min(100, newVolume));
  volume = clampedVolume;
  persistVolume(clampedVolume);
  saveSessionVolume(clampedVolume / 100);
  notifyListeners();

  try {
    if (isCasting()) {
      await castSetVolume(clampedVolume);
      return;
    }

    // Try to set volume on backend - will fail silently if no track is loaded
    await invoke('v2_set_volume', { volume: clampedVolume / 100 });
  } catch (err) {
    // Ignore errors when nothing is playing - volume is saved and will apply on next play
    console.debug('Volume set locally (no active playback):', clampedVolume);
  }
}

/**
 * Toggle mute: saves current volume before muting, restores it on unmute.
 * Persists pre-mute volume in localStorage so it survives across sessions.
 */
export async function toggleMute(): Promise<void> {
  if (volume > 0) {
    // Mute: save current volume and set to 0
    preMuteVolume = volume;
    if (typeof localStorage !== 'undefined') {
      setUserItem('qbz-pre-mute-volume', String(volume));
    }
    await setVolume(0);
  } else {
    // Unmute: restore saved volume
    let restoreVolume = preMuteVolume;
    if (restoreVolume === null && typeof localStorage !== 'undefined') {
      const stored = getUserItem('qbz-pre-mute-volume');
      if (stored) restoreVolume = parseFloat(stored);
    }
    preMuteVolume = null;
    if (typeof localStorage !== 'undefined') {
      removeUserItem('qbz-pre-mute-volume');
    }
    await setVolume(restoreVolume && restoreVolume > 0 ? restoreVolume : 75);
  }
}

/**
 * Stop playback
 */
export async function stop(): Promise<void> {
  try {
    if (isCasting()) {
      await castStop();
    } else {
      await invoke('v2_stop_playback');
    }
    isPlaying = false;
    currentTrack = null;
    currentTime = 0;
    duration = 0;
    notifyListeners();
  } catch (err) {
    console.error('Failed to stop playback:', err);
  }
}

// ============ Event-Based Updates ============

/**
 * Set callback for resuming playback after stop (re-play current queue track)
 */
export function setOnResumeFromStop(callback: () => Promise<void>): void {
  onResumeFromStop = callback;
}

/**
 * Set callback for when track ends (for auto-advance)
 */
export function setOnTrackEnded(callback: () => Promise<void>): void {
  onTrackEnded = callback;
  // Also set the same callback for cast track ended (DLNA auto-advance)
  setOnCastTrackEnded(callback);
}

/**
 * Set callback to get the next track ID for gapless pre-queuing
 */
export function setGaplessGetNextTrackId(callback: () => number | null): void {
  gaplessGetNextTrackId = callback;
}

/**
 * Set callback for handling gapless track transitions (metadata/queue update)
 */
export function setOnGaplessTransition(callback: (trackId: number) => Promise<void>): void {
  onGaplessTransition = callback;
}

/**
 * Handle playback event from backend
 */
async function handlePlaybackEvent(event: PlaybackEvent): Promise<void> {
  // Gapless transition: backend changed track_id because gapless playback advanced
  // Handle this BEFORE the external track change handler to prevent stale queue lookups
  const isGaplessTransition = event.track_id !== 0
    && currentTrack
    && event.track_id !== currentTrack.id
    && event.is_playing
    && event.gapless_next_track_id === 0
    && onGaplessTransition;

  if (isGaplessTransition) {
    console.log('[Gapless] Transition detected, updating to track', event.track_id);
    try {
      await onGaplessTransition!(event.track_id);
    } catch (err) {
      console.error('[Gapless] Failed to handle transition:', err);
    }
    return; // Don't process further — the transition callback updates everything
  }

  // Track changed externally (e.g., from remote control)
  if (event.track_id !== 0 && (!currentTrack || event.track_id !== currentTrack.id)) {
    console.log('[Player] Track changed externally, fetching new track info...');
    // Sync queue state when track changes externally (e.g., from remote control)
    syncQueueState().catch(err => console.error('[Player] Failed to sync queue:', err));
    try {
      const queueTrack = await invoke<QueueTrack | null>('get_current_queue_track');
      if (queueTrack && queueTrack.id === event.track_id) {
        const rawRate = queueTrack.sample_rate ?? undefined;
        const normalizedRate = rawRate == null
          ? undefined
          : (queueTrack.is_local || queueTrack.source === 'plex')
            ? rawRate / 1000
            : rawRate;
        currentTrack = {
          id: queueTrack.id,
          title: queueTrack.title,
          artist: queueTrack.artist,
          album: queueTrack.album,
          artwork: queueTrack.artwork_url || '',
          duration: queueTrack.duration_secs,
          quality: queueTrack.hires ? 'Hi-Res' : 'CD Quality',
          bitDepth: queueTrack.bit_depth ?? undefined,
          samplingRate: normalizedRate,
          isLocal: queueTrack.is_local,
          source: queueTrack.source ?? undefined,
          albumId: queueTrack.album_id ?? undefined,
          artistId: queueTrack.artist_id ?? undefined
        };
        duration = queueTrack.duration_secs;
        // Update playback state from event
        currentTime = event.position;
        isPlaying = event.is_playing;
        // Sync volume from backend
        volume = Math.round(event.volume * 100);
        persistVolume(volume);
        console.log('[Player] Updated to external track:', queueTrack.title, 'isPlaying:', isPlaying, 'volume:', volume);
        notifyListeners();
      } else {
        console.warn('[Player] Queue track mismatch or null:', queueTrack?.id, 'vs event:', event.track_id);
      }
    } catch (err) {
      console.error('[Player] Failed to fetch external track:', err);
    }
  }

  if (!currentTrack) {
    console.log('[Player] No current track, ignoring event');
    return;
  }

  // Update playback state if track matches
  if (event.track_id === currentTrack.id) {
    currentTime = event.position;
    isPlaying = event.is_playing;

    // Update volume from backend (e.g., changed via remote control or system)
    const eventVolume = Math.round(event.volume * 100);
    if (eventVolume !== volume) {
      volume = eventVolume;
      persistVolume(volume);
    }

    // Update track with actual stream quality (issue #34).
    // For Plex this is needed because cached metadata can be stale/inaccurate.
    if (event.sample_rate && event.sample_rate > 0) {
      // Convert Hz to kHz for display (44100 -> 44.1)
      currentTrack.samplingRate = event.sample_rate / 1000;
    }
    if (event.bit_depth && event.bit_depth > 0) {
      currentTrack.bitDepth = event.bit_depth;
    }

    // Update normalization gain state
    normalizationGain = event.normalization_gain;

    notifyListeners();

    // Gapless: when backend signals it's approaching end and wants next track queued
    if (event.gapless_ready && !gaplessRequestInFlight && gaplessGetNextTrackId) {
      const nextId = gaplessGetNextTrackId();
      if (nextId && nextId > 0) {
        gaplessRequestInFlight = true;
        console.log('[Gapless] Backend ready, queueing track', nextId);
        invoke<boolean>('v2_play_next_gapless', { trackId: nextId })
          .then((queued) => {
            if (queued) {
              console.log('[Gapless] Track', nextId, 'queued successfully');
            } else {
              console.log('[Gapless] Track', nextId, 'not cached, will use normal transition');
            }
          })
          .catch((err) => {
            console.error('[Gapless] Failed to queue track:', err);
          })
          .finally(() => {
            gaplessRequestInFlight = false;
          });
      }
    }

    // Check if track ended - auto-advance to next
    if (
      event.duration > 0 &&
      event.position >= event.duration - 1 &&
      !event.is_playing &&
      !isAdvancingTrack &&
      !queueEnded &&
      onTrackEnded
    ) {
      console.log('Track finished, advancing to next...');
      isAdvancingTrack = true;

      try {
        await onTrackEnded();
      } catch (err) {
        console.error('Failed to auto-advance:', err);
      } finally {
        isAdvancingTrack = false;
      }
    }
  }
}

/**
 * Start listening for playback events from backend
 */
export async function startPolling(): Promise<void> {
  if (eventUnlisten) return;

  try {
    eventUnlisten = await listen<PlaybackEvent>('playback:state', (event) => {
      handlePlaybackEvent(event.payload);
    });
    console.log('Started listening for playback events');

    // Sync persisted volume to backend on startup
    // This ensures the backend knows the saved volume before any playback starts
    // Note: at this point userId may not be set yet, so the volume may come from
    // the unscoped key. resyncPersistedVolume() is called after login to fix this.
    const persistedVolume = loadPersistedVolume();
    try {
      await invoke('v2_set_volume', { volume: persistedVolume / 100 });
      console.log('[Player] Synced persisted volume to backend:', persistedVolume);
    } catch {
      // Backend might not be ready yet, volume will be applied on first interaction
      console.debug('[Player] Could not sync volume to backend yet');
    }
  } catch (err) {
    console.error('Failed to start playback event listener:', err);
  }

  // Also subscribe to cast store for DLNA position updates
  if (!castUnsubscribe) {
    castUnsubscribe = subscribeToCast(() => {
      if (isCasting()) {
        const castPos = getCastPosition();
        if (castPos.positionSecs !== currentTime) {
          currentTime = castPos.positionSecs;
          notifyListeners();
        }
      }
    });

    // Set callback for when cast disconnects - reset player state
    setOnCastDisconnected(() => {
      isPlaying = false;
      notifyListeners();
    });
  }
}

/**
 * Stop listening for playback events
 */
export function stopPolling(): void {
  if (eventUnlisten) {
    eventUnlisten();
    eventUnlisten = null;
    console.log('Stopped listening for playback events');
  }
  if (castUnsubscribe) {
    castUnsubscribe();
    castUnsubscribe = null;
  }
}

/**
 * Check if event listener is active
 */
export function isPollingActive(): boolean {
  return eventUnlisten !== null;
}

// ============ Cleanup ============

/**
 * Reset all state (for logout)
 */
export function reset(): void {
  stopPolling();
  currentTrack = null;
  isPlaying = false;
  currentTime = 0;
  duration = 0;
  isFavorite = false;
  isAdvancingTrack = false;
  isSkipping = false;
  queueEnded = false;
  notifyListeners();
}
