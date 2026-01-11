/**
 * Favorites Store
 *
 * Centralized store for tracking favorite tracks.
 * Components can check if a track is favorite and toggle status without prop drilling.
 */

import { invoke } from '@tauri-apps/api/core';
import { logRecoEvent } from '$lib/services/recoService';

// State
let favoriteTrackIds = new Set<number>();
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
 * Load favorites from backend (call once on app init)
 */
export async function loadFavorites(): Promise<void> {
  if (isLoading || isLoaded) return;

  isLoading = true;
  try {
    const response = await invoke<{ tracks: { items: Array<{ id: number }> } }>('get_favorites', {
      favType: 'track',
      limit: 500
    });

    favoriteTrackIds = new Set(response.tracks.items.map(t => t.id));
    isLoaded = true;
    notifyListeners();
  } catch (err) {
    console.error('Failed to load favorites:', err);
  } finally {
    isLoading = false;
  }
}

/**
 * Toggle favorite status for a track
 * Returns the new favorite state
 */
export async function toggleTrackFavorite(trackId: number): Promise<boolean> {
  const wasFavorite = favoriteTrackIds.has(trackId);
  const newState = !wasFavorite;

  // Optimistic update
  if (newState) {
    favoriteTrackIds.add(trackId);
  } else {
    favoriteTrackIds.delete(trackId);
  }
  notifyListeners();

  try {
    if (newState) {
      await invoke('add_favorite', { favType: 'track', itemId: String(trackId) });
      void logRecoEvent({
        eventType: 'favorite',
        itemType: 'track',
        trackId
      });
    } else {
      await invoke('remove_favorite', { favType: 'track', itemId: String(trackId) });
    }
    return newState;
  } catch (err) {
    // Rollback on error
    if (newState) {
      favoriteTrackIds.delete(trackId);
    } else {
      favoriteTrackIds.add(trackId);
    }
    notifyListeners();
    console.error('Failed to toggle favorite:', err);
    return wasFavorite;
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
 * Manually add a track ID to the local set (for when we add favorites elsewhere)
 */
export function markAsFavorite(trackId: number): void {
  if (!favoriteTrackIds.has(trackId)) {
    favoriteTrackIds.add(trackId);
    notifyListeners();
  }
}

/**
 * Manually remove a track ID from the local set
 */
export function unmarkAsFavorite(trackId: number): void {
  if (favoriteTrackIds.has(trackId)) {
    favoriteTrackIds.delete(trackId);
    notifyListeners();
  }
}

/**
 * Reset store (for testing or logout)
 */
export function reset(): void {
  favoriteTrackIds = new Set();
  isLoaded = false;
  isLoading = false;
  notifyListeners();
}
