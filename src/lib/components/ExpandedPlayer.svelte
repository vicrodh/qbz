<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { X, Shuffle, SkipBack, Play, Pause, SkipForward, Repeat, Heart, List, Maximize2, MoreHorizontal, Cast } from 'lucide-svelte';
  import QualityBadge from './QualityBadge.svelte';
  import StackIcon from './StackIcon.svelte';
  import LyricsLines from './lyrics/LyricsLines.svelte';
  import { startActiveLineUpdates } from '$lib/stores/lyricsStore';
  import { handleKeydown as keybindingHandler } from '$lib/stores/keybindingsStore';

  interface HardwareAudioStatus {
    hardware_sample_rate: number | null;
    hardware_format: string | null;
    is_active: boolean;
  }

  interface AudioSettings {
    dac_passthrough: boolean;
  }

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
    originalBitDepth?: number;
    originalSamplingRate?: number;
    format?: string;
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
    isCastConnected?: boolean;
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
    album,
    quality,
    qualityLevel,
    bitDepth,
    samplingRate,
    originalBitDepth,
    originalSamplingRate,
    format,
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
    isCastConnected = false,
    onContextClick,
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
  let hardwareSampleRate = $state<number | null>(null);
  let dacPassthrough = $state(false);

  const progress = $derived((currentTime / duration) * 100 || 0);
  const hasLyrics = $derived(lyricsLines.length > 0);

  // Use hardware sample rate when DAC passthrough is active, otherwise use track sample rate
  const displaySamplingRate = $derived(
    dacPassthrough && hardwareSampleRate
      ? hardwareSampleRate / 1000  // Convert Hz to kHz
      : samplingRate
  );

  // Poll hardware sample rate when DAC passthrough is active
  $effect(() => {
    if (!isOpen) return;

    async function loadStatus() {
      try {
        const [settings, hwStatus] = await Promise.all([
          invoke<AudioSettings>('v2_get_audio_settings'),
          invoke<HardwareAudioStatus>('v2_get_hardware_audio_status').catch(() => null)
        ]);
        dacPassthrough = settings.dac_passthrough;
        hardwareSampleRate = hwStatus?.hardware_sample_rate ?? null;
      } catch (err) {
        console.error('Failed to load audio status:', err);
      }
    }

    loadStatus();

    const pollInterval = setInterval(() => {
      if (dacPassthrough) {
        loadStatus();
      }
    }, 1000); // Poll every second when DAC passthrough is active

    return () => clearInterval(pollInterval);
  });

  // Ensure lyrics updates run when ExpandedPlayer is open with synced lyrics
  $effect(() => {
    if (isOpen && isPlaying && lyricsSynced) {
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
    // Delegate to keybinding manager (global shortcuts handle Escape and Space)
    keybindingHandler(e);
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
          <div class="artist-album-row">
            <StackIcon size={16} class="stack-icon" onClick={onContextClick} />
            <h2 class="artist">{artist}</h2>
            <span class="separator">·</span>
            <h3 class="album">{album}</h3>
          </div>
          <div class="quality-info">
            <QualityBadge {quality} {bitDepth} samplingRate={displaySamplingRate} {originalBitDepth} {originalSamplingRate} {format} />
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
          <button
            class="action-btn"
            class:cast-active={isCastConnected}
            onclick={onCast}
            title={isCastConnected ? 'Casting - Click to manage' : 'Cast to device'}
          >
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
    background-color: var(--bg-primary);
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
    border: 1px solid var(--alpha-10);
    border-radius: 50%;
    color: var(--alpha-70);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
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
    color: var(--text-primary);
    margin: 0 0 6px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist-album-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  .artist-album-row :global(.stack-icon) {
    flex-shrink: 0;
  }

  .artist-album-row .separator {
    color: var(--alpha-50);
    margin: 0 2px;
  }

  .artist {
    font-size: 16px;
    font-weight: 400;
    color: var(--alpha-70);
    margin: 0;
  }

  .album {
    font-size: 16px;
    font-weight: 400;
    color: var(--alpha-50);
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
    color: var(--alpha-50);
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    margin-bottom: 8px;
  }

  .progress-bar {
    height: 4px;
    background-color: var(--alpha-20);
    border-radius: 2px;
    position: relative;
    cursor: pointer;
  }

  .progress-fill {
    height: 100%;
    background-color: var(--text-primary);
    border-radius: 2px;
    transition: width 100ms linear;
  }

  .progress-thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background-color: var(--text-primary);
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
    color: var(--alpha-60);
    cursor: pointer;
    transition: all 150ms ease;
    position: relative;
    padding: 8px;
    border-radius: 50%;
  }

  .control-btn:hover {
    color: var(--text-primary);
    background: var(--alpha-10);
  }

  .control-btn.primary {
    color: var(--text-primary);
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
    background: var(--alpha-15);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .control-btn.play-pause:hover {
    background: var(--alpha-25);
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
    position: relative;
  }

  .volume-bar {
    height: 4px;
    background-color: var(--alpha-20);
    border-radius: 2px;
    position: relative;
    cursor: pointer;
  }

  .volume-fill {
    height: 100%;
    background-color: var(--text-primary);
    border-radius: 2px;
  }

  .volume-thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background-color: var(--text-primary);
    transform: translate(-50%, -50%);
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
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.6);
    color: var(--alpha-90);
    font-size: 12px;
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
    color: var(--alpha-60);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .action-btn:hover {
    color: var(--text-primary);
    background: var(--alpha-10);
  }

  .action-btn.active {
    color: var(--accent-primary);
  }

  .action-btn.cast-active {
    color: #22c55e;
    animation: cast-pulse 2s ease-in-out infinite;
  }

  @keyframes cast-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  /* Right Section: Lyrics */
  .right-section {
    flex: 1;
    min-width: 0;
    min-height: 0; /* Critical for nested flex to respect max-height */
    display: flex;
    flex-direction: column;
    justify-content: center;
    overflow: hidden;
    padding-right: 40px;
    height: 100%; /* Explicit height for proper constraint cascade */
  }

  .lyrics-container {
    flex: 1;
    min-height: 0; /* Critical for flex child to allow shrinking */
    max-height: calc(100vh - 160px);
    overflow: hidden; /* Clip content for mask effect */
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
    --text-primary: var(--alpha-90);
    --text-secondary: var(--alpha-45);
    --text-muted: var(--alpha-20);
    --bg-tertiary: var(--alpha-8);
    padding: 0;
    height: 100%;
    max-height: 100%; /* Constrain to parent height */
    overflow-y: auto; /* Ensure scrolling works */
  }

  .lyrics-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--alpha-40);
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
    border: 2px solid var(--alpha-20);
    border-top-color: var(--alpha-80);
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
