/**
 * Album Favorites Store
 *
 * Centralized store for tracking favorite albums.
 */

import { invoke } from '@tauri-apps/api/core';
import { logRecoEvent } from '$lib/services/recoService';

let favoriteAlbumIds = new Set<string>();
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

export function getFavoriteAlbumIds(): string[] {
  return Array.from(favoriteAlbumIds);
}

export function isFavoritesLoaded(): boolean {
  return isLoaded;
}

export async function loadAlbumFavorites(): Promise<void> {
  if (isLoading || isLoaded) return;
  isLoading = true;

  try {
    let offset = 0;
    const limit = 500;
    const collected: string[] = [];

    while (true) {
      const result = await invoke<{ albums?: { items: Array<{ id: string }>; total?: number } }>('get_favorites', {
        favType: 'albums',
        limit,
        offset
      });

      const items = result.albums?.items ?? [];
      if (!items.length) break;

      collected.push(...items.map(item => item.id));
      offset += items.length;

      if (items.length < limit) break;
      if (result.albums?.total && offset >= result.albums.total) break;
    }

    favoriteAlbumIds = new Set(collected);
    isLoaded = true;
    notifyListeners();
  } catch (err) {
    console.error('Failed to load album favorites:', err);
  } finally {
    isLoading = false;
  }
}

export async function toggleAlbumFavorite(albumId: string): Promise<boolean> {
  const wasFavorite = favoriteAlbumIds.has(albumId);
  const newState = !wasFavorite;

  if (newState) {
    favoriteAlbumIds.add(albumId);
  } else {
    favoriteAlbumIds.delete(albumId);
  }
  notifyListeners();

  try {
    if (newState) {
      await invoke('add_favorite', { favType: 'album', itemId: albumId });
      void logRecoEvent({
        eventType: 'favorite',
        itemType: 'album',
        albumId
      });
    } else {
      await invoke('remove_favorite', { favType: 'album', itemId: albumId });
    }
    return newState;
  } catch (err) {
    if (newState) {
      favoriteAlbumIds.delete(albumId);
    } else {
      favoriteAlbumIds.add(albumId);
    }
    notifyListeners();
    console.error('Failed to toggle album favorite:', err);
    return wasFavorite;
  }
}

export function markAsFavorite(albumId: string): void {
  if (!favoriteAlbumIds.has(albumId)) {
    favoriteAlbumIds.add(albumId);
    notifyListeners();
  }
}

export function unmarkAsFavorite(albumId: string): void {
  if (favoriteAlbumIds.has(albumId)) {
    favoriteAlbumIds.delete(albumId);
    notifyListeners();
  }
}
