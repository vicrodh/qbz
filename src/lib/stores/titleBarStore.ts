/**
 * Title Bar Settings Store
 *
 * Manages title bar visibility settings with localStorage persistence.
 *
 * Settings:
 * - hideTitleBar: Remove title bar completely for tiling WM users (default: false)
 */

const STORAGE_KEY_HIDE = 'qbz-hide-titlebar';

// State
let hideTitleBar = false;

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
 * Get current setting
 */
export function getHideTitleBar(): boolean {
  return hideTitleBar;
}

/**
 * Determine if the title bar should be visible
 */
export function shouldShowTitleBar(): boolean {
  return !hideTitleBar;
}

/**
 * Get the title bar height for layout calculations
 * Returns 0 if title bar is hidden, 32 otherwise
 */
export function getTitleBarHeight(): number {
  return hideTitleBar ? 0 : 32;
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

export interface TitleBarState {
  hideTitleBar: boolean;
  showTitleBar: boolean;
  titleBarHeight: number;
}

export function getTitleBarState(): TitleBarState {
  return {
    hideTitleBar,
    showTitleBar: shouldShowTitleBar(),
    titleBarHeight: getTitleBarHeight()
  };
}
