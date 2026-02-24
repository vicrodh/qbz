/**
 * Custom Album Cover Store
 *
 * Manages global resolution of user-uploaded custom album covers.
 * Loaded on login, exposes resolveAlbumCover() for all views.
 */

import { invoke, convertFileSrc } from '@tauri-apps/api/core';

// ============ State ============

/** Map of album_id -> asset URL (convertFileSrc'd) */
let coverMap: Map<string, string> = new Map();

const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

// ============ Public API ============

/**
 * Load all custom album covers from backend
 */
export async function initCustomAlbumCoverStore(): Promise<void> {
  const mapping = await invoke<Record<string, string>>(
    'v2_library_get_all_custom_album_covers'
  );

  coverMap = new Map();

  for (const [albumId, filePath] of Object.entries(mapping)) {
    coverMap.set(albumId, convertFileSrc(filePath));
  }

  notifyListeners();
}

/**
 * Resolve album cover: returns custom URL if set, otherwise the default.
 */
export function resolveAlbumCover(
  albumId: string,
  defaultUrl: string
): string {
  return coverMap.get(albumId) ?? defaultUrl;
}

/**
 * Check if an album has a custom cover override.
 */
export function hasCustomAlbumCover(albumId: string): boolean {
  return coverMap.has(albumId);
}

/**
 * Update store after user sets a custom cover (no backend call).
 */
export function setCustomAlbumCover(albumId: string, assetUrl: string): void {
  coverMap.set(albumId, assetUrl);
  notifyListeners();
}

/**
 * Update store after user removes a custom cover (no backend call).
 */
export function removeCustomAlbumCover(albumId: string): void {
  coverMap.delete(albumId);
  notifyListeners();
}

/**
 * Clear all custom album covers (on logout).
 */
export function clearCustomAlbumCovers(): void {
  coverMap = new Map();
  notifyListeners();
}

/**
 * Subscribe to store changes.
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
