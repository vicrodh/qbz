/**
 * Mini Player Window Service
 *
 * New strategy:
 * - Keep main window mounted/alive
 * - Open mini player as a second Tauri window
 * - Hide/show + focus windows without route remount in main
 */

import { Window, getCurrentWindow } from '@tauri-apps/api/window';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import {
  getMiniPlayerState,
  initMiniPlayerState,
  setMiniPlayerAlwaysOnTop as setMiniPlayerAlwaysOnTopState,
  setMiniPlayerOpen,
  setMiniPlayerSurface,
  type MiniPlayerSurface
} from '$lib/stores/uiStore';
import { getUseSystemTitleBar } from '$lib/stores/titleBarStore';
import { getUserItem } from '$lib/utils/userStorage';

const MINI_PLAYER_LABEL = 'miniplayer';
const MAIN_WINDOW_LABEL = 'main';
const MINI_PLAYER_DEFAULT_VIEW_KEY = 'qbz-miniplayer-default-view';
const MINI_PLAYER_SURFACE_OPTIONS: MiniPlayerSurface[] = ['micro', 'compact', 'artwork', 'queue', 'lyrics'];

function isMiniPlayerSurface(value: string): value is MiniPlayerSurface {
  return MINI_PLAYER_SURFACE_OPTIONS.includes(value as MiniPlayerSurface);
}

function resolveInitialSurface(requestedSurface?: MiniPlayerSurface): MiniPlayerSurface | null {
  if (requestedSurface) {
    return requestedSurface;
  }

  const configuredDefault = getUserItem(MINI_PLAYER_DEFAULT_VIEW_KEY);
  if (!configuredDefault || configuredDefault === 'remember') {
    return null;
  }

  if (isMiniPlayerSurface(configuredDefault)) {
    return configuredDefault;
  }

  return null;
}

export interface AlwaysOnTopResult {
  applied: boolean;
  reason?: string;
}

function normalizeRoute(url: string): string {
  // adapter-static fallback setup expects the app shell entrypoint.
  // route segment is resolved by the SPA router after bootstrap.
  return url.startsWith('/') ? url : `/${url}`;
}

async function waitForWindowCreation(windowRef: WebviewWindow): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    let done = false;

    void windowRef.once('tauri://created', () => {
      if (done) return;
      done = true;
      resolve();
    });

    void windowRef.once<string>('tauri://error', (event) => {
      if (done) return;
      done = true;
      reject(new Error(String(event.payload)));
    });
  });
}

async function getOrCreateMiniPlayerWindow(): Promise<Window> {
  const existing = await Window.getByLabel(MINI_PLAYER_LABEL);
  if (existing) return existing;

  const miniState = getMiniPlayerState();
  const geometry = miniState.geometry;

  const miniWindow = new WebviewWindow(MINI_PLAYER_LABEL, {
    url: normalizeRoute('/miniplayer'),
    title: 'QBZ Mini Player',
    width: geometry.width,
    height: geometry.height,
    x: geometry.x,
    y: geometry.y,
    minWidth: 340,
    minHeight: 57,
    resizable: true,
    decorations: getUseSystemTitleBar(),
    alwaysOnTop: miniState.alwaysOnTop,
    transparent: true,
    focus: true
  });

  await waitForWindowCreation(miniWindow);
  return miniWindow;
}

async function focusMainWindow(): Promise<void> {
  const mainWindow = await Window.getByLabel(MAIN_WINDOW_LABEL);
  if (!mainWindow) return;
  await mainWindow.show();
  await mainWindow.unminimize();
  await mainWindow.setFocus();
}

/**
 * Enter mini player mode:
 * create/show mini player window and hide main window.
 */
export async function enterMiniplayerMode(surface?: MiniPlayerSurface): Promise<void> {
  initMiniPlayerState();

  const initialSurface = resolveInitialSurface(surface);
  if (initialSurface) {
    setMiniPlayerSurface(initialSurface);
  }

  try {
    const currentWindow = getCurrentWindow();
    const miniWindow = await getOrCreateMiniPlayerWindow();

    await miniWindow.show();
    await miniWindow.unminimize();
    await miniWindow.setFocus();

    // Hide main window only after mini is visible to avoid a blank app moment.
    if (currentWindow.label === MAIN_WINDOW_LABEL) {
      await currentWindow.hide();
    } else {
      const mainWindow = await Window.getByLabel(MAIN_WINDOW_LABEL);
      if (mainWindow) await mainWindow.hide();
    }

    setMiniPlayerOpen(true);
    console.info('[MiniPlayer] Entered mini player mode');
  } catch (err) {
    console.error('[MiniPlayer] Failed to enter mini player mode:', err);
  }
}

/**
 * Exit mini player mode:
 * hide mini player window and show/focus main window.
 */
export async function exitMiniplayerMode(): Promise<void> {
  try {
    const miniWindow = await Window.getByLabel(MINI_PLAYER_LABEL);
    if (miniWindow) {
      await miniWindow.hide();
    }

    await focusMainWindow();

    setMiniPlayerOpen(false);
    console.info('[MiniPlayer] Restored main window');
  } catch (err) {
    console.error('[MiniPlayer] Failed to restore main window:', err);
  }
}

/**
 * Best-effort always-on-top toggle.
 *
 * On some Wayland compositors this can be ignored or denied.
 */
export async function setMiniplayerAlwaysOnTop(alwaysOnTop: boolean): Promise<AlwaysOnTopResult> {
  try {
    const miniWindow = await Window.getByLabel(MINI_PLAYER_LABEL);
    if (!miniWindow) {
      setMiniPlayerAlwaysOnTopState(alwaysOnTop);
      return { applied: false, reason: 'mini_window_not_open' };
    }

    await miniWindow.setAlwaysOnTop(alwaysOnTop);
    setMiniPlayerAlwaysOnTopState(alwaysOnTop);
    return { applied: true };
  } catch (err) {
    // Wayland or WM policy may reject always-on-top requests.
    setMiniPlayerAlwaysOnTopState(false);
    return {
      applied: false,
      reason: err instanceof Error ? err.message : String(err)
    };
  }
}

export async function closeMiniplayerWindow(): Promise<void> {
  await exitMiniplayerMode();
}
