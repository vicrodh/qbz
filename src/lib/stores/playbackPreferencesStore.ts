/**
 * Playback Preferences Store
 *
 * Manages global playback behavior preferences (autoplay mode, etc.)
 */

import { invoke } from '@tauri-apps/api/core';
import { skipIfRemote } from '$lib/services/commandRouter';

// ============ Types ============

export type AutoplayMode = 'continue' | 'track_only' | 'infinite';

export interface PlaybackPreferences {
  autoplay_mode: AutoplayMode;
  show_context_icon: boolean;
  persist_session: boolean;
  /** Sub-preference of persist_session: when true, restore also seeks
   *  to the saved track position. When false (default), the restored
   *  track is shown paused at 0:00 (#360 / issue 317 — the common
   *  "fresh start each day" behavior). */
  resume_playback_position: boolean;
}

// ============ State ============

let preferences: PlaybackPreferences = {
  autoplay_mode: 'continue',
  show_context_icon: true,
  persist_session: false,
  resume_playback_position: false
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
  if (skipIfRemote()) return preferences;
  const prefs = await invoke<PlaybackPreferences>('v2_get_playback_preferences');
  preferences = prefs;
  notifyListeners();
  return prefs;
}

/**
 * Set autoplay mode
 */
export async function setAutoplayMode(mode: AutoplayMode): Promise<void> {
  if (skipIfRemote()) return;
  await invoke('v2_set_autoplay_mode', { mode });
  preferences.autoplay_mode = mode;
  notifyListeners();

  // Sync autoplay mode to QConnect server if controlling a remote renderer
  try {
    await invoke('v2_qconnect_set_autoplay_mode_if_remote', {
      enabled: mode === 'continue'
    });
  } catch {
    // QConnect sync is best-effort — don't fail the preference save
  }
}

/**
 * Set whether to show context icon in player
 */
export async function setShowContextIcon(show: boolean): Promise<void> {
  if (skipIfRemote()) return;
  await invoke('v2_set_show_context_icon', { show });
  preferences.show_context_icon = show;
  notifyListeners();
}

/**
 * Set whether to persist session on close/restart
 */
export async function setPersistSession(persist: boolean): Promise<void> {
  if (skipIfRemote()) return;
  await invoke('v2_set_persist_session', { persist });
  preferences.persist_session = persist;
  notifyListeners();
}

/**
 * Set whether to resume the saved playback position (seek to where the
 * user left off) when restoring a session. Sub-preference of
 * `persist_session` — has no effect when persist is off. Default: false.
 */
export async function setResumePlaybackPosition(resume: boolean): Promise<void> {
  if (skipIfRemote()) return;
  await invoke('v2_set_resume_playback_position', { resume });
  preferences.resume_playback_position = resume;
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
 * Check if infinite play (radio) is enabled
 */
export function isInfinitePlayEnabled(): boolean {
  return preferences.autoplay_mode === 'infinite';
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
  if (skipIfRemote()) return;
  await getPlaybackPreferences();
}
