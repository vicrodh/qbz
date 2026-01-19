<script lang="ts">
  import { SkipBack, Play, Pause, SkipForward, ChevronDown } from 'lucide-svelte';
  import StackIcon from './StackIcon.svelte';
  import LyricsLines from './lyrics/LyricsLines.svelte';
  import { startActiveLineUpdates, stopActiveLineUpdates } from '$lib/stores/lyricsStore';
  import { t } from '$lib/i18n';

  interface LyricsLine {
    text: string;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    artwork: string;
    trackTitle: string;
    artist: string;
    isPlaying: boolean;
    onTogglePlay: () => void;
    onSkipBack?: () => void;
    onSkipForward?: () => void;
    currentTime: number;
    duration: number;
    onSeek: (time: number) => void;
    volume: number;
    onVolumeChange: (volume: number) => void;
    onContextClick?: () => void;
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
    isPlaying,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    currentTime,
    duration,
    onSeek,
    volume,
    onVolumeChange,
    onContextClick,
    lyricsLines = [],
    lyricsActiveIndex = -1,
    lyricsActiveProgress = 0,
    lyricsSynced = false,
    lyricsLoading = false,
    lyricsError = null
  }: Props = $props();

  // Ensure lyrics updates run when FocusMode is open with synced lyrics
  $effect(() => {
    if (isOpen && isPlaying && lyricsSynced) {
      startActiveLineUpdates();
    }
    return () => {
      // Cleanup is handled by the main page effect
    };
  });

  let showControls = $state(true);
  let hideTimeout: ReturnType<typeof setTimeout> | null = null;
  let progressRef: HTMLDivElement | null = $state(null);
  let volumeRef: HTMLDivElement | null = $state(null);
  let isDraggingVolume = $state(false);

  const progress = $derived((currentTime / duration) * 100 || 0);
  const hasLyrics = $derived(lyricsLines.length > 0);
  const showLyricsPane = $derived(hasLyrics || lyricsLoading);

  function formatTime(seconds: number): string {
    if (!seconds || !isFinite(seconds)) return '0:00';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function handleProgressClick(e: MouseEvent) {
    if (progressRef) {
      const rect = progressRef.getBoundingClientRect();
      const percentage = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
      onSeek(Math.round((percentage / 100) * duration));
    }
  }

  function handleProgressKeydown(e: KeyboardEvent) {
    const step = e.shiftKey ? 10 : 5;
    if (e.key === 'ArrowRight') {
      onSeek(Math.min(duration, currentTime + step));
    } else if (e.key === 'ArrowLeft') {
      onSeek(Math.max(0, currentTime - step));
    }
  }

  function handleVolumeMouseDown(e: MouseEvent) {
    e.stopPropagation();
    isDraggingVolume = true;
    updateVolume(e);
    document.addEventListener('mousemove', handleVolumeMouseMove);
    document.addEventListener('mouseup', handleVolumeMouseUp);
  }

  function handleVolumeMouseMove(e: MouseEvent) {
    if (isDraggingVolume) updateVolume(e);
  }

  function handleVolumeMouseUp() {
    isDraggingVolume = false;
    document.removeEventListener('mousemove', handleVolumeMouseMove);
    document.removeEventListener('mouseup', handleVolumeMouseUp);
  }

  function updateVolume(e: MouseEvent) {
    if (!volumeRef) return;
    const rect = volumeRef.getBoundingClientRect();
    const percentage = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
    onVolumeChange(Math.round(percentage));
  }

  function showControlsTemporarily() {
    showControls = true;
    if (hideTimeout) clearTimeout(hideTimeout);
    hideTimeout = setTimeout(() => (showControls = false), 4000);
  }

  function handleKeyPress(e: KeyboardEvent) {
    if (!isOpen) return;
    if (e.key === 'Escape') {
      onClose();
    } else if (e.key === ' ') {
      e.preventDefault();
      onTogglePlay();
    }
  }

  function handleMouseMove() {
    showControlsTemporarily();
  }

  $effect(() => {
    if (isOpen) {
      showControlsTemporarily();
      document.addEventListener('keydown', handleKeyPress);
      document.addEventListener('mousemove', handleMouseMove);
      return () => {
        document.removeEventListener('keydown', handleKeyPress);
        document.removeEventListener('mousemove', handleMouseMove);
        if (hideTimeout) clearTimeout(hideTimeout);
      };
    }
  });
</script>

{#if isOpen}
  <div class="focus-mode" onclick={showControlsTemporarily} role="presentation">
    <!-- Blurred Background Artwork -->
    <div class="background">
      <img src={artwork} alt="" aria-hidden="true" />
      <div class="background-overlay"></div>
    </div>

    <!-- Close Button -->
    <button class="close-btn" class:visible={showControls} onclick={onClose} title={$t('actions.close') + ' (Esc)'}>
      <ChevronDown size={28} />
    </button>

    <!-- Main Layout Container -->
    <div class="main-layout">
      <!-- Left: Large Artwork -->
      <div class="artwork-section">
        <div class="artwork-wrapper">
          <img src={artwork} alt={trackTitle} class="artwork-image" />
        </div>
      </div>

      <!-- Right: Lyrics Panel (overlays when present) -->
      {#if showLyricsPane}
        <div class="lyrics-section">
          {#if lyricsLoading}
            <div class="lyrics-state">
              <div class="spinner"></div>
              <span>{$t('player.fetchingLyrics')}</span>
            </div>
          {:else if lyricsError}
            <div class="lyrics-state">
              <span class="error-text">{lyricsError}</span>
            </div>
          {:else}
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
          {/if}
        </div>
      {/if}
    </div>

    <!-- Bottom Controls Bar -->
    <div class="bottom-bar" class:visible={showControls}>
      <!-- Left: Track Info -->
      <div class="track-info">
        <img src={artwork} alt="" class="mini-artwork" />
        <div class="track-meta">
          <div class="track-title">{trackTitle}</div>
          <div class="track-artist-row">
            <StackIcon size={12} class="stack-icon" onClick={onContextClick} />
            <div class="track-artist">{artist}</div>
          </div>
        </div>
      </div>

      <!-- Center: Playback Controls + Progress -->
      <div class="playback-section">
        <div class="controls">
          <button class="control-btn" onclick={onSkipBack} disabled={!onSkipBack}>
            <SkipBack size={22} />
          </button>
          <button
            class="control-btn play-btn"
            onclick={(e) => {
              e.stopPropagation();
              onTogglePlay();
            }}
          >
            {#if isPlaying}
              <Pause size={26} />
            {:else}
              <Play size={26} class="play-icon" />
            {/if}
          </button>
          <button class="control-btn" onclick={onSkipForward} disabled={!onSkipForward}>
            <SkipForward size={22} />
          </button>
        </div>

        <div class="progress-row">
          <span class="time">{formatTime(currentTime)}</span>
          <div
            class="progress-bar"
            bind:this={progressRef}
            onclick={handleProgressClick}
            onkeydown={handleProgressKeydown}
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

      <!-- Right: Volume Control -->
      <div class="right-controls">
        <div class="volume-control">
          <div class="volume-value" class:visible={isDraggingVolume}>{volume}</div>
          <div
            class="volume-bar"
            bind:this={volumeRef}
            onmousedown={handleVolumeMouseDown}
            role="slider"
            tabindex="0"
            aria-valuenow={volume}
            aria-valuemin={0}
            aria-valuemax={100}
            title={$t('player.volume')}
          >
            <div class="volume-fill" style="width: {volume}%"></div>
            <div class="volume-thumb" style="left: {volume}%"></div>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .focus-mode {
    position: fixed;
    inset: 0;
    z-index: 110;
    background-color: #000;
    cursor: default;
    animation: fadeIn 300ms ease-out;
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
    filter: blur(80px) saturate(1.3) brightness(0.6);
    transform: scale(1.3);
  }

  .background-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      135deg,
      rgba(0, 0, 0, 0.4) 0%,
      rgba(0, 0, 0, 0.3) 50%,
      rgba(0, 0, 0, 0.5) 100%
    );
  }

  /* Close Button */
  .close-btn {
    position: absolute;
    top: 16px;
    left: 16px;
    z-index: 20;
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
    opacity: 0;
    transition: all 200ms ease;
  }

  .close-btn.visible {
    opacity: 1;
  }

  .close-btn:hover {
    color: white;
    background: rgba(0, 0, 0, 0.5);
  }

  /* Main Layout */
  .main-layout {
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
    padding: 80px 48px 140px;
    gap: 60px;
    z-index: 1;
  }

  /* Artwork Section */
  .artwork-section {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
  }

  .artwork-wrapper {
    width: min(65vh, 520px);
    aspect-ratio: 1;
    border-radius: 12px;
    overflow: hidden;
    box-shadow:
      0 32px 80px rgba(0, 0, 0, 0.6),
      0 0 0 1px rgba(255, 255, 255, 0.05);
  }

  .artwork-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Lyrics Section */
  .lyrics-section {
    flex: 1;
    min-width: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    max-width: 560px;
  }

  .lyrics-container {
    flex: 1;
    overflow: hidden;
    mask-image: linear-gradient(
      to bottom,
      transparent 0%,
      black 8%,
      black 85%,
      transparent 100%
    );
    -webkit-mask-image: linear-gradient(
      to bottom,
      transparent 0%,
      black 8%,
      black 85%,
      transparent 100%
    );
  }

  .lyrics-container :global(.lyrics-lines) {
    --text-primary: rgba(255, 255, 255, 0.95);
    --text-secondary: rgba(255, 255, 255, 0.5);
    --text-muted: rgba(255, 255, 255, 0.25);
    --bg-tertiary: rgba(255, 255, 255, 0.08);
    padding: 0;
    height: 100%;
  }

  .lyrics-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 100%;
    color: rgba(255, 255, 255, 0.5);
    font-size: 14px;
  }

  .error-text {
    color: rgba(255, 255, 255, 0.4);
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

  /* Bottom Bar */
  .bottom-bar {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 32px;
    padding: 20px 32px 24px;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.85) 0%, rgba(0, 0, 0, 0.4) 60%, transparent 100%);
    opacity: 0;
    transform: translateY(8px);
    transition: all 250ms ease;
  }

  .bottom-bar.visible {
    opacity: 1;
    transform: translateY(0);
  }

  /* Track Info */
  .track-info {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 200px;
    max-width: 280px;
  }

  .mini-artwork {
    width: 48px;
    height: 48px;
    border-radius: 6px;
    object-fit: cover;
    flex-shrink: 0;
  }

  .track-meta {
    min-width: 0;
    flex: 1;
  }

  .track-title-row {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    overflow: hidden;
  }

  .track-title-row :global(.stack-icon) {
    flex-shrink: 0;
  }

  .track-title {
    font-size: 14px;
    font-weight: 600;
    color: white;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-artist-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 2px;
  }

  .track-artist-row :global(.stack-icon) {
    flex-shrink: 0;
  }

  .track-artist {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.6);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Playback Section */
  .playback-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    max-width: 560px;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 20px;
  }

  .control-btn {
    width: 40px;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: 50%;
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .control-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .control-btn:not(:disabled):hover {
    color: white;
    background: rgba(255, 255, 255, 0.1);
  }

  .play-btn {
    width: 52px;
    height: 52px;
    background: rgba(255, 255, 255, 0.15);
    backdrop-filter: blur(4px);
    color: white;
  }

  .play-btn:hover {
    background: rgba(255, 255, 255, 0.25);
    transform: scale(1.05);
  }

  .play-btn :global(.play-icon) {
    margin-left: 3px;
  }

  /* Progress Row */
  .progress-row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
  }

  .time {
    font-size: 12px;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.6);
    min-width: 40px;
  }

  .time:last-child {
    text-align: right;
  }

  .progress-bar {
    flex: 1;
    height: 4px;
    background: rgba(255, 255, 255, 0.2);
    border-radius: 2px;
    position: relative;
    cursor: pointer;
  }

  .progress-fill {
    height: 100%;
    background: white;
    border-radius: 2px;
    transition: width 100ms linear;
  }

  .progress-thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: white;
    transform: translate(-50%, -50%);
    opacity: 0;
    transition: opacity 150ms ease;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.4);
  }

  .progress-bar:hover .progress-thumb {
    opacity: 1;
  }

  .right-controls {
    min-width: 200px;
    display: flex;
    justify-content: flex-end;
  }

  .volume-control {
    position: relative;
    display: flex;
    align-items: center;
  }

  .volume-bar {
    width: 140px;
    height: 6px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.15);
    cursor: pointer;
    position: relative;
  }

  .volume-fill {
    height: 100%;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.7);
  }

  .volume-thumb {
    position: absolute;
    top: 50%;
    transform: translate(-50%, -50%);
    width: 12px;
    height: 12px;
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
    top: -26px;
    padding: 4px 8px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.5);
    color: #fff;
    font-size: 12px;
    opacity: 0;
    transform: translateY(4px);
    transition: opacity 150ms ease, transform 150ms ease;
  }

  .volume-value.visible {
    opacity: 1;
    transform: translateY(0);
  }

  /* Responsive Breakpoints */
  @media (max-width: 1200px) {
    .main-layout {
      padding: 60px 32px 130px;
      gap: 32px;
    }

    .artwork-wrapper {
      width: min(55vh, 440px);
    }

    .lyrics-section {
      max-width: 480px;
    }
  }

  @media (max-width: 900px) {
    .main-layout {
      flex-direction: column;
      padding: 60px 24px 140px;
      gap: 24px;
      justify-content: flex-start;
    }

    .artwork-section {
      height: auto;
      flex: 0 0 auto;
    }

    .artwork-wrapper {
      width: min(45vh, 320px);
    }

    .lyrics-section {
      flex: 1;
      max-width: 100%;
      width: 100%;
    }

    .track-info {
      display: none;
    }

    .right-controls {
      display: none;
    }

    .playback-section {
      max-width: 100%;
    }
  }

  @media (max-width: 600px) {
    .main-layout {
      padding: 48px 16px 130px;
    }

    .artwork-wrapper {
      width: min(40vh, 280px);
    }

    .bottom-bar {
      padding: 16px 16px 20px;
    }

    .controls {
      gap: 16px;
    }

    .play-btn {
      width: 48px;
      height: 48px;
    }
  }
</style>
