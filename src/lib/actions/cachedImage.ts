/**
 * Svelte action for cached images.
 *
 * Usage:
 *   <img use:cachedSrc={imageUrl} alt="..." />
 *
 * Hides the image until the cached URL is resolved, preventing
 * broken image icons. The placeholder behind it remains visible.
 */

import { getCachedImageUrl, getResolvedIfCached } from '$lib/services/imageCacheService';
import { isHardwareAccelEnabled } from '$lib/runtime/graphicsState';

export function cachedSrc(node: HTMLImageElement, url: string | undefined) {
  let currentUrl = url;

  // WebKitGTK 2.50+ evicts <img> textures from its compositor cache during
  // rapid repaints (hover, scroll) which manifests as a flash of the
  // placeholder background. Forcing the image into its own compositor
  // layer prevents the eviction. We only apply this under hardware
  // acceleration: under software compositing each "layer" is just an
  // extra CPU framebuffer, which makes the cure worse than the disease
  // for big card grids (confirmed 2026-05-12 perf session). See ADR-004.
  if (isHardwareAccelEnabled()) {
    node.style.willChange = 'transform';
    node.style.transform = 'translateZ(0)';
  }

  // Sync cache hit? Set the src immediately, no opacity dance. This is
  // the common path when preloadImages has already primed the map
  // (collection detail views do this on mount), and it's what removes
  // the per-card "dark placeholder flash" during fast scroll.
  function applyImmediately(imageUrl: string | undefined) {
    if (!imageUrl) {
      node.removeAttribute('src');
      node.style.opacity = '0';
      return;
    }
    const pre = getResolvedIfCached(imageUrl);
    if (pre) {
      node.src = pre;
      node.style.opacity = '1';
      return;
    }
    // Fall back to the original URL so the WebView can start fetching
    // in parallel with the backend proxy call. If the proxy returns a
    // better resolved URL (e.g. disk-cached asset://) we swap it in.
    // Opacity stays 1 throughout — an HTTPS image loading from its
    // real URL while we also check for a local copy is still better
    // UX than a flash of nothing.
    node.src = imageUrl;
    node.style.opacity = '1';
  }

  applyImmediately(url);

  async function resolve(imageUrl: string | undefined) {
    if (!imageUrl || getResolvedIfCached(imageUrl)) return;
    try {
      const resolved = await getCachedImageUrl(imageUrl);
      if (imageUrl === currentUrl && resolved !== imageUrl) {
        node.src = resolved;
      }
    } catch {
      // Silent — node.src is already set to the raw URL above.
    }
  }

  void resolve(url);

  return {
    update(newUrl: string | undefined) {
      if (newUrl !== currentUrl) {
        currentUrl = newUrl;
        applyImmediately(newUrl);
        void resolve(newUrl);
      }
    }
  };
}
