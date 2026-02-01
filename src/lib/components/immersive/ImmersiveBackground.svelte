<script lang="ts">
  /**
   * Immersive Background
   *
   * Renders the blurred album artwork background for the immersive player.
   * Uses WebGL2 when available (with ambient motion), falls back to CSS blur.
   */

  import { onMount } from 'svelte';
  import {
    BUILD_IMMERSIVE_ENABLED,
    isWebGL2Available,
    isRuntimeEnabled,
  } from '$lib/immersive';

  interface Props {
    artwork: string;
    /** Enable ambient motion effect (WebGL2 only) */
    enableAmbient?: boolean;
    /** Ambient motion intensity 0-1 (WebGL2 only) */
    ambientIntensity?: number;
  }

  let { artwork, enableAmbient = true, ambientIntensity }: Props = $props();

  // Determine which renderer to use
  let useWebGL = $state(false);
  let useFallback = $state(false);
  let WebGLCanvas: typeof import('$lib/immersive/ImmersiveAmbientCanvas.svelte').default | null = $state(null);

  // Fallback state (CSS-based blur)
  let canvasRef: HTMLCanvasElement | undefined = $state();
  let isLoading = $state(true);
  let currentArtwork = $state('');

  // Check capabilities and load WebGL component if available
  onMount(async () => {
    if (BUILD_IMMERSIVE_ENABLED && isRuntimeEnabled() && isWebGL2Available()) {
      try {
        // Dynamic import - only loads if WebGL2 is available
        const module = await import('$lib/immersive/ImmersiveAmbientCanvas.svelte');
        WebGLCanvas = module.default;
        useWebGL = true;
        console.log('[ImmersiveBackground] Using WebGL2 renderer');
      } catch (e) {
        console.warn('[ImmersiveBackground] Failed to load WebGL canvas:', e);
        useFallback = true;
      }
    } else {
      useFallback = true;
      console.log('[ImmersiveBackground] Using CSS fallback');
    }
  });

  /**
   * Handle WebGL fallback (if WebGL init fails)
   */
  function handleWebGLFallback(): void {
    console.warn('[ImmersiveBackground] WebGL failed, switching to CSS fallback');
    useWebGL = false;
    useFallback = true;
  }

  // =====================================================
  // CSS Fallback Implementation
  // Uses same atmospheric transformation as WebGL path
  // =====================================================

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

  async function generateBlurredBackground(imageUrl: string): Promise<void> {
    if (!canvasRef || !imageUrl) return;

    const img = new Image();
    img.crossOrigin = 'anonymous';

    img.onload = () => {
      const rand = seededRandom(imageUrl);

      // Step 1: Extract random NON-CENTERED crop at small size
      const CROP_SIZE = 32;
      const cropCanvas = document.createElement('canvas');
      cropCanvas.width = CROP_SIZE;
      cropCanvas.height = CROP_SIZE;
      const cropCtx = cropCanvas.getContext('2d');
      if (!cropCtx) { isLoading = false; return; }

      // Zoom factor: 1.0x to 1.1x (takes 90%-100% for ALL colors)
      const zoomFactor = 1.0 + rand() * 0.1;
      const sourceSize = Math.min(img.width, img.height) / zoomFactor;
      const maxOffset = Math.min(img.width, img.height) - sourceSize;

      // Random position, biased away from center
      let sx = rand() * maxOffset;
      let sy = rand() * maxOffset;

      const cropCenterX = sx + sourceSize / 2;
      const cropCenterY = sy + sourceSize / 2;
      const imageCenterX = img.width / 2;
      const imageCenterY = img.height / 2;

      const distFromCenter = Math.sqrt(
        Math.pow(cropCenterX - imageCenterX, 2) +
        Math.pow(cropCenterY - imageCenterY, 2)
      );
      const minDistFromCenter = Math.min(img.width, img.height) * 0.15;

      if (distFromCenter < minDistFromCenter && maxOffset > 0) {
        const corner = Math.floor(rand() * 4);
        switch (corner) {
          case 0: sx = 0; sy = 0; break;
          case 1: sx = maxOffset; sy = 0; break;
          case 2: sx = 0; sy = maxOffset; break;
          case 3: sx = maxOffset; sy = maxOffset; break;
        }
      }

      cropCtx.drawImage(
        img,
        sx, sy, sourceSize, sourceSize,
        0, 0, CROP_SIZE, CROP_SIZE
      );

      // Step 2: Apply extreme blur at small size (destroys structure)
      let current = cropCanvas;
      for (let i = 0; i < 3; i++) {
        const next = document.createElement('canvas');
        next.width = CROP_SIZE;
        next.height = CROP_SIZE;
        const ctx = next.getContext('2d');
        if (!ctx) continue;
        ctx.filter = 'blur(8px)';
        const expand = 16;
        ctx.drawImage(current, -expand, -expand, CROP_SIZE + expand * 2, CROP_SIZE + expand * 2);
        current = next;
      }

      // Step 3: Scale up to final size with smoothing
      const FINAL_SIZE = 128;
      canvasRef!.width = FINAL_SIZE;
      canvasRef!.height = FINAL_SIZE;
      const finalCtx = canvasRef!.getContext('2d');
      if (!finalCtx) { isLoading = false; return; }

      finalCtx.imageSmoothingEnabled = true;
      finalCtx.imageSmoothingQuality = 'high';
      finalCtx.drawImage(current, 0, 0, FINAL_SIZE, FINAL_SIZE);

      // Step 4: Apply color adjustments via pixel manipulation
      const imageData = finalCtx.getImageData(0, 0, FINAL_SIZE, FINAL_SIZE);
      const data = imageData.data;

      for (let i = 0; i < data.length; i += 4) {
        const r = data[i];
        const g = data[i + 1];
        const b = data[i + 2];
        const avg = (r + g + b) / 3;

        // Saturation boost (1.4x) + brightness reduction (0.5x) + contrast reduction
        const satFactor = 1.4;
        let newR = avg + (r - avg) * satFactor;
        let newG = avg + (g - avg) * satFactor;
        let newB = avg + (b - avg) * satFactor;

        // Brightness and contrast
        newR = newR * 0.5;
        newG = newG * 0.5;
        newB = newB * 0.5;

        data[i] = Math.min(255, Math.max(0, newR));
        data[i + 1] = Math.min(255, Math.max(0, newG));
        data[i + 2] = Math.min(255, Math.max(0, newB));
      }

      finalCtx.putImageData(imageData, 0, 0);
      isLoading = false;
    };

    img.onerror = () => {
      isLoading = false;
    };

    img.src = imageUrl;
  }

  // Track artwork changes for fallback renderer
  $effect(() => {
    if (useFallback && artwork && artwork !== currentArtwork) {
      currentArtwork = artwork;
      isLoading = true;
      generateBlurredBackground(artwork);
    }
  });
</script>

<div class="immersive-background">
  {#if useWebGL && WebGLCanvas}
    <!-- WebGL2 Renderer with ambient motion -->
    <WebGLCanvas
      artworkUrl={artwork}
      enableMotion={enableAmbient}
      intensity={ambientIntensity}
      onFallback={handleWebGLFallback}
    />
  {:else if useFallback}
    <!-- CSS Fallback Renderer -->
    <div class="fallback-container" class:loading={isLoading}>
      <canvas
        bind:this={canvasRef}
        class="background-canvas"
        aria-hidden="true"
      ></canvas>
    </div>
  {:else}
    <!-- Loading state while determining renderer -->
    <div class="loading-placeholder"></div>
  {/if}

  <!-- Dark overlay for better contrast -->
  <div class="dark-overlay"></div>
</div>

<style>
  .immersive-background {
    position: absolute;
    inset: 0;
    overflow: hidden;
    z-index: 0;
    background-color: #0a0a0b;
  }

  /* CSS Fallback styles */
  .fallback-container {
    position: absolute;
    inset: 0;
  }

  .background-canvas {
    position: absolute;
    inset: -120px;
    width: calc(100% + 240px);
    height: calc(100% + 240px);
    image-rendering: auto;
    filter: blur(60px);
    transform: scale(1.2) translateZ(0);
    will-change: opacity;
    transition: opacity 500ms ease-out;
  }

  .loading .background-canvas {
    opacity: 0;
  }

  .loading-placeholder {
    position: absolute;
    inset: 0;
    background-color: #0a0a0b;
  }

  /* Dark overlay */
  .dark-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.15);
    pointer-events: none;
    z-index: 1;
  }
</style>
