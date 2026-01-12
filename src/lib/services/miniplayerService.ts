/**
 * MiniPlayer Mode Service
 *
 * Handles switching between normal and miniplayer modes by resizing
 * the main window instead of creating a separate window.
 *
 * Uses localStorage to persist original window state across navigation.
 */

import { getCurrentWindow } from '@tauri-apps/api/window';
import { goto } from '$app/navigation';

// Miniplayer dimensions (with 8px margin on each side)
const MINIPLAYER_WIDTH = 300;
const MINIPLAYER_HEIGHT = 140;

// LocalStorage key for persisting window state
const STORAGE_KEY = 'miniplayer_original_state';

interface OriginalState {
  width: number;
  height: number;
  x: number;
  y: number;
  maximized: boolean;
}

function saveOriginalState(state: OriginalState): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    console.log('[MiniPlayer] Saved state to localStorage:', state);
  } catch (e) {
    console.error('[MiniPlayer] Failed to save state:', e);
  }
}

function loadOriginalState(): OriginalState | null {
  try {
    const data = localStorage.getItem(STORAGE_KEY);
    if (data) {
      const state = JSON.parse(data) as OriginalState;
      console.log('[MiniPlayer] Loaded state from localStorage:', state);
      return state;
    }
  } catch (e) {
    console.error('[MiniPlayer] Failed to load state:', e);
  }
  return null;
}

function clearOriginalState(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
    console.log('[MiniPlayer] Cleared state from localStorage');
  } catch (e) {
    console.error('[MiniPlayer] Failed to clear state:', e);
  }
}

/**
 * Enter miniplayer mode - resize window and navigate to miniplayer route
 */
export async function enterMiniplayerMode(): Promise<void> {
  console.log('[MiniPlayer] Entering miniplayer mode...');

  try {
    const window = getCurrentWindow();

    // Store current window state
    const maximized = await window.isMaximized();
    const size = await window.innerSize();
    const position = await window.innerPosition();

    // Save to localStorage so it survives navigation
    saveOriginalState({
      width: size.width,
      height: size.height,
      x: position.x,
      y: position.y,
      maximized
    });

    // If maximized, unmaximize first
    if (maximized) {
      await window.unmaximize();
    }

    // Set miniplayer dimensions
    await window.setResizable(false);
    await window.setSize({ type: 'Physical', width: MINIPLAYER_WIDTH, height: MINIPLAYER_HEIGHT });
    await window.setDecorations(false);
    await window.setAlwaysOnTop(true);

    // Navigate to miniplayer route
    await goto('/miniplayer');

    console.log('[MiniPlayer] Entered miniplayer mode');
  } catch (err) {
    console.error('[MiniPlayer] Failed to enter miniplayer mode:', err);
  }
}

/**
 * Exit miniplayer mode - restore original window state
 */
export async function exitMiniplayerMode(): Promise<void> {
  console.log('[MiniPlayer] Exiting miniplayer mode...');

  // Load original state from localStorage
  const originalState = loadOriginalState();
  console.log('[MiniPlayer] Original state:', originalState);

  try {
    const window = getCurrentWindow();

    // Restore window properties first
    await window.setAlwaysOnTop(false);
    await window.setDecorations(true);
    await window.setResizable(true);

    // Restore size
    if (originalState) {
      console.log('[MiniPlayer] Restoring size:', originalState.width, 'x', originalState.height);
      await window.setSize({ type: 'Physical', width: originalState.width, height: originalState.height });

      console.log('[MiniPlayer] Restoring position:', originalState.x, ',', originalState.y);
      await window.setPosition({ type: 'Physical', x: originalState.x, y: originalState.y });

      if (originalState.maximized) {
        console.log('[MiniPlayer] Restoring maximized state');
        await window.maximize();
      }
    } else {
      // Fallback to default size
      console.log('[MiniPlayer] No original state, using defaults');
      await window.setSize({ type: 'Physical', width: 1280, height: 800 });
    }

    // Clear saved state
    clearOriginalState();

    // Navigate back to main
    await goto('/');

    console.log('[MiniPlayer] Exited miniplayer mode');
  } catch (err) {
    console.error('[MiniPlayer] Failed to exit miniplayer mode:', err);
  }
}

/**
 * Set miniplayer always on top
 */
export async function setMiniplayerAlwaysOnTop(alwaysOnTop: boolean): Promise<void> {
  try {
    const window = getCurrentWindow();
    await window.setAlwaysOnTop(alwaysOnTop);
  } catch (err) {
    console.error('[MiniPlayer] Failed to set always on top:', err);
  }
}
