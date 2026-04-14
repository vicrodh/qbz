/**
 * Award Favorites Store
 *
 * Centralized store for tracking favorite (followed) awards. Clone of
 * labelFavoritesStore — same SQLite-backed cache, same sync strategy,
 * same toggle flow. Award ids come through as strings (the Qobuz
 * mobile API stringifies them on favorite/create?award_ids=).
 *
 * This is what feeds the "Awards" tab in Release Watch — the set of
 * awards the user is following is used to fetch
 * /favorite/getNewReleases?type=awards.
 */

import { invoke } from '@tauri-apps/api/core';
import { skipIfRemote } from '$lib/services/commandRouter';

let favoriteAwardIds = new Set<string>();
let togglingAwardIds = new Set<string>();
let isLoaded = false;
let isLoading = false;
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) listener();
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function isAwardFavorite(awardId: string | number): boolean {
  return favoriteAwardIds.has(String(awardId));
}

export function isAwardToggling(awardId: string | number): boolean {
  return togglingAwardIds.has(String(awardId));
}

export function getFavoriteAwardIds(): string[] {
  return Array.from(favoriteAwardIds);
}

export function isFavoritesLoaded(): boolean {
  return isLoaded;
}

export async function loadAwardFavorites(): Promise<void> {
  if (skipIfRemote()) return;
  if (isLoading) return;

  isLoading = true;
  try {
    try {
      const cachedIds = await invoke<string[]>('v2_get_cached_favorite_awards');
      if (cachedIds.length > 0) {
        favoriteAwardIds = new Set(cachedIds);
        isLoaded = true;
        notifyListeners();
        console.log(`[AwardFavorites] Loaded ${cachedIds.length} awards from local cache`);
      }
    } catch (cacheErr) {
      console.debug('[AwardFavorites] No local cache available:', cacheErr);
    }

    await syncFromApi();
  } catch (err) {
    console.error('[AwardFavorites] Failed to load favorites:', err);
    if (!isLoaded) {
      favoriteAwardIds = new Set();
      isLoaded = true;
      notifyListeners();
    }
  } finally {
    isLoading = false;
  }
}

export async function syncFromApi(): Promise<void> {
  if (skipIfRemote()) return;
  try {
    const allAwardIds: string[] = [];
    let offset = 0;
    const limit = 500;

    while (true) {
      const result = await invoke<{ awards?: { items: Array<{ id: string | number }>; total?: number } }>(
        'v2_get_favorites',
        { favType: 'awards', limit, offset }
      );

      const items = result.awards?.items ?? [];
      if (!items.length) break;

      allAwardIds.push(...items.map(item => String(item.id)));
      offset += items.length;

      if (items.length < limit) break;
      if (result.awards?.total && offset >= result.awards.total) break;
    }

    await invoke('v2_sync_cached_favorite_awards', { awardIds: allAwardIds });

    favoriteAwardIds = new Set(allAwardIds);
    isLoaded = true;
    notifyListeners();

    console.log(`[AwardFavorites] Synced ${allAwardIds.length} awards from API`);
  } catch (err) {
    console.error('[AwardFavorites] Failed to sync from API:', err);
    throw err;
  }
}

export async function toggleAwardFavorite(awardId: string | number): Promise<boolean> {
  const key = String(awardId);
  if (skipIfRemote()) return favoriteAwardIds.has(key);
  if (togglingAwardIds.has(key)) return favoriteAwardIds.has(key);

  const wasFavorite = favoriteAwardIds.has(key);
  const newState = !wasFavorite;

  togglingAwardIds.add(key);
  notifyListeners();

  try {
    if (newState) {
      await invoke('v2_add_favorite', { favType: 'award', itemId: key });
      await invoke('v2_cache_favorite_award', { awardId: key });
      favoriteAwardIds.add(key);
    } else {
      await invoke('v2_remove_favorite', { favType: 'award', itemId: key });
      await invoke('v2_uncache_favorite_award', { awardId: key });
      favoriteAwardIds.delete(key);
    }
    return newState;
  } catch (err) {
    console.error('[AwardFavorites] Failed to toggle favorite:', err);
    return wasFavorite;
  } finally {
    togglingAwardIds.delete(key);
    notifyListeners();
  }
}

export async function syncCache(awardIds: string[]): Promise<void> {
  if (skipIfRemote()) return;
  try {
    await invoke('v2_sync_cached_favorite_awards', { awardIds });
    favoriteAwardIds = new Set(awardIds);
    notifyListeners();
  } catch (err) {
    console.error('[AwardFavorites] Failed to sync cache:', err);
  }
}
