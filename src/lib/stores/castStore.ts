/**
 * Cast Store
 *
 * Manages casting state across the app.
 * Tracks connected device and protocol.
 */

import { invoke } from '@tauri-apps/api/core';

export type CastProtocol = 'chromecast' | 'dlna' | 'airplay';

export interface CastDevice {
  id: string;
  name: string;
  ip: string;
  port: number;
}

interface CastState {
  isConnected: boolean;
  protocol: CastProtocol | null;
  device: CastDevice | null;
  isPlaying: boolean;
  currentTrackId: number | null;
}

let state: CastState = {
  isConnected: false,
  protocol: null,
  device: null,
  isPlaying: false,
  currentTrackId: null
};

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
 */
export async function connectToDevice(device: CastDevice, protocol: CastProtocol): Promise<void> {
  try {
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
  } catch (err) {
    console.error('[CastStore] Failed to connect:', err);
    throw err;
  }
}

/**
 * Disconnect from current device
 */
export async function disconnect(): Promise<void> {
  if (!state.isConnected || !state.protocol) return;

  try {
    await castStop();
    switch (state.protocol) {
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
    currentTrackId: null
  };
  notifyListeners();
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
      currentTrackId: trackId
    };
    notifyListeners();
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
