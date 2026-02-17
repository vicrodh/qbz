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

  // Throttle rendering to 30fps max
  let lastRenderTime = 0;
  const FRAME_INTERVAL = 1000 / 30;

  // Colors for each band (from warm to cool)
  let bandColors = $state([
    { r: 255, g: 60, b: 60 },    // Sub-bass: red
    { r: 255, g: 160, b: 40 },   // Bass: orange
    { r: 80, g: 220, b: 120 },   // Mids: green
    { r: 60, g: 160, b: 255 },   // Presence: blue
    { r: 180, g: 100, b: 255 },  // Air: purple
  ]);

  // Extract colors from artwork
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
        // Spread across bands, cycling through extracted hues
        for (let i = 0; i < NUM_BANDS; i++) {
          const cidx = Math.floor((i / NUM_BANDS) * colors.length);
          const c = colors[cidx];
          // Brighten and saturate for glow effect
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

  // Album art image for canvas rendering
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

  $effect(() => {
    if (artwork) {
      loadArtwork(artwork);
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

    // Clear with black
    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, width, height);

    const centerX = width / 2;
    const centerY = height / 2 - 20; // Offset up slightly for controls
    const minDim = Math.min(width, height);
    const artSize = minDim * 0.3;
    const artHalf = artSize / 2;

    // Draw concentric glowing rings (outermost = sub-bass, innermost = air)
    for (let i = 0; i < NUM_BANDS; i++) {
      const bandIdx = i; // 0 = sub-bass (outer) → 4 = air (inner)
      const energy = energyData[bandIdx];

      // Ring radius: outer bands have larger base radius
      const baseRadius = artHalf + 30 + (NUM_BANDS - 1 - i) * (minDim * 0.06);
      const pulseRadius = baseRadius + energy * minDim * 0.04;

      const color = bandColors[bandIdx];
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

    // Draw album art in center
    if (artworkImg && artworkLoaded) {
      ctx.save();
      // Round the corners
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

      // Subtle glow behind artwork based on total energy
      const totalEnergy = energyData.reduce((sum, val) => sum + val, 0) / NUM_BANDS;
      if (totalEnergy > 0.05) {
        ctx.save();
        ctx.globalCompositeOperation = 'destination-over';
        const gradient = ctx.createRadialGradient(centerX, centerY, artHalf * 0.8, centerX, centerY, artHalf * 1.6);
        const c = bandColors[1]; // Use bass color for glow
        gradient.addColorStop(0, `rgba(${c.r}, ${c.g}, ${c.b}, ${totalEnergy * 0.3})`);
        gradient.addColorStop(1, 'rgba(0, 0, 0, 0)');
        ctx.fillStyle = gradient;
        ctx.fillRect(0, 0, width, height);
        ctx.restore();
      }
    }

    // Draw band labels below rings
    ctx.font = '10px monospace';
    ctx.textAlign = 'center';
    for (let i = 0; i < NUM_BANDS; i++) {
      const color = bandColors[i];
      const alpha = 0.3 + energyData[i] * 0.5;
      ctx.fillStyle = `rgba(${color.r}, ${color.g}, ${color.b}, ${alpha})`;
      const labelX = centerX - (NUM_BANDS * 30) / 2 + i * 30 + 15;
      const labelY = centerY + artHalf + (NUM_BANDS + 1) * (minDim * 0.06) + 20;
      ctx.fillText(bandNames[i], labelX, Math.min(labelY, height - 150));
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
