/**
 * Discord Rich Presence Service
 *
 * Pushes "now listening" activity to Discord via the Tauri backend.
 * Opt-in: persisted in localStorage as `qbz-discord-rpc-enabled`, off by
 * default. When the toggle is enabled, this module subscribes to playerStore
 * and forwards (track_id, is_playing) transitions to the backend, dropping
 * intermediate currentTime ticks so Discord is not spammed.
 */

import { invoke } from '@tauri-apps/api/core';
import { getUserItem, setUserItem } from '$lib/utils/userStorage';
import {
  subscribe as subscribePlayer,
  getCurrentTrack,
  getIsPlaying,
  getCurrentTime,
  getDuration
} from '$lib/stores/playerStore';

const STORAGE_KEY = 'qbz-discord-rpc-enabled';

let unsubscribePlayer: (() => void) | null = null;
let lastTrackId: number | null = null;
let lastIsPlaying: boolean | null = null;

export function isDiscordRpcEnabled(): boolean {
  return getUserItem(STORAGE_KEY) === 'true';
}

export async function setDiscordRpcEnabled(enabled: boolean): Promise<void> {
  setUserItem(STORAGE_KEY, enabled ? 'true' : 'false');
  try {
    await invoke('v2_discord_rpc_set_enabled', { enabled });
  } catch {
    // Backend unavailable (headless / remote). Toggle still persists in localStorage.
  }
  if (enabled) {
    startSubscription();
    await pushCurrentState();
  } else {
    stopSubscription();
    try {
      await invoke('v2_discord_rpc_clear');
    } catch {
      // ignore
    }
  }
}

/**
 * Called once on app boot. Brings backend state in sync with the persisted
 * toggle and starts the playerStore subscription if enabled.
 */
export async function initDiscordRpc(): Promise<void> {
  const enabled = isDiscordRpcEnabled();
  try {
    await invoke('v2_discord_rpc_set_enabled', { enabled });
  } catch {
    // ignore
  }
  if (enabled) {
    startSubscription();
    await pushCurrentState();
  }
}

function startSubscription(): void {
  if (unsubscribePlayer) return;
  unsubscribePlayer = subscribePlayer(() => {
    const track = getCurrentTrack();
    const isPlaying = getIsPlaying();
    const trackId = track?.id ?? null;
    if (trackId === lastTrackId && isPlaying === lastIsPlaying) return;
    lastTrackId = trackId;
    lastIsPlaying = isPlaying;
    void pushCurrentState();
  });
}

function stopSubscription(): void {
  if (unsubscribePlayer) {
    unsubscribePlayer();
    unsubscribePlayer = null;
  }
  lastTrackId = null;
  lastIsPlaying = null;
}

async function pushCurrentState(): Promise<void> {
  const track = getCurrentTrack();
  if (!track) {
    try {
      await invoke('v2_discord_rpc_clear');
    } catch {
      // ignore
    }
    return;
  }
  try {
    await invoke('v2_discord_rpc_update', {
      title: track.title,
      artist: track.artist,
      album: track.album,
      isPlaying: getIsPlaying(),
      currentTime: getCurrentTime(),
      duration: getDuration(),
      coverUrl: track.artwork || null
    });
  } catch {
    // Backend unavailable; silent.
  }
}
