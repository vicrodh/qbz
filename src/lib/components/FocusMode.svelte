<script lang="ts">
  import { X, SkipBack, Play, Pause, SkipForward } from 'lucide-svelte';
  import LyricsLines from './lyrics/LyricsLines.svelte';

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
    // Lyrics props
    lyricsLines?: LyricsLine[];
    lyricsActiveIndex?: number;
    lyricsActiveProgress?: number;
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
    lyricsLines = [],
    lyricsActiveIndex = -1,
    lyricsActiveProgress = 0,
    lyricsLoading = false,
    lyricsError = null
  }: Props = $props();

  let showControls = $state(true);
  let hideTimeout: ReturnType<typeof setTimeout> | null = null;
  let progressRef: HTMLDivElement;

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
    <button class="close-btn" class:visible={showControls} onclick={onClose}>
      <X size={28} />
    </button>

    <!-- Main Content Grid -->
    <div class="content" class:has-lyrics={showLyricsPane}>
      <!-- Left Side: Artwork -->
      <div class="artwork-side">
        <div class="artwork-container">
          <img src={artwork} alt={trackTitle} />
        </div>
      </div>

      <!-- Right Side: Lyrics -->
      {#if showLyricsPane}
        <div class="lyrics-side">
          {#if lyricsLoading}
            <div class="lyrics-loading">
              <div class="spinner"></div>
              <span>Loading lyrics...</span>
            </div>
          {:else if lyricsError}
            <div class="lyrics-error">{lyricsError}</div>
          {:else}
            <LyricsLines
              lines={lyricsLines}
              activeIndex={lyricsActiveIndex}
              activeProgress={lyricsActiveProgress}
              center={false}
              compact={false}
              immersive={true}
            />
          {/if}
        </div>
      {/if}
    </div>

    <!-- Bottom Bar: Track Info + Controls + Progress -->
    <div class="bottom-bar" class:visible={showControls}>
      <div class="track-info">
        <div class="track-artwork">
          <img src={artwork} alt="" />
        </div>
        <div class="track-meta">
          <div class="track-title">{trackTitle}</div>
          <div class="track-artist">{artist}</div>
        </div>
      </div>

      <div class="center-controls">
        <div class="playback-controls">
          <button class="control-btn" onclick={onSkipBack} disabled={!onSkipBack}>
            <SkipBack size={24} />
          </button>
          <button
            class="control-btn play"
            onclick={(e) => {
              e.stopPropagation();
              onTogglePlay();
            }}
          >
            {#if isPlaying}
              <Pause size={28} />
            {:else}
              <Play size={28} class="play-icon" />
            {/if}
          </button>
          <button class="control-btn" onclick={onSkipForward} disabled={!onSkipForward}>
            <SkipForward size={24} />
          </button>
        </div>

        <div class="progress-section">
          <span class="time">{formatTime(currentTime)}</span>
          <div
            class="progress-bar"
            bind:this={progressRef}
            onclick={handleProgressClick}
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

      <div class="spacer"></div>
    </div>
  </div>
{/if}

<style>
  .focus-mode {
    position: fixed;
    inset: 0;
    z-index: 110;
    display: flex;
    flex-direction: column;
    background-color: #000;
    cursor: default;
    animation: fadeIn 300ms ease-out;
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
    filter: blur(60px) saturate(1.2);
    transform: scale(1.2);
    opacity: 0.6;
  }

  .background-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      to bottom,
      rgba(0, 0, 0, 0.3) 0%,
      rgba(0, 0, 0, 0.5) 50%,
      rgba(0, 0, 0, 0.8) 100%
    );
  }

  /* Close Button */
  .close-btn {
    position: absolute;
    top: 20px;
    right: 20px;
    z-index: 10;
    width: 44px;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.4);
    backdrop-filter: blur(8px);
    border: none;
    border-radius: 50%;
    color: rgba(255, 255, 255, 0.7);
    cursor: pointer;
    opacity: 0;
    transition: all 300ms ease;
  }

  .close-btn.visible {
    opacity: 1;
  }

  .close-btn:hover {
    color: white;
    background: rgba(0, 0, 0, 0.6);
  }

  /* Main Content Grid */
  .content {
    flex: 1;
    display: grid;
    grid-template-columns: 1fr;
    gap: 48px;
    padding: 60px 60px 120px;
    position: relative;
    z-index: 1;
    align-items: center;
    justify-items: center;
  }

  .content.has-lyrics {
    grid-template-columns: 1fr 1fr;
    justify-items: start;
  }

  /* Artwork Side */
  .artwork-side {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
  }

  .content.has-lyrics .artwork-side {
    justify-content: flex-end;
    padding-right: 24px;
  }

  .artwork-container {
    width: min(55vh, 500px);
    aspect-ratio: 1;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
  }

  .artwork-container img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Lyrics Side */
  .lyrics-side {
    width: 100%;
    height: 100%;
    max-height: calc(100vh - 200px);
    display: flex;
    flex-direction: column;
    padding-left: 24px;
  }

  .lyrics-side :global(.lyrics-lines) {
    --text-primary: rgba(255, 255, 255, 0.95);
    --text-secondary: rgba(255, 255, 255, 0.5);
    --text-muted: rgba(255, 255, 255, 0.3);
    --bg-tertiary: rgba(255, 255, 255, 0.1);
    padding: 0;
  }

  .lyrics-loading,
  .lyrics-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 100%;
    color: rgba(255, 255, 255, 0.5);
    font-size: 14px;
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
    gap: 24px;
    padding: 16px 24px 20px;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.8) 0%, transparent 100%);
    opacity: 0;
    transform: translateY(10px);
    transition: all 300ms ease;
  }

  .bottom-bar.visible {
    opacity: 1;
    transform: translateY(0);
  }

  .track-info {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 200px;
  }

  .track-artwork {
    width: 48px;
    height: 48px;
    border-radius: 6px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .track-artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .track-meta {
    min-width: 0;
  }

  .track-title {
    font-size: 14px;
    font-weight: 600;
    color: white;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-artist {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.6);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .center-controls {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    max-width: 600px;
  }

  .playback-controls {
    display: flex;
    align-items: center;
    gap: 16px;
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
    color: rgba(255, 255, 255, 0.8);
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

  .control-btn.play {
    width: 48px;
    height: 48px;
    background: rgba(255, 255, 255, 0.15);
    color: white;
  }

  .control-btn.play:hover {
    background: rgba(255, 255, 255, 0.25);
  }

  .control-btn.play :global(.play-icon) {
    margin-left: 2px;
  }

  .progress-section {
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
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.3);
  }

  .progress-bar:hover .progress-thumb {
    opacity: 1;
  }

  .spacer {
    min-width: 200px;
  }
</style>
