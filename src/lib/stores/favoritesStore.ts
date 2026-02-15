/**
 * Favorites Store
 *
 * Centralized store for tracking favorite tracks.
 * Uses local SQLite persistence for instant UI updates.
 *
 * Sync strategy:
 * - On login: Fetch from Qobuz API → sync to local cache → populate store
 * - On toggle: API call first → on success → update local cache → notify UI
 * - UI reads from in-memory store (backed by local cache)
 */

import { invoke } from '@tauri-apps/api/core';
import { logRecoEvent } from '$lib/services/recoService';

// State
let favoriteTrackIds = new Set<number>();
let togglingTrackIds = new Set<number>(); // Track IDs currently being toggled (API in progress)
let isLoaded = false;
let isLoading = false;
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

// ============ Public API ============

/**
 * Subscribe to favorites changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Check if a track is in favorites
 */
export function isTrackFavorite(trackId: number): boolean {
  return favoriteTrackIds.has(trackId);
}

/**
 * Check if a track favorite is currently being toggled (API call in progress)
 */
export function isTrackToggling(trackId: number): boolean {
  return togglingTrackIds.has(trackId);
}

/**
 * Get all favorite track IDs
 */
export function getFavoriteTrackIds(): number[] {
  return Array.from(favoriteTrackIds);
}

/**
 * Check if favorites have been loaded
 */
export function isFavoritesLoaded(): boolean {
  return isLoaded;
}

/**
 * Load favorites from local cache first, then sync from API
 * Call once on app init/login
 */
export async function loadFavorites(): Promise<void> {
  if (isLoading) return;

  isLoading = true;
  try {
    // Step 1: Load from local cache for instant UI (if available)
    try {
      const cachedIds = await invoke<number[]>('get_cached_favorite_tracks');
      if (cachedIds.length > 0) {
        favoriteTrackIds = new Set(cachedIds);
        isLoaded = true;
        notifyListeners();
        console.log(`[Favorites] Loaded ${cachedIds.length} tracks from local cache`);
      }
    } catch (cacheErr) {
      console.debug('[Favorites] No local cache available:', cacheErr);
    }

    // Step 2: Fetch from API and sync to local cache
    await syncFromApi();

  } catch (err) {
    console.error('[Favorites] Failed to load favorites:', err);
    // If no cache was loaded, initialize as empty
    if (!isLoaded) {
      favoriteTrackIds = new Set();
      isLoaded = true;
      notifyListeners();
    }
  } finally {
    isLoading = false;
  }
}

/**
 * Sync favorites from Qobuz API to local cache
 * Called on login and when FavoritesView loads
 */
export async function syncFromApi(): Promise<void> {
  try {
    // Fetch all favorites from API (paginated)
    const allTrackIds: number[] = [];
    let offset = 0;
    const limit = 500;

    while (true) {
      const response = await invoke<{ tracks?: { items?: Array<{ id: number }>; total?: number } }>('get_favorites', {
        favType: 'tracks',
        limit,
        offset
      });

      const items = response?.tracks?.items ?? [];
      if (items.length === 0) break;

      allTrackIds.push(...items.map(t => t.id));
      offset += items.length;

      // Check if we've fetched all
      if (items.length < limit) break;
      if (response.tracks?.total && offset >= response.tracks.total) break;
    }

    // Sync to local cache
    await invoke('sync_cached_favorite_tracks', { trackIds: allTrackIds });

    // Update in-memory store
    favoriteTrackIds = new Set(allTrackIds);
    isLoaded = true;
    notifyListeners();

    console.log(`[Favorites] Synced ${allTrackIds.length} tracks from API`);
  } catch (err) {
    console.error('[Favorites] Failed to sync from API:', err);
    throw err;
  }
}

/**
 * Toggle favorite status for a track
 * API call first → on success → update local cache → notify UI
 * Returns the new favorite state
 */
export async function toggleTrackFavorite(trackId: number): Promise<boolean> {
  // Prevent double-toggling while API call is in progress
  if (togglingTrackIds.has(trackId)) {
    return favoriteTrackIds.has(trackId);
  }

  const wasFavorite = favoriteTrackIds.has(trackId);
  const newState = !wasFavorite;

  // Mark as toggling (UI shows loading state)
  togglingTrackIds.add(trackId);
  notifyListeners();

  try {
    // Call API first - no optimistic update (V2)
    if (newState) {
      await invoke('v2_add_favorite', { favType: 'track', itemId: String(trackId) });
      // API succeeded - update local cache
      await invoke('cache_favorite_track', { trackId });
      // Update in-memory store
      favoriteTrackIds.add(trackId);
      // Log for recommendations
      void logRecoEvent({
        eventType: 'favorite',
        itemType: 'track',
        trackId
      });
    } else {
      await invoke('v2_remove_favorite', { favType: 'track', itemId: String(trackId) });
      // API succeeded - update local cache
      await invoke('uncache_favorite_track', { trackId });
      // Update in-memory store
      favoriteTrackIds.delete(trackId);
    }

    return newState;
  } catch (err) {
    // API failed - no state change, just log error
    console.error('[Favorites] Failed to toggle favorite:', err);
    return wasFavorite;
  } finally {
    // Always clear toggling state
    togglingTrackIds.delete(trackId);
    notifyListeners();
  }
}

/**
 * Add a track to favorites (used when we know it's not already favorite)
 */
export async function addTrackFavorite(trackId: number): Promise<boolean> {
  if (favoriteTrackIds.has(trackId)) return true;
  return toggleTrackFavorite(trackId);
}

/**
 * Remove a track from favorites
 */
export async function removeTrackFavorite(trackId: number): Promise<boolean> {
  if (!favoriteTrackIds.has(trackId)) return true;
  const result = await toggleTrackFavorite(trackId);
  return !result; // Returns true if successfully removed
}

/**
 * Sync local cache from an array of track IDs
 * Used by FavoritesView after fetching from API
 */
export async function syncCache(trackIds: number[]): Promise<void> {
  try {
    await invoke('sync_cached_favorite_tracks', { trackIds });
    favoriteTrackIds = new Set(trackIds);
    notifyListeners();
  } catch (err) {
    console.error('[Favorites] Failed to sync cache:', err);
  }
}

/**
 * Reset store (for logout)
 */
export async function reset(): Promise<void> {
  try {
    await invoke('clear_favorites_cache');
  } catch (err) {
    console.debug('[Favorites] Failed to clear cache:', err);
  }
  favoriteTrackIds = new Set();
  togglingTrackIds = new Set();
  isLoaded = false;
  isLoading = false;
  notifyListeners();
}

// Legacy functions for backwards compatibility
export function markAsFavorite(trackId: number): void {
  if (!favoriteTrackIds.has(trackId)) {
    favoriteTrackIds.add(trackId);
    // Also update cache
    invoke('cache_favorite_track', { trackId }).catch(() => {});
    notifyListeners();
  }
}

export function unmarkAsFavorite(trackId: number): void {
  if (favoriteTrackIds.has(trackId)) {
    favoriteTrackIds.delete(trackId);
    // Also update cache
    invoke('uncache_favorite_track', { trackId }).catch(() => {});
    notifyListeners();
  }
}
