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
  let unlistenTransient: UnlistenFn | null = null;
  let unlistenEnergy: UnlistenFn | null = null;
  let isInitialized = false;

  let lastRenderTime = 0;
  const FRAME_INTERVAL = 1000 / 30;

  // Ring particles - expanding circles triggered by transients
  interface Ring {
    x: number;
    y: number;
    radius: number;
    maxRadius: number;
    alpha: number;
    lineWidth: number;
    color: { r: number; g: number; b: number };
    speed: number;
  }

  let rings: Ring[] = [];
  const MAX_RINGS = 12;

  // Current global energy for ambient glow
  let globalEnergy = 0;

  // Colors from artwork
  let pulseColors = $state([
    { r: 255, g: 100, b: 100 },
    { r: 100, g: 200, b: 255 },
    { r: 200, g: 100, b: 255 },
  ]);
  let colorIndex = 0;

  // Album art for canvas
  let artworkImg: HTMLImageElement | null = null;
  let artworkLoaded = false;

  function loadArtwork(src: string) {
    if (!src) return;
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      artworkImg = img;
      artworkLoaded = true;
    };
    img.src = src;
  }

  function extractColors(imgSrc: string) {
    if (!imgSrc) return;

    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      const sampleCanvas = document.createElement('canvas');
      const size = 10;
      sampleCanvas.width = size;
      sampleCanvas.height = size;
      const sampleCtx = sampleCanvas.getContext('2d');
      if (!sampleCtx) return;

      sampleCtx.drawImage(img, 0, 0, size, size);
      const data = sampleCtx.getImageData(0, 0, size, size).data;

      const colors: { r: number; g: number; b: number; sat: number }[] = [];
      for (let idx = 0; idx < data.length; idx += 4) {
        const r = data[idx], g = data[idx + 1], b = data[idx + 2];
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const lum = (max + min) / 2;
        const sat = max === min ? 0 : (max - min) / (lum > 127 ? 510 - max - min : max + min);

        if (lum > 50 && lum < 220 && sat > 0.15) {
          colors.push({ r, g, b, sat });
        }
      }

      if (colors.length >= 2) {
        colors.sort((a, b) => b.sat - a.sat);
        pulseColors = [
          { r: Math.min(255, colors[0].r + 30), g: Math.min(255, colors[0].g + 30), b: Math.min(255, colors[0].b + 30) },
          colors.length > 1
            ? { r: Math.min(255, colors[Math.floor(colors.length / 3)].r + 20), g: Math.min(255, colors[Math.floor(colors.length / 3)].g + 20), b: Math.min(255, colors[Math.floor(colors.length / 3)].b + 20) }
            : { r: 100, g: 200, b: 255 },
          colors.length > 2
            ? { r: Math.min(255, colors[Math.floor(colors.length * 2 / 3)].r + 20), g: Math.min(255, colors[Math.floor(colors.length * 2 / 3)].g + 20), b: Math.min(255, colors[Math.floor(colors.length * 2 / 3)].b + 20) }
            : { r: 200, g: 100, b: 255 },
        ];
      }
    };
    img.src = imgSrc;
  }

  $effect(() => {
    if (artwork) {
      extractColors(artwork);
      loadArtwork(artwork);
    }
  });

  function spawnRing(intensity: number, canvasWidth: number, canvasHeight: number) {
    const centerX = canvasWidth / 2;
    const centerY = canvasHeight / 2;
    const color = pulseColors[colorIndex % pulseColors.length];
    colorIndex++;

    const ring: Ring = {
      x: centerX,
      y: centerY,
      radius: Math.min(canvasWidth, canvasHeight) * 0.12,
      maxRadius: Math.min(canvasWidth, canvasHeight) * (0.35 + intensity * 0.25),
      alpha: 0.6 + intensity * 0.4,
      lineWidth: 2 + intensity * 4,
      color,
      speed: 3 + intensity * 5,
    };

    rings.push(ring);
    if (rings.length > MAX_RINGS) {
      rings.shift();
    }
  }

  async function init() {
    if (!canvasRef || isInitialized) return;

    ctx = canvasRef.getContext('2d');
    if (!ctx) return;

    isInitialized = true;

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
    } catch (e) {
      console.error('[TransientPulse] Failed to enable backend:', e);
    }

    unlistenTransient = await listen<number[]>('viz:transient', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload) && canvasRef) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length >= 1) {
          const rect = canvasRef.getBoundingClientRect();
          spawnRing(floats[0], rect.width, rect.height);
        }
      }
    });

    unlistenEnergy = await listen<number[]>('viz:energy', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length >= 5) {
          globalEnergy = (floats[0] + floats[1] + floats[2] + floats[3] + floats[4]) / 5;
        }
      }
    });

    render(0);
  }

  function render(timestamp: number = 0) {
    if (!ctx || !canvasRef) return;

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

    const targetWidth = Math.floor(width * dpr);
    const targetHeight = Math.floor(height * dpr);
    if (canvasRef.width !== targetWidth || canvasRef.height !== targetHeight) {
      canvasRef.width = targetWidth;
      canvasRef.height = targetHeight;
      ctx.scale(dpr, dpr);
    }

    // Clear
    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, width, height);

    const centerX = width / 2;
    const centerY = height / 2 - 20;
    const minDim = Math.min(width, height);
    const artSize = minDim * 0.28;
    const artHalf = artSize / 2;

    // Draw rings (expanding outward)
    for (let i = rings.length - 1; i >= 0; i--) {
      const ring = rings[i];
      ring.radius += ring.speed;
      ring.alpha *= 0.96; // Fade out

      if (ring.alpha < 0.01 || ring.radius > ring.maxRadius * 1.5) {
        rings.splice(i, 1);
        continue;
      }

      const progress = ring.radius / ring.maxRadius;
      const fadeAlpha = ring.alpha * (1 - progress * 0.5);

      ctx.beginPath();
      ctx.arc(ring.x, ring.y, ring.radius, 0, Math.PI * 2);
      ctx.strokeStyle = `rgba(${ring.color.r}, ${ring.color.g}, ${ring.color.b}, ${fadeAlpha})`;
      ctx.lineWidth = ring.lineWidth * (1 - progress * 0.6);
      ctx.shadowColor = `rgba(${ring.color.r}, ${ring.color.g}, ${ring.color.b}, ${fadeAlpha * 0.6})`;
      ctx.shadowBlur = 12;
      ctx.stroke();
      ctx.shadowBlur = 0;
    }

    // Ambient glow behind artwork
    if (globalEnergy > 0.02) {
      const glowColor = pulseColors[0];
      const gradient = ctx.createRadialGradient(centerX, centerY, artHalf * 0.5, centerX, centerY, artHalf * 2);
      gradient.addColorStop(0, `rgba(${glowColor.r}, ${glowColor.g}, ${glowColor.b}, ${globalEnergy * 0.15})`);
      gradient.addColorStop(1, 'rgba(0, 0, 0, 0)');
      ctx.fillStyle = gradient;
      ctx.fillRect(0, 0, width, height);
    }

    // Draw album art in center
    if (artworkImg && artworkLoaded) {
      ctx.save();
      const cornerRadius = 8;
      ctx.beginPath();
      ctx.moveTo(centerX - artHalf + cornerRadius, centerY - artHalf);
      ctx.lineTo(centerX + artHalf - cornerRadius, centerY - artHalf);
      ctx.arcTo(centerX + artHalf, centerY - artHalf, centerX + artHalf, centerY - artHalf + cornerRadius, cornerRadius);
      ctx.lineTo(centerX + artHalf, centerY + artHalf - cornerRadius);
      ctx.arcTo(centerX + artHalf, centerY + artHalf, centerX + artHalf - cornerRadius, centerY + artHalf, cornerRadius);
      ctx.lineTo(centerX - artHalf + cornerRadius, centerY + artHalf);
      ctx.arcTo(centerX - artHalf, centerY + artHalf, centerX - artHalf, centerY + artHalf - cornerRadius, cornerRadius);
      ctx.lineTo(centerX - artHalf, centerY - artHalf + cornerRadius);
      ctx.arcTo(centerX - artHalf, centerY - artHalf, centerX - artHalf + cornerRadius, centerY - artHalf, cornerRadius);
      ctx.closePath();
      ctx.clip();
      ctx.drawImage(artworkImg, centerX - artHalf, centerY - artHalf, artSize, artSize);
      ctx.restore();
    }

    animationFrame = requestAnimationFrame(render);
  }

  async function cleanup() {
    if (animationFrame) {
      cancelAnimationFrame(animationFrame);
      animationFrame = null;
    }
    if (unlistenTransient) {
      unlistenTransient();
      unlistenTransient = null;
    }
    if (unlistenEnergy) {
      unlistenEnergy();
      unlistenEnergy = null;
    }
    try {
      await invoke('v2_set_visualizer_enabled', { enabled: false });
    } catch (e) {
      console.error('[TransientPulse] Failed to disable backend:', e);
    }
    rings.length = 0;
    isInitialized = false;
  }

  onMount(() => {
    if (enabled) init();
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

<div class="transient-pulse-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="transient-canvas"></canvas>

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

<style>
  .transient-pulse-panel {
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

  .transient-pulse-panel.visible {
    opacity: 1;
  }

  .transient-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .track-info {
    position: absolute;
    bottom: 150px;
    left: 0;
    right: 0;
    z-index: 10;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 4px;
  }

  .track-title {
    font-size: clamp(18px, 2.5vw, 24px);
    font-weight: 700;
    color: var(--text-primary, white);
    margin: 0;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.5);
  }

  .track-artist {
    font-size: clamp(13px, 1.8vw, 16px);
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    margin: 0;
  }

  .track-album {
    font-size: clamp(11px, 1.4vw, 13px);
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    margin: 0;
    font-style: italic;
  }

  .quality-badge-wrapper {
    margin-top: 8px;
  }

  @media (max-width: 768px) {
    .track-info {
      bottom: 130px;
    }
  }
</style>
