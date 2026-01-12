/**
 * MiniPlayer Mode Service
 *
 * Handles switching between normal and miniplayer modes by resizing
 * the main window instead of creating a separate window.
 */

import { getCurrentWindow } from '@tauri-apps/api/window';
import type { PhysicalSize, PhysicalPosition } from '@tauri-apps/api/window';
import { goto } from '$app/navigation';

// Miniplayer dimensions (Cider-inspired compact mode)
const MINIPLAYER_WIDTH = 400;
const MINIPLAYER_HEIGHT = 150;

// Store original window state for restoration
let originalSize: PhysicalSize | null = null;
let originalPosition: PhysicalPosition | null = null;
let originalMaximized = false;
let isMiniplayerMode = false;

// Callbacks for state management
let onModeChangeCallback: ((isMini: boolean) => void) | null = null;

/**
 * Set callback for mode changes
 */
export function onModeChange(callback: (isMini: boolean) => void): void {
  onModeChangeCallback = callback;
}

/**
 * Check if currently in miniplayer mode
 */
export function isInMiniplayerMode(): boolean {
  return isMiniplayerMode;
}

/**
 * Enter miniplayer mode - resize window and navigate to miniplayer route
 */
export async function enterMiniplayerMode(): Promise<void> {
  if (isMiniplayerMode) return;

  try {
    const window = getCurrentWindow();

    // Store current window state
    originalMaximized = await window.isMaximized();
    originalSize = await window.innerSize();
    originalPosition = await window.innerPosition();

    console.log('[MiniPlayer] Saving original state:', {
      size: originalSize,
      position: originalPosition,
      maximized: originalMaximized
    });

    // If maximized, unmaximize first
    if (originalMaximized) {
      await window.unmaximize();
    }

    // Set miniplayer dimensions
    await window.setResizable(false);
    await window.setSize({ type: 'Physical', width: MINIPLAYER_WIDTH, height: MINIPLAYER_HEIGHT });
    await window.setDecorations(false);
    await window.setAlwaysOnTop(true);

    // Navigate to miniplayer route
    await goto('/miniplayer');

    isMiniplayerMode = true;
    onModeChangeCallback?.(true);

    console.log('[MiniPlayer] Entered miniplayer mode');
  } catch (err) {
    console.error('[MiniPlayer] Failed to enter miniplayer mode:', err);
  }
}

/**
 * Exit miniplayer mode - restore original window state
 */
export async function exitMiniplayerMode(): Promise<void> {
  if (!isMiniplayerMode) return;

  try {
    const window = getCurrentWindow();

    // Restore window properties
    await window.setAlwaysOnTop(false);
    await window.setDecorations(true);
    await window.setResizable(true);

    // Restore size
    if (originalSize) {
      await window.setSize({ type: 'Physical', width: originalSize.width, height: originalSize.height });
    } else {
      // Fallback to default size
      await window.setSize({ type: 'Physical', width: 1280, height: 800 });
    }

    // Restore position
    if (originalPosition) {
      await window.setPosition({ type: 'Physical', x: originalPosition.x, y: originalPosition.y });
    }

    // Restore maximized state
    if (originalMaximized) {
      await window.maximize();
    }

    // Navigate back to main
    await goto('/');

    isMiniplayerMode = false;
    onModeChangeCallback?.(false);

    console.log('[MiniPlayer] Exited miniplayer mode');
  } catch (err) {
    console.error('[MiniPlayer] Failed to exit miniplayer mode:', err);
  }
}

/**
 * Toggle between normal and miniplayer modes
 */
export async function toggleMiniplayerMode(): Promise<void> {
  if (isMiniplayerMode) {
    await exitMiniplayerMode();
  } else {
    await enterMiniplayerMode();
  }
}

/**
 * Set miniplayer always on top
 */
export async function setMiniplayerAlwaysOnTop(alwaysOnTop: boolean): Promise<void> {
  if (!isMiniplayerMode) return;

  try {
    const window = getCurrentWindow();
    await window.setAlwaysOnTop(alwaysOnTop);
  } catch (err) {
    console.error('[MiniPlayer] Failed to set always on top:', err);
  }
}
