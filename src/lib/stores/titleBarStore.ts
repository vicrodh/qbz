/**
 * Title Bar Settings Store
 *
 * Manages title bar visibility settings with localStorage persistence.
 *
 * Settings:
 * - hideTitleBar: Remove title bar completely for tiling WM users (default: false)
 * - useSystemTitleBar: Use OS native window decorations instead of custom title bar (default: false)
 */

import { invoke } from '@tauri-apps/api/core';
import { skipIfRemote } from '$lib/services/commandRouter';
import { platform } from '$lib/utils/platform';

const STORAGE_KEY_HIDE = 'qbz-hide-titlebar';
const STORAGE_KEY_SYSTEM = 'qbz-use-system-titlebar';
const STORAGE_KEY_WINDOW_CONTROLS = 'qbz-show-window-controls';

// State
let hideTitleBar = false;
let useSystemTitleBar = false;
let showWindowControls = true;

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
  if (skipIfRemote()) return;
  try {
    const savedHide = localStorage.getItem(STORAGE_KEY_HIDE);
    if (savedHide !== null) {
      hideTitleBar = savedHide === 'true';
    }

    const savedSystem = localStorage.getItem(STORAGE_KEY_SYSTEM);
    if (savedSystem !== null) {
      useSystemTitleBar = savedSystem === 'true';
    }

    const savedControls = localStorage.getItem(STORAGE_KEY_WINDOW_CONTROLS);
    if (savedControls !== null) {
      showWindowControls = savedControls !== 'false';
    }
    // Sync localStorage value to Rust backend so it's available at next
    // startup (before window creation). Handles migration from the
    // localStorage-only era and keeps both stores in sync.
    // Always sync, not just when true - otherwise false values never propagate.
    invoke('v2_set_use_system_titlebar', { value: useSystemTitleBar }).catch((e) => {
      console.error('[TitleBarStore] Failed to sync system titlebar to backend:', e);
    });
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
  // macOS always uses native decorations — no custom title bar
  if (platform === 'macos') return false;
  return !hideTitleBar && !useSystemTitleBar;
}

/**
 * Get the title bar height for layout calculations
 * Returns 0 if title bar is hidden or system title bar is active, 40 otherwise
 */
export function getTitleBarHeight(): number {
  if (platform === 'macos') return 0;
  return (hideTitleBar || useSystemTitleBar) ? 0 : 40;
}

/**
 * Get whether to show window control buttons (minimize/maximize/close)
 */
export function getShowWindowControls(): boolean {
  return showWindowControls;
}

/**
 * Set whether to show window control buttons
 */
export function setShowWindowControls(value: boolean): void {
  showWindowControls = value;
  try {
    localStorage.setItem(STORAGE_KEY_WINDOW_CONTROLS, String(value));
  } catch (e) {
    console.error('[TitleBarStore] Failed to save window controls setting:', e);
  }
  notifyListeners();
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
 * Set whether to use system (OS native) title bar.
 * Persists to both localStorage and Rust backend, then restarts the app.
 * Restart is required because the window must be created with the correct
 * decoration state — runtime toggling doesn't work reliably on Linux/Wayland.
 */
export async function setUseSystemTitleBar(value: boolean): Promise<void> {
  if (skipIfRemote()) return;
  useSystemTitleBar = value;
  try {
    localStorage.setItem(STORAGE_KEY_SYSTEM, String(value));
  } catch (e) {
    console.error('[TitleBarStore] Failed to save system titlebar setting:', e);
  }
  try {
    await invoke('v2_set_use_system_titlebar', { value });
    await invoke('v2_restart_app');
  } catch (e) {
    console.error('[TitleBarStore] Failed to apply system titlebar change:', e);
  }
}

export interface TitleBarState {
  hideTitleBar: boolean;
  useSystemTitleBar: boolean;
  showTitleBar: boolean;
  showWindowControls: boolean;
  titleBarHeight: number;
}

export function getTitleBarState(): TitleBarState {
  return {
    hideTitleBar,
    useSystemTitleBar,
    showTitleBar: shouldShowTitleBar(),
    showWindowControls,
    titleBarHeight: getTitleBarHeight()
  };
}
