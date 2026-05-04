/**
 * Title Bar Settings Store
 *
 * Manages titlebar mode (single 4-value enum) plus the orthogonal
 * `showWindowControls` preference.
 *
 * Modes:
 * - 'qbz': default — full custom titlebar with drag/resize/dblclick/controls
 * - 'system': OS-native chrome
 * - 'plasma': KDE-only, KWin SSD via Xwayland + stripped strip
 * - 'hidden': no titlebar (tiling WMs)
 */

import { invoke } from '@tauri-apps/api/core';
import { skipIfRemote } from '$lib/services/commandRouter';
import { platform } from '$lib/utils/platform';

export type TitlebarMode = 'qbz' | 'system' | 'plasma' | 'hidden';

const STORAGE_KEY_MODE = 'qbz-titlebar-mode';
const STORAGE_KEY_WINDOW_CONTROLS = 'qbz-show-window-controls';
const LEGACY_KEY_HIDE = 'qbz-hide-titlebar';
const LEGACY_KEY_SYSTEM = 'qbz-use-system-titlebar';

let mode: TitlebarMode = 'qbz';
let showWindowControls = true;

const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

function migrateLegacyKeys(): TitlebarMode | null {
  // Returns the migrated mode, or null if no migration was needed.
  const hide = localStorage.getItem(LEGACY_KEY_HIDE);
  const system = localStorage.getItem(LEGACY_KEY_SYSTEM);
  if (hide === null && system === null) return null;

  let migrated: TitlebarMode = 'qbz';
  if (hide === 'true') {
    migrated = 'hidden';
  } else if (system === 'true') {
    migrated = 'system';
  }
  return migrated;
}

export function initTitleBarStore(): void {
  if (skipIfRemote()) return;
  try {
    const savedMode = localStorage.getItem(STORAGE_KEY_MODE);
    if (savedMode === 'qbz' || savedMode === 'system' || savedMode === 'plasma' || savedMode === 'hidden') {
      mode = savedMode;
    } else {
      // No persisted mode — try migrating from legacy keys.
      const migrated = migrateLegacyKeys();
      if (migrated !== null) {
        mode = migrated;
        localStorage.setItem(STORAGE_KEY_MODE, mode);
        // Push to backend so next launch reads the correct mode before window creation.
        invoke('v2_set_titlebar_mode', { mode }).catch((e) => {
          console.error('[TitleBarStore] Failed to persist migrated mode to backend:', e);
        });
      }
    }

    const savedControls = localStorage.getItem(STORAGE_KEY_WINDOW_CONTROLS);
    if (savedControls !== null) {
      showWindowControls = savedControls !== 'false';
    }
  } catch (e) {
    console.error('[TitleBarStore] Failed to initialize:', e);
  }
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

export function getMode(): TitlebarMode {
  return mode;
}

/**
 * Whether the custom TitleBar.svelte component should mount.
 * - macOS always uses native overlay → false.
 * - 'qbz' and 'plasma' → true (full or stripped variant).
 * - 'system' and 'hidden' → false.
 */
export function shouldShowTitleBar(): boolean {
  if (platform === 'macos') return false;
  return mode === 'qbz' || mode === 'plasma';
}

/**
 * Variant for the custom TitleBar component when mounted.
 */
export function getTitleBarVariant(): 'full' | 'stripped' {
  return mode === 'plasma' ? 'stripped' : 'full';
}

export function getTitleBarHeight(): number {
  if (platform === 'macos') return 0;
  if (mode === 'system' || mode === 'hidden') return 0;
  return mode === 'plasma' ? 42 : 44;
}

export function getShowWindowControls(): boolean {
  return showWindowControls;
}

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
 * Effective `showWindowControls` after applying mode rules.
 * In 'plasma' mode, KWin draws controls in its SSD, so the custom
 * controls are forced off regardless of the user pref.
 */
export function getEffectiveShowWindowControls(): boolean {
  if (mode !== 'qbz') return false;
  return showWindowControls;
}

/**
 * Set the titlebar mode. Persists to localStorage and backend, then
 * restarts the app when the mode change crosses the decoration boundary
 * (mounting/unmounting decorations requires window re-creation).
 */
export async function setMode(value: TitlebarMode): Promise<void> {
  if (skipIfRemote()) return;

  const previousWantsDecorations =
    mode === 'system' || mode === 'plasma';
  const newWantsDecorations =
    value === 'system' || value === 'plasma';

  // Push to backend FIRST. If it fails, leave local state untouched.
  try {
    await invoke('v2_set_titlebar_mode', { mode: value });
  } catch (e) {
    console.error('[TitleBarStore] Failed to push mode to backend:', e);
    return;
  }

  mode = value;
  try {
    localStorage.setItem(STORAGE_KEY_MODE, value);
  } catch (e) {
    console.error('[TitleBarStore] Failed to save mode setting:', e);
  }

  if (previousWantsDecorations !== newWantsDecorations) {
    // Crossing the decoration boundary requires window re-creation.
    try {
      await invoke('v2_restart_app');
    } catch (e) {
      console.error('[TitleBarStore] Failed to restart app:', e);
    }
  } else {
    notifyListeners();
  }
}

export interface TitleBarState {
  mode: TitlebarMode;
  showTitleBar: boolean;
  variant: 'full' | 'stripped';
  showWindowControls: boolean;
  effectiveShowWindowControls: boolean;
  titleBarHeight: number;
}

export function getTitleBarState(): TitleBarState {
  return {
    mode,
    showTitleBar: shouldShowTitleBar(),
    variant: getTitleBarVariant(),
    showWindowControls,
    effectiveShowWindowControls: getEffectiveShowWindowControls(),
    titleBarHeight: getTitleBarHeight()
  };
}
