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

  // Generate groove rings (40 rings like Figma design)
  const grooveCount = 40;
  const grooves = Array.from({ length: grooveCount }, (_, i) => i);
</script>

<div class="vinyl-panel">
  <div class="vinyl-content">
    <!-- Album Cover (left side) -->
    <div class="album-cover" class:playing={isPlaying}>
      <img src={artwork} alt={trackTitle} />
    </div>

    <!-- Spinning Vinyl Disc -->
    <div class="vinyl-disc-container">
      <div class="vinyl-disc" class:spinning={isPlaying}>
        <!-- Grooves -->
        {#each grooves as i}
          <div
            class="groove"
            style="transform: scale({0.3 + i * 0.015})"
          ></div>
        {/each}

        <!-- Center Label -->
        <div class="center-label">
          <div class="spindle"></div>
        </div>

        <!-- Reflection -->
        <div class="reflection"></div>
      </div>
    </div>

    <!-- Track Info (right side) -->
    <div class="track-info" class:playing={isPlaying}>
      <!-- Vintage Badge -->
      <div class="vintage-badge">
        <span>33&#8531; RPM &bull; Stereo</span>
      </div>

      <h1 class="track-title">{trackTitle}</h1>
      <p class="track-artist">{artist}</p>
      {#if album}
        <p class="track-album">{album}</p>
      {/if}

      <!-- Vintage Details -->
      <div class="vintage-details">
        <div class="detail-row">
          <span class="label">Side</span>
          <span class="value">A</span>
        </div>
        <div class="detail-row">
          <span class="label">Format</span>
          <span class="value">12" Vinyl LP</span>
        </div>
      </div>

      <div class="quality-badge-wrapper">
        <QualityBadge {quality} {bitDepth} {samplingRate} />
      </div>
    </div>
  </div>
</div>

<style>
  .vinyl-panel {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding-top: 70px;
    padding-bottom: 120px;
    padding-left: 40px;
    padding-right: 40px;
    z-index: 5;
  }

  .vinyl-content {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0;
    position: relative;
    width: min(900px, 90vw);
    height: min(450px, 50vh);
  }

  /* Album Cover */
  .album-cover {
    position: absolute;
    left: 0;
    top: 50%;
    transform: translateY(-50%) translateX(80px);
    width: min(340px, 38vh);
    height: min(340px, 38vh);
    border-radius: 8px;
    overflow: hidden;
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.6),
      0 20px 60px rgba(0, 0, 0, 0.4);
    z-index: 10;
    transition: transform 500ms cubic-bezier(0.23, 1, 0.32, 1);
  }

  .album-cover.playing {
    transform: translateY(-50%) translateX(0);
  }

  .album-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Vinyl Disc Container */
  .vinyl-disc-container {
    position: absolute;
    right: 60px;
    top: 50%;
    transform: translateY(-50%);
    width: min(400px, 45vh);
    height: min(400px, 45vh);
  }

  .vinyl-disc {
    position: relative;
    width: 100%;
    height: 100%;
    border-radius: 50%;
    background: linear-gradient(135deg, #1a1a1a 0%, #000 50%, #1a1a1a 100%);
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.5),
      0 20px 60px rgba(0, 0, 0, 0.3),
      inset 0 0 60px rgba(0, 0, 0, 0.8);
  }

  .vinyl-disc.spinning {
    animation: spin 3s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  /* Grooves */
  .groove {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.04);
    pointer-events: none;
  }

  /* Center Label */
  .center-label {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 100px;
    height: 100px;
    border-radius: 50%;
    background: linear-gradient(135deg, #b45309 0%, #78350f 100%);
    border: 4px solid #92400e;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: inset 0 2px 10px rgba(0, 0, 0, 0.3);
  }

  .spindle {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: #000;
    box-shadow: inset 0 2px 4px rgba(255, 255, 255, 0.1);
  }

  /* Reflection */
  .reflection {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: linear-gradient(
      135deg,
      transparent 0%,
      rgba(255, 255, 255, 0.03) 30%,
      transparent 60%
    );
    pointer-events: none;
  }

  /* Track Info */
  .track-info {
    position: absolute;
    right: -60px;
    top: 50%;
    transform: translateY(-50%);
    max-width: 300px;
    opacity: 0.5;
    transition: opacity 300ms ease, transform 300ms ease;
  }

  .track-info.playing {
    opacity: 1;
    transform: translateY(-50%) translateX(-20px);
  }

  .vintage-badge {
    display: inline-block;
    padding: 6px 14px;
    border-radius: 20px;
    background: rgba(245, 158, 11, 0.15);
    border: 1px solid rgba(245, 158, 11, 0.3);
    margin-bottom: 16px;
  }

  .vintage-badge span {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: #fcd34d;
    font-weight: 600;
  }

  .track-title {
    font-family: 'Oswald', var(--font-sans), serif;
    font-size: clamp(24px, 4vw, 36px);
    font-weight: 600;
    color: #fffbeb;
    margin: 0 0 8px 0;
    letter-spacing: 0.01em;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.3);
  }

  .track-artist {
    font-family: 'Oswald', var(--font-sans), serif;
    font-size: clamp(16px, 2.5vw, 20px);
    color: rgba(254, 243, 199, 0.8);
    margin: 0 0 4px 0;
  }

  .track-album {
    font-family: 'Oswald', var(--font-sans), serif;
    font-size: clamp(14px, 2vw, 16px);
    color: rgba(254, 243, 199, 0.6);
    font-style: italic;
    margin: 0 0 20px 0;
  }

  .vintage-details {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 20px;
    font-family: monospace;
    font-size: 12px;
  }

  .detail-row {
    display: flex;
    gap: 12px;
  }

  .detail-row .label {
    color: rgba(252, 211, 77, 0.7);
    min-width: 50px;
  }

  .detail-row .value {
    color: rgba(254, 243, 199, 0.5);
  }

  .quality-badge-wrapper {
    margin-top: 16px;
  }

  /* Responsive */
  @media (max-width: 1000px) {
    .vinyl-content {
      flex-direction: column;
      height: auto;
      gap: 20px;
    }

    .album-cover {
      position: relative;
      left: auto;
      top: auto;
      transform: none;
      width: min(280px, 40vw);
      height: min(280px, 40vw);
    }

    .album-cover.playing {
      transform: none;
    }

    .vinyl-disc-container {
      display: none;
    }

    .track-info {
      position: relative;
      right: auto;
      top: auto;
      transform: none;
      text-align: center;
      opacity: 1;
    }

    .track-info.playing {
      transform: none;
    }

    .vintage-details {
      align-items: center;
    }

    .quality-badge-wrapper {
      display: flex;
      justify-content: center;
    }
  }

  @media (max-height: 600px) {
    .album-cover {
      width: min(200px, 30vh);
      height: min(200px, 30vh);
    }

    .vinyl-disc-container {
      width: min(280px, 35vh);
      height: min(280px, 35vh);
    }

    .center-label {
      width: 70px;
      height: 70px;
    }

    .spindle {
      width: 14px;
      height: 14px;
    }
  }
</style>
