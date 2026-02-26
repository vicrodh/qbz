<script lang="ts">
  import { Shuffle, SkipBack, Play, Pause, SkipForward, Repeat, Repeat1, Volume2, VolumeX, Volume1 } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import MiniPlayerWindowControls from './MiniPlayerWindowControls.svelte';
  import type { MiniPlayerSurface } from './types';

  interface Props {
    isPlaying: boolean;
    currentTime: number;
    duration: number;
    volume: number;
    isShuffle: boolean;
    repeatMode: 'off' | 'all' | 'one';
    compact?: boolean;
    micro?: boolean;
    trackTitle?: string;
    trackArtist?: string;
    activeSurface?: MiniPlayerSurface;
    isPinned?: boolean;
    onTogglePlay: () => void;
    onSkipBack: () => void;
    onSkipForward: () => void;
    onSeek: (time: number) => void;
    onVolumeChange: (volume: number) => void;
    onToggleShuffle: () => void;
    onToggleRepeat: () => void;
    onSurfaceChange?: (surface: MiniPlayerSurface) => void;
    onTogglePin?: () => void;
    onExpand?: () => void;
    onClose?: () => void;
    onStartDrag?: (event: MouseEvent) => void;
  }

  let {
    isPlaying,
    currentTime,
    duration,
    volume,
    isShuffle,
    repeatMode,
    compact = false,
    micro = false,
    trackTitle,
    trackArtist,
    activeSurface,
    isPinned = false,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    onSeek,
    onVolumeChange,
    onToggleShuffle,
    onToggleRepeat,
    onSurfaceChange,
    onTogglePin,
    onExpand,
    onClose,
    onStartDrag
  }: Props = $props();

  let seekRef: HTMLDivElement | null = $state(null);
  let seekBottomRef: HTMLDivElement | null = $state(null);
  let volumeRef: HTMLDivElement | null = $state(null);
  let volumeButtonRef: HTMLButtonElement | null = $state(null);
  let microTrackRef: HTMLDivElement | null = $state(null);
  let microTrackTextRef: HTMLSpanElement | null = $state(null);
  let isDraggingSeek = $state(false);
  let isDraggingVolume = $state(false);
  let volumePopoverOpen = $state(false);
  let isMuted = $state(false);
  let previousVolume = $state(75);
  let microTrackOverflow = $state(0);

  const progress = $derived(duration > 0 ? Math.max(0, Math.min(100, (currentTime / duration) * 100)) : 0);
  const displayVolume = $derived(isMuted ? 0 : volume);
  const tickerSpeed = 40;
  const microTrackOffset = $derived(microTrackOverflow > 0 ? `-${microTrackOverflow + 16}px` : '0px');
  const microTrackDuration = $derived(microTrackOverflow > 0 ? `${(microTrackOverflow + 16) / tickerSpeed}s` : '0s');
  const canRenderInlineWindowControls = $derived(
    micro && !!activeSurface && !!onSurfaceChange && !!onTogglePin && !!onExpand && !!onClose && !!onStartDrag
  );

  function getMicroTrackLine(): string {
    if (!trackTitle) return $t('player.noTrackPlaying');
    const artistName = trackArtist?.trim();
    return artistName ? `${trackTitle} - ${artistName}` : trackTitle;
  }

  function formatTime(seconds: number): string {
    if (!seconds || !isFinite(seconds)) return '0:00';
    const minutes = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${minutes}:${secs.toString().padStart(2, '0')}`;
  }

  function updateMicroTrackOverflow(): void {
    if (!microTrackRef || !microTrackTextRef) {
      microTrackOverflow = 0;
      return;
    }

    const overflow = microTrackTextRef.scrollWidth - microTrackRef.clientWidth;
    microTrackOverflow = overflow > 0 ? overflow : 0;
  }

  function updateSeek(event: MouseEvent, targetRef: HTMLDivElement | null): void {
    if (!targetRef || duration <= 0) return;
    const rect = targetRef.getBoundingClientRect();
    const percentage = Math.max(0, Math.min(100, ((event.clientX - rect.left) / rect.width) * 100));
    onSeek(Math.round((percentage / 100) * duration));
  }

  function updateVolume(event: MouseEvent): void {
    if (!volumeRef) return;
    const rect = volumeRef.getBoundingClientRect();
    const percentage = Math.max(0, Math.min(100, ((event.clientX - rect.left) / rect.width) * 100));
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
      updateSeek(event, micro ? seekBottomRef : seekRef);
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

  $effect(() => {
    if (!micro) return;
    trackTitle;
    trackArtist;

    requestAnimationFrame(() => {
      updateMicroTrackOverflow();
    });
  });

  $effect(() => {
    if (!micro || !microTrackRef || typeof ResizeObserver === 'undefined') return;

    const observer = new ResizeObserver(() => {
      updateMicroTrackOverflow();
    });
    observer.observe(microTrackRef);

    return () => {
      observer.disconnect();
    };
  });
</script>

<div class="footer" class:compact class:micro>
{#if micro}
    <div class="micro-header" data-tauri-drag-region>
      <div
        class="micro-track"
        class:scrollable={microTrackOverflow > 0}
        style="--ticker-offset: {microTrackOffset}; --ticker-duration: {microTrackDuration};"
        bind:this={microTrackRef}
        title={getMicroTrackLine()}
      >
        <span class="micro-track-text" bind:this={microTrackTextRef}>{getMicroTrackLine()}</span>
      </div>
      {#if canRenderInlineWindowControls}
        <div class="micro-window-controls">
          <MiniPlayerWindowControls
            micro
            activeSurface={activeSurface!}
            isPinned={isPinned}
            onSurfaceChange={onSurfaceChange!}
            onTogglePin={onTogglePin!}
            onExpand={onExpand!}
            onClose={onClose!}
            onStartDrag={onStartDrag!}
          />
        </div>
      {/if}
    </div>
  {/if}

  {#if !micro}
    <div
      class="seek-wrapper"
      bind:this={seekRef}
      onmousedown={(event) => {
        isDraggingSeek = true;
        updateSeek(event, seekRef);
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
  {/if}

  <div class="controls-row" class:micro-controls={micro}>
    <div class="transport">
      <button class="ctrl-btn" class:active={isShuffle} onclick={onToggleShuffle} title={$t('player.shuffle')}>
        <Shuffle size={micro ? 8 : compact ? 13 : 16} />
      </button>

      <button class="ctrl-btn" onclick={onSkipBack} title={$t('player.previous')}>
        <SkipBack size={micro ? 9 : compact ? 15 : 18} fill="currentColor" />
      </button>

      <button
        class="ctrl-btn"
        onclick={onTogglePlay}
        title={isPlaying ? $t('player.pause') : $t('player.play')}
        aria-label={isPlaying ? $t('player.pause') : $t('player.play')}
      >
        {#if isPlaying}
          <Pause size={micro ? 9 : compact ? 16 : 20} fill="currentColor" />
        {:else}
          <Play size={micro ? 9 : compact ? 16 : 20} fill="currentColor" class="play-icon" />
        {/if}
      </button>

      <button class="ctrl-btn" onclick={onSkipForward} title={$t('player.next')}>
        <SkipForward size={micro ? 9 : compact ? 15 : 18} fill="currentColor" />
      </button>

      <button
        class="ctrl-btn"
        class:active={repeatMode !== 'off'}
        onclick={onToggleRepeat}
        title={repeatMode === 'off' ? $t('player.repeat') : repeatMode === 'all' ? $t('player.repeatAll') : $t('player.repeatOne')}
      >
        {#if repeatMode === 'one'}
          <Repeat1 size={micro ? 8 : compact ? 13 : 16} />
        {:else}
          <Repeat size={micro ? 8 : compact ? 13 : 16} />
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
          <VolumeX size={micro ? 10 : compact ? 15 : 18} strokeWidth={2.25} />
        {:else if displayVolume < 50}
          <Volume1 size={micro ? 10 : compact ? 15 : 18} strokeWidth={2.25} />
        {:else}
          <Volume2 size={micro ? 10 : compact ? 15 : 18} strokeWidth={2.25} />
        {/if}
      </button>

      {#if volumePopoverOpen}
        <div class="volume-popover" role="group" aria-label={$t('player.volume')} onmousedown={(event) => event.stopPropagation()}>
          <button class="ctrl-btn mute-btn" onclick={toggleMute} title={isMuted ? $t('player.unmute') : $t('player.mute')}>
            <VolumeX size={micro ? 10 : 14} strokeWidth={2.25} />
          </button>

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
            <div class="volume-level" style="width: {displayVolume}%"></div>
            <div class="volume-thumb" style="left: {displayVolume}%" class:visible={isDraggingVolume}></div>
          </div>

          <span class="volume-value">{displayVolume}</span>
        </div>
      {/if}
    </div>
  </div>

  {#if micro}
    <div
      class="seek-wrapper micro-seek"
      bind:this={seekBottomRef}
      onmousedown={(event) => {
        isDraggingSeek = true;
        updateSeek(event, seekBottomRef);
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
  {/if}
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

  .footer.micro {
    border-top: none;
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 1px 8px 0;
  }

  .micro-header {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 17px;
    margin-bottom: 0;
  }

  .micro-track {
    width: calc(100% - 52px);
    max-width: calc(100% - 52px);
    min-width: 0;
    padding: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: center;
    color: var(--alpha-70);
    font-size: 11px;
    line-height: 1.15;
    letter-spacing: 0.01em;
  }

  .micro-track.scrollable {
    text-overflow: clip;
  }

  .micro-track-text {
    display: inline-block;
    white-space: nowrap;
  }

  .micro-header:hover .micro-track.scrollable .micro-track-text {
    animation: micro-track-ticker var(--ticker-duration) linear infinite;
    will-change: transform;
  }

  @keyframes micro-track-ticker {
    0%, 20% {
      transform: translateX(0);
    }
    70%, 80% {
      transform: translateX(var(--ticker-offset));
    }
    90%, 100% {
      transform: translateX(0);
    }
  }

  .micro-window-controls {
    position: absolute;
    right: 0;
    top: 50%;
    transform: translateY(-50%);
    -webkit-app-region: no-drag;
    app-region: no-drag;
    opacity: 0;
    pointer-events: none;
    contain: paint;
    transition: opacity 120ms ease;
  }

  .micro-header:hover .micro-window-controls,
  .micro-window-controls:hover {
    opacity: 1;
    pointer-events: auto;
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
    display: none;
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
    width: 100%;
    margin-top: 3px;
    min-height: 40px;
  }

  .transport {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
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

  .ctrl-btn :global(.play-icon) {
    margin-left: 2px;
  }

  .volume-anchor {
    position: absolute;
    left: 0;
    top: 50%;
    transform: translateY(-50%);
  }

  .volume-trigger {
    width: 30px;
    height: 30px;
  }

  .volume-popover {
    position: absolute;
    left: calc(100% + 8px);
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 7px;
    padding: 7px 8px;
    background: var(--bg-secondary);
    border: 1px solid var(--alpha-12);
    border-radius: 999px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    z-index: 120;
    pointer-events: auto;
  }

  .volume-value {
    color: var(--text-muted);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    min-width: 22px;
    text-align: center;
  }

  .volume-rail {
    position: relative;
    width: 96px;
    height: 5px;
    background: var(--alpha-12);
    border-radius: 999px;
    cursor: pointer;
  }

  .volume-level {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    background: var(--accent-primary);
    border-radius: 999px;
    transition: width 100ms linear;
  }

  .volume-thumb {
    position: absolute;
    top: 50%;
    transform: translate(-50%, -50%);
    width: 9px;
    height: 9px;
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
    width: 20px;
    height: 20px;
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

  .footer.compact .volume-trigger {
    width: 26px;
    height: 26px;
  }

  .footer.micro .controls-row {
    margin-top: 1px;
    min-height: 16px;
  }

  .footer.micro .transport {
    gap: 9px;
  }

  .footer.micro .ctrl-btn {
    width: 16px;
    height: 16px;
  }

  .footer.micro .ctrl-btn :global(.play-icon) {
    margin-left: 0;
  }

  .footer.micro .volume-trigger {
    width: 16px;
    height: 16px;
  }

  .footer.micro .volume-anchor {
    left: 0;
  }

  .footer.micro .volume-popover {
    left: calc(100% + 5px);
    right: auto;
    top: 50%;
    transform: translateY(-50%);
    gap: 5px;
    padding: 6px 7px;
  }

  .footer.micro .volume-rail {
    width: 74px;
    height: 4px;
  }

  .footer.micro .seek-wrapper.micro-seek {
    padding: 0;
    margin: auto -8px 0;
  }

  .footer.micro .seek-wrapper.micro-seek .seek-track,
  .footer.micro .seek-wrapper.micro-seek .seek-fill {
    height: 1px;
    border-radius: 0;
  }

  .footer.micro .seek-wrapper.micro-seek .seek-thumb {
    width: 7px;
    height: 7px;
  }
</style>
