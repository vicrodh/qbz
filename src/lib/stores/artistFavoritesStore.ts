/**
 * Artist Favorites Store
 *
 * Centralized store for tracking favorite artists.
 * Uses local SQLite persistence for instant UI updates.
 *
 * Sync strategy:
 * - On login: Fetch from Qobuz API → sync to local cache → populate store
 * - On toggle: API call first → on success → update local cache → notify UI
 * - UI reads from in-memory store (backed by local cache)
 */

import { invoke } from '@tauri-apps/api/core';
import { logRecoEvent } from '$lib/services/recoService';

let favoriteArtistIds = new Set<number>();
let togglingArtistIds = new Set<number>(); // Artist IDs currently being toggled
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

export function isArtistFavorite(artistId: number): boolean {
  return favoriteArtistIds.has(artistId);
}

export function isArtistToggling(artistId: number): boolean {
  return togglingArtistIds.has(artistId);
}

export function getFavoriteArtistIds(): number[] {
  return Array.from(favoriteArtistIds);
}

export function isFavoritesLoaded(): boolean {
  return isLoaded;
}

/**
 * Load artist favorites from local cache first, then sync from API
 */
export async function loadArtistFavorites(): Promise<void> {
  if (isLoading) return;

  isLoading = true;
  try {
    // Step 1: Load from local cache for instant UI
    try {
      const cachedIds = await invoke<number[]>('v2_get_cached_favorite_artists');
      if (cachedIds.length > 0) {
        favoriteArtistIds = new Set(cachedIds);
        isLoaded = true;
        notifyListeners();
        console.log(`[ArtistFavorites] Loaded ${cachedIds.length} artists from local cache`);
      }
    } catch (cacheErr) {
      console.debug('[ArtistFavorites] No local cache available:', cacheErr);
    }

    // Step 2: Fetch from API and sync to local cache
    await syncFromApi();

  } catch (err) {
    console.error('[ArtistFavorites] Failed to load favorites:', err);
    if (!isLoaded) {
      favoriteArtistIds = new Set();
      isLoaded = true;
      notifyListeners();
    }
  } finally {
    isLoading = false;
  }
}

/**
 * Sync artist favorites from Qobuz API to local cache
 */
export async function syncFromApi(): Promise<void> {
  try {
    const allArtistIds: number[] = [];
    let offset = 0;
    const limit = 500;

    while (true) {
      const result = await invoke<{ artists?: { items: Array<{ id: number }>; total?: number } }>('v2_get_favorites', {
        favType: 'artists',
        limit,
        offset
      });

      const items = result.artists?.items ?? [];
      if (!items.length) break;

      allArtistIds.push(...items.map(item => item.id));
      offset += items.length;

      if (items.length < limit) break;
      if (result.artists?.total && offset >= result.artists.total) break;
    }

    // Sync to local cache
    await invoke('v2_sync_cached_favorite_artists', { artistIds: allArtistIds });

    // Update in-memory store
    favoriteArtistIds = new Set(allArtistIds);
    isLoaded = true;
    notifyListeners();

    console.log(`[ArtistFavorites] Synced ${allArtistIds.length} artists from API`);
  } catch (err) {
    console.error('[ArtistFavorites] Failed to sync from API:', err);
    throw err;
  }
}

/**
 * Toggle artist favorite status
 * API call first → on success → update local cache → notify UI
 */
export async function toggleArtistFavorite(artistId: number): Promise<boolean> {
  if (togglingArtistIds.has(artistId)) {
    return favoriteArtistIds.has(artistId);
  }

  const wasFavorite = favoriteArtistIds.has(artistId);
  const newState = !wasFavorite;

  // Mark as toggling
  togglingArtistIds.add(artistId);
  notifyListeners();

  try {
    if (newState) {
      await invoke('v2_add_favorite', { favType: 'artist', itemId: String(artistId) });
      await invoke('v2_cache_favorite_artist', { artistId });
      favoriteArtistIds.add(artistId);
      void logRecoEvent({
        eventType: 'favorite',
        itemType: 'artist',
        artistId
      });
    } else {
      await invoke('v2_remove_favorite', { favType: 'artist', itemId: String(artistId) });
      await invoke('v2_uncache_favorite_artist', { artistId });
      favoriteArtistIds.delete(artistId);
    }
    return newState;
  } catch (err) {
    console.error('[ArtistFavorites] Failed to toggle favorite:', err);
    return wasFavorite;
  } finally {
    togglingArtistIds.delete(artistId);
    notifyListeners();
  }
}

/**
 * Sync local cache from an array of artist IDs
 * Used by FavoritesView after fetching from API
 */
export async function syncCache(artistIds: number[]): Promise<void> {
  try {
    await invoke('v2_sync_cached_favorite_artists', { artistIds });
    favoriteArtistIds = new Set(artistIds);
    notifyListeners();
  } catch (err) {
    console.error('[ArtistFavorites] Failed to sync cache:', err);
  }
}

export function markAsFavorite(artistId: number): void {
  if (!favoriteArtistIds.has(artistId)) {
    favoriteArtistIds.add(artistId);
    invoke('v2_cache_favorite_artist', { artistId }).catch(() => {});
    notifyListeners();
  }
}

export function unmarkAsFavorite(artistId: number): void {
  if (favoriteArtistIds.has(artistId)) {
    favoriteArtistIds.delete(artistId);
    invoke('v2_uncache_favorite_artist', { artistId }).catch(() => {});
    notifyListeners();
  }
}
