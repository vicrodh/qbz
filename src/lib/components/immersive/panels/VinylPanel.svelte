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

  // Mouse position for parallax effect
  let mouseX = $state(0.5);
  let mouseY = $state(0.5);
  let containerRef: HTMLDivElement | null = $state(null);

  function handleMouseMove(e: MouseEvent) {
    if (!containerRef) return;
    const rect = containerRef.getBoundingClientRect();
    mouseX = (e.clientX - rect.left) / rect.width;
    mouseY = (e.clientY - rect.top) / rect.height;
  }

  function handleMouseLeave() {
    mouseX = 0.5;
    mouseY = 0.5;
  }

  // Parallax offset calculations (more sensitive)
  const coverOffsetX = $derived((mouseX - 0.5) * 30);
  const coverOffsetY = $derived((mouseY - 0.5) * 20);
  const discOffsetX = $derived((mouseX - 0.5) * -18);
  const discOffsetY = $derived((mouseY - 0.5) * -12);

  // Reveal offset when playing (percentage of container)
  const revealOffset = $derived(isPlaying ? -15 : 0);

  // Generate groove rings (40 rings)
  const grooveCount = 40;
  const grooves = Array.from({ length: grooveCount }, (_, i) => i);
</script>

<div
  class="vinyl-panel"
  bind:this={containerRef}
  onmousemove={handleMouseMove}
  onmouseleave={handleMouseLeave}
>
  <div class="vinyl-container" class:playing={isPlaying}>
    <!-- Vinyl Disc (BEHIND the cover) -->
    <div
      class="vinyl-disc-wrapper"
      style="transform: translate({discOffsetX}px, {discOffsetY}px)"
    >
      <div class="vinyl-disc" class:spinning={isPlaying}>
        <!-- Grooves -->
        {#each grooves as i}
          <div
            class="groove"
            style="transform: scale({0.28 + i * 0.017})"
          ></div>
        {/each}

        <!-- Center Label -->
        <div class="center-label">
          <div class="label-inner">
            <div class="spindle"></div>
          </div>
        </div>

        <!-- Light reflection -->
        <div class="reflection"></div>
      </div>
    </div>

    <!-- Album Cover (IN FRONT, slides to reveal disc) -->
    <div
      class="album-cover"
      style="transform: translate(calc(-50% + {coverOffsetX}px + {revealOffset}%), calc(-50% + {coverOffsetY}px))"
    >
      <img src={artwork} alt={trackTitle} />
    </div>
  </div>

  <!-- Track Info (below) -->
  <div class="track-info">
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
  .vinyl-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding-top: 70px;
    padding-bottom: 120px;
    padding-left: 40px;
    padding-right: 40px;
    z-index: 5;
    gap: 24px;
  }

  .vinyl-container {
    position: relative;
    width: min(500px, 55vh);
    height: min(500px, 55vh);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* Vinyl Disc - positioned behind cover */
  .vinyl-disc-wrapper {
    position: absolute;
    width: 100%;
    height: 100%;
    transition: transform 150ms ease-out;
  }

  .vinyl-disc {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 95%;
    height: 95%;
    border-radius: 50%;
    background: radial-gradient(
      circle at 30% 30%,
      #2a2a2a 0%,
      #0a0a0a 40%,
      #1a1a1a 70%,
      #0a0a0a 100%
    );
    box-shadow:
      0 10px 40px rgba(0, 0, 0, 0.6),
      0 20px 60px rgba(0, 0, 0, 0.4),
      inset 0 0 80px rgba(0, 0, 0, 0.9);
  }

  .vinyl-disc.spinning {
    animation: spin 2.5s linear infinite;
  }

  @keyframes spin {
    from { transform: translate(-50%, -50%) rotate(0deg); }
    to { transform: translate(-50%, -50%) rotate(360deg); }
  }

  /* Grooves - more visible */
  .groove {
    position: absolute;
    top: 50%;
    left: 50%;
    transform-origin: center center;
    width: 100%;
    height: 100%;
    margin-left: -50%;
    margin-top: -50%;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.08);
    pointer-events: none;
  }

  /* Alternating groove brightness for more realism */
  .groove:nth-child(odd) {
    border-color: rgba(255, 255, 255, 0.05);
  }
  .groove:nth-child(even) {
    border-color: rgba(255, 255, 255, 0.1);
  }
  .groove:nth-child(3n) {
    border-color: rgba(255, 255, 255, 0.12);
  }

  /* Center Label */
  .center-label {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 28%;
    height: 28%;
    border-radius: 50%;
    background: linear-gradient(135deg, #3a3a3a 0%, #1a1a1a 100%);
    border: 3px solid #2a2a2a;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow:
      inset 0 2px 10px rgba(0, 0, 0, 0.5),
      0 2px 10px rgba(0, 0, 0, 0.3);
  }

  .label-inner {
    width: 70%;
    height: 70%;
    border-radius: 50%;
    background: linear-gradient(135deg, #4a4a4a 0%, #2a2a2a 100%);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .spindle {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: #0a0a0a;
    box-shadow: inset 0 1px 3px rgba(255, 255, 255, 0.1);
  }

  /* Light Reflection */
  .reflection {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: linear-gradient(
      135deg,
      transparent 0%,
      rgba(255, 255, 255, 0.04) 20%,
      transparent 40%,
      rgba(255, 255, 255, 0.02) 60%,
      transparent 80%
    );
    pointer-events: none;
  }

  /* Album Cover - in front of disc, centered */
  .album-cover {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 75%;
    height: 75%;
    border-radius: 8px;
    overflow: hidden;
    box-shadow:
      0 10px 40px rgba(0, 0, 0, 0.5),
      0 20px 60px rgba(0, 0, 0, 0.3);
    z-index: 10;
    transition: transform 600ms cubic-bezier(0.23, 1, 0.32, 1);
  }

  /* revealed class handled by inline style for smooth parallax integration */

  .album-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Track Info */
  .track-info {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 6px;
    max-width: 500px;
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
    .vinyl-panel {
      padding: 70px 24px 130px;
      gap: 20px;
    }

    .vinyl-container {
      width: min(350px, 50vw);
      height: min(350px, 50vw);
    }
  }

  @media (max-height: 600px) {
    .vinyl-container {
      width: min(280px, 40vh);
      height: min(280px, 40vh);
    }

    .track-info {
      gap: 4px;
    }
  }
</style>
