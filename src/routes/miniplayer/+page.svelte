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
    Pin,
    PinOff,
    Maximize2,
    Volume2,
    VolumeX,
    Volume1,
    ListMusic
  } from 'lucide-svelte';
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
  import { exitMiniplayerMode, setMiniplayerAlwaysOnTop } from '$lib/services/miniplayerService';

  // Player state
  let playerState = $state<PlayerState>(getPlayerState());
  let isShuffle = $state(false);
  let repeatMode = $state<RepeatMode>('off');
  let isPinned = $state(true);
  let isDragging = $state(false);
  let isDraggingProgress = $state(false);
  let queueCount = $state(0);

  // Refs
  let progressRef: HTMLDivElement;

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
      isShuffle = qState.isShuffle;
      repeatMode = qState.repeatMode;
      queueCount = qState.tracks.length;
    });

    const qState = getQueueState();
    isShuffle = qState.isShuffle;
    repeatMode = qState.repeatMode;
    queueCount = qState.tracks.length;
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

  // Window controls - stop propagation to prevent drag from capturing
  async function handleRestore(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    console.log('[MiniPlayer] Restore button clicked');
    await exitMiniplayerMode();
  }

  async function togglePin(e: MouseEvent): Promise<void> {
    e.stopPropagation();
    e.preventDefault();
    isPinned = !isPinned;
    await setMiniplayerAlwaysOnTop(isPinned);
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

  function handleOpenQueue(e: MouseEvent): void {
    e.stopPropagation();
    console.log('[MiniPlayer] Queue button clicked, tracks:', queueCount);
  }
</script>

<div
  class="miniplayer"
  class:dragging={isDragging}
  role="application"
  aria-label="MiniPlayer"
  onmousedown={startDrag}
>
  <!-- Album Art -->
  <div class="artwork-section">
    {#if playerState.currentTrack?.artwork}
      <img src={playerState.currentTrack.artwork} alt="Album art" class="artwork" />
    {:else}
      <div class="artwork-placeholder">
        <Play size={24} />
      </div>
    {/if}
  </div>

  <!-- Main Content -->
  <div class="content-section">
    <!-- Header: Track info + window controls -->
    <div class="header">
      <div class="track-info">
        <div class="title">{playerState.currentTrack?.title ?? 'No track'}</div>
        <div class="artist">{playerState.currentTrack?.artist ?? '—'}</div>
      </div>
      <div class="window-controls">
        <button class="window-btn" onclick={togglePin} title={isPinned ? 'Unpin' : 'Pin'}>
          {#if isPinned}
            <Pin size={12} />
          {:else}
            <PinOff size={12} />
          {/if}
        </button>
        <button class="window-btn restore" onclick={handleRestore} title="Restore">
          <Maximize2 size={12} />
        </button>
      </div>
    </div>

    <!-- Progress Bar -->
    <div class="progress-section">
      <span class="time">{formatTime(playerState.currentTime)}</span>
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
      <span class="time">-{formatTime(Math.max(0, playerState.duration - playerState.currentTime))}</span>
    </div>

    <!-- Controls Row -->
    <div class="controls-row">
      <button
        class="ctrl-btn"
        class:active={isShuffle}
        onclick={handleToggleShuffle}
        title="Shuffle"
      >
        <Shuffle size={12} />
      </button>

      <button class="ctrl-btn" onclick={handlePrevious} title="Previous">
        <SkipBack size={14} />
      </button>

      <button class="ctrl-btn play" onclick={handlePlayPause} title={playerState.isPlaying ? 'Pause' : 'Play'}>
        {#if playerState.isPlaying}
          <Pause size={16} />
        {:else}
          <Play size={16} />
        {/if}
      </button>

      <button class="ctrl-btn" onclick={handleNext} title="Next">
        <SkipForward size={14} />
      </button>

      <button
        class="ctrl-btn"
        class:active={repeatMode !== 'off'}
        onclick={handleToggleRepeat}
        title="Repeat"
      >
        {#if repeatMode === 'one'}
          <Repeat1 size={12} />
        {:else}
          <Repeat size={12} />
        {/if}
      </button>

      <button
        class="ctrl-btn"
        onclick={handleOpenQueue}
        title="Queue ({queueCount})"
      >
        <ListMusic size={12} />
      </button>
    </div>
  </div>
</div>

<style>
  .miniplayer {
    display: flex;
    width: 100%;
    height: 100%;
    background: transparent;
    color: white;
    user-select: none;
    overflow: hidden;
    cursor: grab;
  }

  .miniplayer.dragging {
    cursor: grabbing;
  }

  /* Album Art */
  .artwork-section {
    width: 124px;
    height: 124px;
    flex-shrink: 0;
  }

  .artwork {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 6px 0 0 6px;
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    background: linear-gradient(135deg, #27272a 0%, #18181b 100%);
    display: flex;
    align-items: center;
    justify-content: center;
    color: rgba(255, 255, 255, 0.2);
    border-radius: 6px 0 0 6px;
  }

  /* Content */
  .content-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 8px 10px;
    min-width: 0;
    justify-content: space-between;
  }

  /* Header */
  .header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 6px;
  }

  .track-info {
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .title {
    font-weight: 600;
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: #fff;
    line-height: 1.3;
  }

  .artist {
    font-size: 10px;
    color: rgba(255, 255, 255, 0.5);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .window-controls {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }

  .window-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: rgba(255, 255, 255, 0.06);
    border: none;
    color: rgba(255, 255, 255, 0.5);
    cursor: pointer;
    border-radius: 4px;
    transition: all 0.15s ease;
  }

  .window-btn:hover {
    background: rgba(255, 255, 255, 0.12);
    color: rgba(255, 255, 255, 0.9);
  }

  .window-btn.restore:hover {
    background: rgba(99, 102, 241, 0.3);
    color: #a5b4fc;
  }

  /* Progress */
  .progress-section {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .time {
    font-size: 9px;
    font-family: var(--font-mono, monospace);
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.4);
    min-width: 28px;
  }

  .progress-bar {
    flex: 1;
    height: 12px;
    display: flex;
    align-items: center;
    cursor: pointer;
  }

  .progress-track {
    width: 100%;
    height: 3px;
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

  /* Controls */
  .controls-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
  }

  .ctrl-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
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
    width: 28px;
    height: 28px;
    background: rgba(255, 255, 255, 0.1);
  }

  .ctrl-btn.play:hover {
    background: rgba(255, 255, 255, 0.18);
  }
</style>
