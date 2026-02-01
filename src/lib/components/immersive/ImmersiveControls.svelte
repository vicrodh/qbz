<script lang="ts">
  import { Shuffle, SkipBack, Play, Pause, SkipForward, Repeat, Heart } from 'lucide-svelte';
  import ProgressBar from './shared/ProgressBar.svelte';
  import VolumeSlider from './shared/VolumeSlider.svelte';
  import QualityBadge from '../QualityBadge.svelte';

  interface Props {
    visible?: boolean;
    // Playback state
    isPlaying: boolean;
    currentTime: number;
    duration: number;
    // Controls
    isShuffle: boolean;
    repeatMode: 'off' | 'all' | 'one';
    isFavorite: boolean;
    volume: number;
    // Quality info
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    // Callbacks
    onTogglePlay: () => void;
    onSkipBack?: () => void;
    onSkipForward?: () => void;
    onSeek: (time: number) => void;
    onToggleShuffle: () => void;
    onToggleRepeat: () => void;
    onToggleFavorite: () => void;
    onVolumeChange: (volume: number) => void;
  }

  let {
    visible = true,
    isPlaying,
    currentTime,
    duration,
    isShuffle,
    repeatMode,
    isFavorite,
    volume,
    quality,
    bitDepth,
    samplingRate,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    onSeek,
    onToggleShuffle,
    onToggleRepeat,
    onToggleFavorite,
    onVolumeChange
  }: Props = $props();
</script>

<div class="immersive-controls" class:visible>
  <!-- Center: Playback Controls + Progress -->
  <div class="playback-section">
    <div class="controls">
      <button
        class="control-btn secondary"
        class:active={isShuffle}
        onclick={onToggleShuffle}
        title="Shuffle"
      >
        <Shuffle size={18} />
      </button>

      <button
        class="control-btn"
        onclick={onSkipBack}
        disabled={!onSkipBack}
        title="Previous"
      >
        <SkipBack size={20} />
      </button>

      <button
        class="control-btn play-btn"
        onclick={onTogglePlay}
        title={isPlaying ? 'Pause' : 'Play'}
      >
        {#if isPlaying}
          <Pause size={24} />
        {:else}
          <Play size={24} class="play-icon" />
        {/if}
      </button>

      <button
        class="control-btn"
        onclick={onSkipForward}
        disabled={!onSkipForward}
        title="Next"
      >
        <SkipForward size={20} />
      </button>

      <button
        class="control-btn secondary"
        class:active={repeatMode !== 'off'}
        onclick={onToggleRepeat}
        title={repeatMode === 'off' ? 'Repeat' : repeatMode === 'all' ? 'Repeat All' : 'Repeat One'}
      >
        <Repeat size={18} />
        {#if repeatMode === 'one'}
          <span class="repeat-indicator">1</span>
        {/if}
      </button>
    </div>

    <div class="progress-wrapper">
      <ProgressBar {currentTime} {duration} {onSeek} />
    </div>
  </div>

  <!-- Right: Volume + Actions -->
  <div class="right-section">
    <button
      class="action-btn"
      class:active={isFavorite}
      onclick={onToggleFavorite}
      title={isFavorite ? 'Remove from Favorites' : 'Add to Favorites'}
    >
      <Heart
        size={18}
        fill={isFavorite ? 'currentColor' : 'none'}
      />
    </button>

    <div class="volume-wrapper">
      <VolumeSlider {volume} {onVolumeChange} />
    </div>

    {#if quality}
      <div class="quality-wrapper">
        <QualityBadge {quality} {bitDepth} {samplingRate} />
      </div>
    {/if}
  </div>
</div>

<style>
  .immersive-controls {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    z-index: 20;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 20px 32px 28px;
    background: linear-gradient(
      to top,
      rgba(0, 0, 0, 0.85) 0%,
      rgba(0, 0, 0, 0.5) 50%,
      transparent 100%
    );
    opacity: 0;
    transform: translateY(8px);
    transition: opacity 250ms ease, transform 250ms ease;
    pointer-events: none;
  }

  .immersive-controls.visible {
    opacity: 1;
    transform: translateY(0);
    pointer-events: auto;
  }

  /* Playback Section (Center) */
  .playback-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    max-width: 600px;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .control-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    background: none;
    border: none;
    border-radius: 50%;
    color: var(--alpha-85, rgba(255, 255, 255, 0.85));
    cursor: pointer;
    transition: all 150ms ease;
  }

  .control-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .control-btn:not(:disabled):hover {
    color: var(--text-primary, white);
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .control-btn.secondary {
    width: 36px;
    height: 36px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
  }

  .control-btn.secondary.active {
    color: var(--accent-primary, #7c3aed);
  }

  .control-btn.play-btn {
    width: 52px;
    height: 52px;
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
    color: var(--text-primary, white);
  }

  .control-btn.play-btn:hover {
    background: var(--alpha-25, rgba(255, 255, 255, 0.25));
    transform: scale(1.05);
  }

  .control-btn.play-btn :global(.play-icon) {
    margin-left: 2px;
  }

  .repeat-indicator {
    position: absolute;
    top: 4px;
    right: 4px;
    font-size: 9px;
    font-weight: 700;
  }

  .progress-wrapper {
    width: 100%;
  }

  /* Right Section */
  .right-section {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 200px;
    justify-content: flex-end;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    background: none;
    border: none;
    border-radius: 50%;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    cursor: pointer;
    transition: all 150ms ease;
  }

  .action-btn:hover {
    color: var(--text-primary, white);
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .action-btn.active {
    color: var(--accent-primary, #7c3aed);
  }

  .volume-wrapper {
    margin-left: 8px;
  }

  .quality-wrapper {
    margin-left: 8px;
  }

  /* Responsive */
  @media (max-width: 900px) {
    .right-section {
      display: none;
    }

    .immersive-controls {
      padding: 16px 24px 24px;
    }

    .playback-section {
      max-width: 100%;
    }
  }

  @media (max-width: 600px) {
    .immersive-controls {
      padding: 12px 16px 20px;
      gap: 12px;
    }

    .controls {
      gap: 12px;
    }

    .control-btn {
      width: 36px;
      height: 36px;
    }

    .control-btn.play-btn {
      width: 48px;
      height: 48px;
    }

    .control-btn.secondary {
      width: 32px;
      height: 32px;
    }
  }
</style>
