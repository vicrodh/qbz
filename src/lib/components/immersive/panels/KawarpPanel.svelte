<script lang="ts">
  import { t } from 'svelte-i18n';
  import QualityBadge from '$lib/components/QualityBadge.svelte';
  import { Kawarp } from '@kawarp/core';
  import { onMount, onDestroy } from 'svelte';

  interface Props {
    artwork: string;
    trackTitle: string;
    artist: string;
    album?: string;
    explicit?: boolean;
    isPlaying?: boolean;
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    originalBitDepth?: number;
    originalSamplingRate?: number;
    format?: string;
  }

  let {
    artwork,
    trackTitle,
    artist,
    album,
    explicit = false,
    isPlaying = false,
    quality,
    bitDepth,
    samplingRate,
    originalBitDepth,
    originalSamplingRate,
    format
  }: Props = $props();

  let canvasEl: HTMLCanvasElement | undefined = $state();
  let kawarp: Kawarp | undefined;
  let lastLoadedArtwork: string | null = null;
  let resizeObserver: ResizeObserver | undefined;

  onMount(() => {
    if (!canvasEl) return;
    // Demo defaults from kawarp.boidu.dev with two color-side tweaks so
    // sub-dominant colours from the cover come through without dragging
    // the look toward Static's translate-of-detail behaviour. Earlier we
    // tried widening `scale` which surfaced detail but made kawarp look
    // closer to Static — wrong direction. Instead keep scale at default
    // and push the colour pipeline:
    //   - saturation 1.5 → 1.8: amplifies all colours, including the
    //     muted ones that get drowned by the dark dominant tone on
    //     covers like Billie's underwater shot.
    //   - tintIntensity 0.15 → 0.08: half the dark-blue tint pull so
    //     the cover's own palette dominates instead of the global tint.
    kawarp = new Kawarp(canvasEl, {
      warpIntensity: 1.0,
      blurPasses: 8,
      animationSpeed: 1.0,
      transitionDuration: 1000,
      saturation: 1.8,
      tintIntensity: 0.08,
      dithering: 0.008,
      scale: 1.0,
    });
    if (artwork) {
      lastLoadedArtwork = artwork;
      kawarp.loadImage(artwork).catch((e) => console.warn('[KawarpPanel] loadImage failed:', e));
    }
    kawarp.start();

    // Keep the canvas backing store in sync with the rendered size so the
    // shader doesn't stretch when the immersive view resizes.
    resizeObserver = new ResizeObserver(() => kawarp?.resize());
    resizeObserver.observe(canvasEl);
  });

  // React to track changes by reloading the source — kawarp does its own
  // crossfade between the previous and the new image.
  $effect(() => {
    if (kawarp && artwork && artwork !== lastLoadedArtwork) {
      lastLoadedArtwork = artwork;
      kawarp.loadImage(artwork).catch((e) => console.warn('[KawarpPanel] loadImage failed:', e));
    }
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    kawarp?.dispose();
    kawarp = undefined;
  });
</script>

<div class="static-panel">
  <canvas bind:this={canvasEl} class="kawarp-canvas" aria-hidden="true"></canvas>

  <div class="artwork-wrapper">
    <div class="artwork-container" class:playing={isPlaying}>
      <img src={artwork} alt={trackTitle} class="artwork" />
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
        <span>{$t('player.nowPlaying')}</span>
      </div>
    {/if}
    <div class="track-title-row">
      <h1 class="track-title">{trackTitle}</h1>
      {#if explicit}
        <span class="explicit-badge" title="{ $t('library.explicit') }"></span>
      {/if}
    </div>
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
  .static-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 20px;
    /* Offset for header (70px) and controls (120px) to achieve true visual center */
    padding-top: 52px;
    padding-bottom: 88px;
    padding-left: 40px;
    padding-right: 40px;
    z-index: 5;
    /* Solid fallback before WebGL paints, and a base for letterboxing if the
       canvas backs off on a low-spec / WebKit failure. */
    background: var(--bg-primary, #0a0a0b);
  }

  .kawarp-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    z-index: 0;
    pointer-events: none;
    /* Composited on its own layer so WebKitGTK doesn't repaint surrounding
       text on every animation frame — same trick used on the sticky-header
       fix in 1.2.10. */
    will-change: transform;
    transform: translateZ(0);
  }

  .artwork-wrapper {
    display: flex;
    flex-direction: column;
    align-items: center;
    /* Lift above the kawarp canvas (which sits at z:0 inside this same
       stacking context). Without this the cover would still render on top
       because .artwork-container has its own position:relative, but track-info
       below was being painted UNDER the canvas — so we anchor every content
       wrapper above z:0 explicitly. */
    position: relative;
    z-index: 1;
  }

  .artwork-container {
    position: relative;
    width: min(52vh, 460px);
    height: min(52vh, 460px);
    border-radius: 8px;
    overflow: hidden;
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.5),
      0 20px 60px rgba(0, 0, 0, 0.3);
    transition: transform 300ms ease, box-shadow 300ms ease;
  }

  .artwork-container:hover {
    transform: scale(1.02) translateY(-4px);
    box-shadow:
      0 12px 40px rgba(0, 0, 0, 0.5),
      0 28px 80px rgba(0, 0, 0, 0.3);
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
    margin-top: 8px;
    /* See .artwork-wrapper note: lift above z:0 canvas. */
    position: relative;
    z-index: 1;
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

  .track-title-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-width: 0;
  }

  .track-title {
    font-size: clamp(20px, 3vw, 28px);
    font-weight: 700;
    color: var(--text-primary, white);
    margin: 0;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.3);
  }

  .explicit-badge {
    display: inline-block;
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    opacity: 0.45;
    background-color: var(--text-primary, white);
    -webkit-mask: url('/explicit.svg') center / contain no-repeat;
    mask: url('/explicit.svg') center / contain no-repeat;
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

  /* Responsive */
  @media (max-width: 768px) {
    .static-panel {
      padding: 52px 24px 88px;
      gap: 16px;
    }

    .artwork-container {
      width: min(55vw, 320px);
      height: min(55vw, 320px);
    }
  }

  @media (max-height: 600px) {
    .artwork-container {
      width: min(38vh, 280px);
      height: min(38vh, 280px);
    }

    .track-info {
      gap: 4px;
    }
  }
</style>
