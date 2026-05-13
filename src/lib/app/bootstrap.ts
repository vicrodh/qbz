/**
 * App Bootstrap
 *
 * Handles application startup tasks that don't depend on component state.
 * This includes theme initialization, Last.fm session restore, etc.
 */

import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { goBack, goForward } from '$lib/stores/navigationStore';
import { loadToastsPreference } from '$lib/stores/toastStore';
import { loadSystemNotificationsPreference, flushScrobbleQueue } from '$lib/services/playbackService';
import { initOfflineStore, cleanupOfflineStore, onOnlineTransition, syncPendingPlaylists } from '$lib/stores/offlineStore';
import { loadUnavailableTracks } from '$lib/stores/unavailableTracksStore';
import { getNextZoomLevel } from '$lib/utils/zoom';
import { getUserItem } from '$lib/utils/userStorage';
import { getZoom, setZoom } from '$lib/stores/zoomStore';
import { restoreAutoThemeVars } from '$lib/stores/autoThemeStore';
import { bootstrapGraphicsState, isHardwareAccelEnabled } from '$lib/runtime/graphicsState';
import { applyCpuModeDefaultsIfNeeded } from '$lib/stores/immersivePanelsStore';

// ============ Theme Management ============

/**
 * Load and apply saved theme from localStorage
 */
export function loadSavedTheme(): void {
  const savedTheme = localStorage.getItem('qbz-theme');
  if (savedTheme) {
    document.documentElement.setAttribute('data-theme', savedTheme);
  }
}

/**
 * Load and apply saved font family from localStorage
 */
export function loadSavedFont(): void {
  const savedFont = localStorage.getItem('qbz-font-family');
  if (savedFont) {
    document.documentElement.setAttribute('data-font', savedFont);
  }
}

/**
 * Apply saved UI zoom level (Tauri webview zoom)
 */
export async function applySavedZoom(): Promise<void> {
  const savedZoom = localStorage.getItem('qbz-zoom-level');
  if (!savedZoom) return;
  const zoom = Number.parseFloat(savedZoom);
  if (!Number.isFinite(zoom) || zoom <= 0) return;

  try {
    const clamped = setZoom(zoom);
    await getCurrentWebview().setZoom(clamped);
  } catch (err) {
    console.warn('Failed to apply saved zoom:', err);
  }
}

async function applyZoomLevel(zoom: number): Promise<void> {
  const clamped = setZoom(zoom);
  try {
    await getCurrentWebview().setZoom(clamped);
  } catch (err) {
    console.warn('Failed to set zoom level:', err);
  }
}

function handleZoomWheel(event: WheelEvent): void {
  if (!event.ctrlKey && !event.metaKey) return;
  if (event.deltaY === 0) return;

  event.preventDefault();
  const direction = event.deltaY < 0 ? 'in' : 'out';
  const nextZoom = getNextZoomLevel(getZoom(), direction);
  void applyZoomLevel(nextZoom);
}

/**
 * Zoom-on-Ctrl-wheel handling.
 *
 * Previously this registered a non-passive, capture-phase `wheel` listener
 * on `window` for the lifetime of the app. The non-passive flag (required
 * so `preventDefault` actually stops the WebView's native Ctrl+wheel zoom)
 * also tells the browser "wait for my handler before applying scroll" —
 * which kills the native fast-path for *every* wheel event in the app,
 * not just zoom-modified ones. The user perceived this as choppy mouse-wheel
 * scrolling vs perfectly-smooth scrollbar-drag scrolling.
 *
 * New approach: the wheel listener only exists while Ctrl/Meta is held.
 * Keydown registers it, keyup (or window blur) unregisters it. Net:
 *   - 99% of normal scrolling: no wheel listener at all, browser fast-path
 *   - During zoom (Ctrl held): non-passive listener active, preventDefault
 *     still blocks native zoom and our `applyZoomLevel` runs
 */
const ZOOM_WHEEL_OPTIONS: AddEventListenerOptions = { passive: false, capture: true };
let zoomWheelActive = false;

function registerZoomWheel(): void {
  if (zoomWheelActive) return;
  zoomWheelActive = true;
  window.addEventListener('wheel', handleZoomWheel, ZOOM_WHEEL_OPTIONS);
}

function unregisterZoomWheel(): void {
  if (!zoomWheelActive) return;
  zoomWheelActive = false;
  window.removeEventListener('wheel', handleZoomWheel, ZOOM_WHEEL_OPTIONS);
}

function handleZoomKeyDown(e: KeyboardEvent): void {
  if (e.ctrlKey || e.metaKey) registerZoomWheel();
}

function handleZoomKeyUp(e: KeyboardEvent): void {
  // After any keyup, if neither modifier is held anymore, drop the listener.
  if (!e.ctrlKey && !e.metaKey) unregisterZoomWheel();
}

export function setupZoomControls(): () => void {
  window.addEventListener('keydown', handleZoomKeyDown);
  window.addEventListener('keyup', handleZoomKeyUp);
  // Releases the listener if the user alt-tabs with Ctrl held — otherwise
  // the keyup never fires and we stay in the slow path.
  window.addEventListener('blur', unregisterZoomWheel);
  return () => {
    window.removeEventListener('keydown', handleZoomKeyDown);
    window.removeEventListener('keyup', handleZoomKeyUp);
    window.removeEventListener('blur', unregisterZoomWheel);
    unregisterZoomWheel();
  };
}

// ============ Last.fm Session ============

/**
 * Restore Last.fm session from localStorage
 */
export async function restoreLastfmSession(): Promise<void> {
  try {
    const savedSessionKey = getUserItem('qbz-lastfm-session-key');

    // Restore session if available (proxy handles credentials)
    if (savedSessionKey) {
      await invoke('v2_lastfm_set_session', { sessionKey: savedSessionKey });
      console.log('Last.fm session restored on startup');
    }
  } catch (err) {
    console.error('Failed to restore Last.fm session:', err);
  }
}

// ============ Mouse Navigation ============

/**
 * Handle mouse back/forward buttons
 */
function handleMouseNavigation(event: MouseEvent): void {
  if (event.button === 3) {
    event.preventDefault();
    goBack();
  } else if (event.button === 4) {
    event.preventDefault();
    goForward();
  }
}

/**
 * Setup mouse navigation event listener
 * @returns Cleanup function to remove listener
 */
export function setupMouseNavigation(): () => void {
  window.addEventListener('mouseup', handleMouseNavigation);
  return () => window.removeEventListener('mouseup', handleMouseNavigation);
}

// ============ Combined Bootstrap ============

export interface BootstrapResult {
  cleanup: () => void;
}

/**
 * Bootstrap the application
 * Call this in onMount to initialize app-level features
 * @returns Object with cleanup function for onDestroy
 */
export function bootstrapApp(): BootstrapResult {
  // Load theme and font (auto-theme vars first to prevent FOUC)
  restoreAutoThemeVars();
  loadSavedTheme();
  loadSavedFont();
  void applySavedZoom();

  // Load notification preferences
  loadToastsPreference();
  loadSystemNotificationsPreference();

  // Prime the runtime graphics-state cache so hot paths like `cachedSrc`
  // can read it sync. Fire-and-forget; falls back to "HW accel ON"
  // defaults if it errors out (see ADR-004). Once the cache is loaded,
  // re-apply CPU-mode defaults to the immersive panels store — the
  // store's module-load init may have run before this resolved and
  // defaulted to GPU mode, which would leave heavy panels enabled by
  // mistake on first launch for CPU-bound users.
  void bootstrapGraphicsState().then(() => {
    applyCpuModeDefaultsIfNeeded(!isHardwareAccelEnabled());
  });

  // Setup mouse navigation
  const cleanupMouse = setupMouseNavigation();
  const cleanupZoom = setupZoomControls();

  // NOTE: restoreLastfmSession() is NOT called here.
  // It's called from handleLoginSuccess in +page.svelte after session activation.

  // Initialize offline store (async, fire-and-forget)
  initOfflineStore();

  // Load unavailable tracks from localStorage
  loadUnavailableTracks();

  // Register callback to flush scrobble queue and sync playlists when transitioning to online
  onOnlineTransition(() => {
    console.log('[Bootstrap] Online transition detected - flushing scrobble queue and syncing playlists');

    // Flush scrobble queue
    flushScrobbleQueue().then(({ sent, failed }) => {
      if (sent > 0 || failed > 0) {
        console.log(`[Bootstrap] Scrobble queue flush complete: ${sent} sent, ${failed} failed`);
      }
    });

    // Sync pending playlists (with detailed logging enabled)
    syncPendingPlaylists().catch(err => {
      console.error('[Bootstrap] Failed to sync pending playlists:', err);
    });
  });

  return {
    cleanup: () => {
      cleanupMouse();
      cleanupZoom();
      cleanupOfflineStore();
    }
  };
}
