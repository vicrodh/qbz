/**
 * Album Favorites Store
 *
 * Centralized store for tracking favorite albums.
 * Uses local SQLite persistence for instant UI updates.
 *
 * Sync strategy:
 * - On login: Fetch from Qobuz API → sync to local cache → populate store
 * - On toggle: API call first → on success → update local cache → notify UI
 * - UI reads from in-memory store (backed by local cache)
 */

import { invoke } from '@tauri-apps/api/core';
import { logRecoEvent } from '$lib/services/recoService';

let favoriteAlbumIds = new Set<string>();
let togglingAlbumIds = new Set<string>(); // Album IDs currently being toggled
let isLoaded = false;
let isLoading = false;
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function isAlbumFavorite(albumId: string): boolean {
  return favoriteAlbumIds.has(albumId);
}

export function isAlbumToggling(albumId: string): boolean {
  return togglingAlbumIds.has(albumId);
}

export function getFavoriteAlbumIds(): string[] {
  return Array.from(favoriteAlbumIds);
}

export function isFavoritesLoaded(): boolean {
  return isLoaded;
}

/**
 * Load album favorites from local cache first, then sync from API
 */
export async function loadAlbumFavorites(): Promise<void> {
  if (isLoading) return;

  isLoading = true;
  try {
    // Step 1: Load from local cache for instant UI
    try {
      const cachedIds = await invoke<string[]>('get_cached_favorite_albums');
      if (cachedIds.length > 0) {
        favoriteAlbumIds = new Set(cachedIds);
        isLoaded = true;
        notifyListeners();
        console.log(`[AlbumFavorites] Loaded ${cachedIds.length} albums from local cache`);
      }
    } catch (cacheErr) {
      console.debug('[AlbumFavorites] No local cache available:', cacheErr);
    }

    // Step 2: Fetch from API and sync to local cache
    await syncFromApi();

  } catch (err) {
    console.error('[AlbumFavorites] Failed to load favorites:', err);
    if (!isLoaded) {
      favoriteAlbumIds = new Set();
      isLoaded = true;
      notifyListeners();
    }
  } finally {
    isLoading = false;
  }
}

/**
 * Sync album favorites from Qobuz API to local cache
 */
export async function syncFromApi(): Promise<void> {
  try {
    const allAlbumIds: string[] = [];
    let offset = 0;
    const limit = 500;

    while (true) {
      const result = await invoke<{ albums?: { items: Array<{ id: string }>; total?: number } }>('get_favorites', {
        favType: 'albums',
        limit,
        offset
      });

      const items = result.albums?.items ?? [];
      if (!items.length) break;

      allAlbumIds.push(...items.map(item => item.id));
      offset += items.length;

      if (items.length < limit) break;
      if (result.albums?.total && offset >= result.albums.total) break;
    }

    // Sync to local cache
    await invoke('sync_cached_favorite_albums', { albumIds: allAlbumIds });

    // Update in-memory store
    favoriteAlbumIds = new Set(allAlbumIds);
    isLoaded = true;
    notifyListeners();

    console.log(`[AlbumFavorites] Synced ${allAlbumIds.length} albums from API`);
  } catch (err) {
    console.error('[AlbumFavorites] Failed to sync from API:', err);
    throw err;
  }
}

/**
 * Toggle album favorite status
 * API call first → on success → update local cache → notify UI
 */
export async function toggleAlbumFavorite(albumId: string): Promise<boolean> {
  if (togglingAlbumIds.has(albumId)) {
    return favoriteAlbumIds.has(albumId);
  }

  const wasFavorite = favoriteAlbumIds.has(albumId);
  const newState = !wasFavorite;

  // Mark as toggling
  togglingAlbumIds.add(albumId);
  notifyListeners();

  try {
    if (newState) {
      await invoke('add_favorite', { favType: 'album', itemId: albumId });
      await invoke('cache_favorite_album', { albumId });
      favoriteAlbumIds.add(albumId);
      void logRecoEvent({
        eventType: 'favorite',
        itemType: 'album',
        albumId
      });
    } else {
      await invoke('remove_favorite', { favType: 'album', itemId: albumId });
      await invoke('uncache_favorite_album', { albumId });
      favoriteAlbumIds.delete(albumId);
    }
    return newState;
  } catch (err) {
    console.error('[AlbumFavorites] Failed to toggle favorite:', err);
    return wasFavorite;
  } finally {
    togglingAlbumIds.delete(albumId);
    notifyListeners();
  }
}

/**
 * Sync local cache from an array of album IDs
 * Used by FavoritesView after fetching from API
 */
export async function syncCache(albumIds: string[]): Promise<void> {
  try {
    await invoke('sync_cached_favorite_albums', { albumIds });
    favoriteAlbumIds = new Set(albumIds);
    notifyListeners();
  } catch (err) {
    console.error('[AlbumFavorites] Failed to sync cache:', err);
  }
}

export function markAsFavorite(albumId: string): void {
  if (!favoriteAlbumIds.has(albumId)) {
    favoriteAlbumIds.add(albumId);
    invoke('cache_favorite_album', { albumId }).catch(() => {});
    notifyListeners();
  }
}

export function unmarkAsFavorite(albumId: string): void {
  if (favoriteAlbumIds.has(albumId)) {
    favoriteAlbumIds.delete(albumId);
    invoke('uncache_favorite_album', { albumId }).catch(() => {});
    notifyListeners();
  }
}
