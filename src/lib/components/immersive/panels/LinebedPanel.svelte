<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import QualityBadge from '$lib/components/QualityBadge.svelte';
  import { getPanelFrameInterval } from '$lib/immersive/fpsConfig';

  interface Props {
    enabled?: boolean;
    artwork?: string;
    trackTitle?: string;
    artist?: string;
    album?: string;
    explicit?: boolean;
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    originalBitDepth?: number;
    originalSamplingRate?: number;
    format?: string;
  }

  interface CameraVector {
    x: number;
    y: number;
    z: number;
  }

  let {
    enabled = true,
    artwork = '',
    trackTitle = '',
    artist = '',
    album = '',
    explicit = false,
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

  const INPUT_BANDS = 512;
  const VISUAL_BANDS = 256;
  const NUM_LINES = 200;
  const SMOOTHING = 0.03;
  const SPECTRUM_HEIGHT = 770;
  const SPECTRUM_START = 4;
  const SPECTRUM_END = 460;
  const SPECTRUM_SCALE = 1.32;
  const SMOOTHING_PASSES = 1;
  const SMOOTHING_POINTS = 3;
  const SPECTRUM_MAX_EXPONENT = 1.35;
  const SPECTRUM_MIN_EXPONENT = 0.9;
  const SPECTRUM_EXPONENT_SCALE = 2;
  const HEAD_MARGIN = 7;
  const TAIL_MARGIN = 0;
  const MIN_MARGIN_WEIGHT = 0.7;
  const MARGIN_DECAY = 1.6;
  const HEAD_MARGIN_SLOPE = 0.013334120966221101;
  const TAIL_MARGIN_SLOPE = 1;
  const VISUAL_HEIGHT_CAP = 84;
  const HEIGHT_SOFT_CLIP = 3.25;
  const BIN_AVERAGE_WEIGHT = 0.52;
  const BIN_PEAK_WEIGHT = 0.48;

  const LINE_LENGTH = 9;
  const LINE_SPACING = 20;
  const WORLD_AMPLITUDE = 2.4;
  const CAMERA_FOV_DEG = 45;
  const CAMERA_POSITION: CameraVector = { x: 26.1, y: 1738.6, z: 868.8 };
  const CAMERA_ROTATION: CameraVector = { x: -0.6543, y: 0.0156, z: 0.012 };
  const CAMERA_NEAR = 80;
  const LINE_OPACITY = 0.78;
  const LINE_WIDTH = 0.82;
  const PLANE_HALF_WIDTH = ((VISUAL_BANDS - 1) * LINE_LENGTH) / 2;
  const PLANE_HALF_DEPTH = ((NUM_LINES - 1) * LINE_SPACING) / 2;

  const history: Float32Array[] = [];
  for (let i = 0; i < NUM_LINES; i++) {
    history.push(new Float32Array(VISUAL_BANDS));
  }
  let historyIndex = 0;

  const smoothedData = new Float32Array(INPUT_BANDS);
  let lastRenderTime = 0;
  const FRAME_INTERVAL = getPanelFrameInterval('linebed');

  function smoothSpectrum(data: Float32Array): Float32Array {
    const result = new Float32Array(data.length);
    result.set(data);
    const halfPoints = Math.floor(SMOOTHING_POINTS / 2);

    for (let pass = 0; pass < SMOOTHING_PASSES; pass++) {
      const previous = new Float32Array(result);
      for (let i = 0; i < result.length; i++) {
        let sum = 0;
        let count = 0;
        for (let j = -halfPoints; j <= halfPoints; j++) {
          const idx = i + j;
          if (idx >= 0 && idx < previous.length) {
            sum += previous[idx];
            count++;
          }
        }
        result[i] = count > 0 ? sum / count : 0;
      }
    }

    return result;
  }

  function transformToVisualBins(data: Float32Array): Float32Array {
    const start = Math.max(0, SPECTRUM_START);
    const end = Math.min(data.length - 1, SPECTRUM_END);
    const transformed = new Float32Array(VISUAL_BANDS);

    for (let i = 0; i < VISUAL_BANDS; i++) {
      const segmentStartFraction = i / VISUAL_BANDS;
      const segmentEndFraction = (i + 1) / VISUAL_BANDS;
      const scaledStart = Math.pow(segmentStartFraction, SPECTRUM_SCALE);
      const scaledEnd = Math.pow(segmentEndFraction, SPECTRUM_SCALE);
      const segmentStart = start + (end - start) * scaledStart;
      const segmentEnd = start + (end - start) * scaledEnd;
      const lower = Math.max(start, Math.floor(segmentStart));
      const upper = Math.min(end, Math.ceil(segmentEnd));
      let sum = 0;
      let peak = 0;
      let count = 0;

      for (let bin = lower; bin <= upper; bin++) {
        const value = data[bin] ?? 0;
        sum += value;
        peak = Math.max(peak, value);
        count++;
      }

      const average = count > 0 ? sum / count : 0;
      transformed[i] = (average * BIN_AVERAGE_WEIGHT + peak * BIN_PEAK_WEIGHT) * SPECTRUM_HEIGHT;
    }

    return transformed;
  }

  function applyAverageTransform(data: Float32Array): Float32Array {
    const firstPass = new Float32Array(data.length);
    const secondPass = new Float32Array(data.length);

    for (let i = 0; i < data.length; i++) {
      const previous = i > 0 ? data[i - 1] : data[i];
      const current = data[i];
      const next = i < data.length - 1 ? data[i + 1] : data[i];

      if (i === 0 || i === data.length - 1) {
        firstPass[i] = i === 0 ? current : (previous + current) / 2;
      } else if (current >= previous && current >= next) {
        firstPass[i] = current;
      } else {
        firstPass[i] = (current + Math.max(previous, next)) / 2;
      }
    }

    for (let i = 0; i < firstPass.length; i++) {
      const previous = i > 0 ? firstPass[i - 1] : firstPass[i];
      const current = firstPass[i];
      const next = i < firstPass.length - 1 ? firstPass[i + 1] : firstPass[i];

      if (i === 0 || i === firstPass.length - 1) {
        secondPass[i] = i === 0 ? current : (previous + current) / 2;
      } else if (current >= previous && current >= next) {
        secondPass[i] = current;
      } else {
        secondPass[i] = current / 2 + Math.max(previous, next) / 3 + Math.min(previous, next) / 6;
      }
    }

    return secondPass;
  }

  function applyTailTransform(data: Float32Array): void {
    for (let i = 0; i < data.length; i++) {
      if (i < HEAD_MARGIN) {
        data[i] *= HEAD_MARGIN_SLOPE * Math.pow(i + 1, MARGIN_DECAY) + MIN_MARGIN_WEIGHT;
      } else if (data.length - i <= TAIL_MARGIN) {
        data[i] *= TAIL_MARGIN_SLOPE * Math.pow(data.length - i, MARGIN_DECAY) + MIN_MARGIN_WEIGHT;
      }
    }
  }

  function applyExponentialTransform(data: Float32Array): void {
    for (let i = 0; i < data.length; i++) {
      const fraction = i / Math.max(1, data.length - 1);
      const exponent = SPECTRUM_MAX_EXPONENT +
        (SPECTRUM_MIN_EXPONENT - SPECTRUM_MAX_EXPONENT) *
        Math.pow(fraction, SPECTRUM_EXPONENT_SCALE);
      const normalized = Math.max(data[i] / SPECTRUM_HEIGHT, 0);
      const shaped = Math.pow(normalized, exponent);
      const compressed = 1 - Math.exp(-shaped * HEIGHT_SOFT_CLIP);
      data[i] = Math.max(Math.min(compressed * VISUAL_HEIGHT_CAP, VISUAL_HEIGHT_CAP), 0.1);
    }
  }

  let lineColor = $state({ r: 255, g: 255, b: 255 });

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

      const colors: { r: number; g: number; b: number; lum: number }[] = [];
      for (let i = 0; i < data.length; i += 4) {
        const red = data[i];
        const green = data[i + 1];
        const blue = data[i + 2];
        const lum = (red + green + blue) / 3;
        if (lum > 100 && lum < 240) {
          colors.push({ r: red, g: green, b: blue, lum });
        }
      }

      if (colors.length > 0) {
        colors.sort((colorA, colorB) => colorB.lum - colorA.lum);
        lineColor = { r: colors[0].r, g: colors[0].g, b: colors[0].b };
      } else {
        lineColor = { r: 255, g: 255, b: 255 };
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
    } catch (error) {
      console.error('[Linebed] Failed to enable backend:', error);
    }

    unlisten = await listen<number[]>('viz:spectral', (event) => {
      const payload = event.payload;
      if (!Array.isArray(payload)) return;

      const bytes = new Uint8Array(payload);
      const floats = new Float32Array(bytes.buffer);
      if (floats.length !== INPUT_BANDS) return;

      for (let i = 0; i < INPUT_BANDS; i++) {
        smoothedData[i] = smoothedData[i] * SMOOTHING + floats[i] * (1 - SMOOTHING);
      }

      let processed = transformToVisualBins(smoothedData);
      processed = applyAverageTransform(processed);
      applyTailTransform(processed);
      processed = smoothSpectrum(processed);
      applyExponentialTransform(processed);

      history[historyIndex].set(processed);
      historyIndex = (historyIndex + 1) % NUM_LINES;
    });

    render(0);
  }

  function render(timestamp: number = 0) {
    if (!ctx || !canvasRef) return;

    if (FRAME_INTERVAL > 0) {
      const delta = timestamp - lastRenderTime;
      if (delta < FRAME_INTERVAL) {
        animationFrame = requestAnimationFrame(render);
        return;
      }
      lastRenderTime = timestamp;
    }

    const rect = canvasRef.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const width = rect.width;
    const height = rect.height;

    const targetWidth = Math.floor(width * dpr);
    const targetHeight = Math.floor(height * dpr);
    if (canvasRef.width !== targetWidth || canvasRef.height !== targetHeight) {
      canvasRef.width = targetWidth;
      canvasRef.height = targetHeight;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }

    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, width, height);

    const focalLength = (height * 0.5) / Math.tan((CAMERA_FOV_DEG * Math.PI) / 360);
    const cosZ = Math.cos(-CAMERA_ROTATION.z);
    const sinZ = Math.sin(-CAMERA_ROTATION.z);
    const cosY = Math.cos(-CAMERA_ROTATION.y);
    const sinY = Math.sin(-CAMERA_ROTATION.y);
    const cosX = Math.cos(-CAMERA_ROTATION.x);
    const sinX = Math.sin(-CAMERA_ROTATION.x);

    for (let lineIdx = 0; lineIdx < NUM_LINES; lineIdx++) {
      const bufferIndex = (historyIndex - 1 - lineIdx + NUM_LINES * 2) % NUM_LINES;
      const spectrum = history[bufferIndex];
      const depthFactor = lineIdx / Math.max(1, NUM_LINES - 1);
      const worldZ = -PLANE_HALF_DEPTH + depthFactor * (PLANE_HALF_DEPTH * 2);
      const opacity = LINE_OPACITY;
      const lineWeight = LINE_WIDTH;

      ctx.beginPath();
      let hasVisiblePoint = false;

      for (let bandIndex = 0; bandIndex < VISUAL_BANDS; bandIndex++) {
        const worldX = bandIndex * LINE_LENGTH - PLANE_HALF_WIDTH;
        const worldY = spectrum[bandIndex] * WORLD_AMPLITUDE;

        const translatedX = worldX - CAMERA_POSITION.x;
        const translatedY = worldY - CAMERA_POSITION.y;
        const translatedZ = worldZ - CAMERA_POSITION.z;

        const rotatedZX = translatedX * cosZ - translatedY * sinZ;
        const rotatedZY = translatedX * sinZ + translatedY * cosZ;
        const rotatedZZ = translatedZ;

        const rotatedYX = rotatedZX * cosY + rotatedZZ * sinY;
        const rotatedYY = rotatedZY;
        const rotatedYZ = -rotatedZX * sinY + rotatedZZ * cosY;

        const rotatedXX = rotatedYX;
        const rotatedXY = rotatedYY * cosX - rotatedYZ * sinX;
        const rotatedXZ = rotatedYY * sinX + rotatedYZ * cosX;

        const depth = -rotatedXZ;
        if (depth <= CAMERA_NEAR) {
          hasVisiblePoint = false;
          continue;
        }

        const screenX = width * 0.5 + (rotatedXX * focalLength) / depth;
        const screenY = height * 0.5 - (rotatedXY * focalLength) / depth;

        if (!hasVisiblePoint) {
          ctx.moveTo(screenX, screenY);
          hasVisiblePoint = true;
        } else {
          ctx.lineTo(screenX, screenY);
        }
      }

      ctx.strokeStyle = `rgba(${lineColor.r}, ${lineColor.g}, ${lineColor.b}, ${opacity})`;
      ctx.lineWidth = lineWeight;
      ctx.stroke();
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
    } catch (error) {
      console.error('[Linebed] Failed to disable backend:', error);
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

<div class="linebed-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="linebed-canvas"></canvas>

  <div class="bottom-info">
    <div class="track-meta">
      <span class="track-title">{trackTitle}</span>
      {#if explicit}
        <span class="explicit-badge" title="Explicit"></span>
      {/if}
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
  .linebed-panel {
    position: absolute;
    inset: 0;
    opacity: 0;
    transition: opacity 300ms ease;
    z-index: 5;
    background: #000000;
  }

  .linebed-panel.visible {
    opacity: 1;
  }

  .linebed-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .bottom-info {
    position: absolute;
    right: 24px;
    bottom: 24px;
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

  .explicit-badge {
    display: inline-block;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    opacity: 0.45;
    background-color: var(--text-primary, white);
    -webkit-mask: url('/explicit.svg') center / contain no-repeat;
    mask: url('/explicit.svg') center / contain no-repeat;
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
