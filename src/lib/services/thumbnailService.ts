import { invoke, convertFileSrc } from '@tauri-apps/api/core';

// Cache for thumbnail URLs (artwork_path -> thumbnail_url)
const thumbnailCache = new Map<string, string>();

// Pending requests to avoid duplicate calls
const pendingRequests = new Map<string, Promise<string>>();

/**
 * Get a thumbnail URL for an artwork file.
 * Generates the thumbnail on first request and caches subsequent calls.
 */
export async function getThumbnailUrl(artworkPath: string): Promise<string> {
  // Return cached URL if available
  if (thumbnailCache.has(artworkPath)) {
    return thumbnailCache.get(artworkPath)!;
  }

  // If there's already a pending request for this artwork, wait for it
  if (pendingRequests.has(artworkPath)) {
    return pendingRequests.get(artworkPath)!;
  }

  // Create a new request
  const requestPromise = (async () => {
    try {
      // Call backend to get/generate thumbnail
      const thumbnailPath = await invoke<string>('library_get_thumbnail', {
        artworkPath,
      });

      // Convert to asset URL
      const url = convertFileSrc(thumbnailPath);
      thumbnailCache.set(artworkPath, url);
      return url;
    } catch (error) {
      console.error('Failed to get thumbnail:', error);
      // Fall back to original artwork URL
      const fallbackUrl = convertFileSrc(artworkPath);
      thumbnailCache.set(artworkPath, fallbackUrl);
      return fallbackUrl;
    } finally {
      pendingRequests.delete(artworkPath);
    }
  })();

  pendingRequests.set(artworkPath, requestPromise);
  return requestPromise;
}

/**
 * Get thumbnail URL synchronously if cached, otherwise return null.
 * Useful for initial render while async loading completes.
 */
export function getCachedThumbnailUrl(artworkPath: string): string | null {
  return thumbnailCache.get(artworkPath) ?? null;
}

/**
 * Preload thumbnails for a batch of artwork paths.
 * Useful for preloading visible items.
 */
export async function preloadThumbnails(artworkPaths: string[]): Promise<void> {
  await Promise.all(
    artworkPaths
      .filter(path => path && !thumbnailCache.has(path))
      .map(path => getThumbnailUrl(path))
  );
}

/**
 * Clear the thumbnail cache.
 */
export function clearThumbnailCache(): void {
  thumbnailCache.clear();
}

/**
 * Clear thumbnails on the backend and frontend.
 */
export async function clearAllThumbnails(): Promise<void> {
  await invoke('v2_library_clear_thumbnails_cache');
  thumbnailCache.clear();
}

/**
 * Get the size of the thumbnails cache in bytes.
 */
export async function getThumbnailsCacheSize(): Promise<number> {
  return invoke<number>('library_get_thumbnails_cache_size');
}

/**
 * Format cache size for display.
 */
export function formatCacheSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}
