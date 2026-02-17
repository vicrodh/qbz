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

  // Trail buffer: store previous frames for fading trail effect
  const TRAIL_LENGTH = 8;
  const trailBuffers: { left: Float32Array; right: Float32Array }[] = [];
  for (let i = 0; i < TRAIL_LENGTH; i++) {
    trailBuffers.push({
      left: new Float32Array(WAVEFORM_POINTS),
      right: new Float32Array(WAVEFORM_POINTS),
    });
  }
  let trailIndex = 0;

  let lastRenderTime = 0;
  const FRAME_INTERVAL = 1000 / 30;

  // Primary color extracted from artwork
  let plotColor = $state({ r: 0, g: 220, b: 200 });

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

      let bestSat = 0;
      let bestColor = { r: 0, g: 220, b: 200 };
      for (let idx = 0; idx < data.length; idx += 4) {
        const r = data[idx], g = data[idx + 1], b = data[idx + 2];
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const lum = (max + min) / 2;
        const sat = max === min ? 0 : (max - min) / (lum > 127 ? 510 - max - min : max + min);

        if (lum > 60 && lum < 230 && sat > bestSat) {
          bestSat = sat;
          bestColor = {
            r: Math.min(255, Math.floor(r * 1.2)),
            g: Math.min(255, Math.floor(g * 1.2)),
            b: Math.min(255, Math.floor(b * 1.2)),
          };
        }
      }
      plotColor = bestColor;
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
      console.error('[Lissajous] Failed to enable backend:', e);
    }

    unlisten = await listen<number[]>('viz:waveform', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length === WAVEFORM_POINTS * 2) {
          // Store in trail buffer before updating current
          trailBuffers[trailIndex].left.set(leftChannel);
          trailBuffers[trailIndex].right.set(rightChannel);
          trailIndex = (trailIndex + 1) % TRAIL_LENGTH;

          for (let i = 0; i < WAVEFORM_POINTS; i++) {
            leftChannel[i] = floats[i];
            rightChannel[i] = floats[WAVEFORM_POINTS + i];
          }
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

    // Semi-transparent clear for natural trail
    ctx.fillStyle = 'rgba(0, 0, 0, 0.15)';
    ctx.fillRect(0, 0, width, height);

    const centerX = width / 2;
    const centerY = height / 2;
    const scale = Math.min(width, height) * 0.38;

    // Draw trail (older frames, fading)
    for (let frame = 0; frame < TRAIL_LENGTH; frame++) {
      const bufIdx = (trailIndex + frame) % TRAIL_LENGTH;
      const buf = trailBuffers[bufIdx];
      const age = (TRAIL_LENGTH - frame) / TRAIL_LENGTH;
      const alpha = age * 0.08;

      ctx.beginPath();
      ctx.strokeStyle = `rgba(${plotColor.r}, ${plotColor.g}, ${plotColor.b}, ${alpha})`;
      ctx.lineWidth = 1;

      for (let i = 0; i < WAVEFORM_POINTS; i++) {
        const x = centerX + buf.left[i] * scale;
        const y = centerY - buf.right[i] * scale;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.stroke();
    }

    // Draw current frame (bright)
    ctx.beginPath();
    ctx.strokeStyle = `rgba(${plotColor.r}, ${plotColor.g}, ${plotColor.b}, 0.9)`;
    ctx.lineWidth = 1.5;
    ctx.shadowColor = `rgba(${plotColor.r}, ${plotColor.g}, ${plotColor.b}, 0.5)`;
    ctx.shadowBlur = 6;

    for (let i = 0; i < WAVEFORM_POINTS; i++) {
      const x = centerX + leftChannel[i] * scale;
      const y = centerY - rightChannel[i] * scale;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
    ctx.shadowBlur = 0;

    // Draw point cloud dots for extra density
    ctx.fillStyle = `rgba(${plotColor.r}, ${plotColor.g}, ${plotColor.b}, 0.6)`;
    for (let i = 0; i < WAVEFORM_POINTS; i += 3) {
      const x = centerX + leftChannel[i] * scale;
      const y = centerY - rightChannel[i] * scale;
      ctx.fillRect(x - 0.5, y - 0.5, 1, 1);
    }

    // Crosshair guides
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.06)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(centerX, centerY - scale);
    ctx.lineTo(centerX, centerY + scale);
    ctx.moveTo(centerX - scale, centerY);
    ctx.lineTo(centerX + scale, centerY);
    ctx.stroke();

    // Axis labels
    ctx.font = '10px monospace';
    ctx.fillStyle = 'rgba(255, 255, 255, 0.3)';
    ctx.textAlign = 'center';
    ctx.fillText('L', centerX, centerY - scale - 6);
    ctx.fillText('R', centerX + scale + 12, centerY + 4);

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
      console.error('[Lissajous] Failed to disable backend:', e);
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

<div class="lissajous-panel" class:visible={enabled}>
  <!-- Blurred album art watermark background -->
  {#if artwork}
    <div class="watermark-bg">
      <img src={artwork} alt="" class="watermark-img" />
    </div>
  {/if}

  <canvas bind:this={canvasRef} class="lissajous-canvas"></canvas>

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
  .lissajous-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    opacity: 0;
    transition: opacity 300ms ease;
    z-index: 5;
    background: #000000;
    overflow: hidden;
  }

  .lissajous-panel.visible {
    opacity: 1;
  }

  .watermark-bg {
    position: absolute;
    inset: -40px;
    z-index: 0;
    opacity: 0.08;
    filter: blur(60px) saturate(0.5);
    pointer-events: none;
  }

  .watermark-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .lissajous-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    z-index: 1;
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
