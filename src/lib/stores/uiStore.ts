/**
 * UI State Store
 *
 * Manages overlay and modal visibility states across the app.
 */

// Overlay states
let isQueueOpen = false;
let isFullScreenOpen = false;
let isFocusModeOpen = false;
let isCastPickerOpen = false;

// Playlist modal states
let isPlaylistModalOpen = false;
let playlistModalMode: 'create' | 'edit' | 'addTrack' = 'create';
let playlistModalTrackIds: number[] = [];
let isPlaylistImportOpen = false;

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
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

export function openPlaylistModal(mode: 'create' | 'edit' | 'addTrack', trackIds: number[] = []): void {
  isPlaylistModalOpen = true;
  playlistModalMode = mode;
  playlistModalTrackIds = trackIds;
  notifyListeners();
}

export function closePlaylistModal(): void {
  isPlaylistModalOpen = false;
  playlistModalTrackIds = [];
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
  isPlaylistImportOpen: boolean;
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
    isPlaylistImportOpen
  };
}
