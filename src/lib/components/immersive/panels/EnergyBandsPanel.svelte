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

  const NUM_BANDS = 5;
  const bandNames = ['Sub', 'Bass', 'Mid', 'Pres', 'Air'];
  const energyData = new Float32Array(NUM_BANDS);

  let lastRenderTime = 0;
  const FRAME_INTERVAL = 1000 / 60;

  let bandColors = $state([
    { r: 255, g: 60, b: 60 },
    { r: 255, g: 160, b: 40 },
    { r: 80, g: 220, b: 120 },
    { r: 60, g: 160, b: 255 },
    { r: 180, g: 100, b: 255 },
  ]);

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

      const colors: { r: number; g: number; b: number; hue: number }[] = [];
      for (let idx = 0; idx < data.length; idx += 4) {
        const r = data[idx], g = data[idx + 1], b = data[idx + 2];
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const lum = (max + min) / 2;
        const sat = max === min ? 0 : (max - min) / (lum > 127 ? 510 - max - min : max + min);

        if (lum > 40 && lum < 230 && sat > 0.1) {
          let hue = 0;
          if (max !== min) {
            const d = max - min;
            if (max === r) hue = ((g - b) / d + (g < b ? 6 : 0)) / 6;
            else if (max === g) hue = ((b - r) / d + 2) / 6;
            else hue = ((r - g) / d + 4) / 6;
          }
          colors.push({ r, g, b, hue });
        }
      }

      if (colors.length >= 2) {
        colors.sort((a, b) => a.hue - b.hue);
        for (let i = 0; i < NUM_BANDS; i++) {
          const cidx = Math.floor((i / NUM_BANDS) * colors.length);
          const c = colors[cidx];
          bandColors[i] = {
            r: Math.min(255, Math.floor(c.r * 1.2 + 30)),
            g: Math.min(255, Math.floor(c.g * 1.2 + 30)),
            b: Math.min(255, Math.floor(c.b * 1.2 + 30)),
          };
        }
      }
    };
    img.src = imgSrc;
  }

  $effect(() => {
    if (artwork) {
      extractColors(artwork);
    }
  });

  async function init() {
    if (!canvasRef || isInitialized) return;

    ctx = canvasRef.getContext('2d');
    if (!ctx) return;

    isInitialized = true;

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
    } catch (e) {
      console.error('[EnergyBands] Failed to enable backend:', e);
    }

    unlisten = await listen<number[]>('viz:energy', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length === NUM_BANDS) {
          energyData.set(floats);
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

    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, width, height);

    const centerX = width / 2;
    const centerY = height / 2;
    const minDim = Math.min(width, height);

    // Concentric glowing rings (no album art)
    for (let i = 0; i < NUM_BANDS; i++) {
      const energy = energyData[i];
      const baseRadius = 40 + (NUM_BANDS - 1 - i) * (minDim * 0.07);
      const pulseRadius = baseRadius + energy * minDim * 0.05;

      const color = bandColors[i];
      const alpha = 0.15 + energy * 0.6;
      const glowSize = 4 + energy * 16;

      ctx.beginPath();
      ctx.arc(centerX, centerY, pulseRadius, 0, Math.PI * 2);
      ctx.closePath();

      ctx.strokeStyle = `rgba(${color.r}, ${color.g}, ${color.b}, ${alpha})`;
      ctx.lineWidth = 2 + energy * 3;
      ctx.shadowColor = `rgba(${color.r}, ${color.g}, ${color.b}, ${alpha * 0.8})`;
      ctx.shadowBlur = glowSize;
      ctx.stroke();
      ctx.shadowBlur = 0;
    }

    // Ambient center glow based on total energy
    const totalEnergy = energyData.reduce((sum, val) => sum + val, 0) / NUM_BANDS;
    if (totalEnergy > 0.05) {
      const gradient = ctx.createRadialGradient(centerX, centerY, 10, centerX, centerY, minDim * 0.15);
      const c = bandColors[1];
      gradient.addColorStop(0, `rgba(${c.r}, ${c.g}, ${c.b}, ${totalEnergy * 0.25})`);
      gradient.addColorStop(1, 'rgba(0, 0, 0, 0)');
      ctx.fillStyle = gradient;
      ctx.fillRect(0, 0, width, height);
    }

    // Band labels
    ctx.font = '10px monospace';
    ctx.textAlign = 'center';
    for (let i = 0; i < NUM_BANDS; i++) {
      const color = bandColors[i];
      const alpha = 0.3 + energyData[i] * 0.5;
      ctx.fillStyle = `rgba(${color.r}, ${color.g}, ${color.b}, ${alpha})`;
      const labelX = centerX - (NUM_BANDS * 30) / 2 + i * 30 + 15;
      const outerRadius = 40 + (NUM_BANDS - 1) * (minDim * 0.07) + minDim * 0.05;
      const labelY = centerY + outerRadius + 20;
      ctx.fillText(bandNames[i], labelX, Math.min(labelY, height - 40));
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
    } catch (e) {
      console.error('[EnergyBands] Failed to disable backend:', e);
    }
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

<div class="energy-bands-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="energy-canvas"></canvas>

  <div class="bottom-info">
    <div class="track-meta">
      <span class="track-title">{trackTitle}</span>
      {#if album}
        <span class="track-album">{album}</span>
      {/if}
      <span class="track-artist">{artist}</span>
      <QualityBadge {quality} {bitDepth} {samplingRate} {originalBitDepth} {originalSamplingRate} {format} compact />
    </div>
    {#if artwork}
      <div class="artwork-thumb">
        <img src={artwork} alt={trackTitle} />
      </div>
    {/if}
  </div>
</div>

<style>
  .energy-bands-panel {
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

  .energy-bands-panel.visible {
    opacity: 1;
  }

  .energy-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .bottom-info {
    position: absolute;
    bottom: 24px;
    right: 24px;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .track-meta {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
  }

  .track-title {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary, white);
    text-shadow: 0 1px 6px rgba(0, 0, 0, 0.4);
    max-width: 400px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-album {
    font-size: 12px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    font-style: italic;
    max-width: 400px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-artist {
    font-size: 12px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    max-width: 400px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .artwork-thumb {
    width: 72px;
    height: 72px;
    border-radius: 6px;
    overflow: hidden;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    flex-shrink: 0;
  }

  .artwork-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  @media (max-width: 768px) {
    .bottom-info {
      right: 16px;
      bottom: 16px;
    }

    .artwork-thumb {
      width: 56px;
      height: 56px;
    }
  }
</style>
