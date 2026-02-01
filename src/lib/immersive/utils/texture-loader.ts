/**
 * Texture Loader - Atmospheric Background Generator
 *
 * Creates an ATMOSPHERIC COLOR FIELD from album artwork.
 * The artwork is used ONLY as a color/mood source.
 * The original image must be COMPLETELY UNRECOGNIZABLE.
 *
 * Pipeline (NON-NEGOTIABLE):
 * 1. Crop random NON-CENTERED region (150-300% zoom)
 * 2. Downscale to TINY size (destroys all detail)
 * 3. Apply EXTREME blur (destroys remaining structure)
 * 4. Amplify colors while reducing contrast
 * 5. Upload to GPU ONCE
 *
 * Acceptance Test:
 * "If I hide the album cover, can I tell which album this is?"
 * YES = FAIL, NO = PASS
 */

import { createTextureFromImage } from './webgl-utils';

// Cache of generated atmospheric textures
const atmosphereCache = new Map<string, string>();
const MAX_CACHE_SIZE = 20;

// Active load requests (for cancellation)
const activeLoads = new Map<string, AbortController>();

// Seeded random for consistent crops per image URL
function seededRandom(seed: string): () => number {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) {
    const char = seed.charCodeAt(i);
    hash = ((hash << 5) - hash) + char;
    hash = hash & hash;
  }
  return () => {
    hash = Math.imul(hash ^ (hash >>> 16), 2246822507);
    hash = Math.imul(hash ^ (hash >>> 13), 3266489909);
    hash ^= hash >>> 16;
    return (hash >>> 0) / 4294967296;
  };
}

/**
 * Extract a random NON-CENTERED crop from the image.
 * Zoom: 150-300% (takes a portion, not the whole image)
 * Position: Random, avoiding center to break symmetry
 */
function extractRandomCrop(
  img: HTMLImageElement,
  targetSize: number,
  seed: string
): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = targetSize;
  canvas.height = targetSize;
  const ctx = canvas.getContext('2d');
  if (!ctx) return canvas;

  const rand = seededRandom(seed);

  // Zoom factor: 1.1x to 1.6x (we take 62%-90% of the image for more relevant colors)
  const zoomFactor = 1.1 + rand() * 0.5;

  // Source region size (what portion of original image to take)
  const sourceSize = Math.min(img.width, img.height) / zoomFactor;

  // Available range for crop position (avoid edges)
  const maxOffset = Math.min(img.width, img.height) - sourceSize;

  // Random position, biased AWAY from center
  // Generate position, then push away from center if too close
  let sx = rand() * maxOffset;
  let sy = rand() * maxOffset;

  // Calculate center of crop
  const cropCenterX = sx + sourceSize / 2;
  const cropCenterY = sy + sourceSize / 2;
  const imageCenterX = img.width / 2;
  const imageCenterY = img.height / 2;

  // If crop is too centered, push it away
  const distFromCenter = Math.sqrt(
    Math.pow(cropCenterX - imageCenterX, 2) +
    Math.pow(cropCenterY - imageCenterY, 2)
  );
  const minDistFromCenter = Math.min(img.width, img.height) * 0.15;

  if (distFromCenter < minDistFromCenter && maxOffset > 0) {
    // Push towards a random corner
    const corner = Math.floor(rand() * 4);
    switch (corner) {
      case 0: sx = 0; sy = 0; break; // top-left
      case 1: sx = maxOffset; sy = 0; break; // top-right
      case 2: sx = 0; sy = maxOffset; break; // bottom-left
      case 3: sx = maxOffset; sy = maxOffset; break; // bottom-right
    }
  }

  // Draw the cropped region scaled to target size
  ctx.drawImage(
    img,
    sx, sy, sourceSize, sourceSize,
    0, 0, targetSize, targetSize
  );

  return canvas;
}

/**
 * Apply EXTREME blur to completely destroy any remaining structure.
 * Uses multiple passes and very high blur radius.
 */
function applyExtremeBlur(
  sourceCanvas: HTMLCanvasElement,
  passes: number,
  blurRadius: number
): HTMLCanvasElement {
  let current = sourceCanvas;
  const size = sourceCanvas.width;

  for (let i = 0; i < passes; i++) {
    const next = document.createElement('canvas');
    next.width = size;
    next.height = size;
    const ctx = next.getContext('2d');
    if (!ctx) return current;

    // Apply blur with overpaint to avoid edge artifacts
    ctx.filter = `blur(${blurRadius}px)`;

    // Draw larger and offset to eliminate edge darkening
    const expand = blurRadius * 2;
    ctx.drawImage(
      current,
      -expand, -expand,
      size + expand * 2, size + expand * 2
    );

    current = next;
  }

  return current;
}

/**
 * Apply color adjustments to create atmospheric mood.
 * - Boost saturation (preserve mood)
 * - Reduce brightness (background use)
 * - Reduce contrast (smooth gradients)
 */
function applyColorAdjustments(
  sourceCanvas: HTMLCanvasElement
): HTMLCanvasElement {
  const size = sourceCanvas.width;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext('2d');
  if (!ctx) return sourceCanvas;

  // Saturation boost + brightness/contrast reduction
  ctx.filter = 'saturate(1.4) brightness(0.5) contrast(0.85)';
  ctx.drawImage(sourceCanvas, 0, 0);

  return canvas;
}

/**
 * Add subtle vignette to push focus inward.
 */
function applyVignette(
  sourceCanvas: HTMLCanvasElement,
  intensity: number = 0.3
): HTMLCanvasElement {
  const size = sourceCanvas.width;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext('2d');
  if (!ctx) return sourceCanvas;

  // Draw source
  ctx.drawImage(sourceCanvas, 0, 0);

  // Create radial gradient for vignette
  const gradient = ctx.createRadialGradient(
    size / 2, size / 2, size * 0.2,
    size / 2, size / 2, size * 0.7
  );
  gradient.addColorStop(0, `rgba(0, 0, 0, 0)`);
  gradient.addColorStop(1, `rgba(0, 0, 0, ${intensity})`);

  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, size, size);

  return canvas;
}

/**
 * Generate an atmospheric color field from artwork.
 * The result must be COMPLETELY UNRECOGNIZABLE.
 */
async function generateAtmosphere(
  imageUrl: string,
  signal?: AbortSignal
): Promise<string> {
  // Check cache first
  const cached = atmosphereCache.get(imageUrl);
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
        // === TRANSFORMATION PIPELINE ===

        // Step 1: Extract random NON-CENTERED crop
        // Use small initial size - we're destroying detail anyway
        const INITIAL_SIZE = 64;
        const croppedCanvas = extractRandomCrop(img, INITIAL_SIZE, imageUrl);

        // Step 2: First blur pass at small size (very efficient)
        // High blur radius relative to size destroys all structure
        const smallBlurred = applyExtremeBlur(croppedCanvas, 3, 12);

        // Step 3: Scale up to final size
        const FINAL_SIZE = 256;
        const scaledCanvas = document.createElement('canvas');
        scaledCanvas.width = FINAL_SIZE;
        scaledCanvas.height = FINAL_SIZE;
        const scaledCtx = scaledCanvas.getContext('2d');
        if (!scaledCtx) {
          reject(new Error('Could not get canvas context'));
          return;
        }

        // Bilinear interpolation during scale-up adds more blur
        scaledCtx.imageSmoothingEnabled = true;
        scaledCtx.imageSmoothingQuality = 'high';
        scaledCtx.drawImage(smallBlurred, 0, 0, FINAL_SIZE, FINAL_SIZE);

        // Step 4: Additional blur at final size for smoothness
        const finalBlurred = applyExtremeBlur(scaledCanvas, 2, 20);

        // Step 5: Apply color adjustments
        const colorAdjusted = applyColorAdjustments(finalBlurred);

        // Step 6: Apply subtle vignette
        const final = applyVignette(colorAdjusted, 0.25);

        // Convert to data URL
        const dataUrl = final.toDataURL('image/jpeg', 0.9);

        // Cache the result
        if (atmosphereCache.size >= MAX_CACHE_SIZE) {
          const firstKey = atmosphereCache.keys().next().value;
          if (firstKey) atmosphereCache.delete(firstKey);
        }
        atmosphereCache.set(imageUrl, dataUrl);

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
 * Load image from URL or data URL.
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
 * Load an atmospheric texture from artwork URL.
 *
 * The texture is an UNRECOGNIZABLE color field derived from the artwork.
 * Computed ONCE at load time - no per-frame processing.
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

  const controller = new AbortController();
  activeLoads.set(requestId, controller);

  try {
    // Generate atmospheric color field (cached)
    const atmosphereDataUrl = await generateAtmosphere(
      artworkUrl,
      controller.signal
    );

    // Load as image element
    const img = await loadImage(atmosphereDataUrl, controller.signal);

    // Create texture (uploaded ONCE)
    const texture = createTextureFromImage(gl, img);
    if (!texture) {
      throw new Error('Failed to create texture');
    }

    activeLoads.delete(requestId);

    return {
      texture,
      width: img.width,
      height: img.height,
    };
  } catch (err) {
    activeLoads.delete(requestId);

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
 * Clear the atmosphere cache.
 */
export function clearBlurCache(): void {
  atmosphereCache.clear();
}

/**
 * Get cache statistics.
 */
export function getBlurCacheStats(): { size: number; maxSize: number } {
  return {
    size: atmosphereCache.size,
    maxSize: MAX_CACHE_SIZE,
  };
}
