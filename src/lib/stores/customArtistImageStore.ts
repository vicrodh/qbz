/**
 * Custom Artist Image Store
 *
 * Manages global resolution of user-uploaded custom artist images.
 * Loaded on login, exposes resolveArtistImage() for all views.
 */

import { invoke, convertFileSrc } from '@tauri-apps/api/core';

// ============ State ============

/** Map of normalized artist name -> asset URL (convertFileSrc'd) */
let imageMap: Map<string, string> = new Map();

/** Map of normalized name -> original name (for reverse lookup) */
let nameMap: Map<string, string> = new Map();

const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

// ============ Normalization ============

/**
 * Normalize artist name for matching.
 * Lowercase, strip diacritics, strip non-alphanumeric.
 * Handles "Björk" vs "Bjork", "Héctor Lavoe" vs "Hector Lavoe", etc.
 */
function normalize(name: string): string {
  return name
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]/g, '');
}

// ============ Public API ============

/**
 * Load all custom artist images from backend
 */
export async function initCustomArtistImageStore(): Promise<void> {
  const mapping = await invoke<Record<string, string>>(
    'v2_library_get_all_custom_artist_images'
  );

  imageMap = new Map();
  nameMap = new Map();

  for (const [artistName, filePath] of Object.entries(mapping)) {
    const key = normalize(artistName);
    imageMap.set(key, convertFileSrc(filePath));
    nameMap.set(key, artistName);
  }

  notifyListeners();
}

/**
 * Resolve artist image: returns custom URL if set, otherwise the default.
 */
export function resolveArtistImage(
  artistName: string,
  defaultUrl: string
): string {
  const key = normalize(artistName);
  return imageMap.get(key) ?? defaultUrl;
}

/**
 * Check if an artist has a custom image override.
 */
export function hasCustomImage(artistName: string): boolean {
  return imageMap.has(normalize(artistName));
}

/**
 * Update store after user sets a custom image (no backend call).
 */
export function setCustomImage(artistName: string, assetUrl: string): void {
  const key = normalize(artistName);
  imageMap.set(key, assetUrl);
  nameMap.set(key, artistName);
  notifyListeners();
}

/**
 * Update store after user removes a custom image (no backend call).
 */
export function removeCustomImage(artistName: string): void {
  const key = normalize(artistName);
  imageMap.delete(key);
  nameMap.delete(key);
  notifyListeners();
}

/**
 * Clear all custom artist images (on logout).
 */
export function clearCustomArtistImages(): void {
  imageMap = new Map();
  nameMap = new Map();
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
