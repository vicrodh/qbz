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
import { initOfflineStore, cleanupOfflineStore, onOnlineTransition } from '$lib/stores/offlineStore';

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
 * Apply saved UI zoom level (Tauri webview zoom)
 */
export async function applySavedZoom(): Promise<void> {
  const savedZoom = localStorage.getItem('qbz-zoom-level');
  if (!savedZoom) return;
  const zoom = Number.parseFloat(savedZoom);
  if (!Number.isFinite(zoom) || zoom <= 0) return;

  try {
    await getCurrentWebview().setZoom(zoom);
  } catch (err) {
    console.warn('Failed to apply saved zoom:', err);
  }
}

// ============ Last.fm Session ============

/**
 * Restore Last.fm session from localStorage
 */
export async function restoreLastfmSession(): Promise<void> {
  try {
    const savedApiKey = localStorage.getItem('qbz-lastfm-api-key');
    const savedApiSecret = localStorage.getItem('qbz-lastfm-api-secret');
    const savedSessionKey = localStorage.getItem('qbz-lastfm-session-key');

    // Restore credentials if user-provided
    if (savedApiKey && savedApiSecret) {
      await invoke('lastfm_set_credentials', {
        apiKey: savedApiKey,
        apiSecret: savedApiSecret
      });
    }

    // Restore session if available
    if (savedSessionKey) {
      await invoke('lastfm_set_session', { sessionKey: savedSessionKey });
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
  // Load theme
  loadSavedTheme();
  void applySavedZoom();

  // Load notification preferences
  loadToastsPreference();
  loadSystemNotificationsPreference();

  // Setup mouse navigation
  const cleanupMouse = setupMouseNavigation();

  // Restore Last.fm session (async, fire-and-forget)
  restoreLastfmSession();

  // Initialize offline store (async, fire-and-forget)
  initOfflineStore();

  // Register callback to flush scrobble queue when transitioning to online
  onOnlineTransition(() => {
    console.log('[Bootstrap] Online transition detected - flushing scrobble queue');
    flushScrobbleQueue().then(({ sent, failed }) => {
      if (sent > 0 || failed > 0) {
        console.log(`[Bootstrap] Scrobble queue flush complete: ${sent} sent, ${failed} failed`);
      }
    });
  });

  return {
    cleanup: () => {
      cleanupMouse();
      cleanupOfflineStore();
    }
  };
}
