/**
 * App Bootstrap
 *
 * Handles application startup tasks that don't depend on component state.
 * This includes theme initialization, Last.fm session restore, etc.
 */

import { invoke } from '@tauri-apps/api/core';
import { goBack, goForward } from '$lib/stores/navigationStore';

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

  // Setup mouse navigation
  const cleanupMouse = setupMouseNavigation();

  // Restore Last.fm session (async, fire-and-forget)
  restoreLastfmSession();

  return {
    cleanup: () => {
      cleanupMouse();
    }
  };
}
