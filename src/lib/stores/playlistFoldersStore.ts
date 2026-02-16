/**
 * Playlist Folders Store
 *
 * Manages playlist folder state with Tauri backend persistence.
 * Folders are a local-only feature for organizing Qobuz playlists.
 */

import { invoke } from '@tauri-apps/api/core';

// Types matching the Rust backend
export interface PlaylistFolder {
  id: string;
  name: string;
  icon_type: 'preset' | 'custom';
  icon_preset: string;  // lucide icon name
  icon_color: string;   // hex color
  custom_image_path?: string;
  is_hidden: boolean;
  position: number;
  created_at: number;
  updated_at: number;
}

// State
let folders: PlaylistFolder[] = [];
let isLoading = false;
let lastError: string | null = null;

// Session-only expand/collapse state (not persisted per user request)
const expandedFolders = new Set<string>();

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

// ============ Public API ============

/**
 * Subscribe to folder state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Get all folders
 */
export function getFolders(): PlaylistFolder[] {
  return folders;
}

/**
 * Get visible (non-hidden) folders sorted by position
 */
export function getVisibleFolders(): PlaylistFolder[] {
  return folders
    .filter(f => !f.is_hidden)
    .sort((a, b) => a.position - b.position);
}

/**
 * Get a folder by ID
 */
export function getFolderById(id: string): PlaylistFolder | undefined {
  return folders.find(f => f.id === id);
}

/**
 * Check if folder is expanded (session-only state)
 */
export function isFolderExpanded(folderId: string): boolean {
  return expandedFolders.has(folderId);
}

/**
 * Toggle folder expand/collapse (session-only state)
 */
export function toggleFolderExpanded(folderId: string): void {
  if (expandedFolders.has(folderId)) {
    expandedFolders.delete(folderId);
  } else {
    expandedFolders.add(folderId);
  }
  notifyListeners();
}

/**
 * Expand a folder (session-only state)
 */
export function expandFolder(folderId: string): void {
  expandedFolders.add(folderId);
  notifyListeners();
}

/**
 * Collapse a folder (session-only state)
 */
export function collapseFolder(folderId: string): void {
  expandedFolders.delete(folderId);
  notifyListeners();
}

/**
 * Collapse all folders (session-only state)
 */
export function collapseAllFolders(): void {
  expandedFolders.clear();
  notifyListeners();
}

/**
 * Get loading state
 */
export function getIsLoading(): boolean {
  return isLoading;
}

/**
 * Get last error
 */
export function getLastError(): string | null {
  return lastError;
}

// ============ Async Operations ============

/**
 * Load folders from backend
 */
export async function loadFolders(): Promise<void> {
  isLoading = true;
  lastError = null;
  notifyListeners();

  try {
    const result = await invoke<PlaylistFolder[]>('get_playlist_folders');
    folders = result;
    lastError = null;
  } catch (err) {
    console.error('Failed to load playlist folders:', err);
    lastError = String(err);
  } finally {
    isLoading = false;
    notifyListeners();
  }
}

/**
 * Create a new folder
 */
export async function createFolder(
  name: string,
  iconType?: string,
  iconPreset?: string,
  iconColor?: string
): Promise<PlaylistFolder | null> {
  try {
    const folder = await invoke<PlaylistFolder>('create_playlist_folder', {
      name,
      iconType,
      iconPreset,
      iconColor
    });
    folders = [...folders, folder];
    notifyListeners();
    return folder;
  } catch (err) {
    console.error('Failed to create playlist folder:', err);
    lastError = String(err);
    notifyListeners();
    return null;
  }
}

/**
 * Update a folder
 */
export async function updateFolder(
  id: string,
  updates: {
    name?: string;
    iconType?: string;
    iconPreset?: string;
    iconColor?: string;
    customImagePath?: string;
    isHidden?: boolean;
  }
): Promise<PlaylistFolder | null> {
  try {
    const folder = await invoke<PlaylistFolder>('update_playlist_folder', {
      id,
      name: updates.name,
      iconType: updates.iconType,
      iconPreset: updates.iconPreset,
      iconColor: updates.iconColor,
      customImagePath: updates.customImagePath,
      isHidden: updates.isHidden
    });

    // Update local state
    const index = folders.findIndex(f => f.id === id);
    if (index >= 0) {
      folders = [...folders.slice(0, index), folder, ...folders.slice(index + 1)];
    }
    notifyListeners();
    return folder;
  } catch (err) {
    console.error('Failed to update playlist folder:', err);
    lastError = String(err);
    notifyListeners();
    return null;
  }
}

/**
 * Delete a folder (playlists return to root)
 */
export async function deleteFolder(id: string): Promise<boolean> {
  try {
    await invoke('v2_delete_playlist_folder', { id });
    folders = folders.filter(f => f.id !== id);
    expandedFolders.delete(id);
    notifyListeners();
    return true;
  } catch (err) {
    console.error('Failed to delete playlist folder:', err);
    lastError = String(err);
    notifyListeners();
    return false;
  }
}

/**
 * Reorder folders
 */
export async function reorderFolders(folderIds: string[]): Promise<boolean> {
  try {
    await invoke('v2_reorder_playlist_folders', { folderIds });

    // Update local positions
    const folderMap = new Map(folders.map(f => [f.id, f]));
    const reordered: PlaylistFolder[] = [];

    folderIds.forEach((id, index) => {
      const folder = folderMap.get(id);
      if (folder) {
        reordered.push({ ...folder, position: index });
      }
    });

    // Add any folders not in the list (shouldn't happen but be safe)
    for (const folder of folders) {
      if (!folderIds.includes(folder.id)) {
        reordered.push(folder);
      }
    }

    folders = reordered;
    notifyListeners();
    return true;
  } catch (err) {
    console.error('Failed to reorder playlist folders:', err);
    lastError = String(err);
    notifyListeners();
    return false;
  }
}

/**
 * Move a playlist to a folder (or root if folderId is null)
 */
export async function movePlaylistToFolder(
  playlistId: number,
  folderId: string | null
): Promise<boolean> {
  try {
    await invoke('v2_move_playlist_to_folder', {
      playlistId,
      folderId
    });
    notifyListeners();
    return true;
  } catch (err) {
    console.error('Failed to move playlist to folder:', err);
    lastError = String(err);
    notifyListeners();
    return false;
  }
}

/**
 * Toggle folder hidden status
 */
export async function toggleFolderHidden(id: string): Promise<boolean> {
  const folder = folders.find(f => f.id === id);
  if (!folder) return false;

  const result = await updateFolder(id, { isHidden: !folder.is_hidden });
  return result !== null;
}

/**
 * Reset error state
 */
export function clearError(): void {
  lastError = null;
  notifyListeners();
}
