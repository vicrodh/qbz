<script lang="ts">
  import { X, Shuffle, SkipBack, Play, Pause, SkipForward, Repeat, Heart, List, Maximize2, MoreHorizontal, Cast } from 'lucide-svelte';
  import QualityBadge from './QualityBadge.svelte';
  import LyricsLines from './lyrics/LyricsLines.svelte';
  import { startActiveLineUpdates } from '$lib/stores/lyricsStore';

  interface LyricsLine {
    text: string;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    artwork: string;
    trackTitle: string;
    artist: string;
    album: string;
    quality: string;
    qualityLevel: number;
    bitDepth?: number;
    samplingRate?: number;
    isPlaying: boolean;
    onTogglePlay: () => void;
    onSkipBack?: () => void;
    onSkipForward?: () => void;
    currentTime: number;
    duration: number;
    onSeek: (time: number) => void;
    volume: number;
    onVolumeChange: (volume: number) => void;
    isShuffle: boolean;
    onToggleShuffle: () => void;
    repeatMode: 'off' | 'all' | 'one';
    onToggleRepeat: () => void;
    isFavorite: boolean;
    onToggleFavorite: () => void;
    onOpenQueue?: () => void;
    onOpenFocusMode?: () => void;
    onCast?: () => void;
    // Lyrics props
    lyricsLines?: LyricsLine[];
    lyricsActiveIndex?: number;
    lyricsActiveProgress?: number;
    lyricsSynced?: boolean;
    lyricsLoading?: boolean;
    lyricsError?: string | null;
  }

  let {
    isOpen,
    onClose,
    artwork,
    trackTitle,
    artist,
    album,
    quality,
    qualityLevel,
    bitDepth,
    samplingRate,
    isPlaying,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    currentTime,
    duration,
    onSeek,
    volume,
    onVolumeChange,
    isShuffle,
    onToggleShuffle,
    repeatMode,
    onToggleRepeat,
    isFavorite,
    onToggleFavorite,
    onOpenQueue,
    onOpenFocusMode,
    onCast,
    lyricsLines = [],
    lyricsActiveIndex = -1,
    lyricsActiveProgress = 0,
    lyricsSynced = false,
    lyricsLoading = false,
    lyricsError = null
  }: Props = $props();

  let progressRef: HTMLDivElement | null = $state(null);
  let volumeRef: HTMLDivElement | null = $state(null);
  let isDraggingProgress = $state(false);
  let isDraggingVolume = $state(false);

  const progress = $derived((currentTime / duration) * 100 || 0);
  const hasLyrics = $derived(lyricsLines.length > 0);

  // Ensure lyrics updates run when ExpandedPlayer is open with synced lyrics
  $effect(() => {
    console.log('[ExpandedPlayer] Effect check:', { isOpen, isPlaying, lyricsSynced, lyricsActiveIndex, hasLyrics: lyricsLines.length > 0 });
    if (isOpen && isPlaying && lyricsSynced) {
      console.log('[ExpandedPlayer] Starting lyrics updates');
      startActiveLineUpdates();
    }
  });

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
      onVolumeChange(Math.round(percentage));
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

  function handleKeydown(e: KeyboardEvent) {
    if (!isOpen) return;
    if (e.key === 'Escape') {
      onClose();
    } else if (e.key === ' ') {
      e.preventDefault();
      onTogglePlay();
    }
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

  $effect(() => {
    if (isOpen) {
      document.addEventListener('keydown', handleKeydown);
      return () => {
        document.removeEventListener('keydown', handleKeydown);
      };
    }
  });
</script>

{#if isOpen}
  <div class="expanded-player">
    <!-- Blurred Background -->
    <div class="background">
      <img src={artwork} alt="" aria-hidden="true" />
      <div class="background-overlay"></div>
    </div>

    <!-- Close Button -->
    <button class="close-btn" onclick={onClose} title="Close (Esc)">
      <X size={24} />
    </button>

    <!-- Main Content -->
    <div class="main-content">
      <!-- Left Side: Artwork + Track Info -->
      <div class="left-section">
        <div class="artwork">
          <img src={artwork} alt={trackTitle} />
        </div>

        <div class="track-info">
          <h1 class="title">{trackTitle}</h1>
          <h2 class="artist">{artist}</h2>
          <h3 class="album">{album}</h3>
          <div class="quality-info">
            <QualityBadge {quality} {bitDepth} {samplingRate} />
          </div>
        </div>

        <!-- Progress Bar -->
        <div class="progress-container">
          <div class="time-display">
            <span>{formatTime(currentTime)}</span>
            <span>{formatTime(duration)}</span>
          </div>
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
        </div>

        <!-- Playback Controls -->
        <div class="controls">
          <button
            class="control-btn"
            class:active={isShuffle}
            onclick={onToggleShuffle}
            title="Shuffle"
          >
            <Shuffle size={22} />
          </button>
          <button class="control-btn primary" onclick={onSkipBack} title="Previous">
            <SkipBack size={28} />
          </button>
          <button class="control-btn primary play-pause" onclick={onTogglePlay} title={isPlaying ? 'Pause' : 'Play'}>
            {#if isPlaying}
              <Pause size={36} />
            {:else}
              <Play size={36} class="play-icon" />
            {/if}
          </button>
          <button class="control-btn primary" onclick={onSkipForward} title="Next">
            <SkipForward size={28} />
          </button>
          <button
            class="control-btn"
            class:active={repeatMode !== 'off'}
            onclick={onToggleRepeat}
            title={repeatMode === 'off' ? 'Repeat' : repeatMode === 'all' ? 'Repeat All' : 'Repeat One'}
          >
            <Repeat size={22} />
            {#if repeatMode === 'one'}
              <span class="repeat-one">1</span>
            {/if}
          </button>
        </div>

        <!-- Volume Control -->
        <div class="volume-container">
          <div
            class="volume-bar"
            bind:this={volumeRef}
            onmousedown={handleVolumeMouseDown}
            role="slider"
            tabindex="0"
            aria-valuenow={volume}
            aria-valuemin={0}
            aria-valuemax={100}
            title="Volume"
          >
            <div class="volume-fill" style="width: {volume}%"></div>
            <div class="volume-thumb" style="left: {volume}%"></div>
          </div>
        </div>

        <!-- Bottom Actions -->
        <div class="bottom-actions">
          <button class="action-btn" onclick={onOpenQueue} title="Queue">
            <List size={22} />
          </button>
          <button class="action-btn" onclick={onOpenFocusMode} title="Focus Mode">
            <Maximize2 size={22} />
          </button>
          <button class="action-btn" onclick={onCast} title="Cast to device">
            <Cast size={22} />
          </button>
          <button
            class="action-btn"
            class:active={isFavorite}
            onclick={onToggleFavorite}
            title={isFavorite ? 'Remove from Favorites' : 'Add to Favorites'}
          >
            <Heart size={22} fill={isFavorite ? 'var(--accent-primary)' : 'none'} color={isFavorite ? 'var(--accent-primary)' : 'currentColor'} />
          </button>
          <button class="action-btn" title="More options">
            <MoreHorizontal size={22} />
          </button>
        </div>
      </div>

      <!-- Right Side: Lyrics -->
      <div class="right-section">
        {#if lyricsLoading}
          <div class="lyrics-state">
            <div class="spinner"></div>
            <span>Loading lyrics...</span>
          </div>
        {:else if lyricsError}
          <div class="lyrics-state">
            <span class="error-text">{lyricsError}</span>
          </div>
        {:else if hasLyrics}
          <div class="lyrics-container">
            <LyricsLines
              lines={lyricsLines}
              activeIndex={lyricsActiveIndex}
              activeProgress={lyricsActiveProgress}
              isSynced={lyricsSynced}
              center={false}
              compact={false}
              immersive={true}
            />
          </div>
        {:else}
          <div class="lyrics-state">
            <span class="no-lyrics">No lyrics available</span>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .expanded-player {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    flex-direction: column;
    background-color: #0a0a0a;
    animation: fadeIn 200ms ease-out;
    overflow: hidden;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  /* Blurred Background */
  .background {
    position: absolute;
    inset: 0;
    overflow: hidden;
  }

  .background img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    filter: blur(100px) saturate(1.2) brightness(0.4);
    transform: scale(1.4);
  }

  .background-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      to right,
      rgba(0, 0, 0, 0.7) 0%,
      rgba(0, 0, 0, 0.5) 40%,
      rgba(0, 0, 0, 0.3) 100%
    );
  }

  .close-btn {
    position: absolute;
    top: 24px;
    right: 24px;
    z-index: 10;
    width: 40px;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.3);
    backdrop-filter: blur(8px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 50%;
    color: rgba(255, 255, 255, 0.7);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .close-btn:hover {
    color: white;
    background: rgba(0, 0, 0, 0.5);
  }

  /* Main Content */
  .main-content {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 100%;
    max-width: 1400px;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1;
    padding: 60px;
    gap: 60px;
    overflow: hidden;
  }

  /* Left Section */
  .left-section {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    width: 420px;
    max-width: 40%;
  }

  .artwork {
    width: 100%;
    max-width: 340px;
    aspect-ratio: 1;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
    margin-bottom: 24px;
  }

  .artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .track-info {
    text-align: center;
    margin-bottom: 20px;
    width: 100%;
  }

  .title {
    font-size: 22px;
    font-weight: 600;
    color: white;
    margin: 0 0 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist {
    font-size: 16px;
    font-weight: 400;
    color: rgba(255, 255, 255, 0.7);
    margin: 0 0 4px;
  }

  .album {
    font-size: 14px;
    font-weight: 400;
    color: rgba(255, 255, 255, 0.5);
    margin: 0;
  }

  .quality-info {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    margin-top: 12px;
  }

  /* Progress Bar */
  .progress-container {
    width: 100%;
    margin-bottom: 20px;
  }

  .time-display {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: rgba(255, 255, 255, 0.5);
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    margin-bottom: 8px;
  }

  .progress-bar {
    height: 4px;
    background-color: rgba(255, 255, 255, 0.2);
    border-radius: 2px;
    position: relative;
    cursor: pointer;
  }

  .progress-fill {
    height: 100%;
    background-color: white;
    border-radius: 2px;
    transition: width 100ms linear;
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
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3);
  }

  .progress-bar:hover .progress-thumb {
    opacity: 1;
  }

  /* Controls */
  .controls {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 20px;
  }

  .control-btn {
    background: none;
    border: none;
    color: rgba(255, 255, 255, 0.6);
    cursor: pointer;
    transition: all 150ms ease;
    position: relative;
    padding: 8px;
    border-radius: 50%;
  }

  .control-btn:hover {
    color: white;
    background: rgba(255, 255, 255, 0.1);
  }

  .control-btn.primary {
    color: white;
  }

  .control-btn.active {
    color: var(--accent-primary);
  }

  .repeat-one {
    position: absolute;
    top: 2px;
    right: 2px;
    font-size: 10px;
    font-weight: 700;
  }

  .control-btn.play-pause {
    width: 64px;
    height: 64px;
    background: rgba(255, 255, 255, 0.15);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .control-btn.play-pause:hover {
    background: rgba(255, 255, 255, 0.25);
    transform: scale(1.05);
  }

  .control-btn.play-pause :global(.play-icon) {
    margin-left: 4px;
  }

  /* Volume */
  .volume-container {
    width: 100%;
    max-width: 180px;
    margin-bottom: 24px;
  }

  .volume-bar {
    height: 4px;
    background-color: rgba(255, 255, 255, 0.2);
    border-radius: 2px;
    position: relative;
    cursor: pointer;
  }

  .volume-fill {
    height: 100%;
    background-color: white;
    border-radius: 2px;
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

  /* Bottom Actions */
  .bottom-actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .action-btn {
    width: 40px;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: 50%;
    color: rgba(255, 255, 255, 0.6);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .action-btn:hover {
    color: white;
    background: rgba(255, 255, 255, 0.1);
  }

  .action-btn.active {
    color: var(--accent-primary);
  }

  /* Right Section: Lyrics */
  .right-section {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    overflow: hidden;
    padding-right: 40px;
  }

  .lyrics-container {
    flex: 1;
    min-height: 0; /* Critical for flex child to allow shrinking */
    max-height: calc(100vh - 160px);
    mask-image: linear-gradient(
      to bottom,
      transparent 0%,
      black 10%,
      black 85%,
      transparent 100%
    );
    -webkit-mask-image: linear-gradient(
      to bottom,
      transparent 0%,
      black 10%,
      black 85%,
      transparent 100%
    );
  }

  .lyrics-container :global(.lyrics-lines) {
    --text-primary: rgba(255, 255, 255, 0.9);
    --text-secondary: rgba(255, 255, 255, 0.45);
    --text-muted: rgba(255, 255, 255, 0.2);
    --bg-tertiary: rgba(255, 255, 255, 0.08);
    padding: 0;
    height: 100%;
  }

  .lyrics-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: rgba(255, 255, 255, 0.4);
    font-size: 15px;
    gap: 12px;
  }

  .error-text {
    color: rgba(255, 100, 100, 0.6);
  }

  .no-lyrics {
    font-style: italic;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-top-color: rgba(255, 255, 255, 0.8);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Responsive */
  @media (max-width: 1100px) {
    .main-content {
      padding: 40px;
      gap: 40px;
    }

    .left-section {
      width: 360px;
    }

    .artwork {
      max-width: 280px;
    }
  }

  @media (max-width: 900px) {
    .main-content {
      flex-direction: column;
      align-items: center;
      padding: 40px 24px;
      gap: 24px;
      overflow-y: auto;
    }

    .left-section {
      width: 100%;
      max-width: 400px;
    }

    .right-section {
      width: 100%;
      max-width: 500px;
      padding-right: 0;
      max-height: 300px;
    }

    .lyrics-container {
      max-height: 280px;
    }
  }

  @media (max-width: 600px) {
    .main-content {
      padding: 24px 16px;
    }

    .artwork {
      max-width: 240px;
    }

    .title {
      font-size: 18px;
    }

    .controls {
      gap: 12px;
    }

    .control-btn.play-pause {
      width: 56px;
      height: 56px;
    }
  }
</style>
