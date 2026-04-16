/**
 * Window Title Store
 *
 * Manages the opt-in "show track in OS window title" preference.
 *
 * Settings:
 * - enabled: toggle that controls whether the OS window title should reflect
 *   the currently playing track (default: false)
 * - template: format string with {artist}, {track}, {album} placeholders
 *   (default: "{artist} — {track}")
 *
 * Persisted to localStorage — pure UX preference, no backend state.
 */

const STORAGE_KEY_ENABLED = 'qbz-window-title-enabled';
const STORAGE_KEY_TEMPLATE = 'qbz-window-title-template';

export const DEFAULT_WINDOW_TITLE_TEMPLATE = '{artist} — {track}';

// State
let enabled = false;
let template = DEFAULT_WINDOW_TITLE_TEMPLATE;

// Listeners for cross-component reactivity
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Initialize the store from localStorage.
 * Safe to call multiple times; subsequent calls are no-ops beyond re-reading
 * the latest values.
 */
export function initWindowTitleStore(): void {
  try {
    const savedEnabled = localStorage.getItem(STORAGE_KEY_ENABLED);
    if (savedEnabled !== null) {
      enabled = savedEnabled === 'true';
    }
    const savedTemplate = localStorage.getItem(STORAGE_KEY_TEMPLATE);
    if (savedTemplate !== null && savedTemplate.length > 0) {
      template = savedTemplate;
    }
  } catch (e) {
    console.error('[WindowTitleStore] Failed to initialize:', e);
  }
}

/**
 * Subscribe to window-title preference changes.
 * Listener is invoked immediately with current state, and on every change.
 * Returns an unsubscribe function.
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => {
    listeners.delete(listener);
  };
}

export function getWindowTitleEnabled(): boolean {
  return enabled;
}

export function getWindowTitleTemplate(): string {
  return template;
}

export function setWindowTitleEnabled(value: boolean): void {
  enabled = value;
  try {
    localStorage.setItem(STORAGE_KEY_ENABLED, String(value));
  } catch (e) {
    console.error('[WindowTitleStore] Failed to save enabled setting:', e);
  }
  notifyListeners();
}

export function setWindowTitleTemplate(value: string): void {
  template = value;
  try {
    localStorage.setItem(STORAGE_KEY_TEMPLATE, value);
  } catch (e) {
    console.error('[WindowTitleStore] Failed to save template setting:', e);
  }
  notifyListeners();
}

/**
 * Apply the template substitutions with the given track metadata.
 * Returns the rendered title, or an empty string if the template is empty
 * after trimming (caller should fall back to the app name in that case).
 */
export function renderWindowTitle(
  tpl: string,
  track: { artist?: string; title?: string; album?: string } | null
): string {
  if (!track) return '';
  const artist = track.artist ?? '';
  const title = track.title ?? '';
  const album = track.album ?? '';
  const rendered = tpl
    .replaceAll('{artist}', artist)
    .replaceAll('{track}', title)
    .replaceAll('{album}', album);
  return rendered.trim();
}
