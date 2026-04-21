/**
 * Image Cache Service
 *
 * Proxies Qobuz image URLs through the Rust backend cache.
 * Uses an in-memory map to avoid repeated invoke calls for the same URL.
 * When cache is disabled, returns the original URL immediately.
 */

import { invoke } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';

// In-memory URL map: original URL -> resolved URL (cached path or original)
const resolvedUrls = new Map<string, string>();

// Track pending requests to avoid duplicate downloads
const pendingRequests = new Map<string, Promise<string>>();

// Whether the backend cache is enabled (loaded once, updated on settings change)
let cacheEnabled: boolean | null = null;

/**
 * Resolve an image URL through the backend cache.
 * Returns a file:// URL if cached, or the original URL if cache is disabled.
 */
export async function getCachedImageUrl(url: string): Promise<string> {
  if (!url || url.startsWith('file://') || url.startsWith('asset://')) {
    return url;
  }

  // Check in-memory cache first
  const cached = resolvedUrls.get(url);
  if (cached) return cached;

  // Check if there's already a pending request for this URL
  const pending = pendingRequests.get(url);
  if (pending !== undefined) return pending;

  const promise = (async () => {
    try {
      const result = await invoke<string>('v2_get_cached_image', { url });
      // Convert file:// paths to Tauri asset URLs for WebView access
      let resolved: string;
      if (result.startsWith('file://')) {
        const filePath = result.slice(7); // Remove file:// prefix
        resolved = convertFileSrc(filePath);
      } else {
        resolved = result;
      }
      resolvedUrls.set(url, resolved);
      return resolved;
    } catch {
      // Backend failed to proxy — use original URL as last resort.
      // This may fail on AppImage distros with broken GnuTLS but
      // is better than showing nothing on distros where WebKit works.
      resolvedUrls.set(url, url);
      return url;
    } finally {
      pendingRequests.delete(url);
    }
  })();

  pendingRequests.set(url, promise);
  return promise;
}

/**
 * Synchronous peek into the in-memory URL map. Returns the resolved
 * (disk-cached) URL if this image has already been fetched this
 * session; returns undefined otherwise.
 *
 * Used by the cachedSrc action to avoid an `await` microtask — when
 * the map is warm (preloaded at page load or previously resolved)
 * the <img>'s src is set synchronously on mount instead of flashing
 * a placeholder while a Promise resolves. asset:// and file:// URLs
 * short-circuit the pre-check and return themselves, since they
 * never need backend proxying.
 */
export function getResolvedIfCached(url: string): string | undefined {
  if (!url) return undefined;
  if (url.startsWith('file://') || url.startsWith('asset://')) return url;
  return resolvedUrls.get(url);
}

/**
 * Clear the in-memory URL map (e.g., when cache is cleared from settings).
 */
export function clearResolvedUrls(): void {
  resolvedUrls.clear();
}

/**
 * Preload a batch of image URLs into the cache.
 * Fires and forgets — doesn't block rendering.
 */
export function preloadImages(urls: string[]): void {
  for (const url of urls) {
    if (url && !resolvedUrls.has(url) && !url.startsWith('file://') && !url.startsWith('asset://')) {
      getCachedImageUrl(url).catch(() => {});
    }
  }
}
