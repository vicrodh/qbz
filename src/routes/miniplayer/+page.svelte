<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    Shuffle,
    SkipBack,
    SkipForward,
    Play,
    Pause,
    Repeat,
    Repeat1,
    Maximize2,
    ListMusic
  } from 'lucide-svelte';
  import StackIcon from '$lib/components/StackIcon.svelte';
  import {
    subscribe as subscribePlayer,
    getPlayerState,
    togglePlay,
    seek as playerSeek,
    setVolume as playerSetVolume,
    startPolling,
    stopPolling,
    type PlayerState
  } from '$lib/stores/playerStore';
  import {
    subscribe as subscribeQueue,
    getQueueState,
    toggleShuffle,
    toggleRepeat,
    nextTrack,
    previousTrack,
    type RepeatMode
  } from '$lib/stores/queueStore';
  import { exitMiniplayerMode } from '$lib/services/miniplayerService';

  // Player state
  let playerState = $state<PlayerState>(getPlayerState());
  let isShuffle = $state(false);
  let repeatMode = $state<RepeatMode>('off');
  let isDragging = $state(false);
  let isDraggingProgress = $state(false);
  let isDraggingVolume = $state(false);
  let queueCount = $state(0);

  // Refs
  let progressRef: HTMLDivElement;
  let volumeRef: HTMLDivElement | null = null;

  // Derived state
  const progress = $derived(playerState.duration > 0 ? (playerState.currentTime / playerState.duration) * 100 : 0);

  function formatTime(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  // Store subscriptions
  let unsubscribePlayer: (() => void) | null = null;
  let unsubscribeQueue: (() => void) | null = null;

  onMount(async () => {
    console.log('[MiniPlayer] Mounting, starting polling...');
    await startPolling();

    unsubscribePlayer = subscribePlayer(() => {
      playerState = getPlayerState();
    });

    unsubscribeQueue = subscribeQueue(() => {
      const qState = getQueueState();
      if (qState) {
        isShuffle = qState.isShuffle ?? false;
        repeatMode = qState.repeatMode ?? 'off';
        queueCount = qState.tracks?.length ?? 0;
      }
    });

    const qState = getQueueState();
    if (qState) {
      isShuffle = qState.isShuffle ?? false;
      repeatMode = qState.repeatMode ?? 'off';
      queueCount = qState.tracks?.length ?? 0;
    }
  });

  onDestroy(() => {
    console.log('[MiniPlayer] Unmounting, stopping polling...');
    unsubscribePlayer?.();
    unsubscribeQueue?.();
    stopPolling();
  });

  // Playback controls
  async function handlePlayPause(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    try {
      await togglePlay();
    } catch (err) {
      console.error('[MiniPlayer] Failed to toggle playback:', err);
    }
  }

  async function handleNext(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    try {
      await nextTrack();
    } catch (err) {
      console.error('[MiniPlayer] Failed to skip to next:', err);
    }
  }

  async function handlePrevious(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    try {
      await previousTrack();
    } catch (err) {
      console.error('[MiniPlayer] Failed to skip to previous:', err);
    }
  }

  async function handleToggleShuffle(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    try {
      await toggleShuffle();
    } catch (err) {
      console.error('[MiniPlayer] Failed to toggle shuffle:', err);
    }
  }

  async function handleToggleRepeat(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    try {
      await toggleRepeat();
    } catch (err) {
      console.error('[MiniPlayer] Failed to toggle repeat:', err);
    }
  }

  // Progress bar
  function handleProgressMouseDown(e: MouseEvent): void {
    e.stopPropagation();
    isDraggingProgress = true;
    updateProgress(e);
    document.addEventListener('mousemove', handleProgressMouseMove);
    document.addEventListener('mouseup', handleProgressMouseUp);
  }

  function handleProgressMouseMove(e: MouseEvent): void {
    if (isDraggingProgress) updateProgress(e);
  }

  function handleProgressMouseUp(): void {
    isDraggingProgress = false;
    document.removeEventListener('mousemove', handleProgressMouseMove);
    document.removeEventListener('mouseup', handleProgressMouseUp);
  }

  function updateProgress(e: MouseEvent): void {
    if (progressRef && playerState.duration > 0) {
      const rect = progressRef.getBoundingClientRect();
      const percentage = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
      const newTime = Math.round((percentage / 100) * playerState.duration);
      playerSeek(newTime);
    }
  }

  function handleVolumeMouseDown(e: MouseEvent): void {
    e.stopPropagation();
    isDraggingVolume = true;
    updateVolume(e);
    document.addEventListener('mousemove', handleVolumeMouseMove);
    document.addEventListener('mouseup', handleVolumeMouseUp);
  }

  function handleVolumeMouseMove(e: MouseEvent): void {
    if (isDraggingVolume) updateVolume(e);
  }

  function handleVolumeMouseUp(): void {
    isDraggingVolume = false;
    document.removeEventListener('mousemove', handleVolumeMouseMove);
    document.removeEventListener('mouseup', handleVolumeMouseUp);
  }

  function updateVolume(e: MouseEvent): void {
    if (!volumeRef) return;
    const rect = volumeRef.getBoundingClientRect();
    const percentage = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
    playerSetVolume(Math.round(percentage));
  }

  // Window controls - stop propagation to prevent drag from capturing
  async function handleRestore(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    console.log('[MiniPlayer] Restore button clicked');
    await exitMiniplayerMode();
  }

  function handleRestoreMouseDown(e: MouseEvent): void {
    e.stopPropagation();
  }

  async function startDrag(): Promise<void> {
    try {
      isDragging = true;
      const window = getCurrentWindow();
      await window.startDragging();
    } catch (err) {
      console.error('[MiniPlayer] Failed to start dragging:', err);
    } finally {
      isDragging = false;
    }
  }

  function handleTopMouseDown(e: MouseEvent): void {
    const target = e.target as HTMLElement | null;
    if (target?.closest('button')) {
      return;
    }
    void startDrag();
  }

  function handleOpenQueue(e: MouseEvent): void {
    e.stopPropagation();
    console.log('[MiniPlayer] Queue button clicked, tracks:', queueCount);
  }

  function handleArtworkClick(e: MouseEvent): void {
    e.stopPropagation();
    // TODO: Cycle through view modes
    console.log('[MiniPlayer] Artwork clicked');
  }
</script>

<div
  class="miniplayer"
  class:dragging={isDragging}
  role="application"
  aria-label="MiniPlayer"
>
  <!-- Top Section: Artwork + Info + Restore -->
  <div class="top-section" onmousedown={handleTopMouseDown}>
    <!-- Album Art (clickable for view modes) -->
    <button class="artwork-section" onclick={handleArtworkClick}>
      {#if playerState.currentTrack?.artwork}
        <img src={playerState.currentTrack.artwork} alt="Album art" class="artwork" />
      {:else}
        <div class="artwork-placeholder"></div>
      {/if}
    </button>

    <!-- Track Info + Restore Button -->
    <div class="info-section">
      <button class="restore-btn" onclick={handleRestore} onmousedown={handleRestoreMouseDown} title="Restore">
        <Maximize2 size={14} />
      </button>
      <div class="track-info">
        <div class="title">{playerState.currentTrack?.title ?? 'No track'}</div>
        <div class="artist-album">
          <StackIcon size={12} class="stack-icon" />
          {playerState.currentTrack?.artist ?? '—'}
          {#if playerState.currentTrack?.album}
            <span class="separator">—</span>
            <span class="album">{playerState.currentTrack.album}</span>
          {/if}
        </div>
      </div>
    </div>
  </div>

  <!-- Seek Bar (full width) -->
  <div class="seek-section">
    <div
      class="progress-bar"
      bind:this={progressRef}
      onmousedown={handleProgressMouseDown}
      role="slider"
      tabindex="0"
      aria-valuenow={playerState.currentTime}
      aria-valuemin={0}
      aria-valuemax={playerState.duration}
    >
      <div class="progress-track">
        <div class="progress-fill" style="width: {progress}%"></div>
      </div>
    </div>
  </div>

  <!-- Bottom: Media Controls + Window Controls -->
  <div class="bottom-section">
    <div class="media-controls">
      <button
        class="ctrl-btn"
        class:active={isShuffle}
        onclick={handleToggleShuffle}
        title="Shuffle"
      >
        <Shuffle size={14} />
      </button>

      <button class="ctrl-btn" onclick={handlePrevious} title="Previous">
        <SkipBack size={16} />
      </button>

      <button class="ctrl-btn play" onclick={handlePlayPause} title={playerState.isPlaying ? 'Pause' : 'Play'}>
        {#if playerState.isPlaying}
          <Pause size={18} />
        {:else}
          <Play size={18} />
        {/if}
      </button>

      <button class="ctrl-btn" onclick={handleNext} title="Next">
        <SkipForward size={16} />
      </button>

      <button
        class="ctrl-btn"
        class:active={repeatMode !== 'off'}
        onclick={handleToggleRepeat}
        title="Repeat"
      >
        {#if repeatMode === 'one'}
          <Repeat1 size={14} />
        {:else}
          <Repeat size={14} />
        {/if}
      </button>
    </div>

    <div class="volume-control">
      <div class="volume-value" class:visible={isDraggingVolume}>{Math.round(playerState.volume)}</div>
      <div
        class="volume-bar"
        bind:this={volumeRef}
        onmousedown={handleVolumeMouseDown}
        role="slider"
        tabindex="0"
        aria-valuenow={playerState.volume}
        aria-valuemin={0}
        aria-valuemax={100}
        title="Volume"
      >
        <div class="volume-fill" style="width: {playerState.volume}%"></div>
        <div class="volume-thumb" style="left: {playerState.volume}%"></div>
      </div>
    </div>

    <div class="window-controls">
      <button class="ctrl-btn" onclick={handleOpenQueue} title="Queue ({queueCount})">
        <ListMusic size={14} />
      </button>
    </div>
  </div>
</div>

<style>
  .miniplayer {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: transparent;
    color: white;
    user-select: none;
    overflow: hidden;
    padding: 8px;
    box-sizing: border-box;
    gap: 6px;
  }

  .miniplayer.dragging {
    cursor: grabbing;
  }

  .miniplayer.dragging * {
    cursor: grabbing;
  }

  /* Top Section: Artwork + Info */
  .top-section {
    display: flex;
    flex: 1;
    gap: 10px;
    min-height: 0;
    cursor: grab;
  }

  /* Album Art */
  .artwork-section {
    aspect-ratio: 1;
    height: 100%;
    flex-shrink: 0;
    cursor: pointer;
    background: none;
    border: none;
    padding: 0;
  }

  .artwork {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 6px;
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    background: rgba(255, 255, 255, 0.06);
    border-radius: 6px;
  }

  /* Info Section */
  .info-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    position: relative;
  }

  .restore-btn {
    position: absolute;
    top: 0;
    right: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: rgba(255, 255, 255, 0.06);
    border: none;
    color: rgba(255, 255, 255, 0.5);
    cursor: pointer;
    border-radius: 4px;
    transition: all 0.15s ease;
  }

  .restore-btn:hover {
    background: rgba(99, 102, 241, 0.3);
    color: #a5b4fc;
  }

  .track-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-width: 0;
    overflow: hidden;
    padding-right: 30px;
  }

  .title {
    font-weight: 600;
    font-size: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: #fff;
    line-height: 1.4;
  }

  .artist-album {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    color: rgba(255, 255, 255, 0.5);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 2px;
  }

  .artist-album :global(.stack-icon) {
    flex-shrink: 0;
  }

  .separator {
    margin: 0 4px;
    opacity: 0.5;
  }

  .album {
    color: rgba(255, 255, 255, 0.4);
  }

  /* Seek Bar */
  .seek-section {
    flex-shrink: 0;
  }

  .progress-bar {
    width: 100%;
    height: 14px;
    display: flex;
    align-items: center;
    cursor: pointer;
  }

  .progress-track {
    width: 100%;
    height: 4px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 2px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: #6366f1;
    border-radius: 2px;
    transition: width 100ms linear;
  }

  /* Bottom Section */
  .bottom-section {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-shrink: 0;
    gap: 12px;
  }

  .media-controls {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .window-controls {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .volume-control {
    position: relative;
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }

  .volume-bar {
    width: 90px;
    height: 4px;
    background: rgba(255, 255, 255, 0.18);
    border-radius: 999px;
    cursor: pointer;
    position: relative;
  }

  .volume-fill {
    height: 100%;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.8);
  }

  .volume-thumb {
    position: absolute;
    top: 50%;
    transform: translate(-50%, -50%);
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #fff;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .volume-bar:hover .volume-thumb {
    opacity: 1;
  }

  .volume-value {
    position: absolute;
    right: 0;
    top: -22px;
    padding: 2px 6px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
    font-size: 11px;
    opacity: 0;
    transform: translateY(4px);
    transition: opacity 150ms ease, transform 150ms ease;
  }

  .volume-value.visible {
    opacity: 1;
    transform: translateY(0);
  }

  .ctrl-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.7);
    cursor: pointer;
    border-radius: 50%;
    transition: all 0.15s ease;
  }

  .ctrl-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }

  .ctrl-btn:active {
    transform: scale(0.95);
  }

  .ctrl-btn.active {
    color: #818cf8;
  }

  .ctrl-btn.play {
    width: 32px;
    height: 32px;
    background: rgba(255, 255, 255, 0.1);
  }

  .ctrl-btn.play:hover {
    background: rgba(255, 255, 255, 0.18);
  }
</style>
