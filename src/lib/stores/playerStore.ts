/**
 * Player State Store
 *
 * Manages playback state including current track, play/pause, position, volume.
 * Uses Tauri events for real-time updates from the backend.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  isCasting,
  castPlay,
  castPause,
  castSeek,
  castSetVolume,
  castStop
} from '$lib/stores/castStore';

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
  isLocal?: boolean;
  // Optional IDs for recommendation tracking
  albumId?: string;
  artistId?: number;
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
}

// ============ State ============

let currentTrack: PlayingTrack | null = null;
let isPlaying = false;
let currentTime = 0;
let duration = 0;
let volume = 75;
let isFavorite = false;
// Event listener state (replaces polling)
let eventUnlisten: UnlistenFn | null = null;
let isAdvancingTrack = false;
let isSkipping = false;
let queueEnded = false;

// Callbacks for track advancement (set by consumer)
let onTrackEnded: (() => Promise<void>) | null = null;

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

// ============ State Setter ============

export interface PlayerState {
  currentTrack: PlayingTrack | null;
  isPlaying: boolean;
  currentTime: number;
  duration: number;
  volume: number;
  isFavorite: boolean;
  isSkipping: boolean;
}

export function getPlayerState(): PlayerState {
  return {
    currentTrack,
    isPlaying,
    currentTime,
    duration,
    volume,
    isFavorite,
    isSkipping
  };
}

// ============ Track Actions ============

/**
 * Set the current track (called when starting playback)
 */
export function setCurrentTrack(track: PlayingTrack | null): void {
  currentTrack = track;
  if (track) {
    duration = track.duration;
    currentTime = 0;
    queueEnded = false;
  } else {
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
  if (!currentTrack) return;

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
        // Load the track from Qobuz
        await invoke('play_track', { trackId: currentTrack.id });

        // Seek to saved position after a short delay to let audio load
        if (savedPosition > 0) {
          setTimeout(async () => {
            try {
              await invoke('seek', { position: savedPosition });
              console.log('[Player] Seeked to restored position:', savedPosition);
            } catch (seekErr) {
              console.error('[Player] Failed to seek to restored position:', seekErr);
            }
          }, 500);
        }

      } else {
        await invoke('resume_playback');
      }
    } else {
      await invoke('pause_playback');
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

    await invoke('seek', { position: Math.floor(clampedPosition) });
  } catch (err) {
    console.error('Failed to seek:', err);
  }
}

/**
 * Set volume (0-100)
 */
export async function setVolume(newVolume: number): Promise<void> {
  const clampedVolume = Math.max(0, Math.min(100, newVolume));
  volume = clampedVolume;
  notifyListeners();

  try {
    if (isCasting()) {
      await castSetVolume(clampedVolume);
      return;
    }

    await invoke('set_volume', { volume: clampedVolume / 100 });
  } catch (err) {
    console.error('Failed to set volume:', err);
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
      await invoke('stop_playback');
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
 * Set callback for when track ends (for auto-advance)
 */
export function setOnTrackEnded(callback: () => Promise<void>): void {
  onTrackEnded = callback;
}

/**
 * Handle playback event from backend
 */
async function handlePlaybackEvent(event: PlaybackEvent): Promise<void> {
  if (!currentTrack) return;

  // Only update if we have a matching track
  if (event.track_id === currentTrack.id) {
    currentTime = event.position;
    isPlaying = event.is_playing;
    notifyListeners();

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
  } catch (err) {
    console.error('Failed to start playback event listener:', err);
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
