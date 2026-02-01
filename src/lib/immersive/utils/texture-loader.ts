/**
 * Texture Loader
 *
 * Handles async loading of artwork textures with pre-blur processing.
 * Supports request cancellation for rapid track changes.
 */

import { createTextureFromImage } from './webgl-utils';

// Cache of loaded blurred images (data URLs)
const blurCache = new Map<string, string>();
const MAX_CACHE_SIZE = 20;

// Active load requests (for cancellation)
const activeLoads = new Map<string, AbortController>();

/**
 * Generate a pre-blurred version of an image.
 * Uses canvas 2D for blur (one-time CPU cost, then GPU texture).
 *
 * @param imageUrl - Source image URL
 * @param size - Output size (smaller = more blur when scaled)
 * @param blurRadius - Canvas filter blur radius
 * @param signal - AbortSignal for cancellation
 */
async function generateBlurredImage(
  imageUrl: string,
  size: number = 64,
  blurRadius: number = 20,
  signal?: AbortSignal
): Promise<string> {
  // Check cache first
  const cacheKey = `${imageUrl}-${size}-${blurRadius}`;
  const cached = blurCache.get(cacheKey);
  if (cached) return cached;

  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new DOMException('Aborted', 'AbortError'));
      return;
    }

    const img = new Image();
    img.crossOrigin = 'anonymous';

    const abortHandler = () => {
      img.src = '';
      reject(new DOMException('Aborted', 'AbortError'));
    };

    signal?.addEventListener('abort', abortHandler);

    img.onload = () => {
      signal?.removeEventListener('abort', abortHandler);

      if (signal?.aborted) {
        reject(new DOMException('Aborted', 'AbortError'));
        return;
      }

      try {
        // Multi-pass blur to match CSS blur(40px) quality
        // Use larger canvases to allow proper blur spreading

        // Step 1: Draw image to source canvas
        const sourceCanvas = document.createElement('canvas');
        sourceCanvas.width = size;
        sourceCanvas.height = size;
        const sourceCtx = sourceCanvas.getContext('2d');
        if (!sourceCtx) {
          reject(new Error('Could not get source canvas context'));
          return;
        }
        sourceCtx.drawImage(img, 0, 0, size, size);

        // Step 2: First blur pass (large canvas to capture blur overflow)
        const blurCanvas1 = document.createElement('canvas');
        const expandedSize = size + blurRadius * 4;
        blurCanvas1.width = expandedSize;
        blurCanvas1.height = expandedSize;
        const blurCtx1 = blurCanvas1.getContext('2d');
        if (!blurCtx1) {
          reject(new Error('Could not get blur canvas context'));
          return;
        }

        // Apply first blur pass with color adjustments
        blurCtx1.filter = `blur(${blurRadius}px) saturate(1.3) brightness(0.55)`;
        blurCtx1.drawImage(sourceCanvas, blurRadius * 2, blurRadius * 2);

        // Step 3: Second blur pass
        const blurCanvas2 = document.createElement('canvas');
        blurCanvas2.width = expandedSize;
        blurCanvas2.height = expandedSize;
        const blurCtx2 = blurCanvas2.getContext('2d');
        if (!blurCtx2) {
          reject(new Error('Could not get blur canvas 2 context'));
          return;
        }

        blurCtx2.filter = `blur(${blurRadius}px)`;
        blurCtx2.drawImage(blurCanvas1, 0, 0);

        // Step 4: Third blur pass for maximum smoothness
        const blurCanvas3 = document.createElement('canvas');
        blurCanvas3.width = expandedSize;
        blurCanvas3.height = expandedSize;
        const blurCtx3 = blurCanvas3.getContext('2d');
        if (!blurCtx3) {
          reject(new Error('Could not get blur canvas 3 context'));
          return;
        }

        blurCtx3.filter = `blur(${Math.floor(blurRadius / 2)}px)`;
        blurCtx3.drawImage(blurCanvas2, 0, 0);

        // Step 5: Crop to final size
        const finalCanvas = document.createElement('canvas');
        finalCanvas.width = size;
        finalCanvas.height = size;
        const finalCtx = finalCanvas.getContext('2d');
        if (!finalCtx) {
          reject(new Error('Could not get final canvas context'));
          return;
        }

        // Extract center portion
        const offset = blurRadius * 2;
        finalCtx.drawImage(
          blurCanvas3,
          offset, offset, size, size,
          0, 0, size, size
        );

        // Convert to data URL
        const dataUrl = finalCanvas.toDataURL('image/jpeg', 0.7);

        // Cache the result
        if (blurCache.size >= MAX_CACHE_SIZE) {
          const firstKey = blurCache.keys().next().value;
          if (firstKey) blurCache.delete(firstKey);
        }
        blurCache.set(cacheKey, dataUrl);

        resolve(dataUrl);
      } catch (err) {
        reject(err);
      }
    };

    img.onerror = () => {
      signal?.removeEventListener('abort', abortHandler);
      reject(new Error(`Failed to load image: ${imageUrl}`));
    };

    img.src = imageUrl;
  });
}

/**
 * Load an image from a URL or data URL.
 */
function loadImage(src: string, signal?: AbortSignal): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new DOMException('Aborted', 'AbortError'));
      return;
    }

    const img = new Image();
    img.crossOrigin = 'anonymous';

    const abortHandler = () => {
      img.src = '';
      reject(new DOMException('Aborted', 'AbortError'));
    };

    signal?.addEventListener('abort', abortHandler);

    img.onload = () => {
      signal?.removeEventListener('abort', abortHandler);
      resolve(img);
    };

    img.onerror = () => {
      signal?.removeEventListener('abort', abortHandler);
      reject(new Error(`Failed to load image: ${src}`));
    };

    img.src = src;
  });
}

export interface LoadTextureResult {
  texture: WebGLTexture;
  width: number;
  height: number;
}

/**
 * Load a blurred texture from artwork URL.
 *
 * Process:
 * 1. Generate pre-blurred image (cached)
 * 2. Load as HTMLImageElement
 * 3. Upload to WebGL texture
 *
 * Cancels any previous load for the same request ID.
 *
 * @param gl - WebGL2 context
 * @param artworkUrl - URL of the artwork
 * @param requestId - Unique ID for this load request (for cancellation)
 */
export async function loadBlurredTexture(
  gl: WebGL2RenderingContext,
  artworkUrl: string,
  requestId: string = 'default'
): Promise<LoadTextureResult | null> {
  // Cancel any previous load with the same request ID
  const existingController = activeLoads.get(requestId);
  if (existingController) {
    existingController.abort();
  }

  // Create new abort controller
  const controller = new AbortController();
  activeLoads.set(requestId, controller);

  try {
    // Generate pre-processed image
    // Use 256x256 for good quality - GPU shader will handle the blur
    const blurredDataUrl = await generateBlurredImage(
      artworkUrl,
      256,  // Higher quality source for GPU blur
      8,    // Light pre-blur, GPU shader does the heavy lifting
      controller.signal
    );

    // Load as image element
    const img = await loadImage(blurredDataUrl, controller.signal);

    // Create texture
    const texture = createTextureFromImage(gl, img);
    if (!texture) {
      throw new Error('Failed to create texture');
    }

    // Clean up active load
    activeLoads.delete(requestId);

    return {
      texture,
      width: img.width,
      height: img.height,
    };
  } catch (err) {
    activeLoads.delete(requestId);

    // Don't log abort errors - they're expected during rapid track changes
    if (err instanceof DOMException && err.name === 'AbortError') {
      return null;
    }

    console.warn('[TextureLoader] Failed to load texture:', err);
    return null;
  }
}

/**
 * Cancel all active texture loads.
 */
export function cancelAllLoads(): void {
  for (const controller of activeLoads.values()) {
    controller.abort();
  }
  activeLoads.clear();
}

/**
 * Clear the blur cache.
 */
export function clearBlurCache(): void {
  blurCache.clear();
}

/**
 * Get cache statistics for debugging.
 */
export function getBlurCacheStats(): { size: number; maxSize: number } {
  return {
    size: blurCache.size,
    maxSize: MAX_CACHE_SIZE,
  };
}
