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
    ListMusic,
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

  interface Props {
    artwork?: string;
    trackTitle?: string;
    artist?: string;
    album?: string;
    quality?: string;
    qualityLevel?: number;
    bitDepth?: number;
    samplingRate?: number;
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
    onOpenMiniPlayer?: () => void;
    onCast?: () => void;
    onToggleLyrics?: () => void;
    lyricsActive?: boolean;
    isCastConnected?: boolean;
    onArtistClick?: () => void;
    onAlbumClick?: () => void;
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
    onOpenFullScreen,
    onOpenMiniPlayer,
    onCast,
    onToggleLyrics,
    lyricsActive = false,
    isCastConnected = false,
    onArtistClick,
    onAlbumClick
  }: Props = $props();

  let progressRef: HTMLDivElement;
  let volumeRef: HTMLDivElement;
  let isDraggingProgress = $state(false);
  let isDraggingVolume = $state(false);
  let showArtworkPreview = $state(false);

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
        title="Shuffle"
      >
        <Shuffle size={16} />
      </button>

      <button class="control-btn" onclick={onSkipBack} title="Previous">
        <SkipBack size={18} />
      </button>

      <button class="control-btn play-btn" onclick={onTogglePlay} title={isPlaying ? 'Pause' : 'Play'}>
        {#if isPlaying}
          <Pause size={20} />
        {:else}
          <Play size={20} />
        {/if}
      </button>

      <button class="control-btn" onclick={onSkipForward} title="Next">
        <SkipForward size={18} />
      </button>

      <button
        class="control-btn"
        class:active={repeatMode !== 'off'}
        onclick={onToggleRepeat}
        title={repeatMode === 'off' ? 'Repeat' : repeatMode === 'all' ? 'Repeat All' : 'Repeat One'}
      >
        {#if repeatMode === 'one'}
          <Repeat1 size={16} />
        {:else}
          <Repeat size={16} />
        {/if}
      </button>

      <button class="control-btn" title="Add to Playlist">
        <Plus size={16} />
      </button>

      <button
        class="control-btn"
        class:active={isFavorite}
        onclick={onToggleFavorite}
        title={isFavorite ? 'Remove from Favorites' : 'Add to Favorites'}
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
            <span class="song-title" title={trackTitle}>{trackTitle}</span>
            <div class="song-meta">
              {#if artist}
                <button class="meta-link" onclick={onArtistClick} title="Go to Artist">
                  {artist}
                </button>
              {/if}
              {#if artist && album}
                <span class="meta-separator">·</span>
              {/if}
              {#if album}
                <button class="meta-link" onclick={onAlbumClick} title="Go to Album">
                  {album}
                </button>
              {/if}
            </div>
          </div>

          <div class="quality-indicator">
            <QualityBadge {quality} {bitDepth} {samplingRate} />
            <div class="audio-badges-row">
              <AudioOutputBadges />
            </div>
          </div>
        </div>
      {:else}
        <div class="empty-state">
          <span>No track playing</span>
        </div>
      {/if}
    </div>

    <!-- Right: Actions & Volume -->
    <div class="right-section">
      <button
        class="control-btn"
        class:cast-active={isCastConnected}
        onclick={onCast}
        title={isCastConnected ? 'Casting - Click to manage' : 'Cast to device'}
      >
        <Cast size={16} />
      </button>

      <button
        class="control-btn"
        class:active={lyricsActive}
        onclick={onToggleLyrics}
        title="Lyrics"
      >
        <Mic2 size={16} />
      </button>

      <button class="control-btn" onclick={onOpenQueue} title="Queue">
        <ListMusic size={16} />
      </button>

      <button class="control-btn" onclick={onOpenMiniPlayer} title="Mini Player">
        <PictureInPicture2 size={16} />
      </button>

      <button class="control-btn" onclick={onOpenFullScreen} title="Full Screen">
        <Maximize2 size={16} />
      </button>

      <!-- Volume Control -->
      <div class="volume-control">
        <button
          class="control-btn volume-btn"
          onclick={() => onVolumeChange?.(volume === 0 ? 70 : 0)}
          title={volume === 0 ? 'Unmute' : 'Mute'}
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
    background: linear-gradient(to top, rgba(18, 18, 20, 0.98) 0%, rgba(24, 24, 28, 0.95) 100%);
    backdrop-filter: blur(20px);
    border-top: 1px solid rgba(255, 255, 255, 0.06);
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
    color: rgba(255, 255, 255, 0.5);
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
    background: rgba(255, 255, 255, 0.1);
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
    background: white;
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
    color: rgba(255, 255, 255, 0.6);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .control-btn:hover {
    color: white;
    background: rgba(255, 255, 255, 0.08);
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

  @keyframes cast-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  .play-btn {
    width: 34px;
    height: 34px;
    color: white;
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
    background: rgba(255, 255, 255, 0.04);
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
    background: rgba(255, 255, 255, 0.06);
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
    color: white;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .song-meta {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.5);
    white-space: nowrap;
    overflow: hidden;
  }

  .meta-link {
    background: none;
    border: none;
    padding: 0;
    color: rgba(255, 255, 255, 0.5);
    font-size: 11px;
    cursor: pointer;
    transition: color 150ms ease;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta-link:hover {
    color: white;
    text-decoration: underline;
  }

  .meta-separator {
    color: rgba(255, 255, 255, 0.3);
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
    flex: 1;
    min-height: 0;
  }

  .empty-state {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.4);
  }

  /* ===== Volume Control ===== */
  .volume-control {
    display: flex;
    align-items: center;
    gap: 8px;
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
    background: rgba(255, 255, 255, 0.1);
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
    background: white;
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
</style>
