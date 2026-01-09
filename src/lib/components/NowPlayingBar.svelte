<script lang="ts">
  import {
    Shuffle,
    SkipBack,
    Play,
    Pause,
    SkipForward,
    Repeat,
    Heart,
    Plus,
    List,
    Volume2,
    VolumeX,
    Volume1
  } from 'lucide-svelte';

  interface Props {
    artwork?: string;
    trackTitle?: string;
    artist?: string;
    quality?: string;
    qualityLevel?: number;
    isPlaying?: boolean;
    onTogglePlay?: () => void;
    onSkipBack?: () => void;
    onSkipForward?: () => void;
    currentTime?: number;
    duration?: number;
    onSeek?: (time: number) => void;
    volume?: number;
    onVolumeChange?: (volume: number) => void;
    isShuffle?: boolean;
    onToggleShuffle?: () => void;
    repeatMode?: 'off' | 'all' | 'one';
    onToggleRepeat?: () => void;
    isFavorite?: boolean;
    onToggleFavorite?: () => void;
    onOpenQueue?: () => void;
    onOpenFullScreen?: () => void;
  }

  let {
    artwork = '',
    trackTitle = '',
    artist = '',
    quality = '',
    qualityLevel = 0,
    isPlaying = false,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    currentTime = 0,
    duration = 0,
    onSeek,
    volume = 70,
    onVolumeChange,
    isShuffle = false,
    onToggleShuffle,
    repeatMode = 'off',
    onToggleRepeat,
    isFavorite = false,
    onToggleFavorite,
    onOpenQueue,
    onOpenFullScreen
  }: Props = $props();

  let progressRef: HTMLDivElement;
  let volumeRef: HTMLDivElement;
  let isDraggingProgress = $state(false);
  let isDraggingVolume = $state(false);

  const progress = $derived(duration > 0 ? (currentTime / duration) * 100 : 0);
  const hasTrack = $derived(trackTitle !== '');

  function formatTime(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function handleProgressMouseDown(e: MouseEvent) {
    isDraggingProgress = true;
    updateProgress(e);
  }

  function updateProgress(e: MouseEvent) {
    if (progressRef) {
      const rect = progressRef.getBoundingClientRect();
      const percentage = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
      const newTime = Math.round((percentage / 100) * duration);
      onSeek?.(newTime);
    }
  }

  function handleVolumeMouseDown(e: MouseEvent) {
    isDraggingVolume = true;
    updateVolume(e);
  }

  function updateVolume(e: MouseEvent) {
    if (volumeRef) {
      const rect = volumeRef.getBoundingClientRect();
      const percentage = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
      onVolumeChange?.(Math.round(percentage));
    }
  }

  function handleMouseMove(e: MouseEvent) {
    if (isDraggingProgress) updateProgress(e);
    if (isDraggingVolume) updateVolume(e);
  }

  function handleMouseUp() {
    isDraggingProgress = false;
    isDraggingVolume = false;
  }

  function handleRepeatClick() {
    onToggleRepeat?.();
  }

  $effect(() => {
    if (isDraggingProgress || isDraggingVolume) {
      document.addEventListener('mousemove', handleMouseMove);
      document.addEventListener('mouseup', handleMouseUp);
      return () => {
        document.removeEventListener('mousemove', handleMouseMove);
        document.removeEventListener('mouseup', handleMouseUp);
      };
    }
  });
</script>

<div class="now-playing-bar">
  <!-- Left Section - Track Info -->
  <div class="track-info">
    {#if hasTrack}
      <button class="artwork-btn" onclick={onOpenFullScreen}>
        {#if artwork}
          <img src={artwork} alt={trackTitle} />
        {:else}
          <div class="artwork-placeholder">
            <Play size={20} />
          </div>
        {/if}
      </button>
      <div class="track-details">
        <div class="track-title">{trackTitle}</div>
        <div class="track-artist">{artist}</div>
        <div class="track-quality">
          <span>{quality}</span>
          {#if quality}
            <span>•</span>
            <div class="quality-dots">
              {#each Array(5) as _, i}
                <div class="dot" class:active={i < qualityLevel}></div>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    {:else}
      <div class="empty-state">
        <span class="empty-text">No track playing</span>
      </div>
    {/if}
  </div>

  <!-- Center Section - Controls & Progress -->
  <div class="controls-section">
    <div class="control-buttons">
      <button
        class="control-btn"
        class:active={isShuffle}
        onclick={onToggleShuffle}
      >
        <Shuffle size={20} />
      </button>
      <button class="control-btn primary" onclick={onSkipBack}>
        <SkipBack size={24} />
      </button>
      <button class="play-btn" onclick={onTogglePlay}>
        {#if isPlaying}
          <Pause size={24} fill="white" color="white" />
        {:else}
          <Play size={24} fill="white" color="white" class="play-icon" />
        {/if}
      </button>
      <button class="control-btn primary" onclick={onSkipForward}>
        <SkipForward size={24} />
      </button>
      <button
        class="control-btn"
        class:active={repeatMode !== 'off'}
        onclick={handleRepeatClick}
      >
        <Repeat size={20} />
        {#if repeatMode === 'one'}
          <span class="repeat-one">1</span>
        {/if}
      </button>
    </div>

    <!-- Progress Bar -->
    <div class="progress-container">
      <span class="time">{formatTime(currentTime)}</span>
      <div
        class="progress-bar"
        bind:this={progressRef}
        onmousedown={handleProgressMouseDown}
        role="slider"
        tabindex="0"
        aria-valuenow={currentTime}
        aria-valuemin={0}
        aria-valuemax={duration}
      >
        <div class="progress-fill" style="width: {progress}%"></div>
        <div class="progress-thumb" style="left: {progress}%"></div>
      </div>
      <span class="time">{formatTime(duration)}</span>
    </div>
  </div>

  <!-- Right Section - Actions & Volume -->
  <div class="actions-section">
    <button
      class="action-btn"
      class:active={isFavorite}
      onclick={onToggleFavorite}
    >
      <Heart size={20} fill={isFavorite ? 'var(--accent-primary)' : 'none'} color={isFavorite ? 'var(--accent-primary)' : '#888888'} />
    </button>
    <button class="action-btn">
      <Plus size={20} />
    </button>
    <button class="action-btn" onclick={onOpenQueue}>
      <List size={20} />
    </button>

    <!-- Volume Control -->
    <div class="volume-control">
      <button class="action-btn" onclick={() => onVolumeChange?.(volume === 0 ? 70 : 0)}>
        {#if volume === 0}
          <VolumeX size={20} />
        {:else if volume < 50}
          <Volume1 size={20} />
        {:else}
          <Volume2 size={20} />
        {/if}
      </button>
      <div
        class="volume-bar"
        bind:this={volumeRef}
        onmousedown={handleVolumeMouseDown}
        role="slider"
        tabindex="0"
        aria-valuenow={volume}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div class="volume-fill" style="width: {volume}%"></div>
        <div class="volume-thumb" style="left: {volume}%"></div>
      </div>
    </div>

    <span class="remaining-time">-{formatTime(Math.max(0, duration - currentTime))}</span>
  </div>
</div>

<style>
  .now-playing-bar {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    height: 80px;
    display: flex;
    align-items: center;
    padding: 0 16px;
    gap: 24px;
    background-color: var(--bg-secondary);
    border-top: 1px solid var(--bg-tertiary);
  }

  /* Track Info */
  .track-info {
    width: 240px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .artwork-btn {
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    transition: opacity 150ms ease;
  }

  .artwork-btn:hover {
    opacity: 0.8;
  }

  .artwork-btn img {
    width: 56px;
    height: 56px;
    border-radius: 4px;
    object-fit: cover;
  }

  .artwork-placeholder {
    width: 56px;
    height: 56px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .empty-state {
    display: flex;
    align-items: center;
    height: 56px;
  }

  .empty-text {
    font-size: 14px;
    color: var(--text-muted);
  }

  .track-details {
    flex: 1;
    min-width: 0;
  }

  .track-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-artist {
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-quality {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: #666666;
    margin-top: 2px;
  }

  .quality-dots {
    display: flex;
    align-items: center;
    gap: 3px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background-color: #444444;
  }

  .dot.active {
    background-color: #888888;
  }

  /* Controls */
  .controls-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .control-buttons {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .control-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 150ms ease;
    position: relative;
  }

  .control-btn:hover {
    color: var(--text-primary);
  }

  .control-btn.primary {
    color: var(--text-primary);
  }

  .control-btn.primary:hover {
    color: var(--text-muted);
  }

  .control-btn.active {
    color: var(--accent-primary);
  }

  .repeat-one {
    position: absolute;
    font-size: 10px;
    font-weight: 700;
    top: -8px;
    right: -4px;
  }

  .play-btn {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    border: none;
    background-color: var(--accent-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .play-btn:hover {
    background-color: var(--accent-hover);
  }

  .play-btn :global(.play-icon) {
    margin-left: 2px;
  }

  /* Progress Bar */
  .progress-container {
    width: 100%;
    max-width: 600px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .time {
    font-size: 12px;
    color: #666666;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  .progress-bar {
    flex: 1;
    height: 4px;
    background-color: #333333;
    border-radius: 9999px;
    position: relative;
    cursor: pointer;
  }

  .progress-fill {
    height: 100%;
    background-color: var(--accent-primary);
    border-radius: 9999px;
    transition: width 50ms linear;
  }

  .progress-thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background-color: white;
    transform: translate(-50%, -50%);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .progress-bar:hover .progress-thumb {
    opacity: 1;
  }

  /* Actions */
  .actions-section {
    width: 200px;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
  }

  .action-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 150ms ease;
  }

  .action-btn:hover {
    color: var(--text-primary);
  }

  /* Volume */
  .volume-control {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: 8px;
  }

  .volume-bar {
    width: 80px;
    height: 4px;
    background-color: #333333;
    border-radius: 9999px;
    position: relative;
    cursor: pointer;
  }

  .volume-fill {
    height: 100%;
    background-color: var(--accent-primary);
    border-radius: 9999px;
  }

  .volume-thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background-color: white;
    transform: translate(-50%, -50%);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .volume-bar:hover .volume-thumb {
    opacity: 1;
  }

  .remaining-time {
    font-size: 12px;
    color: #666666;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    margin-left: 8px;
  }
</style>
