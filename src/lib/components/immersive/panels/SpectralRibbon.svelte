<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import QualityBadge from '$lib/components/QualityBadge.svelte';
  import { getPanelFrameInterval } from '$lib/immersive/fpsConfig';

  interface Props {
    enabled?: boolean;
    isPlaying?: boolean;
    currentTime?: number;
    duration?: number;
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
    isPlaying = false,
    currentTime = 0,
    duration = 0,
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

  const NUM_BANDS = 190;
  const HISTORY_BINS = 4096;
  const FRAME_INTERVAL_MS = getPanelFrameInterval('spectral-ribbon');

  let canvasRef: HTMLCanvasElement | null = $state(null);
  let canvasCtx: CanvasRenderingContext2D | null = null;
  let offscreenCanvas: HTMLCanvasElement | null = null;
  let offscreenCtx: CanvasRenderingContext2D | null = null;
  let columnImageData: ImageData | null = null;
  let animationFrame: number | null = null;
  let unlistenSpectral: UnlistenFn | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let initialized = false;
  let lastRenderTs = 0;
  let needsPresent = true;
  let hasNewColumn = false;
  let prevTime = 0;
  let previousColumnX = -1;
  let fallbackColumnX = 0;
  let smoothedWaterlineY = -1;
  let prevTrackKey = '';

  const latestBands = new Float32Array(NUM_BANDS);
  const prevBands = new Float32Array(NUM_BANDS);
  const interpBands = new Float32Array(NUM_BANDS);
  let hasPrevBands = false;
  const historyBands = new Float32Array(HISTORY_BINS * NUM_BANDS);
  const historyMask = new Uint8Array(HISTORY_BINS);
  const historyScratch = new Float32Array(NUM_BANDS);

  type PlotRect = { x: number; y: number; width: number; height: number };

  function formatSeconds(totalSec: number): string {
    const clamped = Math.max(0, Math.floor(totalSec));
    const mm = Math.floor(clamped / 60);
    const ss = (clamped % 60).toString().padStart(2, '0');
    return `${mm}:${ss}`;
  }

  function normalizedSampleRateHz(): number {
    const sr = typeof samplingRate === 'number' && Number.isFinite(samplingRate)
      ? samplingRate
      : (typeof originalSamplingRate === 'number' && Number.isFinite(originalSamplingRate) ? originalSamplingRate : 44_100);
    return sr > 1000 ? sr : sr * 1000;
  }

  function formatTypeLabel(): string {
    const raw = (format || '').toLowerCase();
    if (raw.includes('flac')) return 'FLAC (Free Lossless Audio Codec)';
    if (raw.includes('mp3')) return 'MP3';
    if (raw.includes('aac')) return 'AAC';
    if (raw.includes('opus')) return 'Opus';
    if (raw.includes('alac')) return 'ALAC';
    return 'Audio Stream';
  }

  function getStreamInfoText(): string {
    const srHz = Math.round(normalizedSampleRateHz());
    const bits = bitDepth ?? originalBitDepth ?? 0;
    const bitsLabel = bits > 0 ? `${bits} bits` : 'unknown bits';
    return `Stream 1/1: ${formatTypeLabel()}, ${srHz} Hz, ${bitsLabel}, FFT:1024, Bands:${NUM_BANDS}`;
  }

  function computePlotRect(width: number, height: number): PlotRect {
    // Centered plot area (50h/50v feeling), slightly smaller to avoid overlap with controls/card.
    const plotWidth = Math.max(2, Math.floor(width * 0.74));
    const plotHeight = Math.max(2, Math.floor(height * 0.62));
    const x = Math.max(0, Math.floor((width - plotWidth) * 0.5));
    const y = Math.max(0, Math.floor((height - plotHeight) * 0.5));
    return { x, y, width: plotWidth, height: plotHeight };
  }

  function getTrackKey(): string {
    return `${trackTitle}|${artist}|${album}`;
  }

  function normalizeTimeValues(rawCurrent: number, rawDuration: number): { currentSec: number; durationSec: number } {
    let currentSec = Math.max(0, Number.isFinite(rawCurrent) ? rawCurrent : 0);
    let durationSec = Math.max(0, Number.isFinite(rawDuration) ? rawDuration : 0);

    // Heuristic: duration in ms, current in sec (common mixed-source case).
    if (durationSec > 10_000 && currentSec < 10_000) {
      durationSec /= 1000;
    }

    // Heuristic: both in ms.
    if (durationSec > 10_000 && currentSec > 10_000) {
      durationSec /= 1000;
      currentSec /= 1000;
    }

    // Heuristic: current in ms, duration in sec.
    if (currentSec > 10_000 && durationSec > 0 && durationSec < 10_000) {
      currentSec /= 1000;
    }

    return { currentSec, durationSec };
  }

  function getNyquistKhz(): number {
    // samplingRate can come in Hz (44100) or kHz-ish (44.1), normalize both.
    const sr = typeof samplingRate === 'number' && Number.isFinite(samplingRate)
      ? samplingRate
      : (typeof originalSamplingRate === 'number' && Number.isFinite(originalSamplingRate) ? originalSamplingRate : 44_100);
    const srHz = sr > 1000 ? sr : sr * 1000;
    return Math.max(1, srHz / 2000); // Hz -> kHz Nyquist
  }

  function clearHistory() {
    historyBands.fill(0);
    historyMask.fill(0);
  }

  function clearRibbon(preserveHistory: boolean = false) {
    if (!offscreenCanvas || !offscreenCtx) return;
    offscreenCtx.setTransform(1, 0, 0, 1, 0, 0);
    offscreenCtx.clearRect(0, 0, offscreenCanvas.width, offscreenCanvas.height);
    offscreenCtx.fillStyle = '#03070c';
    offscreenCtx.fillRect(0, 0, offscreenCanvas.width, offscreenCanvas.height);
    fallbackColumnX = 0;
    smoothedWaterlineY = -1;
    if (!preserveHistory) clearHistory();
    needsPresent = true;
  }

  function saveLatestToHistory(historyIdx: number) {
    const idx = Math.max(0, Math.min(HISTORY_BINS - 1, historyIdx));
    const base = idx * NUM_BANDS;
    for (let bandIdx = 0; bandIdx < NUM_BANDS; bandIdx++) {
      historyBands[base + bandIdx] = latestBands[bandIdx];
    }
    historyMask[idx] = 1;
  }

  function drawHistoryToOffscreen() {
    if (!offscreenCanvas || !offscreenCtx) return;
    const plot = computePlotRect(offscreenCanvas.width, offscreenCanvas.height);
    if (plot.width <= 1 || plot.height <= 1) return;

    for (let x = plot.x; x < plot.x + plot.width; x++) {
      const progress = (x - plot.x) / Math.max(1, plot.width - 1);
      const idx = Math.round(progress * (HISTORY_BINS - 1));
      if (historyMask[idx] !== 1) continue;
      const base = idx * NUM_BANDS;
      for (let bandIdx = 0; bandIdx < NUM_BANDS; bandIdx++) {
        historyScratch[bandIdx] = historyBands[base + bandIdx];
      }
      drawColumnFromBands(x, historyScratch);
    }
  }

  function ensureCanvasSize() {
    if (!canvasRef || !canvasCtx || !offscreenCanvas || !offscreenCtx) return;
    const rect = canvasRef.getBoundingClientRect();
    const dpr = Math.max(1, window.devicePixelRatio || 1);
    const width = Math.max(1, Math.round(rect.width * dpr));
    const height = Math.max(1, Math.round(rect.height * dpr));

    if (
      canvasRef.width === width &&
      canvasRef.height === height &&
      offscreenCanvas.width === width &&
      offscreenCanvas.height === height
    ) return;

    canvasRef.width = width;
    canvasRef.height = height;
    offscreenCanvas.width = width;
    offscreenCanvas.height = height;
    canvasCtx.imageSmoothingEnabled = false;
    offscreenCtx.imageSmoothingEnabled = false;

    columnImageData = offscreenCtx.createImageData(1, height);
    clearRibbon(true);
    // Time-aligned rebuild from history so pre-resize content maps to correct timeline positions.
    drawHistoryToOffscreen();
    previousColumnX = -1;
  }

  function drawColumnAtX(columnX: number) {
    drawColumnFromBands(columnX, latestBands);
  }

  function drawInterpolatedColumnAtX(columnX: number, t01: number) {
    const interpWeight = Math.max(0, Math.min(1, t01));
    for (let bandIdx = 0; bandIdx < NUM_BANDS; bandIdx++) {
      interpBands[bandIdx] = prevBands[bandIdx] + (latestBands[bandIdx] - prevBands[bandIdx]) * interpWeight;
    }
    drawColumnFromBands(columnX, interpBands);
  }

  function drawColumnFromBands(columnX: number, bands: Float32Array) {
    if (!offscreenCanvas || !offscreenCtx || !columnImageData) return;
    const width = offscreenCanvas.width;
    const height = offscreenCanvas.height;
    if (width < 2 || height < 2) return;
    const plot = computePlotRect(width, height);
    const x = Math.max(plot.x, Math.min(plot.x + plot.width - 1, columnX));

    const data = columnImageData.data;
    const bandsMinusOne = NUM_BANDS - 1;
    const heightMinusOne = Math.max(1, plot.height - 1);

    for (let y = 0; y < height; y++) {
      if (y < plot.y || y >= plot.y + plot.height) {
        const off = y * 4;
        data[off] = 0;
        data[off + 1] = 0;
        data[off + 2] = 0;
        data[off + 3] = 0;
        continue;
      }

      const localY = y - plot.y;
      const normalizedY = 1.0 - localY / heightMinusOne; // bottom=0(low), top=1(high)
      const bandPos = normalizedY * bandsMinusOne;
      const lowerIdx = Math.floor(bandPos);
      const upperIdx = Math.min(bandsMinusOne, lowerIdx + 1);
      const frac = bandPos - lowerIdx;

      const low = bands[lowerIdx];
      const high = bands[upperIdx];
      const amp = Math.max(0, Math.min(1, low + (high - low) * frac));
      const db = 20.0 * Math.log10(Math.max(1e-6, amp));
      const dbNorm = Math.max(0, Math.min(1, (db + 120.0) / 120.0));
      const toned = Math.pow(dbNorm, 2.15);
      const [r, g, b] = spekColor(toned);
      const a = Math.floor(10 + toned * 156);

      const offset = y * 4;
      data[offset] = r;
      data[offset + 1] = g;
      data[offset + 2] = b;
      data[offset + 3] = a;
    }

    offscreenCtx.putImageData(columnImageData, x, 0);

    needsPresent = true;
  }

  function lerp(a: number, b: number, fraction: number): number {
    return a + (b - a) * fraction;
  }

  // Spek-like palette tuned for less saturation in normal listening levels.
  function spekColor(norm: number): [number, number, number] {
    const v = Math.max(0, Math.min(1, norm));
    const stops: [number, number, number, number][] = [
      [0.00, 0, 0, 0],
      [0.36, 0, 0, 62],
      [0.60, 14, 0, 100],
      [0.78, 92, 0, 112],
      [0.92, 190, 0, 72],
      [0.98, 220, 48, 0],
      [1.00, 238, 120, 32],
    ];

    for (let idx = 0; idx < stops.length - 1; idx++) {
      const [s0, r0, g0, b0] = stops[idx];
      const [s1, r1, g1, b1] = stops[idx + 1];
      if (v >= s0 && v <= s1) {
        const frac = (v - s0) / Math.max(1e-6, s1 - s0);
        return [
          Math.round(lerp(r0, r1, frac)),
          Math.round(lerp(g0, g1, frac)),
          Math.round(lerp(b0, b1, frac)),
        ];
      }
    }
    const [, r, g, b] = stops[stops.length - 1];
    return [r, g, b];
  }

  function present() {
    if (!canvasCtx || !offscreenCanvas || !needsPresent || !canvasRef) return;
    canvasCtx.setTransform(1, 0, 0, 1, 0, 0);
    canvasCtx.clearRect(0, 0, canvasRef!.width, canvasRef!.height);
    canvasCtx.drawImage(offscreenCanvas, 0, 0);
    drawAxes(canvasCtx, canvasRef.width, canvasRef.height);
    drawWaterline(canvasCtx, canvasRef.width, canvasRef.height);
    needsPresent = false;
  }

  function drawAxes(ctx: CanvasRenderingContext2D, width: number, height: number) {
    const plot = computePlotRect(width, height);
    const axisColor = 'rgba(90, 180, 120, 0.65)';
    const labelColor = 'rgba(120, 200, 150, 0.72)';
    const nyquistKhz = getNyquistKhz();

    ctx.strokeStyle = axisColor;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(plot.x, plot.y);
    ctx.lineTo(plot.x, plot.y + plot.height);
    ctx.lineTo(plot.x + plot.width, plot.y + plot.height);
    ctx.stroke();

    // Dynamic Y labels based on Nyquist.
    const yTicks = 4;
    ctx.fillStyle = labelColor;
    ctx.font = `${Math.max(9, Math.floor(height * 0.013))}px monospace`;
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    for (let tickIdx = 0; tickIdx <= yTicks; tickIdx++) {
      const frac = tickIdx / yTicks;
      const y = plot.y + (1 - frac) * plot.height;
      const valueKhz = nyquistKhz * frac;
      const label = `${valueKhz.toFixed(valueKhz >= 10 ? 0 : 1)}k`;
      ctx.fillText(label, plot.x - 8, y);
    }

    // X-axis time marks every 30s + final duration mark.
    const { durationSec } = normalizeTimeValues(currentTime, duration);
    if (durationSec > 0) {
      const tickStepSec = 30;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'top';
      for (let sec = 0; sec <= Math.floor(durationSec); sec += tickStepSec) {
        const progress = sec / durationSec;
        const x = Math.round(plot.x + progress * plot.width);
        ctx.beginPath();
        ctx.moveTo(x, plot.y + plot.height);
        ctx.lineTo(x, plot.y + plot.height + 5);
        ctx.strokeStyle = 'rgba(90, 180, 120, 0.35)';
        ctx.stroke();
        ctx.fillText(formatSeconds(sec), x, plot.y + plot.height + 7);
      }

      // Ensure final marker is present even if not aligned to 30s grid.
      const endX = plot.x + plot.width;
      ctx.beginPath();
      ctx.moveTo(endX, plot.y + plot.height);
      ctx.lineTo(endX, plot.y + plot.height + 5);
      ctx.strokeStyle = 'rgba(90, 180, 120, 0.6)';
      ctx.stroke();
      ctx.fillText(formatSeconds(durationSec), endX, plot.y + plot.height + 7);
    }
  }

  function drawWaterline(ctx: CanvasRenderingContext2D, width: number, height: number) {
    const plot = computePlotRect(width, height);
    let peakAmp = 0;
    for (let bandIdx = 0; bandIdx < NUM_BANDS; bandIdx++) {
      if (latestBands[bandIdx] > peakAmp) peakAmp = latestBands[bandIdx];
    }
    const targetY = plot.y + (1 - peakAmp) * (plot.height - 1);
    if (smoothedWaterlineY < 0) smoothedWaterlineY = targetY;
    smoothedWaterlineY = smoothedWaterlineY * 0.9 + targetY * 0.1;
    const lineY = Math.round(Math.max(plot.y, Math.min(plot.y + plot.height - 1, smoothedWaterlineY)));

    ctx.strokeStyle = `rgba(170, 230, 255, ${0.18 + peakAmp * 0.35})`;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(plot.x, lineY);
    ctx.lineTo(plot.x + plot.width, lineY);
    ctx.stroke();
  }

  function render(ts: number) {
    if (!enabled || !initialized) return;
    animationFrame = requestAnimationFrame(render);

    if (ts - lastRenderTs < FRAME_INTERVAL_MS) return;
    lastRenderTs = ts;

    ensureCanvasSize();

    if (isPlaying && hasNewColumn) {
      const width = offscreenCanvas?.width ?? 0;
      if (width > 1) {
        const plot = computePlotRect(width, offscreenCanvas?.height ?? 1);
        const { currentSec, durationSec } = normalizeTimeValues(currentTime, duration);
        if (durationSec > 0) {
          const progress = Math.max(0, Math.min(1, currentSec / durationSec));
          const currentX = Math.max(
            plot.x,
            Math.min(plot.x + plot.width - 1, plot.x + Math.floor(progress * (plot.width - 1)))
          );
          const historyIdx = Math.floor(progress * (HISTORY_BINS - 1));
          saveLatestToHistory(historyIdx);

          // Fill any gap columns in case events/time advance faster than render cadence.
          if (previousColumnX >= 0) {
            const from = Math.min(previousColumnX + 1, currentX);
            const count = Math.max(1, currentX - from + 1);
            for (let x = from; x <= currentX; x++) {
              if (hasPrevBands && count > 1) {
                const frac = (x - from + 1) / count;
                // Adaptive interpolation "on the fly":
                // larger column gaps get smoother easing to avoid visible snap-back.
                const eased = frac * frac * (3 - 2 * frac);
                const interpFrac = count > 6
                  ? (0.56 + eased * 0.44)
                  : (0.72 + eased * 0.28);
                drawInterpolatedColumnAtX(x, interpFrac);
              } else {
                drawColumnAtX(x);
              }
            }
          } else {
            // Re-attach point (first frame after resize/open): draw only current column.
            // Full backfill here causes a flattened block with repeated spectrum.
            drawColumnAtX(currentX);
          }
          previousColumnX = currentX;
        } else {
          // Unknown duration: still grow left->right without scrolling.
          const x = plot.x + Math.min(plot.width - 1, fallbackColumnX);
          saveLatestToHistory(Math.min(HISTORY_BINS - 1, fallbackColumnX));
          drawColumnAtX(x);
          fallbackColumnX = Math.min(plot.width - 1, fallbackColumnX + 1);
        }
      }
      hasNewColumn = false;
    }

    present();
  }

  async function init() {
    if (!enabled || initialized || !canvasRef) return;
    canvasCtx = canvasRef.getContext('2d');
    if (!canvasCtx) return;

    offscreenCanvas = document.createElement('canvas');
    offscreenCtx = offscreenCanvas.getContext('2d');
    if (!offscreenCtx) return;

    ensureCanvasSize();
    prevTime = currentTime;
    prevTrackKey = getTrackKey();

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
    } catch (err) {
      console.error('[SpectralRibbon] Failed to enable visualizer backend:', err);
    }

    unlistenSpectral = await listen<number[]>('viz:spectral', (event) => {
      const payload = event.payload;
      if (!Array.isArray(payload)) return;
      const bytes = new Uint8Array(payload);
      const floats = new Float32Array(bytes.buffer);
      if (floats.length !== NUM_BANDS) return;
      prevBands.set(latestBands);
      latestBands.set(floats);
      hasPrevBands = true;
      hasNewColumn = true;
    });

    resizeObserver = new ResizeObserver(() => {
      ensureCanvasSize();
      needsPresent = true;
    });
    if (canvasRef) resizeObserver.observe(canvasRef);
    window.addEventListener('resize', ensureCanvasSize);

    // First-open layout settle: some windowed launches report transient rects.
    requestAnimationFrame(() => {
      ensureCanvasSize();
      requestAnimationFrame(() => {
        ensureCanvasSize();
      });
    });

    initialized = true;
    animationFrame = requestAnimationFrame(render);
  }

  async function cleanup() {
    if (animationFrame) {
      cancelAnimationFrame(animationFrame);
      animationFrame = null;
    }
    if (unlistenSpectral) {
      unlistenSpectral();
      unlistenSpectral = null;
    }
    if (resizeObserver) {
      resizeObserver.disconnect();
      resizeObserver = null;
    }
    window.removeEventListener('resize', ensureCanvasSize);
    try {
      await invoke('v2_set_visualizer_enabled', { enabled: false });
    } catch (err) {
      console.error('[SpectralRibbon] Failed to disable visualizer backend:', err);
    }
    initialized = false;
  }

  $effect(() => {
    const currentTrackKey = getTrackKey();
    const delta = currentTime - prevTime;
    const seekBackward = delta < -1.0;
    const seekForward = delta > 10.0;
    const jumped = seekBackward || seekForward;
    const changedTrack = currentTrackKey !== prevTrackKey;
    if (jumped || changedTrack) {
      clearRibbon();
      previousColumnX = -1;
    }
    prevTime = currentTime;
    prevTrackKey = currentTrackKey;
  });

  onMount(() => {
    if (enabled) init();
    return cleanup;
  });

  $effect(() => {
    if (enabled && !initialized) {
      init();
    } else if (!enabled && initialized) {
      cleanup();
    }
  });
</script>

<div class="spectral-ribbon-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="spectral-canvas"></canvas>

  <div class="spek-header">
    <span class="spek-header-text">{getStreamInfoText()}</span>
  </div>

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
  .spectral-ribbon-panel {
    position: absolute;
    inset: 0;
    opacity: 0;
    transition: opacity 220ms ease;
    z-index: 5;
    background: #03070c;
  }

  .spectral-ribbon-panel.visible {
    opacity: 1;
  }

  .spectral-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .spek-header {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    top: 86px;
    z-index: 9;
    pointer-events: none;
    max-width: min(84vw, 1080px);
  }

  .spek-header-text {
    display: inline-block;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;
    font-size: 15px;
    color: rgba(215, 255, 225, 0.94);
    text-shadow: 0 1px 5px rgba(0, 0, 0, 0.45);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    background: rgba(0, 0, 0, 0.18);
    border: 1px solid rgba(95, 185, 120, 0.4);
    border-radius: 4px;
    padding: 2px 8px;
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

  @media (max-width: 768px) {
    .spek-header {
      left: 50%;
      top: 78px;
      transform: translateX(-50%);
      max-width: calc(100vw - 24px);
    }

    .spek-header-text {
      font-size: 13px;
      padding: 2px 6px;
    }

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
