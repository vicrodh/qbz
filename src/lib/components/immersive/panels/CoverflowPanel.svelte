<script lang="ts">
  import QualityBadge from '$lib/components/QualityBadge.svelte';

  interface Props {
    artwork: string;
    trackTitle: string;
    artist: string;
    album?: string;
    isPlaying?: boolean;
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
  }

  let {
    artwork,
    trackTitle,
    artist,
    album,
    isPlaying = false,
    quality,
    bitDepth,
    samplingRate
  }: Props = $props();
</script>

<div class="coverflow-panel">
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
        <span>Now Playing</span>
      </div>
    {/if}
    <h1 class="track-title">{trackTitle}</h1>
    <p class="track-artist">{artist}</p>
    {#if album}
      <p class="track-album">{album}</p>
    {/if}
    <div class="quality-badge-wrapper">
      <QualityBadge {quality} {bitDepth} {samplingRate} />
    </div>
  </div>
</div>

<style>
  .coverflow-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 20px;
    /* Offset for header (70px) and controls (120px) to achieve true visual center */
    padding-top: 70px;
    padding-bottom: 120px;
    padding-left: 40px;
    padding-right: 40px;
    z-index: 5;
  }

  .artwork-wrapper {
    display: flex;
    flex-direction: column;
    align-items: center;
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

  /* Responsive */
  @media (max-width: 768px) {
    .coverflow-panel {
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
