/**
 * Playback Target Store
 *
 * Tracks whether the app is controlling local playback or a remote qbzd daemon.
 * When targeting a remote daemon, playback/queue/search commands route to
 * the HTTP API instead of Tauri invoke().
 */
import { writable, get, derived } from 'svelte/store';

export interface PlaybackTarget {
  type: 'local' | 'qbzd';
  /** HTTP base URL of the daemon (e.g., "http://192.168.1.50:8182") */
  baseUrl?: string;
  /** API token for authentication */
  token?: string;
  /** Human-readable name (e.g., "raspberrypi", "living-room") */
  name?: string;
}

/** Current playback target — local by default */
export const playbackTarget = writable<PlaybackTarget>({ type: 'local' });

/** Whether we're currently controlling a remote daemon */
export const isRemoteMode = derived(playbackTarget, ($t) => $t.type === 'qbzd');

/** Connect to a remote qbzd daemon */
export function connectToRemote(baseUrl: string, token: string, name?: string) {
  // Strip trailing slashes without a regex (regex `\/+$` was flagged as a
  // potential ReDoS risk; the linear loop below is trivially bounded).
  let normalizedUrl = baseUrl;
  while (normalizedUrl.endsWith('/')) {
    normalizedUrl = normalizedUrl.slice(0, -1);
  }
  playbackTarget.set({
    type: 'qbzd',
    baseUrl: normalizedUrl,
    token,
    name: name || new URL(baseUrl).hostname,
  });
  console.log(`[PlaybackTarget] Connected to remote: ${name || baseUrl}`);
}

/** Disconnect from remote, return to local playback */
export function disconnectFromRemote() {
  playbackTarget.set({ type: 'local' });
  console.log('[PlaybackTarget] Switched to local playback');
}

/** Get the current target (non-reactive, for use in async functions) */
export function getTarget(): PlaybackTarget {
  return get(playbackTarget);
}
