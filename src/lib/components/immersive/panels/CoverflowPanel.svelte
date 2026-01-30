<script lang="ts">
  import { BarChart2 } from 'lucide-svelte';

  interface Props {
    artwork: string;
    trackTitle: string;
    artist: string;
    album?: string;
    isPlaying?: boolean;
  }

  let {
    artwork,
    trackTitle,
    artist,
    album,
    isPlaying = false
  }: Props = $props();
</script>

<div class="coverflow-panel">
  <div class="artwork-container" class:playing={isPlaying}>
    <img src={artwork} alt={trackTitle} class="artwork" />
  </div>

  <div class="track-info">
    {#if isPlaying}
      <div class="now-playing-indicator">
        <BarChart2 size={16} />
      </div>
    {/if}
    <h1 class="track-title">{trackTitle}</h1>
    <p class="track-artist">{artist}</p>
    {#if album}
      <p class="track-album">{album}</p>
    {/if}
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
    gap: 24px;
    padding: 80px 40px 140px;
    z-index: 5;
  }

  .artwork-container {
    position: relative;
    width: min(50vh, 400px);
    height: min(50vh, 400px);
    border-radius: 8px;
    overflow: hidden;
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.4),
      0 16px 64px rgba(0, 0, 0, 0.2);
    transition: transform 300ms ease;
  }

  .artwork-container:hover {
    transform: scale(1.02);
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

  .now-playing-indicator {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--accent-primary, #7c3aed);
    margin-bottom: 4px;
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

  /* Responsive */
  @media (max-width: 768px) {
    .coverflow-panel {
      padding: 70px 24px 130px;
      gap: 20px;
    }

    .artwork-container {
      width: min(60vw, 300px);
      height: min(60vw, 300px);
    }
  }

  @media (max-height: 600px) {
    .artwork-container {
      width: min(35vh, 250px);
      height: min(35vh, 250px);
    }

    .track-info {
      gap: 4px;
    }
  }
</style>
