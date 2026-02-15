/**
 * Cast Store
 *
 * Manages casting state across the app.
 * Tracks connected device and protocol.
 */

import { invoke } from '@tauri-apps/api/core';
import {
  getCurrentTrack,
  getIsPlaying,
  getCurrentTime,
  setIsPlaying,
  type PlayingTrack
} from './playerStore';

export type CastProtocol = 'chromecast' | 'dlna' | 'airplay';

export interface CastDevice {
  id: string;
  name: string;
  ip: string;
  port: number;
}

export interface CastPositionInfo {
  positionSecs: number;
  durationSecs: number;
  transportState: string;
}

interface CastState {
  isConnected: boolean;
  protocol: CastProtocol | null;
  device: CastDevice | null;
  isPlaying: boolean;
  currentTrackId: number | null;
  // Position tracking for DLNA
  positionSecs: number;
  durationSecs: number;
}

let state: CastState = {
  isConnected: false,
  protocol: null,
  device: null,
  isPlaying: false,
  currentTrackId: null,
  positionSecs: 0,
  durationSecs: 0
};

// Polling interval for DLNA position updates
let positionPollInterval: ReturnType<typeof setInterval> | null = null;
const POSITION_POLL_INTERVAL_MS = 1000;

// Callback for when track ends (for auto-advance)
let onCastTrackEnded: (() => Promise<void>) | null = null;

// Callback for when cast disconnects (to reset player state)
let onCastDisconnected: (() => void) | null = null;

// Callback for asking user if they want to continue locally after disconnect
// Returns true if user wants to continue, false otherwise
let onAskContinueLocally: ((track: PlayingTrack, position: number) => Promise<boolean>) | null = null;

// Track end detection state
let lastTransportState: string = '';
let trackEndDetected = false;

const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Subscribe to cast state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

/**
 * Get current cast state
 */
export function getCastState(): CastState {
  return { ...state };
}

/**
 * Check if currently casting
 */
export function isCasting(): boolean {
  return state.isConnected;
}

/**
 * Get connected device info
 */
export function getConnectedDevice(): CastDevice | null {
  return state.device;
}

/**
 * Get connected protocol
 */
export function getConnectedProtocol(): CastProtocol | null {
  return state.protocol;
}

/**
 * Connect to a cast device
 *
 * IMPORTANT: If local playback is active, we capture the current track/position,
 * stop local playback, establish the cast connection, then resume on the cast device.
 */
export async function connectToDevice(device: CastDevice, protocol: CastProtocol): Promise<void> {
  // Capture current playback state BEFORE stopping
  const wasPlaying = getIsPlaying();
  const currentTrack = getCurrentTrack();
  const currentPosition = getCurrentTime();

  console.log('[CastStore] Connecting to device, current state:', {
    wasPlaying,
    trackId: currentTrack?.id,
    position: currentPosition
  });

  // Stop local playback BEFORE connecting to avoid stream conflicts
  if (wasPlaying || currentTrack) {
    console.log('[CastStore] Stopping local playback before cast connection...');
    try {
      await invoke('v2_stop_playback');
      setIsPlaying(false);
    } catch (err) {
      // Ignore errors - player might not be playing
      console.log('[CastStore] v2_stop_playback returned:', err);
    }
  }

  try {
    // Now establish the cast connection
    switch (protocol) {
      case 'chromecast':
        await invoke('cast_connect', { deviceId: device.id });
        break;
      case 'dlna':
        await invoke('dlna_connect', { deviceId: device.id });
        break;
      case 'airplay':
        await invoke('airplay_connect', { deviceId: device.id });
        break;
    }

    state = {
      ...state,
      isConnected: true,
      protocol,
      device,
      isPlaying: false,
      currentTrackId: null
    };
    notifyListeners();

    // If there was a track playing, resume it on the cast device
    if (wasPlaying && currentTrack && !currentTrack.isLocal) {
      console.log('[CastStore] Resuming track on cast device:', currentTrack.title);
      try {
        await castTrack(currentTrack.id, {
          title: currentTrack.title,
          artist: currentTrack.artist,
          album: currentTrack.album,
          artworkUrl: currentTrack.artwork,
          durationSecs: currentTrack.duration
        });

        // Update playerStore to reflect playing state (fixes play button UI)
        setIsPlaying(true);

        // Try to seek to the saved position (if significant)
        if (currentPosition > 5) {
          console.log('[CastStore] Seeking to saved position:', currentPosition);
          // Small delay to let the stream start
          setTimeout(async () => {
            try {
              await castSeek(currentPosition);
            } catch (seekErr) {
              console.log('[CastStore] Could not restore position:', seekErr);
            }
          }, 2000);
        }
      } catch (castErr) {
        console.error('[CastStore] Failed to resume track on cast device:', castErr);
        // Don't throw - connection succeeded, just playback resume failed
      }
    }
  } catch (err) {
    console.error('[CastStore] Failed to connect:', err);
    throw err;
  }
}

/**
 * Disconnect from current device
 *
 * If a track was playing, asks user if they want to continue locally.
 */
export async function disconnect(): Promise<void> {
  if (!state.isConnected || !state.protocol) return;

  // Capture current state BEFORE disconnecting
  const wasPlaying = state.isPlaying;
  const currentTrack = getCurrentTrack();
  const currentPosition = state.positionSecs;
  const savedProtocol = state.protocol;

  console.log('[CastStore] Disconnecting, current state:', {
    wasPlaying,
    trackId: currentTrack?.id,
    position: currentPosition
  });

  // Stop position polling first
  stopPositionPolling();

  try {
    await castStop();
    switch (savedProtocol) {
      case 'chromecast':
        await invoke('cast_disconnect');
        break;
      case 'dlna':
        await invoke('dlna_disconnect');
        break;
      case 'airplay':
        await invoke('airplay_disconnect');
        break;
    }
  } catch (err) {
    console.error('[CastStore] Failed to disconnect:', err);
  }

  state = {
    isConnected: false,
    protocol: null,
    device: null,
    isPlaying: false,
    currentTrackId: null,
    positionSecs: 0,
    durationSecs: 0
  };
  notifyListeners();

  // If there was a track playing, ask user if they want to continue locally
  if (wasPlaying && currentTrack && !currentTrack.isLocal && onAskContinueLocally) {
    console.log('[CastStore] Asking user if they want to continue locally...');
    try {
      const continueLocally = await onAskContinueLocally(currentTrack, currentPosition);
      if (continueLocally) {
        console.log('[CastStore] User wants to continue locally, starting playback...');
        // Start local playback - the callback handler will do this
        // since it has access to playbackService
      } else {
        console.log('[CastStore] User declined to continue locally');
        // Just notify that cast disconnected
        if (onCastDisconnected) {
          onCastDisconnected();
        }
      }
    } catch (err) {
      console.error('[CastStore] Failed to handle continue locally:', err);
      if (onCastDisconnected) {
        onCastDisconnected();
      }
    }
  } else {
    // Notify player to reset its state
    if (onCastDisconnected) {
      onCastDisconnected();
    }
  }
}

/**
 * Set callback for when cast disconnects
 */
export function setOnCastDisconnected(callback: () => void): void {
  onCastDisconnected = callback;
}

/**
 * Set callback for asking user if they want to continue locally after disconnect
 * The callback receives the track and position, and should return true if user wants to continue
 */
export function setOnAskContinueLocally(callback: (track: PlayingTrack, position: number) => Promise<boolean>): void {
  onAskContinueLocally = callback;
}

/**
 * Cast a track to the connected device
 */
export async function castTrack(
  trackId: number,
  metadata: {
    title: string;
    artist: string;
    album: string;
    artworkUrl?: string;
    durationSecs?: number;
  }
): Promise<void> {
  if (!state.isConnected || !state.protocol) {
    throw new Error('Not connected to any cast device');
  }

  try {
    switch (state.protocol) {
      case 'chromecast':
        await invoke('cast_play_track', {
          trackId,
          metadata: {
            title: metadata.title,
            artist: metadata.artist,
            album: metadata.album,
            artwork_url: metadata.artworkUrl,
            duration_secs: metadata.durationSecs
          }
        });
        break;
      case 'dlna':
        await invoke('dlna_play_track', {
          trackId: trackId,
          metadata: {
            title: metadata.title,
            artist: metadata.artist,
            album: metadata.album,
            artwork_url: metadata.artworkUrl,
            duration_secs: metadata.durationSecs
          }
        });
        break;
      case 'airplay':
        await invoke('airplay_load_media', {
          metadata: {
            title: metadata.title,
            artist: metadata.artist,
            album: metadata.album,
            artwork_url: metadata.artworkUrl,
            duration_secs: metadata.durationSecs
          }
        });
        await invoke('airplay_play');
        break;
    }

    state = {
      ...state,
      isPlaying: true,
      currentTrackId: trackId,
      positionSecs: 0,
      durationSecs: metadata.durationSecs || 0
    };
    notifyListeners();
    
    // Start position polling for DLNA and Chromecast
    if (state.protocol === 'dlna' || state.protocol === 'chromecast') {
      startPositionPolling();
    }
  } catch (err) {
    console.error('[CastStore] Failed to cast track:', err);
    throw err;
  }
}

/**
 * Play/resume on cast device
 */
export async function castPlay(): Promise<void> {
  if (!state.isConnected || !state.protocol) return;

  try {
    switch (state.protocol) {
      case 'chromecast':
        await invoke('cast_play');
        break;
      case 'dlna':
        await invoke('dlna_play');
        break;
      case 'airplay':
        await invoke('airplay_play');
        break;
    }
    state = { ...state, isPlaying: true };
    notifyListeners();
  } catch (err) {
    console.error('[CastStore] Failed to play:', err);
  }
}

/**
 * Pause on cast device
 */
export async function castPause(): Promise<void> {
  if (!state.isConnected || !state.protocol) return;

  try {
    switch (state.protocol) {
      case 'chromecast':
        await invoke('cast_pause');
        break;
      case 'dlna':
        await invoke('dlna_pause');
        break;
      case 'airplay':
        await invoke('airplay_pause');
        break;
    }
    state = { ...state, isPlaying: false };
    notifyListeners();
  } catch (err) {
    console.error('[CastStore] Failed to pause:', err);
  }
}

/**
 * Stop on cast device
 */
export async function castStop(): Promise<void> {
  if (!state.isConnected || !state.protocol) return;

  try {
    switch (state.protocol) {
      case 'chromecast':
        await invoke('cast_stop');
        break;
      case 'dlna':
        await invoke('dlna_stop');
        break;
      case 'airplay':
        await invoke('airplay_stop');
        break;
    }
    state = { ...state, isPlaying: false, currentTrackId: null };
    notifyListeners();
  } catch (err) {
    console.error('[CastStore] Failed to stop:', err);
  }
}

/**
 * Seek on cast device
 */
export async function castSeek(positionSecs: number): Promise<void> {
  if (!state.isConnected || !state.protocol) return;

  try {
    switch (state.protocol) {
      case 'chromecast':
        await invoke('cast_seek', { positionSecs });
        break;
      case 'dlna':
        await invoke('dlna_seek', { positionSecs });
        break;
      case 'airplay':
        // AirPlay seek - not implemented
        break;
    }
  } catch (err) {
    console.error('[CastStore] Failed to seek:', err);
  }
}

/**
 * Set volume on cast device
 */
export async function castSetVolume(volume: number): Promise<void> {
  if (!state.isConnected || !state.protocol) return;

  const normalizedVolume = Math.max(0, Math.min(1, volume / 100));

  try {
    switch (state.protocol) {
      case 'chromecast':
        await invoke('cast_set_volume', { volume: normalizedVolume });
        break;
      case 'dlna':
        await invoke('dlna_set_volume', { volume: normalizedVolume });
        break;
      case 'airplay':
        await invoke('airplay_set_volume', { volume: normalizedVolume });
        break;
    }
  } catch (err) {
    console.error('[CastStore] Failed to set volume:', err);
  }
}

/**
 * Get current cast position (for seekbar display)
 */
export function getCastPosition(): { positionSecs: number; durationSecs: number } {
  return {
    positionSecs: state.positionSecs,
    durationSecs: state.durationSecs
  };
}

/**
 * Set callback for when cast track ends (for auto-advance)
 */
export function setOnCastTrackEnded(callback: (() => Promise<void>) | null): void {
  onCastTrackEnded = callback;
}

/**
 * Start polling for cast position updates (DLNA and Chromecast)
 */
export function startPositionPolling(): void {
  if (positionPollInterval) return;
  if (state.protocol !== 'dlna' && state.protocol !== 'chromecast') return;

  console.log(`[CastStore] Starting ${state.protocol?.toUpperCase()} position polling`);
  trackEndDetected = false;
  lastTransportState = 'PLAYING';
  
  positionPollInterval = setInterval(async () => {
    if (!state.isConnected || (state.protocol !== 'dlna' && state.protocol !== 'chromecast')) {
      stopPositionPolling();
      return;
    }

    try {
      let positionSecs = 0;
      let durationSecs = 0;
      let transportState = 'PLAYING';
      let idleReason: string | null = null;

      if (state.protocol === 'dlna') {
        const positionInfo = await invoke<{
          position_secs: number;
          duration_secs: number;
          transport_state: string;
        }>('dlna_get_position');
        
        positionSecs = positionInfo.position_secs;
        durationSecs = positionInfo.duration_secs;
        transportState = positionInfo.transport_state;
      } else if (state.protocol === 'chromecast') {
        const positionInfo = await invoke<{
          position_secs: number;
          duration_secs: number;
          player_state: string;
          idle_reason: string | null;
        }>('cast_get_position');
        
        positionSecs = positionInfo.position_secs;
        durationSecs = positionInfo.duration_secs;
        transportState = positionInfo.player_state;
        idleReason = positionInfo.idle_reason;
      }

      const isNowPlaying = transportState === 'PLAYING';

      state = {
        ...state,
        positionSecs,
        durationSecs,
        isPlaying: isNowPlaying
      };
      
      notifyListeners();

      // Detect track ended based on protocol
      let trackEnded = false;
      
      if (state.protocol === 'dlna') {
        // DLNA: was PLAYING, now STOPPED or NO_MEDIA_PRESENT
        trackEnded = lastTransportState === 'PLAYING' && 
          (transportState === 'STOPPED' || transportState === 'NO_MEDIA_PRESENT');
      } else if (state.protocol === 'chromecast') {
        // Chromecast: IDLE state with FINISHED reason
        trackEnded = transportState === 'IDLE' && idleReason === 'FINISHED';
      }
      
      if (trackEnded && !trackEndDetected) {
        console.log(`[CastStore] ${state.protocol?.toUpperCase()} track ended, state:`, transportState, 'idle_reason:', idleReason);
        trackEndDetected = true;
        
        if (onCastTrackEnded) {
          try {
            await onCastTrackEnded();
          } catch (err) {
            console.error('[CastStore] Failed to auto-advance:', err);
          }
        }
      }
      
      // Reset track end detection when a new track starts playing
      if (transportState === 'PLAYING' && trackEndDetected) {
        trackEndDetected = false;
      }
      
      lastTransportState = transportState;
    } catch (err) {
      // Silently ignore polling errors (device may be temporarily unavailable)
    }
  }, POSITION_POLL_INTERVAL_MS);
}

/**
 * Stop polling for position updates
 */
export function stopPositionPolling(): void {
  if (positionPollInterval) {
    console.log('[CastStore] Stopping position polling');
    clearInterval(positionPollInterval);
    positionPollInterval = null;
  }
}
