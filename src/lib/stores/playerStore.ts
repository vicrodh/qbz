/**
 * Player State Store
 *
 * Manages playback state including current track, play/pause, position, volume.
 * Uses Tauri events for real-time updates from the backend.
 */

import { invoke } from '@tauri-apps/api/core';
import { cmdPause, cmdResume, cmdStop, cmdSeek, cmdSetVolume, cmdPlayTrack } from '$lib/services/commandRouter';
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
  /** Qobuz subtitle/edition info (e.g. "Player's Ball Mix"). Render
   * via formatTrackTitle() in UI components (#360). */
  version?: string | null;
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
  parental_warning?: boolean;
  /** Start of this track inside the underlying audio file (CUE virtual
   * tracks). When set, the seekbar subtracts it from event.position so
   * the displayed elapsed/remaining are relative to the virtual track,
   * not the entire FLAC. */
  cueStartSecs?: number;
  /** End of this track inside the underlying audio file. When set with
   * cueStartSecs, drives the seekbar's max (= end - start) instead of
   * the full file duration. */
  cueEndSecs?: number;
}

interface BackendPlaybackState {
  is_playing: boolean;
  position: number;
  duration: number;
  track_id: number;
  volume: number;
}

// Mirrors EPHEMERAL_ID_FLOOR in src-tauri/src/ephemeral_library/mod.rs.
// Track ids at or above this value are synthetic ids from the in-memory
// ephemeral cache, not DB rows; the playback logic special-cases them
// in a couple of places (gapless transition is_playing carve-out, CUE
// virtual-track position translation).
// 2 ** 48 (NOT 1 << 48 — JavaScript bitwise shift is 32-bit, 1 << 48
// silently truncates to 65536 and every Qobuz track ends up classified
// as ephemeral, suppressing favorites and breaking session save).
const EPHEMERAL_ID_FLOOR = 2 ** 48;

/**
 * Bit-perfect mode of the active audio stream, reported by the Rust backend.
 * Matches the `qbz_audio::BitPerfectMode` enum via serde.
 */
export type BitPerfectMode = 'DirectHardware' | 'PluginFallback' | 'Disabled';

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
  buffer_progress?: number | null; // Streaming buffer progress (0-1, null = fully cached)
  bit_perfect_mode?: BitPerfectMode | null; // None until first stream opens
}

// Queue track from backend (for external track sync)
interface QueueTrack {
  id: number;
  title: string;
  /** Subtitle/edition info from Qobuz (e.g. "Player's Ball Mix") (#360). */
  version?: string | null;
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
  source_item_id_hint?: string | null;
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
    const parsed = Number.parseFloat(stored);
    if (!Number.isNaN(parsed) && parsed >= 0 && parsed <= 100) {
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

// Wall-clock anchor for currentTime extrapolation. The Rust backend emits
// position events at 250ms cadence — at 60Hz rAF, getCurrentTime() would
// otherwise return a staircase that's frozen for ~15 frames between
// updates, causing karaoke lyrics to start "midway" (detection lag) and
// to end before the gradient reaches the end (final-segment cutoff).
//
// We anchor (audio position, wall clock) on every event-driven update,
// then extrapolate forward at 1x while playing. Backend position is
// already wall-clock-based on the Rust side (see lib.rs comment near the
// 250ms emit loop), so JS extrapolation does the same math the backend
// is doing — no semantic divergence, just finer time resolution.
let positionAnchorWallClockMs = 0;
let positionAnchorSecs = 0;

function anchorPosition(secs: number): void {
  positionAnchorSecs = secs;
  positionAnchorWallClockMs = performance.now();
}
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
let bufferProgress: number | null = null;  // Streaming buffer progress (0-1, null = fully cached)
let bitPerfectMode: BitPerfectMode | null = null;  // Reported by backend when stream opens
let pendingSeekPosition: number | null = null;
let seekRequestInFlight = false;
let seekTargetPosition: number | null = null;
let seekGuardUntilMs = 0;
const SEEK_GUARD_WINDOW_MS = 1500;
const SEEK_SETTLE_TOLERANCE_SECS = 1;

// Callbacks for track advancement (set by consumer)
let onTrackEnded: (() => Promise<void>) | null = null;
let onResumeFromStop: (() => Promise<void>) | null = null;
let onTogglePlayOverride: (() => Promise<boolean>) | null = null;

// Remote control mode: when active, external service (QConnect) controls playback.
// Disables gapless interception, auto-advance, and resume-from-stop in the event handler.
let remoteControlMode = false;

// Gapless: callback to get the next track ID for pre-queuing
let gaplessGetNextTrackId: (() => number | null) | null = null;
// Gapless: callback when backend transitions to next track (update frontend metadata/queue)
let onGaplessTransition: ((trackId: number) => Promise<void>) | null = null;
// Track whether gapless pre-queue request is in flight
let gaplessRequestInFlight = false;
// One-shot guard: attempt gapless pre-queue only once per current track.
let gaplessAttemptTrackId: number | null = null;

// Session restore state — when set, next play will load the track first.
// `positionSecs` (when present) is honored only if the user enabled the
// "resume playback position" preference (issue #317). Default is to
// start the restored track from the beginning, so opening the app the
// next morning lands on a clean 0:00.
let pendingSessionRestore: { trackId: number; positionSecs?: number } | null = null;

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
  // When paused or before the first anchor, return the stored value
  // (which is what listeners last persisted). While playing, extrapolate
  // from the anchor at 1x audio rate.
  if (!isPlaying || positionAnchorWallClockMs === 0) return currentTime;
  const elapsedSecs = (performance.now() - positionAnchorWallClockMs) / 1000;
  // Cap extrapolation at 1s to bound drift if the backend stops emitting
  // (network stall on cast, suspended audio thread). After the cap, we
  // freeze at the last + cap rather than runway indefinitely.
  return positionAnchorSecs + Math.min(elapsedSecs, 1);
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

export function getBitPerfectMode(): BitPerfectMode | null {
  return bitPerfectMode;
}

async function flushPendingSeek(): Promise<void> {
  if (seekRequestInFlight) return;
  if (pendingSeekPosition === null) return;

  const targetPosition = pendingSeekPosition;
  pendingSeekPosition = null;
  seekTargetPosition = targetPosition;
  seekGuardUntilMs = Date.now() + SEEK_GUARD_WINDOW_MS;

  seekRequestInFlight = true;
  try {
    await cmdSeek(targetPosition);
  } catch (err) {
    console.error('Failed to seek:', err);
  } finally {
    seekRequestInFlight = false;
    if (pendingSeekPosition !== null) {
      void flushPendingSeek();
    }
  }
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
  bufferProgress: number | null;
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
    normalizationGain,
    bufferProgress
  };
}

// ============ Track Actions ============

/**
 * Attach CUE virtual-track boundary info to the current track. Called
 * by views that know about CUE structure (today: LocalLibraryView for
 * ephemeral CUE folders) so the seekbar position/duration can be
 * recomputed against the virtual track instead of the whole audio
 * file. Pass `null` for both args to clear.
 */
export function patchCurrentTrackCueInfo(
  startSecs: number | null,
  endSecs: number | null
): void {
  if (!currentTrack) return;
  const nextStart = startSecs != null ? startSecs : undefined;
  const nextEnd = endSecs != null ? endSecs : undefined;
  // Skip work if nothing changed; prevents unnecessary listener spam
  // since the LocalLibraryView $effect that calls this fires on every
  // activeTrackId tick.
  if (currentTrack.cueStartSecs === nextStart && currentTrack.cueEndSecs === nextEnd) {
    return;
  }
  currentTrack = {
    ...currentTrack,
    cueStartSecs: nextStart,
    cueEndSecs: nextEnd,
  };
  // Recompute the displayed duration: virtual track length when both
  // boundaries are known, otherwise fall back to the track's intrinsic
  // duration field (which is already the CUE virtual duration coming
  // from cue_to_tracks anyway, but keep the dual-source for robustness).
  if (nextStart != null && nextEnd != null) {
    duration = Math.max(0, nextEnd - nextStart);
  } else {
    duration = currentTrack.duration;
  }
  notifyListeners();
}

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
    anchorPosition(0);
    queueEnded = false;
  } else {
    currentTrack = null;
    duration = 0;
    currentTime = 0;
    anchorPosition(0);
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
 * Set pending session restore — will load track on next play. Pass
 * `positionSecs` only when the user opted into resume-playback-position
 * (issue #317). Without it, the restored track starts from 0.
 */
export function setPendingSessionRestore(trackId: number, positionSecs?: number): void {
  pendingSessionRestore = { trackId, positionSecs };
  // Pre-position the seekbar at the saved offset so the visual state
  // reflects "paused at X" from the moment the app opens, instead of
  // sitting at 0:00 until the user hits play. The backend hasn't
  // loaded the track yet, but currentTime is the source for the
  // seekbar render — when togglePlay eventually fires it'll already be
  // anchored to the same position the backend resumes at.
  if (positionSecs && positionSecs > 0) {
    currentTime = positionSecs;
    notifyListeners();
  }
  console.log(
    '[Player] Set pending session restore:',
    trackId,
    positionSecs ? `(resume @ ${positionSecs}s)` : '(start at 0:00)'
  );
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
  if (onTogglePlayOverride) {
    try {
      const handled = await onTogglePlayOverride();
      if (handled) {
        return;
      }
    } catch (err) {
      console.error('Remote toggle playback override failed:', err);
      return;
    }
  }

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
        const restorePosition = pendingSessionRestore.positionSecs ?? 0;
        console.log(
          '[Player] Loading restored track:',
          pendingSessionRestore.trackId,
          restorePosition > 0 ? `(will seek to ${restorePosition}s)` : '(from start)'
        );
        pendingSessionRestore = null; // Clear before loading

        // Restore source-specific playback (always from start; if the
        // user enabled resume-playback-position the seek is queued
        // below and applied once the stream is loaded).
        if (currentTrack.source === 'plex') {
          const plexBaseUrl = getUserItem('qbz-plex-poc-base-url') || '';
          const plexToken = getUserItem('qbz-plex-poc-token') || '';
          if (!plexBaseUrl || !plexToken) {
            throw new Error('Missing Plex configuration for session restore');
          }
          const result = await invoke<PlexPlayTrackResult>('v2_plex_play_track', {
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
          // Plex loads via Symphonia just like a local file — Seek
          // works once the engine is up.
          if (restorePosition > 1) {
            pendingSeekPosition = restorePosition;
            void flushPendingSeek();
          }
        } else if (currentTrack.isLocal || currentTrack.id < 0) {
          // Local filesystem track. Apply the user's saved playback
          // position (#317 opt-in) via the seek path — local tracks
          // load full audio data into the player, so Seek works as soon
          // as the engine is up.
          const localTrackId = Math.abs(currentTrack.id);
          await invoke('v2_library_play_track', { trackId: localTrackId });
          if (restorePosition > 1) {
            pendingSeekPosition = restorePosition;
            void flushPendingSeek();
          }
        } else {
          // Qobuz track — use v2_play_track. Pass duration so the
          // streaming backend's current_position() doesn't clamp to 0
          // (the value flows into thread_state.duration and seekbar
          // progress is capped by .min(duration)). For session resume,
          // hand the restore offset directly to v2_play_track via
          // startPositionSecs (#315 extension): the streaming path
          // waits for enough buffer to cover the offset and pre-skips
          // decoder output before engaging audio, instead of the
          // play-then-seek dance that silently dropped the seek
          // because the audio thread's Seek handler bails when there
          // is no `current_audio_data` (streaming uses
          // `current_streaming_source` instead).
          await cmdPlayTrack(
            currentTrack.id,
            getStreamingQuality(),
            currentTrack.duration ? Math.round(currentTrack.duration) : null,
            null,
            restorePosition > 1 ? restorePosition : null,
          );
        }

      } else {
        await cmdResume();
      }
    } else {
      await cmdPause();
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
  anchorPosition(clampedPosition);
  notifyListeners();

  try {
    if (isCasting()) {
      await castSeek(Math.floor(clampedPosition));
      return;
    }
    pendingSeekPosition = clampedPosition;
    void flushPendingSeek();
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
    await cmdSetVolume(persistedVolume / 100);
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
    await cmdSetVolume(clampedVolume / 100);
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
      if (stored) restoreVolume = Number.parseFloat(stored);
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
      await cmdStop();
    }
    isPlaying = false;
    currentTrack = null;
    currentTime = 0;
    anchorPosition(0);
    duration = 0;
    gaplessAttemptTrackId = null;
    gaplessRequestInFlight = false;
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
 * Optional transport handoff for play/pause.
 * Returns true when the action was handled outside the local player.
 */
export function setOnTogglePlayOverride(
  callback: (() => Promise<boolean>) | null
): void {
  onTogglePlayOverride = callback;
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
 * Enable/disable remote control mode (e.g. QConnect renderer).
 * When active, gapless interception and auto-advance are disabled
 * so the external service controls track transitions.
 */
export function setRemoteControlMode(active: boolean): void {
  remoteControlMode = active;
}

/**
 * Handle playback event from backend
 */
async function handlePlaybackEvent(event: PlaybackEvent): Promise<void> {
  const prevTrackId = currentTrack?.id ?? 0;
  const prevDuration = duration;
  const prevPosition = currentTime;
  const prevWasPlaying = isPlaying;
  // Captured BEFORE the null-out below: did we set up a gapless prefetch for
  // the track we currently believe is playing? gaplessAttemptTrackId is set
  // to currentTrack.id at the v2_play_next_gapless call site (line ~803).
  // Without this gate, every external track change (QConnect renderer command,
  // MPRIS, etc.) while qbz is the local renderer would mis-classify as a
  // gapless transition and run the onGaplessTransition callback every backend
  // tick (~1Hz) without ever updating currentTrack — leaving the UI stale and
  // spamming v2_next_track.
  const gaplessAttemptedForCurrent =
    gaplessAttemptTrackId !== null
    && currentTrack !== null
    && gaplessAttemptTrackId === currentTrack.id;

  if (event.track_id !== 0 && gaplessAttemptTrackId !== null && gaplessAttemptTrackId !== event.track_id) {
    gaplessAttemptTrackId = null;
  }

  // Gapless transition: backend changed track_id because gapless playback advanced
  // Handle this BEFORE the external track change handler to prevent stale queue lookups
  // Skip when remote control mode is active — external service controls transitions.
  // Requires gaplessAttemptedForCurrent so external track changes fall through
  // to the "track changed externally" block (which actually updates currentTrack).
  //
  // Ephemeral tracks (id >= 2^48) skip the `event.is_playing` requirement.
  // The audio engine briefly reports `is_playing=false` during the swap
  // between two ephemeral file-data buffers, and that transient state
  // arrives BEFORE the player can flip is_playing back to true. Without
  // this carve-out the event misclassifies as an external change, the
  // queue's current_index hasn't advanced (it's a separate state machine),
  // and the UI freezes on the previous track. Qobuz/Plex tracks don't
  // see this race because their playback path keeps is_playing pinned
  // through the streaming buffer transition.
  const isEphemeralTransition = event.track_id >= EPHEMERAL_ID_FLOOR;
  const isGaplessTransition = !remoteControlMode
    && event.track_id !== 0
    && currentTrack
    && event.track_id !== currentTrack.id
    && (event.is_playing || isEphemeralTransition)
    && event.gapless_next_track_id === 0
    && gaplessAttemptedForCurrent
    && onGaplessTransition;

  if (isGaplessTransition) {
    console.log('[Gapless] Transition detected, updating to track', event.track_id);
    try {
      await onGaplessTransition!(event.track_id);
      gaplessAttemptTrackId = null;
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
      const queueTrack = await invoke<QueueTrack | null>('v2_get_current_queue_track');
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
          version: queueTrack.version ?? null,
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
        anchorPosition(event.position);
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

  // Fallback end-of-track detection:
  // some backend paths can emit a terminal stop with track_id=0 instead of
  // a final same-track frame at duration. Treat as natural end only when
  // previous progress was already at the tail and playback was active.
  if (
    !remoteControlMode &&
    event.track_id === 0 &&
    prevTrackId !== 0 &&
    prevDuration > 0 &&
    prevPosition >= prevDuration - 2 &&
    !event.is_playing &&
    prevWasPlaying &&
    !isAdvancingTrack &&
    !queueEnded &&
    onTrackEnded
  ) {
    console.log('[Player] Track ended (fallback), advancing to next...');
    isAdvancingTrack = true;

    try {
      await onTrackEnded();
    } catch (err) {
      console.error('[Player] Failed fallback auto-advance:', err);
    } finally {
      isAdvancingTrack = false;
    }
    return;
  }

  // CUE virtual tracks share an underlying audio file with each other
  // (one FLAC, multiple cue slices). After a boundary auto-advance, the
  // player still emits events with the FIRST slice's track_id while the
  // displayed currentTrack has moved on to a later virtual track. Accept
  // those events as belonging to the same logical playback as long as
  // both ids are in the ephemeral range — within a session that means
  // they share the same source file, so position is meaningful when
  // adjusted by the displayed virtual track's cueStartSecs.
  const isCueVirtualUnderlying =
    currentTrack.cueStartSecs != null
    && event.track_id !== currentTrack.id
    && event.track_id >= EPHEMERAL_ID_FLOOR
    && currentTrack.id >= EPHEMERAL_ID_FLOOR;

  // Update playback state if track matches
  if (event.track_id === currentTrack.id || isCueVirtualUnderlying) {
    // Translate the player's absolute position into the displayed
    // (virtual) position when the current track is a CUE virtual slice.
    // Without the offset, a CUE track that lives at e.g. 7:26 inside the
    // FLAC would surface 7:26 / 0:00 on the seekbar from frame zero.
    const cueOffset = currentTrack.cueStartSecs ?? 0;
    const toDisplayed = (raw: number) => Math.max(0, raw - cueOffset);
    const now = Date.now();
    const seekGuardActive = seekTargetPosition !== null && now < seekGuardUntilMs;
    if (seekGuardActive) {
      const target = seekTargetPosition;
      if (target === null) {
        currentTime = toDisplayed(event.position);
        anchorPosition(currentTime);
        isPlaying = event.is_playing;
        return;
      }
      const delta = Math.abs(event.position - target);
      if (delta <= SEEK_SETTLE_TOLERANCE_SECS) {
        seekTargetPosition = null;
        seekGuardUntilMs = 0;
        currentTime = toDisplayed(event.position);
        anchorPosition(currentTime);
      } else {
        // Ignore stale position updates briefly after seek to avoid UI snap-back.
      }
    } else {
      if (seekTargetPosition !== null && now >= seekGuardUntilMs) {
        seekTargetPosition = null;
        seekGuardUntilMs = 0;
      }
      currentTime = toDisplayed(event.position);
      anchorPosition(currentTime);
    }
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
    bufferProgress = event.buffer_progress ?? null;
    bitPerfectMode = event.bit_perfect_mode ?? null;

    notifyListeners();

    // Gapless: when backend signals it's approaching end and wants next track queued
    if (
      event.gapless_ready &&
      !gaplessRequestInFlight &&
      gaplessGetNextTrackId &&
      gaplessAttemptTrackId !== currentTrack.id
    ) {
      const nextId = gaplessGetNextTrackId();
      if (nextId && nextId > 0) {
        gaplessRequestInFlight = true;
        gaplessAttemptTrackId = currentTrack.id;
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
            // Keep gaplessAttemptTrackId pinned to currentTrack.id so the
            // one-shot guard above blocks re-attempts for the same
            // playing track. It is cleared naturally when the playback
            // event reports a new track_id (see line ~634). Clearing it
            // here defeats the one-shot intent and causes per-second
            // spam when the backend reports "not cached".
          });
      }
    }

    // Check if track ended - auto-advance to next
    if (
      !remoteControlMode &&
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
      await cmdSetVolume(persistedVolume / 100);
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
          anchorPosition(castPos.positionSecs);
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
  pendingSeekPosition = null;
  seekRequestInFlight = false;
  seekTargetPosition = null;
  seekGuardUntilMs = 0;
  onTogglePlayOverride = null;
  stopPolling();
  currentTrack = null;
  isPlaying = false;
  currentTime = 0;
  anchorPosition(0);
  duration = 0;
  isFavorite = false;
  isAdvancingTrack = false;
  isSkipping = false;
  queueEnded = false;
  gaplessAttemptTrackId = null;
  gaplessRequestInFlight = false;
  notifyListeners();
}
