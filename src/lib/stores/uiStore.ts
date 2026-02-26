/**
 * UI State Store
 *
 * Manages overlay and modal visibility states across the app.
 */

import { closeAll as closeAllMenus } from './floatingMenuStore';
import { hideSidebar as hideLyricsSidebar } from './lyricsStore';

// Overlay states
let isQueueOpen = false;
let isFullScreenOpen = false;
let isFocusModeOpen = false;
let isCastPickerOpen = false;

export type MiniPlayerSurface = 'micro' | 'compact' | 'artwork' | 'queue' | 'lyrics';

export interface MiniPlayerGeometry {
  width: number;
  height: number;
  x?: number;
  y?: number;
}

export interface MiniPlayerState {
  open: boolean;
  surface: MiniPlayerSurface;
  alwaysOnTop: boolean;
  geometry: MiniPlayerGeometry;
}

const MINI_PLAYER_STORAGE_KEY = 'qbz-mini-player-state';
const MINI_PLAYER_SURFACES: MiniPlayerSurface[] = ['micro', 'compact', 'artwork', 'queue', 'lyrics'];
const DEFAULT_MINI_PLAYER_STATE: MiniPlayerState = {
  open: false,
  surface: 'artwork',
  alwaysOnTop: false,
  geometry: {
    width: 380,
    height: 540
  }
};

let miniPlayer: MiniPlayerState = {
  ...DEFAULT_MINI_PLAYER_STATE,
  geometry: { ...DEFAULT_MINI_PLAYER_STATE.geometry }
};

// Playlist modal states
let isPlaylistModalOpen = false;
let playlistModalMode: 'create' | 'edit' | 'addTrack' = 'create';
let playlistModalTrackIds: number[] = [];
let playlistModalTracksAreLocal = false;
let isPlaylistImportOpen = false;

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

function persistMiniPlayerState(): void {
  try {
    localStorage.setItem(MINI_PLAYER_STORAGE_KEY, JSON.stringify(miniPlayer));
  } catch {
    // ignore localStorage errors
  }
}

function loadMiniPlayerState(): void {
  try {
    const raw = localStorage.getItem(MINI_PLAYER_STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw) as Partial<MiniPlayerState>;
    const width = parsed.geometry?.width;
    const height = parsed.geometry?.height;
    miniPlayer = {
      open: false,
      surface: MINI_PLAYER_SURFACES.includes(parsed.surface as MiniPlayerSurface)
        ? (parsed.surface as MiniPlayerSurface)
        : DEFAULT_MINI_PLAYER_STATE.surface,
      alwaysOnTop: parsed.alwaysOnTop ?? DEFAULT_MINI_PLAYER_STATE.alwaysOnTop,
      geometry: {
        width: typeof width === 'number' && width >= 320 ? width : DEFAULT_MINI_PLAYER_STATE.geometry.width,
        height: typeof height === 'number' && height >= 57 ? height : DEFAULT_MINI_PLAYER_STATE.geometry.height,
        x: typeof parsed.geometry?.x === 'number' ? parsed.geometry.x : undefined,
        y: typeof parsed.geometry?.y === 'number' ? parsed.geometry.y : undefined
      }
    };
  } catch {
    miniPlayer = {
      ...DEFAULT_MINI_PLAYER_STATE,
      geometry: { ...DEFAULT_MINI_PLAYER_STATE.geometry }
    };
  }
}

/**
 * Subscribe to UI state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener(); // Immediately notify with current state
  return () => listeners.delete(listener);
}

export function initMiniPlayerState(): void {
  loadMiniPlayerState();
}

// ============ Queue Panel ============

export function getQueueOpen(): boolean {
  return isQueueOpen;
}

export function openQueue(): void {
  isQueueOpen = true;
  notifyListeners();
}

export function closeQueue(): void {
  isQueueOpen = false;
  notifyListeners();
}

export function toggleQueue(): void {
  isQueueOpen = !isQueueOpen;
  notifyListeners();
}

// ============ Full Screen Now Playing ============

export function getFullScreenOpen(): boolean {
  return isFullScreenOpen;
}

export function openFullScreen(): void {
  closeAllMenus();
  hideLyricsSidebar();
  isFullScreenOpen = true;
  notifyListeners();
}

export function closeFullScreen(): void {
  isFullScreenOpen = false;
  notifyListeners();
}

export function toggleFullScreen(): void {
  isFullScreenOpen = !isFullScreenOpen;
  notifyListeners();
}

// ============ Focus Mode ============

export function getFocusModeOpen(): boolean {
  return isFocusModeOpen;
}

export function openFocusMode(): void {
  closeAllMenus();
  hideLyricsSidebar();
  isFocusModeOpen = true;
  notifyListeners();
}

export function closeFocusMode(): void {
  isFocusModeOpen = false;
  notifyListeners();
}

export function toggleFocusMode(): void {
  isFocusModeOpen = !isFocusModeOpen;
  notifyListeners();
}

// ============ Cast Picker ============

export function getCastPickerOpen(): boolean {
  return isCastPickerOpen;
}

export function openCastPicker(): void {
  isCastPickerOpen = true;
  notifyListeners();
}

export function closeCastPicker(): void {
  isCastPickerOpen = false;
  notifyListeners();
}

export function toggleCastPicker(): void {
  isCastPickerOpen = !isCastPickerOpen;
  notifyListeners();
}

// ============ Mini Player ============

export function getMiniPlayerState(): MiniPlayerState {
  return {
    ...miniPlayer,
    geometry: { ...miniPlayer.geometry }
  };
}

export function setMiniPlayerOpen(open: boolean): void {
  miniPlayer = { ...miniPlayer, open };
  persistMiniPlayerState();
  notifyListeners();
}

export function setMiniPlayerSurface(surface: MiniPlayerSurface): void {
  miniPlayer = { ...miniPlayer, surface };
  persistMiniPlayerState();
  notifyListeners();
}

export function setMiniPlayerAlwaysOnTop(alwaysOnTop: boolean): void {
  miniPlayer = { ...miniPlayer, alwaysOnTop };
  persistMiniPlayerState();
  notifyListeners();
}

export function setMiniPlayerGeometry(geometry: Partial<MiniPlayerGeometry>): void {
  miniPlayer = {
    ...miniPlayer,
    geometry: {
      ...miniPlayer.geometry,
      ...geometry
    }
  };
  persistMiniPlayerState();
  notifyListeners();
}

// ============ Playlist Modal ============

export function getPlaylistModalOpen(): boolean {
  return isPlaylistModalOpen;
}

export function getPlaylistModalMode(): 'create' | 'edit' | 'addTrack' {
  return playlistModalMode;
}

export function getPlaylistModalTrackIds(): number[] {
  return playlistModalTrackIds;
}

export function getPlaylistModalTracksAreLocal(): boolean {
  return playlistModalTracksAreLocal;
}

export function openPlaylistModal(mode: 'create' | 'edit' | 'addTrack', trackIds: number[] = [], isLocal = false): void {
  isPlaylistModalOpen = true;
  playlistModalMode = mode;
  playlistModalTrackIds = trackIds;
  playlistModalTracksAreLocal = isLocal;
  notifyListeners();
}

export function closePlaylistModal(): void {
  isPlaylistModalOpen = false;
  playlistModalTrackIds = [];
  playlistModalTracksAreLocal = false;
  notifyListeners();
}

// ============ Playlist Import Modal ============

export function getPlaylistImportOpen(): boolean {
  return isPlaylistImportOpen;
}

export function openPlaylistImport(): void {
  isPlaylistImportOpen = true;
  notifyListeners();
}

export function closePlaylistImport(): void {
  isPlaylistImportOpen = false;
  notifyListeners();
}

// ============ Escape Key Handler ============

/**
 * Handle escape key - closes overlays in priority order
 * Returns true if an overlay was closed
 */
export function handleEscapeKey(): boolean {
  if (isFocusModeOpen) {
    closeFocusMode();
    return true;
  }
  if (isFullScreenOpen) {
    closeFullScreen();
    return true;
  }
  if (isQueueOpen) {
    closeQueue();
    return true;
  }
  if (isCastPickerOpen) {
    closeCastPicker();
    return true;
  }
  if (isPlaylistModalOpen) {
    closePlaylistModal();
    return true;
  }
  if (isPlaylistImportOpen) {
    closePlaylistImport();
    return true;
  }
  return false;
}

// ============ Bulk State Getter ============

export interface UIState {
  isQueueOpen: boolean;
  isFullScreenOpen: boolean;
  isFocusModeOpen: boolean;
  isCastPickerOpen: boolean;
  isPlaylistModalOpen: boolean;
  playlistModalMode: 'create' | 'edit' | 'addTrack';
  playlistModalTrackIds: number[];
  playlistModalTracksAreLocal: boolean;
  isPlaylistImportOpen: boolean;
  miniPlayer: MiniPlayerState;
}

export function getUIState(): UIState {
  return {
    isQueueOpen,
    isFullScreenOpen,
    isFocusModeOpen,
    isCastPickerOpen,
    isPlaylistModalOpen,
    playlistModalMode,
    playlistModalTrackIds,
    playlistModalTracksAreLocal,
    isPlaylistImportOpen,
    miniPlayer: getMiniPlayerState()
  };
}
