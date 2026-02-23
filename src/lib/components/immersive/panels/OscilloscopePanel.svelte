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

  const WAVEFORM_POINTS = 256;
  const leftChannel = new Float32Array(WAVEFORM_POINTS);
  const rightChannel = new Float32Array(WAVEFORM_POINTS);
  const smoothedLeft = new Float32Array(WAVEFORM_POINTS);
  const smoothedRight = new Float32Array(WAVEFORM_POINTS);

  const SMOOTHING = 0.3;

  let lastRenderTime = 0;
  const FRAME_INTERVAL = 1000 / 30;

  // Colors extracted from artwork
  let colorLeft = $state({ r: 0, g: 220, b: 160 });
  let colorRight = $state({ r: 80, g: 140, b: 255 });

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

        if (lum > 60 && lum < 220 && sat > 0.15) {
          colors.push({ r, g, b, sat });
        }
      }

      if (colors.length >= 2) {
        colors.sort((a, b) => b.sat - a.sat);
        const c1 = colors[0];
        colorLeft = { r: Math.floor(c1.r * 0.6), g: Math.min(255, Math.floor(c1.g * 1.3)), b: Math.floor(c1.b * 0.7) };
        const midIdx = Math.floor(colors.length / 2);
        const c2 = colors[midIdx];
        colorRight = { r: Math.floor(c2.r * 0.6), g: Math.floor(c2.g * 0.7), b: Math.min(255, Math.floor(c2.b * 1.3)) };
      } else if (colors.length === 1) {
        const c = colors[0];
        colorLeft = { r: Math.floor(c.r * 0.6), g: Math.min(255, Math.floor(c.g * 1.3)), b: Math.floor(c.b * 0.7) };
        colorRight = { r: Math.floor(c.b * 0.6), g: Math.floor(c.r * 0.7), b: Math.min(255, Math.floor(c.g * 1.3)) };
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
      console.error('[Oscilloscope] Failed to enable backend:', e);
    }

    unlisten = await listen<number[]>('viz:waveform', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length === WAVEFORM_POINTS * 2) {
          for (let i = 0; i < WAVEFORM_POINTS; i++) {
            smoothedLeft[i] = smoothedLeft[i] * SMOOTHING + floats[i] * (1 - SMOOTHING);
            smoothedRight[i] = smoothedRight[i] * SMOOTHING + floats[WAVEFORM_POINTS + i] * (1 - SMOOTHING);
          }
          leftChannel.set(smoothedLeft);
          rightChannel.set(smoothedRight);
        }
      }
    });

    render(0);
  }

  function drawWaveform(
    width: number,
    yCenter: number,
    amplitude: number,
    channelData: Float32Array,
    color: { r: number; g: number; b: number }
  ) {
    if (!ctx) return;

    const colorStr = `rgb(${color.r}, ${color.g}, ${color.b})`;
    const glowStr = `rgba(${color.r}, ${color.g}, ${color.b}, 0.4)`;

    ctx.beginPath();
    ctx.strokeStyle = colorStr;
    ctx.lineWidth = 2;
    ctx.shadowColor = glowStr;
    ctx.shadowBlur = 8;

    for (let i = 0; i < WAVEFORM_POINTS; i++) {
      const x = (i / (WAVEFORM_POINTS - 1)) * width;
      const y = yCenter + channelData[i] * amplitude;
      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        ctx.lineTo(x, y);
      }
    }
    ctx.stroke();

    ctx.shadowBlur = 0;
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

    // Constrain drawing area to 85% of height, centered
    const drawHeight = height * 0.85;
    const yOffset = (height - drawHeight) / 2;

    const topZone = drawHeight * 0.45;
    const bottomZoneStart = drawHeight * 0.55;
    const amplitude = topZone * 0.7;

    // Draw L channel waveform
    drawWaveform(width, yOffset + topZone / 2, amplitude, leftChannel, colorLeft);

    // Draw R channel waveform
    drawWaveform(width, yOffset + bottomZoneStart + topZone / 2, amplitude, rightChannel, colorRight);

    // Center divider line
    ctx.beginPath();
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.15)';
    ctx.lineWidth = 1;
    ctx.shadowBlur = 0;
    ctx.moveTo(0, yOffset + drawHeight / 2);
    ctx.lineTo(width, yOffset + drawHeight / 2);
    ctx.stroke();

    // Channel labels
    ctx.font = '11px monospace';
    ctx.fillStyle = `rgba(${colorLeft.r}, ${colorLeft.g}, ${colorLeft.b}, 0.6)`;
    ctx.fillText('L', 12, yOffset + topZone / 2 - amplitude * 0.5 - 8);
    ctx.fillStyle = `rgba(${colorRight.r}, ${colorRight.g}, ${colorRight.b}, 0.6)`;
    ctx.fillText('R', 12, yOffset + bottomZoneStart + topZone / 2 - amplitude * 0.5 - 8);

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
      console.error('[Oscilloscope] Failed to disable backend:', e);
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

<div class="oscilloscope-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="oscilloscope-canvas"></canvas>

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
  .oscilloscope-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    opacity: 0;
    transition: opacity 300ms ease;
    z-index: 5;
    background: #000000;
  }

  .oscilloscope-panel.visible {
    opacity: 1;
  }

  .oscilloscope-canvas {
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
