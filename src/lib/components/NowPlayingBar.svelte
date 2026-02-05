<script lang="ts">
  import {
    Shuffle,
    SkipBack,
    Play,
    Pause,
    SkipForward,
    Repeat,
    Repeat1,
    Heart,
    Plus,
    Volume2,
    VolumeX,
    Volume1,
    Cast,
    Mic2,
    Maximize2,
    PictureInPicture2
  } from 'lucide-svelte';
  import QualityBadge from './QualityBadge.svelte';
  import AudioOutputBadges from './AudioOutputBadges.svelte';
  import StackIcon from './StackIcon.svelte';
  import { t } from '$lib/i18n';
  import {
    subscribe as subscribeOffline,
    isOffline as checkIsOffline,
    getOfflineReason,
    type OfflineReason
  } from '$lib/stores/offlineStore';
  import { toggleMute } from '$lib/stores/playerStore';

  interface Props {
    artwork?: string;
    trackTitle?: string;
    artist?: string;
    album?: string;
    quality?: string;
    qualityLevel?: number;
    bitDepth?: number;
    samplingRate?: number;
    originalBitDepth?: number;
    originalSamplingRate?: number;
    format?: string;
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
    onAddToPlaylist?: () => void;
    onOpenQueue?: () => void;
    onOpenFullScreen?: () => void;
    onOpenMiniPlayer?: () => void;
    onCast?: () => void;
    onToggleLyrics?: () => void;
    lyricsActive?: boolean;
    isCastConnected?: boolean;
    onArtistClick?: () => void;
    onAlbumClick?: () => void;
    onTrackClick?: () => void;
    onContextClick?: () => void;
    queueOpen?: boolean;
  }

  let {
    artwork = '',
    trackTitle = '',
    artist = '',
    album = '',
    quality = '',
    qualityLevel = 0,
    bitDepth,
    samplingRate,
    originalBitDepth,
    originalSamplingRate,
    format,
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
    onAddToPlaylist,
    onOpenQueue,
    onOpenFullScreen,
    onOpenMiniPlayer,
    onCast,
    onToggleLyrics,
    lyricsActive = false,
    isCastConnected = false,
    onArtistClick,
    onAlbumClick,
    onTrackClick,
    onContextClick,
    queueOpen = false
  }: Props = $props();

  let progressRef: HTMLDivElement;
  let volumeRef: HTMLDivElement;
  let isDraggingProgress = $state(false);
  let isDraggingVolume = $state(false);
  let showArtworkPreview = $state(false);

  // Offline state
  let isOffline = $state(checkIsOffline());
  let offlineReason = $state<OfflineReason | null>(getOfflineReason());

  $effect(() => {
    const unsubscribe = subscribeOffline(() => {
      isOffline = checkIsOffline();
      offlineReason = getOfflineReason();
    });
    return unsubscribe;
  });

  // Get human-readable offline reason
  function getOfflineReasonText(reason: OfflineReason | null): string {
    switch (reason) {
      case 'no_network':
        return $t('offline.noNetwork');
      case 'not_logged_in':
        return $t('offline.notLoggedIn');
      case 'manual_override':
        return $t('offline.manualMode');
      default:
        return $t('offline.indicator');
    }
  }

  const progress = $derived(duration > 0 ? (currentTime / duration) * 100 : 0);
  const hasTrack = $derived(trackTitle !== '');
  const remainingTime = $derived(Math.max(0, duration - currentTime));

  function formatTime(seconds: number): string {
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
  <!-- Top: Full-width Seekbar -->
  <div class="seekbar-container">
    <span class="time current">{formatTime(currentTime)}</span>
    <div
      class="seekbar"
      bind:this={progressRef}
      onmousedown={handleProgressMouseDown}
      role="slider"
      tabindex="0"
      aria-valuenow={currentTime}
      aria-valuemin={0}
      aria-valuemax={duration}
    >
      <div class="seekbar-track">
        <div class="seekbar-fill" style="width: {progress}%"></div>
      </div>
      <div class="seekbar-thumb" style="left: {progress}%"></div>
    </div>
    <span class="time remaining">-{formatTime(remainingTime)}</span>
  </div>

  <!-- Bottom: Controls Row -->
  <div class="controls-row">
    <!-- Left: Playback Controls -->
    <div class="left-section">
      <button
        class="control-btn"
        class:active={isShuffle}
        onclick={onToggleShuffle}
        title={$t('player.shuffle')}
      >
        <Shuffle size={16} />
      </button>

      <button class="control-btn" onclick={onSkipBack} title={$t('player.previous')}>
        <SkipBack size={18} />
      </button>

      <button class="control-btn play-btn" onclick={onTogglePlay} title={isPlaying ? $t('player.pause') : $t('player.play')}>
        {#if isPlaying}
          <Pause size={20} />
        {:else}
          <Play size={20} />
        {/if}
      </button>

      <button class="control-btn" onclick={onSkipForward} title={$t('player.next')}>
        <SkipForward size={18} />
      </button>

      <button
        class="control-btn"
        class:active={repeatMode !== 'off'}
        onclick={onToggleRepeat}
        title={repeatMode === 'off' ? $t('player.repeat') : repeatMode === 'all' ? $t('player.repeatAll') : $t('player.repeatOne')}
      >
        {#if repeatMode === 'one'}
          <Repeat1 size={16} />
        {:else}
          <Repeat size={16} />
        {/if}
      </button>

      <button class="control-btn" onclick={onAddToPlaylist} title={$t('actions.addToPlaylist')}>
        <Plus size={16} />
      </button>

      <button
        class="control-btn"
        class:active={isFavorite}
        onclick={onToggleFavorite}
        title={isFavorite ? $t('actions.removeFromFavorites') : $t('actions.addToFavorites')}
      >
        <Heart size={16} fill={isFavorite ? 'currentColor' : 'none'} />
      </button>
    </div>

    <!-- Center: Song Card -->
    <div class="center-section">
      {#if hasTrack}
        <div class="song-card">
          <button
            class="artwork-container"
            onclick={onOpenFullScreen}
            onmouseenter={() => showArtworkPreview = true}
            onmouseleave={() => showArtworkPreview = false}
          >
            {#if artwork}
              <img src={artwork} alt={trackTitle} class="artwork" />
            {:else}
              <div class="artwork-placeholder"></div>
            {/if}

            <!-- Artwork Preview on Hover -->
            {#if showArtworkPreview && artwork}
              <div class="artwork-preview">
                <img src={artwork} alt={trackTitle} />
              </div>
            {/if}
          </button>

          <div class="song-info">
            <button class="song-title" title={$t('actions.trackInfo')} onclick={onTrackClick}>{trackTitle}</button>
            <div class="song-meta">
              <StackIcon size={12} class="stack-icon" onClick={onContextClick} />
              {#if artist}
                <button class="meta-link" onclick={onArtistClick} title={$t('actions.goToArtist')}>
                  {artist}
                </button>
              {/if}
              {#if artist && album}
                <span class="meta-separator">·</span>
              {/if}
              {#if album}
                <button class="meta-link" onclick={onAlbumClick} title={$t('actions.goToAlbum')}>
                  {album}
                </button>
              {/if}
            </div>
          </div>

          <div class="quality-indicator">
            <QualityBadge {quality} {bitDepth} {samplingRate} {originalBitDepth} {originalSamplingRate} {format} />
            <div class="audio-badges-row">
              <AudioOutputBadges {samplingRate} />
            </div>
          </div>
        </div>
      {:else}
        <div class="empty-state">
          <span>{$t('player.noTrackPlaying')}</span>
        </div>
      {/if}
    </div>

    <!-- Right: Actions & Volume -->
    <div class="right-section">
      {#if isOffline}
        <div
          class="offline-indicator"
          title={getOfflineReasonText(offlineReason)}
          role="status"
          aria-label={getOfflineReasonText(offlineReason)}
        >
          <img src="/offline-small.svg" alt="" class="offline-icon" aria-hidden="true" />
        </div>
      {/if}

      <button
        class="control-btn"
        class:cast-active={isCastConnected}
        onclick={onCast}
        title={isCastConnected ? $t('player.castingManage') : $t('player.castToDevice')}
      >
        <Cast size={16} />
      </button>

      <button
        class="control-btn"
        class:active={lyricsActive && !isOffline}
        class:disabled={isOffline}
        onclick={isOffline ? undefined : onToggleLyrics}
        disabled={isOffline}
        title={isOffline ? $t('offline.featureDisabled') : $t('player.lyrics')}
        aria-label={isOffline ? $t('offline.featureDisabled') : $t('player.lyrics')}
      >
        <Mic2 size={16} aria-hidden="true" />
      </button>

      <button class="control-btn" onclick={onOpenMiniPlayer} title={$t('player.miniPlayer')}>
        <PictureInPicture2 size={16} />
      </button>

      <button class="control-btn" onclick={onOpenFullScreen} title={$t('player.fullScreen')}>
        <Maximize2 size={16} />
      </button>

      <!-- Volume Control -->
      <div class="volume-control">
        <div class="volume-value" class:visible={isDraggingVolume}>{volume}</div>
        <button
          class="control-btn volume-btn"
          onclick={() => toggleMute()}
          title={volume === 0 ? $t('player.unmute') : $t('player.mute')}
        >
          {#if volume === 0}
            <VolumeX size={16} />
          {:else if volume < 50}
            <Volume1 size={16} />
          {:else}
            <Volume2 size={16} />
          {/if}
        </button>

        <div
          class="volume-slider"
          bind:this={volumeRef}
          onmousedown={handleVolumeMouseDown}
          role="slider"
          tabindex="0"
          aria-valuenow={volume}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <div class="volume-track">
            <div class="volume-fill" style="width: {volume}%"></div>
          </div>
          <div class="volume-thumb" style="left: {volume}%"></div>
        </div>
      </div>

      <!-- Separator -->
      <div class="section-separator"></div>

      <!-- Queue Button (far right) -->
      <button
        class="control-btn queue-btn"
        class:active={queueOpen}
        onclick={onOpenQueue}
        title={$t('player.queue')}
      >
        <svg width="18" height="18" viewBox="0 0 256 256" class="queue-icon" class:open={queueOpen}>
          <path class="queue-play" d="M240,160l-64,40V120Z"/>
          <path class="queue-lines" d="M32,64a8,8,0,0,1,8-8H216a8,8,0,0,1,0,16H40A8,8,0,0,1,32,64Zm104,56H40a8,8,0,0,0,0,16h96a8,8,0,0,0,0-16Zm0,64H40a8,8,0,0,0,0,16h96a8,8,0,0,0,0-16Zm112-24a8,8,0,0,1-3.76,6.78l-64,40A8,8,0,0,1,168,200V120a8,8,0,0,1,12.24-6.78l64,40A8,8,0,0,1,248,160Zm-23.09,0L184,134.43v51.13Z"/>
        </svg>
      </button>
    </div>
  </div>
</div>

<style>
  .now-playing-bar {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    height: 104px;
    background: var(--bg-secondary);
    backdrop-filter: blur(20px);
    border-top: 1px solid var(--border-subtle);
    z-index: 100;
    display: flex;
    flex-direction: column;
  }

  /* ===== Seekbar ===== */
  .seekbar-container {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    height: 24px;
    flex-shrink: 0;
  }

  .time {
    font-size: 11px;
    font-family: var(--font-mono, monospace);
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
    min-width: 40px;
  }

  .time.current {
    text-align: right;
  }

  .time.remaining {
    text-align: left;
  }

  .seekbar {
    flex: 1;
    height: 24px;
    display: flex;
    align-items: center;
    cursor: pointer;
    position: relative;
  }

  .seekbar-track {
    width: 100%;
    height: 3px;
    background: var(--border-subtle);
    border-radius: 2px;
    overflow: hidden;
  }

  .seekbar-fill {
    height: 100%;
    background: var(--accent-primary, #6366f1);
    border-radius: 2px;
    transition: width 100ms linear;
  }

  .seekbar-thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    background: var(--text-primary);
    border-radius: 50%;
    transform: translate(-50%, -50%);
    opacity: 0;
    transition: opacity 150ms ease;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3);
  }

  .seekbar:hover .seekbar-thumb {
    opacity: 1;
  }

  .seekbar:hover .seekbar-track {
    height: 4px;
  }

  /* ===== Controls Row ===== */
  .controls-row {
    flex: 1;
    display: flex;
    align-items: center;
    padding: 6px 16px 26px 16px;
    gap: 50px;
  }

  .left-section,
  .right-section {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .center-section {
    flex: 1;
    display: flex;
    justify-content: center;
    min-width: 0;
  }

  /* ===== Control Buttons ===== */
  .control-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .control-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .control-btn:active {
    transform: scale(0.95);
  }

  .control-btn.active {
    color: var(--accent-primary, #6366f1);
  }

  .control-btn.cast-active {
    color: #22c55e;
    animation: cast-pulse 2s ease-in-out infinite;
  }

  .control-btn.disabled {
    color: var(--text-disabled);
    opacity: 0.5;
    cursor: not-allowed;
  }

  .control-btn.disabled:hover {
    color: var(--text-disabled);
    background: transparent;
  }

  @keyframes cast-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  /* Section Separator */
  .section-separator {
    width: 1px;
    height: 20px;
    background: var(--border-subtle);
    margin: 0 8px;
    flex-shrink: 0;
  }

  /* Queue Button & Icon */
  .queue-btn {
    width: 32px;
    height: 32px;
  }

  .queue-icon {
    display: block;
  }

  .queue-icon .queue-lines {
    fill: currentColor;
  }

  .queue-icon .queue-play {
    fill: currentColor;
    opacity: 0.4;
    transition: fill 150ms ease, opacity 150ms ease;
  }

  .queue-icon.open .queue-play {
    fill: var(--accent-primary, #6366f1);
    opacity: 1;
  }

  .queue-btn.active {
    color: var(--text-primary);
  }

  /* Offline Indicator */
  .offline-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 6px;
    background: rgba(234, 179, 8, 0.15);
    cursor: help;
  }

  .offline-icon {
    width: 16px;
    height: 16px;
    opacity: 0.9;
  }

  .play-btn {
    width: 34px;
    height: 34px;
    color: var(--text-primary);
    margin: 0 4px;
  }

  .play-btn:hover {
    color: var(--accent-primary, #6366f1);
  }

  /* ===== Song Card ===== */
  .song-card {
    display: flex;
    align-items: stretch;
    gap: 12px;
    padding: 2px;
    background: var(--bg-tertiary);
    border-radius: 8px;
    min-width: 580px;
    flex: 1;
    max-width: 800px;
  }

  .artwork-container {
    position: relative;
    background: none;
    border: none;
    padding: 0;
    margin: 0;
    cursor: pointer;
    flex-shrink: 0;
    line-height: 0;
    align-self: center;
  }

  .artwork {
    width: 56px;
    height: 56px;
    border-radius: 6px;
    object-fit: cover;
    display: block;
  }

  .artwork-placeholder {
    width: 56px;
    height: 56px;
    border-radius: 6px;
    background: var(--bg-hover);
  }

  .artwork-preview {
    position: absolute;
    bottom: calc(100% + 12px);
    left: 50%;
    transform: translateX(-50%);
    z-index: 200;
    pointer-events: none;
    animation: preview-appear 200ms ease;
  }

  .artwork-preview img {
    width: 200px;
    height: 200px;
    border-radius: 8px;
    object-fit: cover;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  @keyframes preview-appear {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(8px) scale(0.95);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0) scale(1);
    }
  }

  .song-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 2px;
    align-self: center;
  }

  .song-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    background: none;
    border: none;
    padding: 0;
    cursor: help;
    text-align: left;
  }

  .song-meta {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
  }

  .song-meta :global(.stack-icon) {
    flex-shrink: 0;
    margin-right: 2px;
  }

  .meta-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--text-muted);
    font-size: 11px;
    cursor: pointer;
    transition: color 150ms ease;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta-link:hover {
    color: var(--text-primary);
    text-decoration: underline;
  }

  .meta-separator {
    color: var(--text-disabled);
  }

  .quality-indicator {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    align-self: stretch;
    margin: 0;
  }

  .audio-badges-row {
    display: flex;
    height: 20px;
    min-width: 70px;
  }

  .empty-state {
    font-size: 13px;
    color: var(--text-disabled);
  }

  /* ===== Volume Control ===== */
  .volume-control {
    display: flex;
    align-items: center;
    gap: 8px;
    position: relative;
  }

  .volume-slider {
    width: 100px;
    height: 24px;
    display: flex;
    align-items: center;
    cursor: pointer;
    position: relative;
  }

  .volume-track {
    width: 100%;
    height: 4px;
    background: var(--border-subtle);
    border-radius: 2px;
    position: relative;
    overflow: visible;
  }

  .volume-fill {
    height: 100%;
    background: var(--accent-primary, #6366f1);
    border-radius: 2px;
    position: relative;
    z-index: 1;
  }

  .volume-thumb {
    position: absolute;
    top: 50%;
    width: 14px;
    height: 14px;
    background: var(--text-primary);
    border-radius: 50%;
    transform: translate(-50%, -50%);
    opacity: 0;
    transition: opacity 150ms ease;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3);
    z-index: 2;
  }

  .volume-slider:hover .volume-thumb {
    opacity: 1;
  }

  .volume-value {
    position: absolute;
    right: 0;
    bottom: calc(100% + 6px);
    padding: 4px 6px;
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 600;
    opacity: 0;
    transform: translateY(4px);
    transition: opacity 120ms ease, transform 120ms ease;
    pointer-events: none;
  }

  .volume-value.visible {
    opacity: 1;
    transform: translateY(0);
  }
</style>
