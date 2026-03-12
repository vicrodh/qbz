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

  // Linebed parameters
  const NUM_BANDS = 190; // viz:spectral sends 190 bands
  const NUM_LINES = 120; // Dense terrain (musicvid.org uses 200 with WebGL, 120 is Canvas 2D safe)
  const SMOOTHING = 0.03; // Temporal smoothing (musicvid: smoothingTimeConstant 0.03)

  // Spectrum processing params — scaled for 190 bins (musicvid uses 1024 visual bins)
  // musicvid: 9 points / 1024 bins = 0.88% width per pass
  // ours:     5 points / 190 bins  = 2.6% width per pass (comparable ratio)
  const SMOOTHING_PASSES = 2;
  const SMOOTHING_POINTS = 5;
  // Power-law reduced: musicvid maps 647→1024 bins (upsampling) at 2.5.
  // We map 190→190 (no upsampling) — 2.5 stretches 33 bass bins to 50% width
  // creating a uniform plateau. 1.5 keeps more frequency diversity visible.
  const SPECTRUM_POWER = 1.5;

  // Variable exponential transform (musicvid: Exponential settings)
  // Bass gets higher exponent = sharper peaks; treble gets lower = softer
  const SPECTRUM_MAX_EXPONENT = 6; // Applied to bass (bin 0)
  const SPECTRUM_MIN_EXPONENT = 3; // Applied to treble (last bin)
  const SPECTRUM_EXPONENT_SCALE = 2; // Power curve for exponent interpolation

  // Head margin (musicvid: Smoothing > headMargin/tailMargin settings)
  const HEAD_MARGIN = 7;
  const MIN_MARGIN_WEIGHT = 0.7;
  const MARGIN_DECAY = 1.6;

  // Ring buffer of spectrum snapshots
  const history: Float32Array[] = [];
  for (let i = 0; i < NUM_LINES; i++) {
    history.push(new Float32Array(NUM_BANDS));
  }
  let historyIndex = 0;
  let frameCounter = 0;

  // Current smoothed spectrum
  const smoothedData = new Float32Array(NUM_BANDS);

  let lastRenderTime = 0;
  const FRAME_INTERVAL = getPanelFrameInterval('linebed');

  // Multi-pass moving average smoothing (from musicvid.org AnalyseFunctions.js)
  function smoothSpectrum(data: Float32Array): Float32Array {
    const result = new Float32Array(data.length);
    result.set(data);
    const halfPoints = Math.floor(SMOOTHING_POINTS / 2);

    for (let pass = 0; pass < SMOOTHING_PASSES; pass++) {
      const prev = new Float32Array(result);
      for (let i = 0; i < result.length; i++) {
        let sum = 0;
        let count = 0;
        for (let j = -halfPoints; j <= halfPoints; j++) {
          const idx = i + j;
          if (idx >= 0 && idx < prev.length) {
            sum += prev[idx];
            count++;
          }
        }
        result[i] = sum / count;
      }
    }
    return result;
  }

  // Power-law frequency redistribution (from musicvid.org transformToVisualBins)
  // Remaps linear FFT bins so bass frequencies get more visual width.
  // Without this, all energy clusters in the first ~30 bins (left side).
  function transformToVisualBins(data: Float32Array): Float32Array {
    const result = new Float32Array(NUM_BANDS);
    for (let i = 0; i < NUM_BANDS; i++) {
      const bin = Math.pow(i / NUM_BANDS, SPECTRUM_POWER) * (NUM_BANDS - 1);
      const binFloor = Math.floor(bin);
      const binCeil = Math.min(binFloor + 1, NUM_BANDS - 1);
      const frac = bin - binFloor;
      // Linear interpolation between neighboring bins for smoothness
      result[i] = data[binFloor] * (1 - frac) + data[binCeil] * frac;
    }
    return result;
  }

  // Normalize spectrum to [0, 1] range — required before exponential transform.
  // Without normalization, exponents 3-6 produce wrong results.
  function normalizeSpectrum(data: Float32Array): void {
    let max = 0;
    for (let i = 0; i < data.length; i++) {
      if (data[i] > max) max = data[i];
    }
    if (max > 0) {
      for (let i = 0; i < data.length; i++) {
        data[i] /= max;
      }
    }
  }

  // Head margin transform (musicvid: tailTransform with headMargin settings).
  // Reduces the first N bins with a decay curve to avoid harsh bass edge.
  function applyTailTransform(data: Float32Array): void {
    for (let i = 0; i < HEAD_MARGIN; i++) {
      const frac = i / HEAD_MARGIN;
      const weight = MIN_MARGIN_WEIGHT + (1 - MIN_MARGIN_WEIGHT) * Math.pow(frac, 1 / MARGIN_DECAY);
      data[i] *= weight;
    }
  }

  // Variable exponential transform (musicvid: exponentialTransform).
  // Bass bins get exponent 6 (very sharp — only peaks survive),
  // treble bins get exponent 3 (still sharp but less aggressive).
  // This is what creates "many peaks" instead of "fat mountains".
  function applyExponentialTransform(data: Float32Array): void {
    for (let i = 0; i < data.length; i++) {
      const fraction = i / data.length;
      const exponent = SPECTRUM_MAX_EXPONENT +
        (SPECTRUM_MIN_EXPONENT - SPECTRUM_MAX_EXPONENT) *
        Math.pow(fraction, SPECTRUM_EXPONENT_SCALE);
      data[i] = Math.pow(data[i], exponent);
    }
  }

  // Colors from artwork
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
        const r = data[i], g = data[i + 1], b = data[i + 2];
        const lum = (r + g + b) / 3;
        if (lum > 100 && lum < 240) {
          colors.push({ r, g, b, lum });
        }
      }

      if (colors.length > 0) {
        // Pick the brightest suitable color
        colors.sort((a, b) => b.lum - a.lum);
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
    } catch (e) {
      console.error('[Linebed] Failed to enable backend:', e);
    }

    unlisten = await listen<number[]>('viz:spectral', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length === NUM_BANDS) {
          // Pipeline matches musicvid.org's SpectrumAnalyser transform chain:
          // temporal → combineBins → normalize → tail → exponential → smoothing

          // 1. Temporal smoothing (smoothingTimeConstant: 0.03)
          for (let i = 0; i < NUM_BANDS; i++) {
            smoothedData[i] = smoothedData[i] * SMOOTHING + floats[i] * (1 - SMOOTHING);
          }

          // 2. Power-law frequency redistribution (combineBins / spectrumScale: 2.5)
          const visualBins = transformToVisualBins(smoothedData);

          // 3. Normalize to [0,1] (enableNormalizeTransform)
          normalizeSpectrum(visualBins);

          // 4. Head margin (enableTailTransform — headMargin: 7)
          applyTailTransform(visualBins);

          // 5. Variable exponential (enableExponentialTransform — exponents 3→6)
          applyExponentialTransform(visualBins);

          // 6. Spatial smoothing (enableSmoothingTransform — 3 passes, 9 points)
          const finalSpectrum = smoothSpectrum(visualBins);

          // Push processed snapshot into history every frame for fluid scrolling
          history[historyIndex].set(finalSpectrum);
          historyIndex = (historyIndex + 1) % NUM_LINES;
        }
      }
    });

    render(0);
  }

  // Build a smooth curve path through spectrum points
  function buildSpectrumPath(
    spectrum: Float32Array,
    lineLeft: number,
    currentLineWidth: number,
    baseY: number,
    maxAmplitude: number
  ) {
    if (!ctx) return;

    ctx.moveTo(lineLeft, baseY);

    for (let p = 0; p < NUM_BANDS; p++) {
      const xFraction = p / (NUM_BANDS - 1);
      const xPos = lineLeft + xFraction * currentLineWidth;
      const amp = spectrum[p];
      const yPos = baseY - amp * maxAmplitude;

      if (p === 0) {
        ctx.lineTo(xPos, yPos);
      } else {
        // Quadratic curve smoothing between consecutive points
        const prevFraction = (p - 1) / (NUM_BANDS - 1);
        const prevX = lineLeft + prevFraction * currentLineWidth;
        const prevY = baseY - spectrum[p - 1] * maxAmplitude;
        const cpX = (prevX + xPos) / 2;
        const cpY = (prevY + yPos) / 2;
        ctx.quadraticCurveTo(prevX, prevY, cpX, cpY);
      }
    }

    // Final point
    const lastX = lineLeft + currentLineWidth;
    const lastY = baseY - spectrum[NUM_BANDS - 1] * maxAmplitude;
    ctx.lineTo(lastX, lastY);
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

    // Centered symmetric perspective — steeper "standing on a chair" angle.
    // Camera: musicvid posY=1738, rotX=-0.6543 (looking down ~37.5°).
    // Simulated via less aggressive depth compression + larger vertical spread.

    // Back line (far, top of canvas): narrow, centered
    const backY = height * 0.12;
    const backLineWidth = width * 0.18;

    // Front line (close, bottom of canvas): wide, centered
    const frontY = height * 0.92;
    const frontLineWidth = width * 1.1;

    // Draw lines from back to front (painter's algorithm).
    // Data direction: NEWEST at back, scrolls toward front/viewer.
    for (let lineIdx = 0; lineIdx < NUM_LINES; lineIdx++) {
      // Reverse mapping: lineIdx=0 (back) = newest data, lineIdx=N-1 (front) = oldest
      const bufIdx = (historyIndex - 1 - lineIdx + NUM_LINES * 2) % NUM_LINES;
      const spectrum = history[bufIdx];

      // Less aggressive depth compression = more uniform spacing = steeper viewing angle
      const rawFactor = lineIdx / (NUM_LINES - 1);
      const depthFactor = Math.pow(rawFactor, 1.35);

      // Interpolate Y and width from back to front
      const baseY = backY + (frontY - backY) * depthFactor;
      const currentLineWidth = backLineWidth + (frontLineWidth - backLineWidth) * depthFactor;
      const lineLeft = (width - currentLineWidth) / 2; // Centered

      // Amplitude: tall peaks like musicvid (spectrumHeight: 770, spacing: 20 = 38.5x ratio)
      const amplitudeScale = 0.06 + depthFactor * 0.94;
      const maxAmplitude = height * 0.55 * amplitudeScale;

      // Opacity: fades at back, bright at front
      const opacity = 0.08 + depthFactor * 0.92;

      // Occlusion pass: fill below the spectrum line with black
      ctx.beginPath();
      buildSpectrumPath(spectrum, lineLeft, currentLineWidth, baseY, maxAmplitude);
      ctx.lineTo(lineLeft + currentLineWidth, baseY + 3);
      ctx.lineTo(lineLeft, baseY + 3);
      ctx.closePath();
      ctx.fillStyle = '#000000';
      ctx.fill();

      // Stroke pass: draw the spectrum line on top
      ctx.beginPath();
      buildSpectrumPath(spectrum, lineLeft, currentLineWidth, baseY, maxAmplitude);

      const lineWeight = 0.2 + depthFactor * 1.1;
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
    } catch (e) {
      console.error('[Linebed] Failed to disable backend:', e);
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
