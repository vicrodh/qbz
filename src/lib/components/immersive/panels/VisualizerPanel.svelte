<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import QualityBadge from '$lib/components/QualityBadge.svelte';

  interface Props {
    enabled?: boolean;
    artwork?: string;
    trackTitle?: string;
    artist?: string;
    album?: string;
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    originalBitDepth?: number;
    originalSamplingRate?: number;
    format?: string;
  }

  let {
    enabled = true,
    artwork = '',
    trackTitle = '',
    artist = '',
    album = '',
    quality,
    bitDepth,
    samplingRate,
    originalBitDepth,
    originalSamplingRate,
    format
  }: Props = $props();

  let canvasRef: HTMLCanvasElement | null = $state(null);
  let ctx: CanvasRenderingContext2D | null = null;
  let animationFrame: number | null = null;
  let unlisten: UnlistenFn | null = null;
  let isInitialized = false;

  const NUM_BARS = 16; // Backend sends 16, we mirror for 32 visual bars
  const frequencyData = new Float32Array(NUM_BARS);
  const smoothedData = new Float32Array(NUM_BARS);

  // Smoothing for visual continuity
  const SMOOTHING = 0.6;

  // Throttle rendering to 30fps max
  let lastRenderTime = 0;
  const FRAME_INTERVAL = 1000 / 30; // ~33ms

  // Colors extracted from artwork (Material You style)
  let colorPrimary = $state({ r: 0, g: 220, b: 200 });   // Default cyan
  let colorSecondary = $state({ r: 150, g: 50, b: 255 }); // Default purple

  // Extract dominant colors from artwork
  function extractColors(imgSrc: string) {
    if (!imgSrc) return;

    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      // Use tiny canvas for fast sampling
      const sampleCanvas = document.createElement('canvas');
      const size = 10;
      sampleCanvas.width = size;
      sampleCanvas.height = size;
      const sampleCtx = sampleCanvas.getContext('2d');
      if (!sampleCtx) return;

      sampleCtx.drawImage(img, 0, 0, size, size);
      const data = sampleCtx.getImageData(0, 0, size, size).data;

      // Collect vibrant colors (avoid black/dark and very light)
      const colors: { r: number; g: number; b: number; sat: number }[] = [];
      for (let i = 0; i < data.length; i += 4) {
        const r = data[i], g = data[i + 1], b = data[i + 2];
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const lum = (max + min) / 2;
        const sat = max === min ? 0 : (max - min) / (lum > 127 ? 510 - max - min : max + min);

        // Skip black/near-black (lum > 60) and very light (lum < 220)
        // Also require decent saturation for vibrant colors
        if (lum > 60 && lum < 220 && sat > 0.15) {
          colors.push({ r, g, b, sat });
        }
      }

      if (colors.length >= 2) {
        // Sort by saturation, pick most vibrant
        colors.sort((a, b) => b.sat - a.sat);
        colorPrimary = { r: colors[0].r, g: colors[0].g, b: colors[0].b };
        // Pick a contrasting color (from the other half)
        const midIdx = Math.floor(colors.length / 2);
        colorSecondary = { r: colors[midIdx].r, g: colors[midIdx].g, b: colors[midIdx].b };
      } else if (colors.length === 1) {
        colorPrimary = { r: colors[0].r, g: colors[0].g, b: colors[0].b };
        // Create secondary by shifting hue
        colorSecondary = { r: colors[0].b, g: colors[0].r, b: colors[0].g };
      }
    };
    img.src = imgSrc;
  }

  // Re-extract colors when artwork changes
  $effect(() => {
    if (artwork) {
      extractColors(artwork);
    }
  });

  async function init() {
    if (!canvasRef || isInitialized) return;

    ctx = canvasRef.getContext('2d');
    if (!ctx) {
      console.warn('Canvas 2D not available');
      return;
    }

    isInitialized = true;

    // Enable backend
    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
      console.log('[Visualizer] Backend enabled');
    } catch (e) {
      console.error('[Visualizer] Failed to enable backend:', e);
    }

    // Listen for frequency data
    unlisten = await listen<number[]>('viz:data', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length === NUM_BARS) {
          // Apply smoothing
          for (let i = 0; i < NUM_BARS; i++) {
            smoothedData[i] = smoothedData[i] * SMOOTHING + floats[i] * (1 - SMOOTHING);
          }
          frequencyData.set(smoothedData);
        }
      }
    });

    render(0);
  }

  function render(timestamp: number = 0) {
    if (!ctx || !canvasRef) return;

    // Throttle to 30fps
    const delta = timestamp - lastRenderTime;
    if (delta < FRAME_INTERVAL) {
      animationFrame = requestAnimationFrame(render);
      return;
    }
    lastRenderTime = timestamp;

    const rect = canvasRef.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const width = rect.width;
    const height = rect.height;

    // Only resize canvas when needed (expensive operation)
    const targetWidth = Math.floor(width * dpr);
    const targetHeight = Math.floor(height * dpr);
    if (canvasRef.width !== targetWidth || canvasRef.height !== targetHeight) {
      canvasRef.width = targetWidth;
      canvasRef.height = targetHeight;
      ctx.scale(dpr, dpr);
    }

    // Clear with black
    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, width, height);

    // Bar visualization with cubes - mirrored from center
    const visualBars = NUM_BARS * 2; // 16 real + 16 mirrored = 32 visual
    const barGap = 4;
    const totalBarWidth = width / visualBars;
    const barWidth = totalBarWidth - barGap;
    const cubeHeight = 6;
    const cubeGap = 2;
    const maxBarHeight = height * 0.7;
    const baseY = height * 0.85;
    const centerX = width / 2;

    // Draw bars from edges inward (mirrored, bass at edges)
    for (let i = 0; i < NUM_BARS; i++) {
      const amplitude = frequencyData[i];
      const barHeight = amplitude * maxBarHeight;
      const numCubes = Math.floor(barHeight / (cubeHeight + cubeGap));

      // Left side (from left edge toward center)
      const xLeft = i * totalBarWidth + barGap / 2;
      // Right side (from right edge toward center)
      const xRight = width - (i + 1) * totalBarWidth + barGap / 2;

      for (let j = 0; j < numCubes; j++) {
        const cubeY = baseY - (j + 1) * (cubeHeight + cubeGap);
        const gradientRatio = Math.min(j / 20, 1); // Normalized 0-1

        // Interpolate between primary (bottom) and secondary (top)
        const r = Math.floor(colorPrimary.r + gradientRatio * (colorSecondary.r - colorPrimary.r));
        const g = Math.floor(colorPrimary.g + gradientRatio * (colorSecondary.g - colorPrimary.g));
        const b = Math.floor(colorPrimary.b + gradientRatio * (colorSecondary.b - colorPrimary.b));

        ctx.fillStyle = `rgb(${r}, ${g}, ${b})`;
        ctx.fillRect(xLeft, cubeY, barWidth, cubeHeight);
        ctx.fillRect(xRight, cubeY, barWidth, cubeHeight);
      }
    }

    animationFrame = requestAnimationFrame(render);
  }

  async function cleanup() {
    if (animationFrame) {
      cancelAnimationFrame(animationFrame);
      animationFrame = null;
    }

    if (unlisten) {
      unlisten();
      unlisten = null;
    }

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: false });
      console.log('[Visualizer] Backend disabled');
    } catch (e) {
      console.error('[Visualizer] Failed to disable backend:', e);
    }

    isInitialized = false;
  }

  onMount(() => {
    if (enabled) {
      init();
    }
    return cleanup;
  });

  $effect(() => {
    if (enabled && !isInitialized) {
      init();
    } else if (!enabled && isInitialized) {
      cleanup();
    }
  });
</script>

<div class="visualizer-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="visualizer-canvas"></canvas>

  <div class="center-content">
    {#if artwork}
      <div class="artwork-container">
        <img src={artwork} alt={trackTitle} class="artwork" />
      </div>
    {/if}

    <div class="track-info">
      <h1 class="track-title">{trackTitle}</h1>
      <p class="track-artist">{artist}</p>
      {#if album}
        <p class="track-album">{album}</p>
      {/if}
      <div class="quality-badge-wrapper">
        <QualityBadge {quality} {bitDepth} {samplingRate} {originalBitDepth} {originalSamplingRate} {format} />
      </div>
    </div>
  </div>
</div>

<style>
  .visualizer-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 300ms ease;
    z-index: 5;
    background: #000000;
  }

  .visualizer-panel.visible {
    opacity: 1;
  }

  .visualizer-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .center-content {
    position: relative;
    z-index: 10;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 20px;
    padding-top: 70px;
    padding-bottom: 120px;
  }

  .artwork-container {
    width: min(45vh, 360px);
    height: min(45vh, 360px);
    border-radius: 8px;
    overflow: hidden;
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.5),
      0 20px 60px rgba(0, 0, 0, 0.3);
  }

  .artwork {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .track-info {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 6px;
    max-width: 600px;
  }

  .track-title {
    font-size: clamp(20px, 3vw, 28px);
    font-weight: 700;
    color: var(--text-primary, white);
    margin: 0;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.3);
  }

  .track-artist {
    font-size: clamp(14px, 2vw, 18px);
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    margin: 0;
  }

  .track-album {
    font-size: clamp(12px, 1.5vw, 14px);
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    margin: 0;
    font-style: italic;
  }

  .quality-badge-wrapper {
    margin-top: 12px;
  }

  @media (max-width: 768px) {
    .center-content {
      padding: 70px 24px 130px;
      gap: 16px;
    }

    .artwork-container {
      width: min(55vw, 280px);
      height: min(55vw, 280px);
    }
  }

  @media (max-height: 600px) {
    .artwork-container {
      width: min(32vh, 220px);
      height: min(32vh, 220px);
    }

    .track-info {
      gap: 4px;
    }
  }
</style>
