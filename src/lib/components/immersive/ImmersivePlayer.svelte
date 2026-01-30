<script lang="ts">
  import { startActiveLineUpdates } from '$lib/stores/lyricsStore';
  import ImmersiveBackground from './ImmersiveBackground.svelte';
  import ImmersiveArtwork from './ImmersiveArtwork.svelte';
  import ImmersiveHeader, { type ImmersiveTab } from './ImmersiveHeader.svelte';
  import ImmersiveControls from './ImmersiveControls.svelte';
  import LyricsPanel from './panels/LyricsPanel.svelte';
  import CreditsPanel from './panels/CreditsPanel.svelte';
  import SuggestionsPanel from './panels/SuggestionsPanel.svelte';
  import VisualizerPanel from './panels/VisualizerPanel.svelte';
  import QueuePanel from './panels/QueuePanel.svelte';

  interface LyricsLine {
    text: string;
  }

  interface QueueTrack {
    id: string | number;
    title: string;
    artist: string;
    artwork: string;
    duration?: string | number;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    // Track info
    artwork: string;
    trackTitle: string;
    artist: string;
    album?: string;
    trackId?: number;
    artistId?: number;
    // Quality
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    // Playback state
    isPlaying: boolean;
    currentTime: number;
    duration: number;
    volume: number;
    isShuffle: boolean;
    repeatMode: 'off' | 'all' | 'one';
    isFavorite: boolean;
    // Callbacks
    onTogglePlay: () => void;
    onSkipBack?: () => void;
    onSkipForward?: () => void;
    onSeek: (time: number) => void;
    onVolumeChange: (volume: number) => void;
    onToggleShuffle: () => void;
    onToggleRepeat: () => void;
    onToggleFavorite: () => void;
    onOpenQueue?: () => void;
    // Lyrics
    lyricsLines?: LyricsLine[];
    lyricsActiveIndex?: number;
    lyricsActiveProgress?: number;
    lyricsSynced?: boolean;
    lyricsLoading?: boolean;
    lyricsError?: string | null;
    // Feature flags
    enableCredits?: boolean;
    enableSuggestions?: boolean;
    enableVisualizer?: boolean;
    // Queue
    queueTracks?: QueueTrack[];
    queueCurrentIndex?: number;
    onQueuePlayTrack?: (index: number) => void;
    onQueueClear?: () => void;
  }

  let {
    isOpen,
    onClose,
    artwork,
    trackTitle,
    artist,
    album,
    trackId,
    artistId,
    quality,
    bitDepth,
    samplingRate,
    isPlaying,
    currentTime,
    duration,
    volume,
    isShuffle,
    repeatMode,
    isFavorite,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    onSeek,
    onVolumeChange,
    onToggleShuffle,
    onToggleRepeat,
    onToggleFavorite,
    onOpenQueue,
    lyricsLines = [],
    lyricsActiveIndex = -1,
    lyricsActiveProgress = 0,
    lyricsSynced = false,
    lyricsLoading = false,
    lyricsError = null,
    enableCredits = true,
    enableSuggestions = true,
    enableVisualizer = false,
    queueTracks = [],
    queueCurrentIndex = 0,
    onQueuePlayTrack,
    onQueueClear
  }: Props = $props();

  // UI State
  let activeTab: ImmersiveTab = $state('lyrics');
  let showUI = $state(true);
  let hideTimeout: ReturnType<typeof setTimeout> | null = null;

  const hasLyrics = $derived(lyricsLines.length > 0 || lyricsLoading);
  const AUTO_HIDE_DELAY = 4000;

  // Auto-hide UI after inactivity
  function resetHideTimer() {
    showUI = true;
    if (hideTimeout) clearTimeout(hideTimeout);
    hideTimeout = setTimeout(() => {
      showUI = false;
    }, AUTO_HIDE_DELAY);
  }

  function handleMouseMove() {
    resetHideTimer();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!isOpen) return;

    switch (e.key) {
      case 'Escape':
        onClose();
        break;
      case ' ':
        e.preventDefault();
        onTogglePlay();
        break;
      case 'ArrowLeft':
        if (e.shiftKey && onSkipBack) {
          onSkipBack();
        } else {
          onSeek(Math.max(0, currentTime - 5));
        }
        break;
      case 'ArrowRight':
        if (e.shiftKey && onSkipForward) {
          onSkipForward();
        } else {
          onSeek(Math.min(duration, currentTime + 5));
        }
        break;
      case 'l':
      case 'L':
        activeTab = 'lyrics';
        break;
      case 'c':
      case 'C':
        if (enableCredits) activeTab = 'credits';
        break;
      case 's':
      case 'S':
        if (enableSuggestions) activeTab = 'suggestions';
        break;
      case 'v':
      case 'V':
        if (enableVisualizer) activeTab = 'visualizer';
        break;
      case 'q':
      case 'Q':
        activeTab = 'queue';
        break;
    }
    resetHideTimer();
  }

  // Setup event listeners when open
  $effect(() => {
    if (isOpen) {
      resetHideTimer();
      document.addEventListener('keydown', handleKeydown);

      return () => {
        document.removeEventListener('keydown', handleKeydown);
        if (hideTimeout) clearTimeout(hideTimeout);
      };
    }
  });

  // Start lyrics updates when open and playing synced lyrics
  $effect(() => {
    if (isOpen && isPlaying && lyricsSynced) {
      startActiveLineUpdates();
    }
  });
</script>

{#if isOpen}
  <div
    class="immersive-player"
    onmousemove={handleMouseMove}
    role="dialog"
    aria-modal="true"
    aria-label="Now Playing"
    tabindex="-1"
  >
    <!-- Background -->
    <ImmersiveBackground {artwork} mode="blur" />

    <!-- Header with tabs -->
    <ImmersiveHeader
      {activeTab}
      onTabChange={(tab) => activeTab = tab}
      {onClose}
      visible={showUI}
      hasLyrics={true}
      hasCredits={enableCredits}
      hasSuggestions={enableSuggestions}
      hasVisualizer={enableVisualizer}
    />

    <!-- Main Content -->
    <div class="main-content">
      <!-- Left: Artwork -->
      <div class="artwork-section">
        <ImmersiveArtwork {artwork} {trackTitle} variant="floating" />
      </div>

      <!-- Right: Active Panel -->
      <div class="panel-section">
        {#if activeTab === 'lyrics'}
          <LyricsPanel
            lines={lyricsLines}
            activeIndex={lyricsActiveIndex}
            activeProgress={lyricsActiveProgress}
            isSynced={lyricsSynced}
            isLoading={lyricsLoading}
            error={lyricsError}
          />
        {:else if activeTab === 'credits'}
          <CreditsPanel {trackId} />
        {:else if activeTab === 'suggestions'}
          <SuggestionsPanel {trackId} {artistId} />
        {:else if activeTab === 'visualizer'}
          <VisualizerPanel {isPlaying} />
        {:else if activeTab === 'queue'}
          <QueuePanel
            tracks={queueTracks}
            currentIndex={queueCurrentIndex}
            onPlayTrack={(index) => onQueuePlayTrack?.(index)}
            onClear={onQueueClear}
          />
        {/if}
      </div>
    </div>

    <!-- Bottom Controls -->
    <ImmersiveControls
      visible={showUI}
      {artwork}
      {trackTitle}
      {artist}
      {isPlaying}
      {currentTime}
      {duration}
      {isShuffle}
      {repeatMode}
      {isFavorite}
      {volume}
      {quality}
      {bitDepth}
      {samplingRate}
      {onTogglePlay}
      {onSkipBack}
      {onSkipForward}
      {onSeek}
      {onToggleShuffle}
      {onToggleRepeat}
      {onToggleFavorite}
      {onVolumeChange}
      {onOpenQueue}
    />
  </div>
{/if}

<style>
  .immersive-player {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    flex-direction: column;
    background-color: var(--bg-primary, #0a0a0b);
    animation: fadeIn 200ms ease-out;
    overflow: hidden;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

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
    padding: 80px 48px 140px;
    gap: 60px;
    z-index: 1;
  }

  .artwork-section {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .panel-section {
    flex: 1;
    min-width: 0;
    min-height: 0;
    max-width: 560px;
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  /* Responsive */
  @media (max-width: 1200px) {
    .main-content {
      padding: 70px 32px 130px;
      gap: 40px;
    }

    .panel-section {
      max-width: 480px;
    }
  }

  @media (max-width: 900px) {
    .main-content {
      flex-direction: column;
      padding: 70px 24px 140px;
      gap: 24px;
      justify-content: flex-start;
    }

    .artwork-section {
      flex: 0 0 auto;
    }

    .panel-section {
      flex: 1;
      max-width: 100%;
      width: 100%;
    }
  }

  @media (max-width: 600px) {
    .main-content {
      padding: 60px 16px 130px;
      gap: 20px;
    }
  }
</style>
