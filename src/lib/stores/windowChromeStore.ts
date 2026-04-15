/**
 * Window Chrome Store
 *
 * Manages the "match system window chrome" setting. When enabled (and the
 * custom title bar is active — i.e. `useSystemTitleBar=false` AND
 * `hideTitleBar=false`), QBZ applies a rounded border that approximates
 * the active desktop's decoration radius. This makes the window feel
 * integrated with Plasma/GNOME instead of a bare rectangle.
 *
 * Phase 1: border-radius + subtle edge outline. Shadow is deferred to a
 * future phase because it requires `transparent: true` on the Tauri
 * window, which has compositor stability trade-offs.
 */

const STORAGE_KEY = 'qbz-match-system-window-chrome';
const DEFAULT_RADIUS_PX = 10;

let matchSystemWindowChrome = false;
let cornerRadiusPx = DEFAULT_RADIUS_PX;

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

export function initWindowChromeStore(): void {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved !== null) matchSystemWindowChrome = saved === 'true';
  } catch {}
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

export function getMatchSystemWindowChrome(): boolean {
  return matchSystemWindowChrome;
}

export function setMatchSystemWindowChrome(value: boolean): void {
  matchSystemWindowChrome = value;
  try {
    localStorage.setItem(STORAGE_KEY, String(value));
  } catch {}
  // Persist on the Rust side too so `should_use_main_window_transparency`
  // can read it BEFORE window creation on next launch. Failure here is
  // non-fatal — localStorage covers the in-session state.
  void (async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('v2_set_match_system_window_chrome', { value });
    } catch (e) {
      console.warn('[windowChrome] Failed to persist to Rust:', e);
    }
  })();
  notify();
}

/**
 * Hint from the Rust side: whether the Tauri window was actually built
 * transparent for this session. The frontend uses this to decide whether
 * to apply the border-radius (otherwise the webview paints the area
 * outside the radius white). Populated by the root layout at startup.
 */
let windowIsTransparent = false;

export function setWindowIsTransparent(value: boolean): void {
  if (windowIsTransparent === value) return;
  windowIsTransparent = value;
  notify();
}

export function getWindowIsTransparent(): boolean {
  return windowIsTransparent;
}

export function getCornerRadiusPx(): number {
  return cornerRadiusPx;
}

/**
 * Override the computed radius. Called after reading the detected desktop
 * theme (see `desktop_theme.rs`). When `value` is null we fall back to the
 * sensible default.
 */
export function setCornerRadiusPx(value: number | null | undefined): void {
  const next = typeof value === 'number' && value > 0 && value <= 32 ? value : DEFAULT_RADIUS_PX;
  if (next === cornerRadiusPx) return;
  cornerRadiusPx = next;
  notify();
}
