/**
 * Svelte action for cached images.
 *
 * Usage:
 *   <img use:cachedSrc={imageUrl} alt="..." />
 *
 * Hides the image until the cached URL is resolved, preventing
 * broken image icons. The placeholder behind it remains visible.
 */

import { getCachedImageUrl } from '$lib/services/imageCacheService';

export function cachedSrc(node: HTMLImageElement, url: string | undefined) {
  let currentUrl = url;

  // Force own compositing layer to prevent WebKitGTK 2.50+ texture
  // eviction during repaints (causes placeholder flash on hover/scroll)
  node.style.willChange = 'transform';
  node.style.transform = 'translateZ(0)';

  // Hide until resolved so placeholder shows through
  node.style.opacity = '0';
  node.removeAttribute('src');

  async function resolve(imageUrl: string | undefined) {
    if (!imageUrl) {
      node.removeAttribute('src');
      node.style.opacity = '0';
      return;
    }

    node.style.opacity = '0';

    try {
      const resolved = await getCachedImageUrl(imageUrl);
      if (imageUrl === currentUrl) {
        node.src = resolved;
        node.style.opacity = '1';
      }
    } catch {
      if (imageUrl === currentUrl) {
        node.src = imageUrl;
        node.style.opacity = '1';
      }
    }
  }

  resolve(url);

  return {
    update(newUrl: string | undefined) {
      if (newUrl !== currentUrl) {
        currentUrl = newUrl;
        resolve(newUrl);
      }
    }
  };
}
