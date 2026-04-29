<script lang="ts">
  /**
   * Immersive Background
   *
   * Renders the blurred album artwork background for the immersive player.
   * Three modes (set via getConfig().backgroundMode, with watchdog
   * degradation):
   *
   *   - 'full' — Kawarp WebGL renderer (Kawase blur + domain warping).
   *              The default and the look the marketing copy promises.
   *   - 'lite' — CSS-transform of a pre-blurred bitmap. Cheap, uses the
   *              compositor only. Fallback for memory-constrained hosts
   *              (Pi-class) and the watchdog's degradation target.
   *   - 'off'  — Solid color extracted from the cover. The most muted
   *              option, for users who want the immersive view without
   *              any motion at all.
   *
   * Kawarp parameter tuning is the result of an A/B benchmark against
   * the previous WebGL2 path on the Static panel — see the POC notes
   * for the full reasoning. Short version: demo defaults from
   * kawarp.boidu.dev with three colour-side adjustments so cover-driven
   * palette dominates the look instead of a global tint.
   */

  import { onMount, onDestroy } from 'svelte';
  import { Kawarp } from '@kawarp/core';
  import {
    isRuntimeEnabled,
    getConfig,
    generateAtmosphere,
  } from '$lib/immersive';
  import type { BackgroundMode } from '$lib/immersive';

  interface Props {
    artwork: string;
  }

  let { artwork }: Props = $props();

  let backgroundMode: BackgroundMode = $state('full');
  let useKawarp = $state(false);
  let useSolidColor = $state(false);
  let useLiteMode = $state(false);
  let solidColor = $state('#0a0a0b');
  let liteImageUrl = $state('');

  // Kawarp state
  let kawarpCanvas: HTMLCanvasElement | undefined = $state();
  let kawarp: Kawarp | undefined;
  let kawarpResizeObserver: ResizeObserver | undefined;
  let lastLoadedArtwork: string | null = null;

  // CSS-mode crossfade state (lite/off rely on this; kawarp does its own
  // crossfade via transitionDuration so it doesn't touch isTransitioning).
  let isTransitioning = $state(false);
  let previousArtwork = $state('');

  /**
   * Switch to a degraded background mode (called by performance watchdog).
   */
  async function switchToMode(mode: BackgroundMode): Promise<void> {
    console.log(`[ImmersiveBackground] Switching to mode: ${mode}`);

    // Tear down kawarp if it was active — switching away from full means
    // we don't need the WebGL context anymore.
    if (kawarp) {
      kawarpResizeObserver?.disconnect();
      kawarpResizeObserver = undefined;
      kawarp.dispose();
      kawarp = undefined;
      lastLoadedArtwork = null;
    }

    // Reset all mode flags
    useKawarp = false;
    useSolidColor = false;
    useLiteMode = false;
    backgroundMode = mode;

    if (mode === 'off') {
      useSolidColor = true;
      if (artwork) extractDominantColor(artwork);
    } else if (mode === 'lite') {
      useLiteMode = true;
      if (artwork) {
        try {
          liteImageUrl = await generateAtmosphere(artwork);
        } catch (err) {
          console.warn('[ImmersiveBackground] Lite mode texture failed:', err);
        }
      }
    } else if (mode === 'full') {
      useKawarp = true;
      // The canvas-bind effect below will pick up the new mount and
      // initialise kawarp once the canvas is in the DOM.
    }
  }

  function handleBackgroundDegraded(event: Event): void {
    const detail = (event as CustomEvent).detail;
    if (detail?.to) {
      switchToMode(detail.to as BackgroundMode);
    }
  }

  onMount(async () => {
    window.addEventListener('immersive:background-degraded', handleBackgroundDegraded);

    const config = getConfig();
    backgroundMode = config.backgroundMode ?? 'full';

    if (backgroundMode === 'off') {
      useSolidColor = true;
      console.log('[ImmersiveBackground] Background mode: off (solid color)');
      if (artwork) extractDominantColor(artwork);
      return;
    }

    if (backgroundMode === 'lite') {
      useLiteMode = true;
      console.log('[ImmersiveBackground] Background mode: lite (CSS transform)');
      if (artwork) {
        try {
          liteImageUrl = await generateAtmosphere(artwork);
        } catch (err) {
          console.warn('[ImmersiveBackground] Lite mode texture failed:', err);
        }
      }
      return;
    }

    // full mode → kawarp. The runtime gate is intentionally loose:
    // BUILD_IMMERSIVE_ENABLED was the historical gate for the bespoke
    // WebGL2 renderer; kawarp is part of the standard build, so all we
    // need is the immersive runtime to be enabled.
    if (isRuntimeEnabled()) {
      useKawarp = true;
      console.log('[ImmersiveBackground] Background mode: full (Kawarp)');
    } else {
      // Runtime disabled — fall back to lite. Solid colour is too austere
      // for users who didn't explicitly opt into 'off'.
      useLiteMode = true;
      console.log('[ImmersiveBackground] Background mode: lite (runtime disabled)');
      if (artwork) {
        try {
          liteImageUrl = await generateAtmosphere(artwork);
        } catch (err) {
          console.warn('[ImmersiveBackground] Lite mode texture failed:', err);
        }
      }
    }
  });

  // Mount kawarp once the canvas element is bound. Re-runs if the canvas
  // gets re-mounted (e.g. after a switchToMode round trip).
  $effect(() => {
    if (!useKawarp || !kawarpCanvas || kawarp) return;

    try {
      kawarp = new Kawarp(kawarpCanvas, {
        // See KawarpPanel notes for the rationale on each value.
        warpIntensity: 1.0,
        blurPasses: 7,
        animationSpeed: 1.15,
        transitionDuration: 1000,
        saturation: 1.5,
        tintIntensity: 0.06,
        dithering: 0.008,
        scale: 1.0,
      });
    } catch (err) {
      console.warn('[ImmersiveBackground] Kawarp init failed, falling back to lite:', err);
      kawarp = undefined;
      switchToMode('lite');
      return;
    }

    if (artwork) {
      lastLoadedArtwork = artwork;
      kawarp.loadImage(artwork).catch((e) =>
        console.warn('[ImmersiveBackground] Kawarp loadImage failed:', e),
      );
    }
    kawarp.start();

    kawarpResizeObserver = new ResizeObserver(() => kawarp?.resize());
    kawarpResizeObserver.observe(kawarpCanvas);
  });

  // Track artwork changes. Kawarp does its own crossfade (transitionDuration
  // ms) so we don't trigger the CSS opacity dance for it; lite / off /
  // legacy CSS modes still go through the manual fade.
  $effect(() => {
    if (artwork === previousArtwork) return;

    if (useKawarp && kawarp && artwork && artwork !== lastLoadedArtwork) {
      lastLoadedArtwork = artwork;
      kawarp.loadImage(artwork).catch((e) =>
        console.warn('[ImmersiveBackground] Kawarp loadImage failed:', e),
      );
      previousArtwork = artwork;
      return;
    }

    if (artwork && artwork !== previousArtwork) {
      isTransitioning = true;
      setTimeout(async () => {
        previousArtwork = artwork;

        if (useSolidColor) {
          extractDominantColor(artwork);
        }

        if (useLiteMode) {
          try {
            liteImageUrl = await generateAtmosphere(artwork);
          } catch (err) {
            console.warn('[ImmersiveBackground] Lite mode texture update failed:', err);
          }
        }

        setTimeout(() => {
          isTransitioning = false;
        }, 100);
      }, 400);
    }
  });

  onDestroy(() => {
    window.removeEventListener('immersive:background-degraded', handleBackgroundDegraded);
    kawarpResizeObserver?.disconnect();
    kawarp?.dispose();
    kawarp = undefined;
  });

  // =====================================================
  // Solid Color Mode (blur disabled)
  // Extracts dominant color from artwork at tiny resolution
  // =====================================================

  function extractDominantColor(imageUrl: string): void {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      const canvas = document.createElement('canvas');
      canvas.width = 4;
      canvas.height = 4;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;
      ctx.drawImage(img, 0, 0, 4, 4);
      const data = ctx.getImageData(0, 0, 4, 4).data;
      let rSum = 0, gSum = 0, bSum = 0;
      const pixels = 16;
      for (let i = 0; i < data.length; i += 4) {
        rSum += data[i];
        gSum += data[i + 1];
        bSum += data[i + 2];
      }
      const darken = 0.3;
      const r = Math.round((rSum / pixels) * darken);
      const g = Math.round((gSum / pixels) * darken);
      const b = Math.round((bSum / pixels) * darken);
      solidColor = `rgb(${r}, ${g}, ${b})`;
    };
    img.src = imageUrl;
  }
</script>

<div class="immersive-background">
  <div class="background-layer" class:transitioning={isTransitioning}>
    {#if useSolidColor}
      <!-- Solid color mode (off) -->
      <div class="solid-background" style="background-color: {solidColor}"></div>
    {:else if useLiteMode}
      <!-- Lite mode: pre-blurred image with CSS transform animation -->
      {#if liteImageUrl}
        <div
          class="lite-background"
          style="background-image: url({liteImageUrl})"
          aria-hidden="true"
        ></div>
      {/if}
    {:else if useKawarp}
      <!-- Full mode: kawarp WebGL renderer (mounted by $effect once bound) -->
      <canvas
        bind:this={kawarpCanvas}
        class="kawarp-canvas"
        aria-hidden="true"
      ></canvas>
    {:else}
      <!-- Loading state while determining renderer -->
      <div class="loading-placeholder"></div>
    {/if}
  </div>

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

  /* Crossfade transition layer (used only by lite / off modes — kawarp
     handles its own crossfade internally via transitionDuration). */
  .background-layer {
    position: absolute;
    inset: 0;
    opacity: 1;
    transition: opacity 400ms ease-out;
  }

  .background-layer.transitioning {
    opacity: 0;
  }

  .kawarp-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    /* Composite on its own GPU layer to avoid WebKitGTK repainting
       surrounding chrome on every animation frame — same trick the
       1.2.10 sticky-header fix used. */
    will-change: transform;
    transform: translateZ(0);
  }

  .solid-background {
    position: absolute;
    inset: 0;
    transition: background-color 500ms ease-out;
  }

  /* Lite mode: pre-blurred image with compositor-driven CSS animation */
  .lite-background {
    position: absolute;
    inset: -80px;
    width: calc(100% + 160px);
    height: calc(100% + 160px);
    background-size: cover;
    background-position: center;
    animation: lite-drift 40s ease-in-out infinite alternate;
    will-change: transform;
  }

  @keyframes lite-drift {
    0%   { transform: scale(1.15) translate(0, 0) rotate(0deg); }
    33%  { transform: scale(1.20) translate(15px, -10px) rotate(1deg); }
    66%  { transform: scale(1.12) translate(-10px, 12px) rotate(-0.5deg); }
    100% { transform: scale(1.18) translate(8px, -8px) rotate(0.5deg); }
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
