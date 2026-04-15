/**
 * Label Favorites Store
 *
 * Centralized store for tracking favorite (followed) labels. Mirrors
 * artistFavoritesStore one-for-one — same SQLite-backed cache, same
 * sync strategy, same toggle flow. Added as part of the Follow Label
 * feature; the underlying Qobuz endpoints (/favorite/create,
 * /favorite/delete, /favorite/getUserFavorites) already accept
 * `label_ids` / `fav_type=labels` per
 * qbz-nix-docs/qobuz-api-inferred-openapi-v9.7.0.3.yaml.
 */

import { invoke } from '@tauri-apps/api/core';
import { skipIfRemote } from '$lib/services/commandRouter';

let favoriteLabelIds = new Set<number>();
let togglingLabelIds = new Set<number>();
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

export function isLabelFavorite(labelId: number): boolean {
  return favoriteLabelIds.has(labelId);
}

export function isLabelToggling(labelId: number): boolean {
  return togglingLabelIds.has(labelId);
}

export function getFavoriteLabelIds(): number[] {
  return Array.from(favoriteLabelIds);
}

export function isFavoritesLoaded(): boolean {
  return isLoaded;
}

/**
 * Load label favorites from local cache first, then sync from API.
 */
export async function loadLabelFavorites(): Promise<void> {
  if (skipIfRemote()) return;
  if (isLoading) return;

  isLoading = true;
  try {
    try {
      const cachedIds = await invoke<number[]>('v2_get_cached_favorite_labels');
      if (cachedIds.length > 0) {
        favoriteLabelIds = new Set(cachedIds);
        isLoaded = true;
        notifyListeners();
        console.log(`[LabelFavorites] Loaded ${cachedIds.length} labels from local cache`);
      }
    } catch (cacheErr) {
      console.debug('[LabelFavorites] No local cache available:', cacheErr);
    }

    await syncFromApi();
  } catch (err) {
    console.error('[LabelFavorites] Failed to load favorites:', err);
    if (!isLoaded) {
      favoriteLabelIds = new Set();
      isLoaded = true;
      notifyListeners();
    }
  } finally {
    isLoading = false;
  }
}

/**
 * Sync label favorites from Qobuz API to local cache.
 */
export async function syncFromApi(): Promise<void> {
  if (skipIfRemote()) return;
  try {
    const allLabelIds: number[] = [];
    let offset = 0;
    const limit = 500;

    while (true) {
      const result = await invoke<{ labels?: { items: Array<{ id: number }>; total?: number } }>('v2_get_favorites', {
        favType: 'labels',
        limit,
        offset
      });

      const items = result.labels?.items ?? [];
      if (!items.length) break;

      allLabelIds.push(...items.map(item => item.id));
      offset += items.length;

      if (items.length < limit) break;
      if (result.labels?.total && offset >= result.labels.total) break;
    }

    await invoke('v2_sync_cached_favorite_labels', { labelIds: allLabelIds });

    favoriteLabelIds = new Set(allLabelIds);
    isLoaded = true;
    notifyListeners();

    console.log(`[LabelFavorites] Synced ${allLabelIds.length} labels from API`);
  } catch (err) {
    console.error('[LabelFavorites] Failed to sync from API:', err);
    throw err;
  }
}

/**
 * Toggle label favorite status: API call first → on success → update
 * local cache → notify UI.
 */
export async function toggleLabelFavorite(labelId: number): Promise<boolean> {
  if (skipIfRemote()) return favoriteLabelIds.has(labelId);
  if (togglingLabelIds.has(labelId)) {
    return favoriteLabelIds.has(labelId);
  }

  const wasFavorite = favoriteLabelIds.has(labelId);
  const newState = !wasFavorite;

  togglingLabelIds.add(labelId);
  notifyListeners();

  try {
    if (newState) {
      await invoke('v2_add_favorite', { favType: 'label', itemId: String(labelId) });
      await invoke('v2_cache_favorite_label', { labelId });
      favoriteLabelIds.add(labelId);
    } else {
      await invoke('v2_remove_favorite', { favType: 'label', itemId: String(labelId) });
      await invoke('v2_uncache_favorite_label', { labelId });
      favoriteLabelIds.delete(labelId);
    }
    return newState;
  } catch (err) {
    console.error('[LabelFavorites] Failed to toggle favorite:', err);
    return wasFavorite;
  } finally {
    togglingLabelIds.delete(labelId);
    notifyListeners();
  }
}

/**
 * Sync local cache from an array of label IDs (e.g. after fetching the
 * full favorites page elsewhere).
 */
export async function syncCache(labelIds: number[]): Promise<void> {
  if (skipIfRemote()) return;
  try {
    await invoke('v2_sync_cached_favorite_labels', { labelIds });
    favoriteLabelIds = new Set(labelIds);
    notifyListeners();
  } catch (err) {
    console.error('[LabelFavorites] Failed to sync cache:', err);
  }
}

export function markAsFavorite(labelId: number): void {
  if (skipIfRemote()) return;
  if (!favoriteLabelIds.has(labelId)) {
    favoriteLabelIds.add(labelId);
    invoke('v2_cache_favorite_label', { labelId }).catch(() => {});
    notifyListeners();
  }
}

export function unmarkAsFavorite(labelId: number): void {
  if (skipIfRemote()) return;
  if (favoriteLabelIds.has(labelId)) {
    favoriteLabelIds.delete(labelId);
    invoke('v2_uncache_favorite_label', { labelId }).catch(() => {});
    notifyListeners();
  }
}
