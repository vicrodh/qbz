/**
 * Playback Preferences Store
 *
 * Manages global playback behavior preferences (autoplay mode, etc.)
 */

import { invoke } from '@tauri-apps/api/core';

// ============ Types ============

export type AutoplayMode = 'continue' | 'track_only';

export interface PlaybackPreferences {
  autoplay_mode: AutoplayMode;
  show_context_icon: boolean;
}

// ============ State ============

let preferences: PlaybackPreferences = {
  autoplay_mode: 'continue',
  show_context_icon: true
};

const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

// ============ Public API ============

/**
 * Get current playback preferences
 */
export async function getPlaybackPreferences(): Promise<PlaybackPreferences> {
  const prefs = await invoke<PlaybackPreferences>('get_playback_preferences');
  preferences = prefs;
  notifyListeners();
  return prefs;
}

/**
 * Set autoplay mode
 */
export async function setAutoplayMode(mode: AutoplayMode): Promise<void> {
  await invoke('set_autoplay_mode', { mode });
  preferences.autoplay_mode = mode;
  notifyListeners();
}

/**
 * Set whether to show context icon in player
 */
export async function setShowContextIcon(show: boolean): Promise<void> {
  await invoke('set_show_context_icon', { show });
  preferences.show_context_icon = show;
  notifyListeners();
}

/**
 * Get cached preferences (no backend call)
 */
export function getCachedPreferences(): PlaybackPreferences {
  return preferences;
}

/**
 * Check if autoplay is enabled (continue within source)
 */
export function isAutoplayEnabled(): boolean {
  return preferences.autoplay_mode === 'continue';
}

/**
 * Subscribe to preference changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Initialize preferences store (call on app startup)
 */
export async function initPlaybackPreferences(): Promise<void> {
  await getPlaybackPreferences();
}
