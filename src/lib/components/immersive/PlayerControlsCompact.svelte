<script lang="ts">
  import {
    Play,
    Pause,
    SkipBack,
    SkipForward,
    Shuffle,
    Repeat,
    Heart,
    Volume2,
    VolumeX,
    MoreHorizontal,
    Infinity,
    Maximize,
    Minimize2,
    Minus,
    X,
    MonitorUp,
    Copy
  } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    visible?: boolean;
    isPlaying: boolean;
    currentTime: number;
    duration: number;
    volume: number;
    isShuffle: boolean;
    repeatMode: 'off' | 'all' | 'one';
    isFavorite: boolean;
    isInfinitePlay?: boolean;
    onTogglePlay: () => void;
    onSkipBack?: () => void;
    onSkipForward?: () => void;
    onSeek: (time: number) => void;
    onToggleShuffle: () => void;
    onToggleRepeat: () => void;
    onToggleFavorite: () => void;
    onToggleInfinitePlay?: () => void;
    onVolumeChange: (volume: number) => void;
    // Window controls
    isFullscreen?: boolean;
    isMaximized?: boolean;
    onClose?: () => void;
    onMinimize?: () => void;
    onToggleFullscreen?: () => void;
    onToggleMaximize?: () => void;
  }

  let {
    visible = true,
    isPlaying,
    currentTime,
    duration,
    volume,
    isShuffle,
    repeatMode,
    isFavorite,
    isInfinitePlay = false,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    onSeek,
    onToggleShuffle,
    onToggleRepeat,
    onToggleFavorite,
    onToggleInfinitePlay,
    onVolumeChange,
    isFullscreen = false,
    isMaximized = false,
    onClose,
    onMinimize,
    onToggleFullscreen,
    onToggleMaximize
  }: Props = $props();

  let progressRef: HTMLDivElement | null = $state(null);
  let volumeRef: HTMLDivElement | null = $state(null);
  let isDraggingProgress = $state(false);
  let isDraggingVolume = $state(false);
  let isMuted = $state(false);
  let previousVolume = $state(75);

  const progress = $derived((currentTime / duration) * 100 || 0);
  const displayVolume = $derived(isMuted ? 0 : volume);

  function formatTime(seconds: number): string {
    if (!seconds || !isFinite(seconds)) return '0:00';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
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
      onSeek(Math.round((percentage / 100) * duration));
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
      const newVolume = Math.round(percentage);
      onVolumeChange(newVolume);
      if (newVolume > 0) isMuted = false;
    }
  }

  function toggleMute() {
    if (isMuted) {
      isMuted = false;
      onVolumeChange(previousVolume || 75);
    } else {
      previousVolume = volume;
      isMuted = true;
      onVolumeChange(0);
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

  // Window controls dropdown
  let menuOpen = $state(false);
  let menuBtnRef: HTMLButtonElement | undefined = $state(undefined);
  let menuRef: HTMLDivElement | undefined = $state(undefined);

  function toggleMenu() {
    menuOpen = !menuOpen;
  }

  function handleMenuAction(action: () => void) {
    action();
    menuOpen = false;
  }

  $effect(() => {
    if (menuOpen) {
      const handleClickOutside = (e: MouseEvent) => {
        if (
          menuBtnRef && !menuBtnRef.contains(e.target as Node) &&
          menuRef && !menuRef.contains(e.target as Node)
        ) {
          menuOpen = false;
        }
      };
      document.addEventListener('mousedown', handleClickOutside);
      return () => document.removeEventListener('mousedown', handleClickOutside);
    }
  });
</script>

<div class="controls-wrapper" class:visible>
  <div class="player-bar">
    <!-- All Controls in Single Row -->
    <div class="controls-row">
      <!-- Left: Fav + Shuffle + Repeat -->
      <div class="controls-group left">
        <button
          class="control-btn"
          class:active={isFavorite}
          onclick={onToggleFavorite}
          title={isFavorite ? 'Remove from Favorites' : 'Add to Favorites'}
        >
          <Heart size={12} fill={isFavorite ? 'currentColor' : 'none'} />
        </button>
        <button
          class="control-btn"
          class:active={isShuffle}
          onclick={onToggleShuffle}
          title="Shuffle"
        >
          <Shuffle size={12} />
        </button>
        <button
          class="control-btn"
          class:active={repeatMode !== 'off'}
          onclick={onToggleRepeat}
          title={repeatMode === 'off' ? 'Repeat' : repeatMode === 'all' ? 'Repeat All' : 'Repeat One'}
        >
          <Repeat size={12} />
          {#if repeatMode === 'one'}
            <span class="repeat-one">1</span>
          {/if}
        </button>
        <button
          class="control-btn"
          class:active={isInfinitePlay}
          onclick={onToggleInfinitePlay}
          title={isInfinitePlay ? 'Disable Infinite Play' : 'Enable Infinite Play'}
        >
          <Infinity size={12} />
        </button>
      </div>

      <!-- Center: Time + Playback + Time -->
      <div class="playback-group">
        <span class="time-text">{formatTime(currentTime)}</span>

        <button
          class="control-btn nav"
          onclick={onSkipBack}
          disabled={!onSkipBack}
          title="Previous"
        >
          <SkipBack size={14} fill="currentColor" />
        </button>

        <button
          class="control-btn play-btn"
          onclick={onTogglePlay}
          title={isPlaying ? 'Pause' : 'Play'}
        >
          {#if isPlaying}
            <Pause size={20} fill="currentColor" />
          {:else}
            <Play size={20} fill="currentColor" class="play-icon" />
          {/if}
        </button>

        <button
          class="control-btn nav"
          onclick={onSkipForward}
          disabled={!onSkipForward}
          title="Next"
        >
          <SkipForward size={14} fill="currentColor" />
        </button>

        <span class="time-text">{formatTime(duration)}</span>
      </div>

      <!-- Right: Volume + Menu -->
      <div class="controls-group right">
        <div class="volume-group">
          <button
            class="control-btn"
            onclick={toggleMute}
            title={isMuted ? 'Unmute' : 'Mute'}
          >
            {#if isMuted || displayVolume === 0}
              <VolumeX size={12} />
            {:else}
              <Volume2 size={12} />
            {/if}
          </button>
          <div
            class="volume-bar"
            bind:this={volumeRef}
            onmousedown={handleVolumeMouseDown}
            role="slider"
            tabindex="0"
            aria-valuenow={displayVolume}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <div class="volume-track">
              <div class="volume-fill" style="width: {displayVolume}%"></div>
            </div>
          </div>
        </div>

        <div class="menu-anchor">
          <button
            class="control-btn"
            bind:this={menuBtnRef}
            onclick={toggleMenu}
            title={$t('actions.moreOptions')}
          >
            <MoreHorizontal size={12} />
          </button>
          {#if menuOpen}
            <div class="window-menu" bind:this={menuRef}>
              {#if onToggleFullscreen}
                <button class="menu-item" onclick={() => handleMenuAction(onToggleFullscreen!)}>
                  {#if isFullscreen}
                    <Minimize2 size={14} />
                    <span>{$t('player.exitFullScreen')}</span>
                  {:else}
                    <Maximize size={14} />
                    <span>{$t('player.fullScreen')}</span>
                  {/if}
                </button>
              {/if}
              {#if onToggleMaximize}
                <button class="menu-item" onclick={() => handleMenuAction(onToggleMaximize!)}>
                  {#if isMaximized}
                    <Copy size={14} />
                    <span>{$t('player.restore')}</span>
                  {:else}
                    <MonitorUp size={14} />
                    <span>{$t('player.maximize')}</span>
                  {/if}
                </button>
              {/if}
              {#if onMinimize}
                <button class="menu-item" onclick={() => handleMenuAction(onMinimize!)}>
                  <Minus size={14} />
                  <span>{$t('player.minimize')}</span>
                </button>
              {/if}
              <div class="menu-divider"></div>
              {#if onClose}
                <button class="menu-item" onclick={() => handleMenuAction(onClose!)}>
                  <X size={14} />
                  <span>{$t('player.exitImmersive')}</span>
                </button>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    </div>

    <!-- Progress Bar Below -->
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
      <div class="progress-track">
        <div class="progress-fill" style="width: {progress}%"></div>
      </div>
    </div>
  </div>
</div>

<style>
  .controls-wrapper {
    position: absolute;
    bottom: 24px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 40;
    opacity: 0;
    pointer-events: none;
    transition: opacity 300ms ease, transform 300ms ease;
  }

  .controls-wrapper.visible {
    opacity: 1;
    pointer-events: auto;
  }

  .player-bar {
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(40px);
    -webkit-backdrop-filter: blur(40px);
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: 16px;
    padding: 12px 24px;
    min-width: 700px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .controls-row {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .controls-group {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  /* Equal width for left and right groups to center playback */
  .controls-group.left,
  .controls-group.right {
    min-width: 140px;
  }

  .controls-group.left {
    justify-content: flex-start;
  }

  .controls-group.right {
    gap: 8px;
    justify-content: flex-end;
  }

  .playback-group {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
  }

  .time-text {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.75);
    font-variant-numeric: tabular-nums;
    min-width: 36px;
  }

  .time-text:first-of-type {
    text-align: right;
  }

  .control-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: none;
    border: none;
    border-radius: 50%;
    color: rgba(255, 255, 255, 0.65);
    cursor: pointer;
    transition: all 150ms ease;
    position: relative;
  }

  .control-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .control-btn:not(:disabled):hover {
    color: white;
  }

  .control-btn.active {
    background: rgba(255, 255, 255, 0.25);
    color: white;
  }

  .control-btn.nav {
    width: 28px;
    height: 28px;
    background: rgba(255, 255, 255, 0.15);
    color: white;
  }

  .control-btn.nav:not(:disabled):hover {
    background: rgba(255, 255, 255, 0.25);
  }

  .control-btn.play-btn {
    width: 48px;
    height: 48px;
    background: white;
    color: black;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .control-btn.play-btn:hover {
    background: rgba(255, 255, 255, 0.9);
    transform: scale(1.05);
  }

  .control-btn.play-btn :global(.play-icon) {
    margin-left: 2px;
  }

  .repeat-one {
    position: absolute;
    top: 1px;
    right: 1px;
    font-size: 7px;
    font-weight: 700;
    color: white;
  }

  /* Volume Group */
  .volume-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .volume-bar {
    width: 64px;
    position: relative;
    cursor: pointer;
    padding: 4px 0;
  }

  .volume-track {
    height: 2px;
    background: rgba(255, 255, 255, 0.2);
    border-radius: 1px;
    overflow: hidden;
  }

  .volume-fill {
    height: 100%;
    background: white;
    border-radius: 1px;
  }

  /* Progress Bar */
  .progress-bar {
    margin-top: 8px;
    margin-left: -8px;
    margin-right: -8px;
    position: relative;
    cursor: pointer;
    padding: 4px 0;
  }

  .progress-track {
    height: 2px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 1px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: white;
    border-radius: 1px;
    transition: width 100ms linear;
  }

  /* Window Controls Menu */
  .menu-anchor {
    position: relative;
  }

  .window-menu {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(40px);
    -webkit-backdrop-filter: blur(40px);
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: 12px;
    padding: 6px;
    min-width: 200px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    z-index: 50;
    animation: menuFadeIn 150ms ease-out;
  }

  @keyframes menuFadeIn {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    border-radius: 8px;
    color: rgba(255, 255, 255, 0.8);
    font-size: 13px;
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease;
    white-space: nowrap;
  }

  .menu-item:hover {
    background: rgba(255, 255, 255, 0.12);
    color: white;
  }

  .menu-divider {
    height: 1px;
    background: rgba(255, 255, 255, 0.1);
    margin: 4px 8px;
  }

  /* Responsive */
  @media (max-width: 800px) {
    .player-bar {
      min-width: auto;
      width: calc(100vw - 32px);
      max-width: 700px;
      padding: 10px 16px;
    }

    .controls-row {
      gap: 8px;
    }

    .volume-bar {
      width: 48px;
    }

    .control-btn.play-btn {
      width: 44px;
      height: 44px;
    }
  }

  @media (max-width: 600px) {
    .controls-wrapper {
      bottom: 16px;
    }

    .player-bar {
      border-radius: 12px;
      padding: 8px 12px;
    }

    .volume-group {
      display: none;
    }

    .playback-group {
      gap: 8px;
    }
  }
</style>
