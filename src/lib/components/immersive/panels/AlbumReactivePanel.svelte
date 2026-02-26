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
    isPlaying?: boolean;
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
    isPlaying = false,
    quality,
    bitDepth,
    samplingRate,
    originalBitDepth,
    originalSamplingRate,
    format
  }: Props = $props();

  let unlisten: UnlistenFn | null = null;
  let isInitialized = false;

  // Reactive energy values for CSS transforms
  let globalEnergy = $state(0);
  let bassEnergy = $state(0);
  let smoothedGlobal = 0;
  let smoothedBass = 0;

  let animationFrame: number | null = null;
  let lastRenderTime = 0;
  const FRAME_INTERVAL = getPanelFrameInterval('album-reactive');

  // Artwork-derived glow color
  let glowColor = $state('rgba(100, 100, 255, 0.3)');

  function extractGlowColor(imgSrc: string) {
    if (!imgSrc) return;

    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      const sampleCanvas = document.createElement('canvas');
      const size = 8;
      sampleCanvas.width = size;
      sampleCanvas.height = size;
      const sampleCtx = sampleCanvas.getContext('2d');
      if (!sampleCtx) return;

      sampleCtx.drawImage(img, 0, 0, size, size);
      const data = sampleCtx.getImageData(0, 0, size, size).data;

      let bestSat = 0;
      let bestR = 100, bestG = 100, bestB = 255;
      for (let idx = 0; idx < data.length; idx += 4) {
        const r = data[idx], g = data[idx + 1], b = data[idx + 2];
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const lum = (max + min) / 2;
        const sat = max === min ? 0 : (max - min) / (lum > 127 ? 510 - max - min : max + min);

        if (lum > 50 && lum < 220 && sat > bestSat) {
          bestSat = sat;
          bestR = r;
          bestG = g;
          bestB = b;
        }
      }
      glowColor = `rgba(${bestR}, ${bestG}, ${bestB}, 0.35)`;
    };
    img.src = imgSrc;
  }

  $effect(() => {
    if (artwork) {
      extractGlowColor(artwork);
    }
  });

  async function init() {
    if (isInitialized) return;
    isInitialized = true;

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
    } catch (e) {
      console.error('[AlbumReactive] Failed to enable backend:', e);
    }

    unlisten = await listen<number[]>('viz:energy', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length >= 5) {
          // Smooth with faster attack for visible reactivity
          const rawGlobal = (floats[0] + floats[1] + floats[2] + floats[3] + floats[4]) / 5;
          const rawBass = (floats[0] + floats[1]) / 2;

          // Very fast attack, fast decay — punchy for heavy music
          if (rawGlobal > smoothedGlobal) {
            smoothedGlobal = smoothedGlobal * 0.1 + rawGlobal * 0.9;
          } else {
            smoothedGlobal = smoothedGlobal * 0.6 + rawGlobal * 0.4;
          }
          if (rawBass > smoothedBass) {
            smoothedBass = smoothedBass * 0.1 + rawBass * 0.9;
          } else {
            smoothedBass = smoothedBass * 0.6 + rawBass * 0.4;
          }

          globalEnergy = smoothedGlobal;
          bassEnergy = smoothedBass;
        }
      }
    });
  }

  async function cleanup() {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }
    try {
      await invoke('v2_set_visualizer_enabled', { enabled: false });
    } catch (e) {
      console.error('[AlbumReactive] Failed to disable backend:', e);
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

  // Computed transform values — aggressive breathing for heavy music
  const artScale = $derived(1 + globalEnergy * 0.25);
  const glowSpread = $derived(15 + bassEnergy * 200);
  const glowOpacity = $derived(Math.min(0.1 + globalEnergy * 1.5, 1));
</script>

<div class="album-reactive-panel" class:visible={enabled}>
  <div class="artwork-wrapper">
    <div
      class="glow-layer"
      style:box-shadow="0 0 {glowSpread}px {glowSpread * 0.6}px {glowColor}"
      style:opacity={glowOpacity}
    ></div>

    <div
      class="artwork-container"
      class:playing={isPlaying}
      style:transform="scale({artScale})"
    >
      {#if artwork}
        <img src={artwork} alt={trackTitle} class="artwork" />
      {/if}
    </div>
  </div>

  <div class="track-info">
    {#if isPlaying}
      <div class="now-playing-indicator">
        <div class="equalizer">
          <span class="bar"></span>
          <span class="bar"></span>
          <span class="bar"></span>
          <span class="bar"></span>
        </div>
        <span>Now Playing</span>
      </div>
    {/if}
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
  .album-reactive-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 20px;
    padding-top: 70px;
    padding-bottom: 120px;
    padding-left: 40px;
    padding-right: 40px;
    z-index: 5;
    opacity: 0;
    transition: opacity 300ms ease;
  }

  .album-reactive-panel.visible {
    opacity: 1;
  }

  .artwork-wrapper {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .glow-layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border-radius: 8px;
    pointer-events: none;
    transition: box-shadow 100ms ease, opacity 100ms ease;
  }

  .artwork-container {
    position: relative;
    width: min(45vh, 360px);
    height: min(45vh, 360px);
    border-radius: 8px;
    overflow: hidden;
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.5),
      0 20px 60px rgba(0, 0, 0, 0.3);
    transition: transform 100ms ease;
    will-change: transform;
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
    margin-top: 40px;
  }

  .now-playing-indicator {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--accent-primary, #7c3aed);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 8px;
  }

  .equalizer {
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 14px;
  }

  .equalizer .bar {
    width: 3px;
    background: var(--accent-primary, #7c3aed);
    border-radius: 1px;
    animation: equalize 0.8s ease-in-out infinite;
  }

  .equalizer .bar:nth-child(1) { animation-delay: 0s; height: 60%; }
  .equalizer .bar:nth-child(2) { animation-delay: 0.2s; height: 100%; }
  .equalizer .bar:nth-child(3) { animation-delay: 0.1s; height: 40%; }
  .equalizer .bar:nth-child(4) { animation-delay: 0.3s; height: 80%; }

  @keyframes equalize {
    0%, 100% { transform: scaleY(0.3); }
    50% { transform: scaleY(1); }
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
    .album-reactive-panel {
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
