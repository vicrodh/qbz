/**
 * Title Bar Settings Store
 *
 * Manages title bar visibility settings with localStorage persistence.
 *
 * Settings:
 * - hideTitleBar: Remove title bar completely for tiling WM users (default: false)
 * - useSystemTitleBar: Use OS native window decorations instead of custom title bar (default: false)
 */

import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';

const STORAGE_KEY_HIDE = 'qbz-hide-titlebar';
const STORAGE_KEY_SYSTEM = 'qbz-use-system-titlebar';

// State
let hideTitleBar = false;
let useSystemTitleBar = false;

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Initialize the store from localStorage
 */
export function initTitleBarStore(): void {
  try {
    const savedHide = localStorage.getItem(STORAGE_KEY_HIDE);
    if (savedHide !== null) {
      hideTitleBar = savedHide === 'true';
    }

    const savedSystem = localStorage.getItem(STORAGE_KEY_SYSTEM);
    if (savedSystem !== null) {
      useSystemTitleBar = savedSystem === 'true';
    }
    // Note: decorations are set by Rust at window creation (before show),
    // so no need to call setDecorations() here on init.
  } catch (e) {
    console.error('[TitleBarStore] Failed to initialize:', e);
  }
}

/**
 * Subscribe to title bar state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

/**
 * Get current hide setting
 */
export function getHideTitleBar(): boolean {
  return hideTitleBar;
}

/**
 * Get current system title bar setting
 */
export function getUseSystemTitleBar(): boolean {
  return useSystemTitleBar;
}

/**
 * Determine if the custom title bar should be visible
 * Hidden when either system title bar is active or hide mode is on
 */
export function shouldShowTitleBar(): boolean {
  return !hideTitleBar && !useSystemTitleBar;
}

/**
 * Get the title bar height for layout calculations
 * Returns 0 if title bar is hidden or system title bar is active, 32 otherwise
 */
export function getTitleBarHeight(): number {
  return (hideTitleBar || useSystemTitleBar) ? 0 : 32;
}

/**
 * Set whether to hide title bar completely
 */
export function setHideTitleBar(value: boolean): void {
  hideTitleBar = value;
  try {
    localStorage.setItem(STORAGE_KEY_HIDE, String(value));
  } catch (e) {
    console.error('[TitleBarStore] Failed to save hide titlebar setting:', e);
  }
  notifyListeners();
}

/**
 * Set whether to use system (OS native) title bar
 * Toggles Tauri window decorations and hides the custom title bar.
 * Persists to both localStorage (for fast frontend init) and Rust backend
 * (so decorations are set before the window is shown on next launch).
 */
export function setUseSystemTitleBar(value: boolean): void {
  useSystemTitleBar = value;
  try {
    localStorage.setItem(STORAGE_KEY_SYSTEM, String(value));
  } catch (e) {
    console.error('[TitleBarStore] Failed to save system titlebar setting:', e);
  }
  // Persist to Rust backend (read at startup before window creation)
  invoke('set_use_system_titlebar', { value }).catch((e) => {
    console.error('[TitleBarStore] Failed to persist system titlebar to backend:', e);
  });
  // Apply runtime decoration change for immediate effect
  getCurrentWindow().setDecorations(value).catch((e) => {
    console.error('[TitleBarStore] Failed to set window decorations:', e);
  });
  notifyListeners();
}

export interface TitleBarState {
  hideTitleBar: boolean;
  useSystemTitleBar: boolean;
  showTitleBar: boolean;
  titleBarHeight: number;
}

export function getTitleBarState(): TitleBarState {
  return {
    hideTitleBar,
    useSystemTitleBar,
    showTitleBar: shouldShowTitleBar(),
    titleBarHeight: getTitleBarHeight()
  };
}
