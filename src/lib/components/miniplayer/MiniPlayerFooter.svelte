<script lang="ts">
  import { Shuffle, SkipBack, Play, Pause, SkipForward, Repeat, Repeat1, Volume2, VolumeX, Volume1 } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    isPlaying: boolean;
    currentTime: number;
    duration: number;
    volume: number;
    isShuffle: boolean;
    repeatMode: 'off' | 'all' | 'one';
    compact?: boolean;
    onTogglePlay: () => void;
    onSkipBack: () => void;
    onSkipForward: () => void;
    onSeek: (time: number) => void;
    onVolumeChange: (volume: number) => void;
    onToggleShuffle: () => void;
    onToggleRepeat: () => void;
  }

  let {
    isPlaying,
    currentTime,
    duration,
    volume,
    isShuffle,
    repeatMode,
    compact = false,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    onSeek,
    onVolumeChange,
    onToggleShuffle,
    onToggleRepeat
  }: Props = $props();

  let seekRef: HTMLDivElement | null = $state(null);
  let volumeRef: HTMLDivElement | null = $state(null);
  let volumeButtonRef: HTMLButtonElement | null = $state(null);
  let isDraggingSeek = $state(false);
  let isDraggingVolume = $state(false);
  let volumePopoverOpen = $state(false);
  let isMuted = $state(false);
  let previousVolume = $state(75);

  const progress = $derived(duration > 0 ? Math.max(0, Math.min(100, (currentTime / duration) * 100)) : 0);
  const displayVolume = $derived(isMuted ? 0 : volume);

  function formatTime(seconds: number): string {
    if (!seconds || !isFinite(seconds)) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
  }

  function updateSeek(event: MouseEvent): void {
    if (!seekRef || duration <= 0) return;
    const rect = seekRef.getBoundingClientRect();
    const percentage = Math.max(0, Math.min(100, ((event.clientX - rect.left) / rect.width) * 100));
    onSeek(Math.round((percentage / 100) * duration));
  }

  function updateVolume(event: MouseEvent): void {
    if (!volumeRef) return;
    const rect = volumeRef.getBoundingClientRect();
    const percentage = Math.max(0, Math.min(100, ((rect.bottom - event.clientY) / rect.height) * 100));
    const nextVolume = Math.round(percentage);
    onVolumeChange(nextVolume);
    if (nextVolume > 0) {
      isMuted = false;
    }
  }

  function toggleMute(): void {
    if (isMuted) {
      isMuted = false;
      onVolumeChange(previousVolume || 75);
      return;
    }

    previousVolume = volume;
    isMuted = true;
    onVolumeChange(0);
  }

  function handleDocumentMouseDown(event: MouseEvent): void {
    const target = event.target as Node | null;
    if (!target) return;
    const clickedVolume = volumeRef?.contains(target);
    const clickedButton = volumeButtonRef?.contains(target);
    if (!clickedVolume && !clickedButton) {
      volumePopoverOpen = false;
    }
  }

  function handleMouseMove(event: MouseEvent): void {
    if (isDraggingSeek) {
      updateSeek(event);
    }
    if (isDraggingVolume) {
      updateVolume(event);
    }
  }

  function handleMouseUp(): void {
    isDraggingSeek = false;
    isDraggingVolume = false;
  }

  $effect(() => {
    if (!isDraggingSeek && !isDraggingVolume) return;

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  });

  $effect(() => {
    if (!volumePopoverOpen) return;

    document.addEventListener('mousedown', handleDocumentMouseDown);
    return () => {
      document.removeEventListener('mousedown', handleDocumentMouseDown);
    };
  });
</script>

<div class="footer" class:compact>
  <div
    class="seek-wrapper"
    bind:this={seekRef}
    onmousedown={(event) => {
      isDraggingSeek = true;
      updateSeek(event);
    }}
    role="slider"
    tabindex="0"
    aria-valuenow={Math.round(currentTime)}
    aria-valuemin={0}
    aria-valuemax={Math.round(duration)}
    aria-label={$t('player.nowPlaying')}
  >
    <div class="seek-track">
      <div class="seek-fill" style="width: {progress}%"></div>
    </div>
    <div class="seek-thumb" style="left: {progress}%" class:visible={isDraggingSeek}></div>
  </div>

  <div class="times">
    <span>{formatTime(currentTime)}</span>
    <span>{formatTime(duration)}</span>
  </div>

  <div class="controls-row">
    <div class="transport">
      <button class="ctrl-btn" class:active={isShuffle} onclick={onToggleShuffle} title={$t('player.shuffle')}>
        <Shuffle size={compact ? 13 : 16} />
      </button>

      <button class="ctrl-btn" onclick={onSkipBack} title={$t('player.previous')}>
        <SkipBack size={compact ? 15 : 18} fill="currentColor" />
      </button>

      <button
        class="ctrl-btn play"
        onclick={onTogglePlay}
        title={isPlaying ? $t('player.pause') : $t('player.play')}
        aria-label={isPlaying ? $t('player.pause') : $t('player.play')}
      >
        {#if isPlaying}
          <Pause size={compact ? 16 : 20} fill="currentColor" />
        {:else}
          <Play size={compact ? 16 : 20} fill="currentColor" class="play-icon" />
        {/if}
      </button>

      <button class="ctrl-btn" onclick={onSkipForward} title={$t('player.next')}>
        <SkipForward size={compact ? 15 : 18} fill="currentColor" />
      </button>

      <button
        class="ctrl-btn"
        class:active={repeatMode !== 'off'}
        onclick={onToggleRepeat}
        title={repeatMode === 'off' ? $t('player.repeat') : repeatMode === 'all' ? $t('player.repeatAll') : $t('player.repeatOne')}
      >
        {#if repeatMode === 'one'}
          <Repeat1 size={compact ? 13 : 16} />
        {:else}
          <Repeat size={compact ? 13 : 16} />
        {/if}
      </button>
    </div>

    <div class="volume-anchor">
      <button
        class="ctrl-btn volume-trigger"
        bind:this={volumeButtonRef}
        class:active={volumePopoverOpen}
        onclick={() => (volumePopoverOpen = !volumePopoverOpen)}
        title={$t('player.volume')}
        aria-label={$t('player.volume')}
      >
        {#if displayVolume === 0}
          <VolumeX size={compact ? 13 : 14} />
        {:else if displayVolume < 50}
          <Volume1 size={compact ? 13 : 14} />
        {:else}
          <Volume2 size={compact ? 13 : 14} />
        {/if}
      </button>

      {#if volumePopoverOpen}
        <div class="volume-popover" role="group" aria-label={$t('player.volume')}>
          <span class="volume-value">{displayVolume}</span>

          <div
            class="volume-rail"
            bind:this={volumeRef}
            onmousedown={(event) => {
              isDraggingVolume = true;
              updateVolume(event);
            }}
            role="slider"
            tabindex="0"
            aria-valuenow={Math.round(displayVolume)}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <div class="volume-level" style="height: {displayVolume}%"></div>
            <div class="volume-thumb" style="bottom: {displayVolume}%" class:visible={isDraggingVolume}></div>
          </div>

          <button class="ctrl-btn mute-btn" onclick={toggleMute} title={isMuted ? $t('player.unmute') : $t('player.mute')}>
            <VolumeX size={13} />
          </button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .footer {
    position: relative;
    flex-shrink: 0;
    border-top: 1px solid var(--alpha-10);
    background: var(--bg-secondary);
    padding: 0 14px 10px;
    overflow: visible;
  }

  .footer.compact {
    padding: 0 10px 6px;
  }

  .seek-wrapper {
    position: relative;
    cursor: pointer;
    padding: 6px 0;
  }

  .seek-track {
    height: 3px;
    background: var(--alpha-12);
    border-radius: 999px;
    overflow: hidden;
  }

  .seek-fill {
    height: 100%;
    background: var(--accent-primary);
    border-radius: 999px;
    transition: width 120ms linear;
  }

  .seek-thumb {
    position: absolute;
    top: 50%;
    transform: translate(-50%, -50%);
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--accent-primary);
    opacity: 0;
    pointer-events: none;
    transition: opacity 120ms ease;
  }

  .seek-wrapper:hover .seek-thumb,
  .seek-thumb.visible {
    opacity: 1;
  }

  .times {
    display: flex;
    justify-content: space-between;
    margin-top: -1px;
    color: var(--text-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .controls-row {
    position: relative;
    display: flex;
    justify-content: center;
    align-items: center;
    margin-top: 3px;
    min-height: 40px;
  }

  .transport {
    display: flex;
    align-items: center;
    gap: 18px;
  }

  .ctrl-btn {
    width: 30px;
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--alpha-70);
    cursor: pointer;
    transition: background 120ms ease, color 120ms ease, transform 120ms ease;
  }

  .ctrl-btn:hover {
    background: var(--alpha-8);
    color: var(--text-primary);
  }

  .ctrl-btn.active {
    color: var(--accent-primary);
  }

  .ctrl-btn.play {
    width: 40px;
    height: 40px;
    background: var(--accent-primary);
    color: var(--btn-primary-text, #ffffff);
    box-shadow: 0 4px 14px color-mix(in srgb, var(--accent-primary) 38%, transparent);
  }

  .ctrl-btn.play:hover {
    background: color-mix(in srgb, var(--accent-primary) 88%, white);
    transform: scale(1.04);
  }

  .ctrl-btn :global(.play-icon) {
    margin-left: 2px;
  }

  .volume-anchor {
    position: absolute;
    right: 0;
    top: 50%;
    transform: translateY(-50%);
  }

  .volume-trigger {
    width: 28px;
    height: 28px;
  }

  .volume-popover {
    position: absolute;
    right: 0;
    bottom: calc(100% + 8px);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 10px 8px;
    background: var(--bg-secondary);
    border: 1px solid var(--alpha-12);
    border-radius: 10px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    z-index: 50;
  }

  .volume-value {
    color: var(--text-muted);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }

  .volume-rail {
    position: relative;
    width: 6px;
    height: 78px;
    background: var(--alpha-12);
    border-radius: 999px;
    cursor: pointer;
  }

  .volume-level {
    position: absolute;
    bottom: 0;
    left: 0;
    width: 100%;
    background: var(--accent-primary);
    border-radius: 999px;
    transition: height 100ms linear;
  }

  .volume-thumb {
    position: absolute;
    left: 50%;
    transform: translate(-50%, 50%);
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--accent-primary);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.28);
    opacity: 0;
    transition: opacity 120ms ease;
    pointer-events: none;
  }

  .volume-rail:hover .volume-thumb,
  .volume-thumb.visible {
    opacity: 1;
  }

  .mute-btn {
    width: 24px;
    height: 24px;
  }

  .footer.compact .seek-wrapper {
    padding: 4px 0;
  }

  .footer.compact .seek-track {
    height: 2px;
  }

  .footer.compact .times {
    font-size: 10px;
  }

  .footer.compact .controls-row {
    margin-top: 2px;
    min-height: 34px;
  }

  .footer.compact .transport {
    gap: 14px;
  }

  .footer.compact .ctrl-btn {
    width: 26px;
    height: 26px;
  }

  .footer.compact .ctrl-btn.play {
    width: 34px;
    height: 34px;
  }

  .footer.compact .volume-trigger {
    width: 24px;
    height: 24px;
  }
</style>
