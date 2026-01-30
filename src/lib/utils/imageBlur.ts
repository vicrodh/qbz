/**
 * Optimized Image Blur Utility
 *
 * Pre-computes blurred background images for the immersive player.
 * Uses a small canvas (256x256) with blur which, when scaled up, produces
 * a smooth background effect with minimal GPU cost.
 *
 * Performance comparison:
 * - CSS blur(100px) on 600x600 image: ~15-30% GPU, constant
 * - Pre-computed 64x64 blur: ~0.1% GPU, one-time computation
 */

// Cache to avoid re-computing for same artwork
const blurCache = new Map<string, string>();
const MAX_CACHE_SIZE = 20;

/**
 * Generate a pre-blurred background image from artwork URL.
 * Returns a data URL that can be used as background-image.
 *
 * @param artworkUrl - The original artwork URL
 * @param size - Output canvas size (default 512, higher = smoother result)
 * @param blurRadius - Canvas blur radius (default 40, produces smooth gradient effect)
 * @returns Promise<string> - Data URL of blurred image
 */
export async function generateBlurredBackground(
  artworkUrl: string,
  size: number = 128,
  blurRadius: number = 60
): Promise<string> {
  // Check cache first
  const cacheKey = `${artworkUrl}-${size}-${blurRadius}`;
  const cached = blurCache.get(cacheKey);
  if (cached) {
    return cached;
  }

  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';

    img.onload = () => {
      try {
        const canvas = document.createElement('canvas');
        canvas.width = size;
        canvas.height = size;

        const ctx = canvas.getContext('2d');
        if (!ctx) {
          reject(new Error('Could not get canvas context'));
          return;
        }

        // Apply blur filter before drawing
        ctx.filter = `blur(${blurRadius}px) saturate(1.2) brightness(0.5)`;

        // Draw image scaled down to canvas size
        // Offset by blur radius to avoid edge artifacts
        const offset = blurRadius;
        const drawSize = size + offset * 2;
        ctx.drawImage(img, -offset, -offset, drawSize, drawSize);

        // Convert to data URL with moderate quality (blur hides compression artifacts)
        const dataUrl = canvas.toDataURL('image/jpeg', 0.7);

        // Cache the result
        if (blurCache.size >= MAX_CACHE_SIZE) {
          // Remove oldest entry
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
      reject(new Error(`Failed to load image: ${artworkUrl}`));
    };

    // Start loading
    img.src = artworkUrl;
  });
}

/**
 * Extract dominant colors from an image for gradient backgrounds.
 * Alternative to blur - uses solid gradient which is even lighter on GPU.
 *
 * @param artworkUrl - The original artwork URL
 * @returns Promise<string[]> - Array of hex colors (2-3 colors)
 */
export async function extractDominantColors(artworkUrl: string): Promise<string[]> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = 'anonymous';

    img.onload = () => {
      try {
        const canvas = document.createElement('canvas');
        // Use tiny canvas for color sampling
        canvas.width = 10;
        canvas.height = 10;

        const ctx = canvas.getContext('2d');
        if (!ctx) {
          resolve(['#1a1a2e', '#16213e']); // Fallback colors
          return;
        }

        ctx.drawImage(img, 0, 0, 10, 10);
        const imageData = ctx.getImageData(0, 0, 10, 10).data;

        // Sample corners and center for color variety
        const samples = [
          [0, 0],     // top-left
          [9, 0],     // top-right
          [0, 9],     // bottom-left
          [9, 9],     // bottom-right
          [4, 4],     // center
        ];

        const colors: string[] = [];
        for (const [x, y] of samples) {
          const idx = (y * 10 + x) * 4;
          const r = imageData[idx];
          const g = imageData[idx + 1];
          const b = imageData[idx + 2];

          // Darken colors for background use
          const darkenFactor = 0.4;
          const dr = Math.round(r * darkenFactor);
          const dg = Math.round(g * darkenFactor);
          const db = Math.round(b * darkenFactor);

          const hex = `#${dr.toString(16).padStart(2, '0')}${dg.toString(16).padStart(2, '0')}${db.toString(16).padStart(2, '0')}`;
          colors.push(hex);
        }

        // Return unique colors (dedupe similar ones)
        const uniqueColors = [...new Set(colors)].slice(0, 3);
        resolve(uniqueColors.length >= 2 ? uniqueColors : ['#1a1a2e', '#16213e']);
      } catch (err) {
        resolve(['#1a1a2e', '#16213e']); // Fallback on error
      }
    };

    img.onerror = () => {
      resolve(['#1a1a2e', '#16213e']); // Fallback colors
    };

    img.src = artworkUrl;
  });
}

/**
 * Generate CSS gradient from dominant colors.
 * Ultra-lightweight alternative to blur backgrounds.
 *
 * @param colors - Array of hex colors
 * @returns CSS gradient string
 */
export function createGradientFromColors(colors: string[]): string {
  if (colors.length === 1) {
    return colors[0];
  }

  if (colors.length === 2) {
    return `linear-gradient(135deg, ${colors[0]} 0%, ${colors[1]} 100%)`;
  }

  // 3+ colors: radial gradient for more interesting effect
  return `radial-gradient(ellipse at 30% 20%, ${colors[0]} 0%, transparent 50%),
          radial-gradient(ellipse at 70% 80%, ${colors[1]} 0%, transparent 50%),
          linear-gradient(135deg, ${colors[2] || colors[0]} 0%, ${colors[0]} 100%)`;
}

/**
 * Clear the blur cache (e.g., on logout or memory pressure)
 */
export function clearBlurCache(): void {
  blurCache.clear();
}

/**
 * Get cache statistics for debugging
 */
export function getBlurCacheStats(): { size: number; maxSize: number } {
  return {
    size: blurCache.size,
    maxSize: MAX_CACHE_SIZE,
  };
}
