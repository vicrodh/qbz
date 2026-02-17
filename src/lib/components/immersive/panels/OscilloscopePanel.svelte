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

  // Throttle rendering to 30fps max
  let lastRenderTime = 0;
  const FRAME_INTERVAL = 1000 / 30;

  // Colors extracted from artwork
  let colorLeft = $state({ r: 0, g: 220, b: 160 });    // Default green-tinted
  let colorRight = $state({ r: 80, g: 140, b: 255 });   // Default blue-tinted

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
        // Left: most vibrant color, tinted green
        const c1 = colors[0];
        colorLeft = { r: Math.floor(c1.r * 0.6), g: Math.min(255, Math.floor(c1.g * 1.3)), b: Math.floor(c1.b * 0.7) };
        // Right: contrasting color, tinted blue
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
    if (!ctx) {
      console.warn('[Oscilloscope] Canvas 2D not available');
      return;
    }

    isInitialized = true;

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
      console.log('[Oscilloscope] Backend enabled');
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

    // Reset shadow for next draw
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

    // Clear with black
    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, width, height);

    // Constrain drawing area to 85% of height, centered
    const drawHeight = height * 0.85;
    const yOffset = (height - drawHeight) / 2;

    // Layout: L in top 45%, R in bottom 45%, center 10% for divider
    const topZone = drawHeight * 0.45;
    const bottomZoneStart = drawHeight * 0.55;
    const amplitude = topZone * 0.7; // max waveform excursion

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
      console.log('[Oscilloscope] Backend disabled');
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
