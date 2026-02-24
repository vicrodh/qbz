<script lang="ts">
  import QualityBadge from '$lib/components/QualityBadge.svelte';

  interface QueueTrack {
    id: string | number;
    title: string;
    artist: string;
    artwork: string;
  }

  interface Props {
    artwork: string;
    trackTitle: string;
    artist: string;
    album?: string;
    isPlaying?: boolean;
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    originalBitDepth?: number;
    originalSamplingRate?: number;
    format?: string;
    // Queue for coverflow
    queueTracks?: QueueTrack[];
    queueCurrentIndex?: number;
    onNavigate?: (index: number) => void;
  }

  let {
    artwork,
    trackTitle,
    artist,
    album,
    isPlaying = false,
    quality,
    bitDepth,
    samplingRate,
    originalBitDepth,
    originalSamplingRate,
    format,
    queueTracks = [],
    queueCurrentIndex = 0,
    onNavigate
  }: Props = $props();

  // Build coverflow items: 2 prev + current + 2 next
  const coverflowItems = $derived.by(() => {
    const items: { index: number; artwork: string; title: string; artist: string; position: number }[] = [];

    // Position: -2, -1, 0 (center), 1, 2
    for (let offset = -2; offset <= 2; offset++) {
      const queueIndex = queueCurrentIndex + offset;

      if (queueIndex >= 0 && queueIndex < queueTracks.length) {
        const track = queueTracks[queueIndex];
        items.push({
          index: queueIndex,
          artwork: track.artwork,
          title: track.title,
          artist: track.artist,
          position: offset
        });
      } else if (offset === 0) {
        // Always show current even if queue is empty
        items.push({
          index: queueCurrentIndex,
          artwork,
          title: trackTitle,
          artist,
          position: 0
        });
      }
    }

    return items;
  });

  // Track previous index for animation direction
  let previousIndex = $state(queueCurrentIndex);
  let animationDirection = $state<'left' | 'right' | null>(null);
  let animationTimeout: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    if (queueCurrentIndex !== previousIndex) {
      animationDirection = queueCurrentIndex > previousIndex ? 'left' : 'right';
      previousIndex = queueCurrentIndex;

      if (animationTimeout) clearTimeout(animationTimeout);
      animationTimeout = setTimeout(() => {
        animationDirection = null;
        animationTimeout = null;
      }, 500);

      return () => {
        if (animationTimeout) { clearTimeout(animationTimeout); animationTimeout = null; }
      };
    }
  });

  function handleCoverClick(index: number) {
    if (index !== queueCurrentIndex && onNavigate) {
      onNavigate(index);
    }
  }
</script>

<div class="coverflow-panel">
  <div class="coverflow-container" class:animate-left={animationDirection === 'left'} class:animate-right={animationDirection === 'right'}>
    {#each coverflowItems as item (item.index)}
      <button
        class="coverflow-item"
        class:center={item.position === 0}
        class:left-1={item.position === -1}
        class:left-2={item.position === -2}
        class:right-1={item.position === 1}
        class:right-2={item.position === 2}
        onclick={() => handleCoverClick(item.index)}
        disabled={item.position === 0}
        title={item.position === 0 ? undefined : `${item.title} - ${item.artist}`}
      >
        <div class="cover-wrapper">
          <img src={item.artwork} alt={item.title} class="cover-image" />
          <div class="cover-reflection"></div>
        </div>
      </button>
    {/each}
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
  .coverflow-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 24px;
    padding-top: 70px;
    padding-bottom: 120px;
    padding-left: 40px;
    padding-right: 40px;
    z-index: 5;
    perspective: 1000px;
  }

  .coverflow-container {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    max-width: 900px;
    height: min(45vh, 360px);
    perspective: 1200px;
    transform-style: preserve-3d;
  }

  .coverflow-item {
    position: absolute;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    transition: all 500ms cubic-bezier(0.4, 0, 0.2, 1);
    transform-style: preserve-3d;
  }

  .coverflow-item:disabled {
    cursor: default;
  }

  .coverflow-item:not(:disabled):hover {
    transform: translateX(var(--hover-x, 0)) rotateY(var(--hover-rotate, 0)) scale(1.05);
  }

  .cover-wrapper {
    position: relative;
    width: min(38vh, 300px);
    height: min(38vh, 300px);
    border-radius: 8px;
    overflow: visible;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    transition: box-shadow 300ms ease;
  }

  .coverflow-item:not(:disabled):hover .cover-wrapper {
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.3);
  }

  .cover-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 8px;
  }

  .cover-reflection {
    position: absolute;
    bottom: -40%;
    left: 0;
    right: 0;
    height: 40%;
    background: linear-gradient(
      to bottom,
      rgba(255, 255, 255, 0.05) 0%,
      transparent 100%
    );
    transform: scaleY(-1);
    mask-image: linear-gradient(to bottom, rgba(0, 0, 0, 0.15) 0%, transparent 40%);
    -webkit-mask-image: linear-gradient(to bottom, rgba(0, 0, 0, 0.15) 0%, transparent 40%);
    pointer-events: none;
    border-radius: 8px;
  }

  /* Center item */
  .coverflow-item.center {
    z-index: 5;
    transform: translateX(0) rotateY(0deg) scale(1);
  }

  .coverflow-item.center .cover-wrapper {
    width: min(45vh, 360px);
    height: min(45vh, 360px);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.3);
  }

  /* Left items */
  .coverflow-item.left-1 {
    z-index: 4;
    transform: translateX(-180px) rotateY(45deg) scale(0.85);
    --hover-x: -170px;
    --hover-rotate: 40deg;
    opacity: 0.9;
  }

  .coverflow-item.left-2 {
    z-index: 3;
    transform: translateX(-300px) rotateY(55deg) scale(0.7);
    --hover-x: -290px;
    --hover-rotate: 50deg;
    opacity: 0.6;
  }

  /* Right items */
  .coverflow-item.right-1 {
    z-index: 4;
    transform: translateX(180px) rotateY(-45deg) scale(0.85);
    --hover-x: 170px;
    --hover-rotate: -40deg;
    opacity: 0.9;
  }

  .coverflow-item.right-2 {
    z-index: 3;
    transform: translateX(300px) rotateY(-55deg) scale(0.7);
    --hover-x: 290px;
    --hover-rotate: -50deg;
    opacity: 0.6;
  }

  /* Animation classes */
  .coverflow-container.animate-left .coverflow-item {
    animation: slideLeft 500ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  .coverflow-container.animate-right .coverflow-item {
    animation: slideRight 500ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  @keyframes slideLeft {
    0% { transform: translateX(calc(var(--translate-x, 0px) + 100px)); }
  }

  @keyframes slideRight {
    0% { transform: translateX(calc(var(--translate-x, 0px) - 100px)); }
  }

  /* Track info */
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
  @media (max-width: 900px) {
    .coverflow-item.left-1 {
      transform: translateX(-140px) rotateY(45deg) scale(0.8);
      --hover-x: -130px;
    }

    .coverflow-item.left-2 {
      transform: translateX(-230px) rotateY(55deg) scale(0.65);
      --hover-x: -220px;
    }

    .coverflow-item.right-1 {
      transform: translateX(140px) rotateY(-45deg) scale(0.8);
      --hover-x: 130px;
    }

    .coverflow-item.right-2 {
      transform: translateX(230px) rotateY(-55deg) scale(0.65);
      --hover-x: 220px;
    }
  }

  @media (max-width: 768px) {
    .coverflow-panel {
      padding: 70px 16px 130px;
      gap: 16px;
    }

    .cover-wrapper {
      width: min(30vh, 200px);
      height: min(30vh, 200px);
    }

    .coverflow-item.center .cover-wrapper {
      width: min(38vh, 260px);
      height: min(38vh, 260px);
    }

    .coverflow-item.left-1 {
      transform: translateX(-100px) rotateY(45deg) scale(0.75);
    }

    .coverflow-item.left-2 {
      display: none;
    }

    .coverflow-item.right-1 {
      transform: translateX(100px) rotateY(-45deg) scale(0.75);
    }

    .coverflow-item.right-2 {
      display: none;
    }
  }

  @media (max-height: 600px) {
    .cover-wrapper {
      width: min(28vh, 180px);
      height: min(28vh, 180px);
    }

    .coverflow-item.center .cover-wrapper {
      width: min(32vh, 220px);
      height: min(32vh, 220px);
    }

    .track-info {
      gap: 4px;
    }
  }
</style>
