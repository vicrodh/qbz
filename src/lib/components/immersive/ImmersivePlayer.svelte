<script lang="ts">
  import { startActiveLineUpdates, setProgressTrackingEnabled } from '$lib/stores/lyricsStore';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import ImmersiveBackground from './ImmersiveBackground.svelte';
  import ImmersiveArtwork from './ImmersiveArtwork.svelte';
  import ImmersiveHeader, { type ImmersiveTab, type FocusTab, type ViewMode } from './ImmersiveHeader.svelte';
  import PlayerControlsCompact from './PlayerControlsCompact.svelte';
  import LyricsPanel from './panels/LyricsPanel.svelte';
  import TrackInfoPanel from './panels/TrackInfoPanel.svelte';
  import SuggestionsPanel from './panels/SuggestionsPanel.svelte';
  import QueuePanel from './panels/QueuePanel.svelte';
  import CoverflowPanel from './panels/CoverflowPanel.svelte';
  import StaticPanel from './panels/StaticPanel.svelte';
  import VinylPanel from './panels/VinylPanel.svelte';
  import VisualizerPanel from './panels/VisualizerPanel.svelte';
  import LyricsFocusPanel from './panels/LyricsFocusPanel.svelte';
  import QualityBadge from '$lib/components/QualityBadge.svelte';

  interface LyricsLine {
    text: string;
    timeMs?: number; // Timing for CSS-only karaoke animation
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
    // Infinite Play
    isInfinitePlay?: boolean;
    onToggleInfinitePlay?: () => void;
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
    // Queue
    queueTracks?: QueueTrack[];
    queueCurrentIndex?: number;
    onQueuePlayTrack?: (index: number) => void;
    onQueueClear?: () => void;
    // History
    historyTracks?: QueueTrack[];
    onPlayHistoryTrack?: (trackId: string) => void;
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
    isInfinitePlay = false,
    onToggleInfinitePlay,
    lyricsLines = [],
    lyricsActiveIndex = -1,
    lyricsActiveProgress = 0,
    lyricsSynced = false,
    lyricsLoading = false,
    lyricsError = null,
    enableCredits = true,
    enableSuggestions = true,
    queueTracks = [],
    queueCurrentIndex = 0,
    onQueuePlayTrack,
    onQueueClear,
    historyTracks = [],
    onPlayHistoryTrack
  }: Props = $props();

  // UI State
  let viewMode: ViewMode = $state('focus');
  let activeTab: ImmersiveTab = $state('lyrics');
  let activeFocusTab: FocusTab = $state('coverflow');
  let showUI = $state(true);
  let hideTimeout: ReturnType<typeof setTimeout> | null = null;
  let isFullscreen = $state(false);
  let isMaximized = $state(false);

  const hasLyrics = $derived(lyricsLines.length > 0 || lyricsLoading);
  const AUTO_HIDE_DELAY = 4000;

  // Immersive view persistence
  type ImmersiveViewKey = 'coverflow' | 'static' | 'vinyl' | 'visualizer' | 'lyrics-focus' | 'queue-focus' | 'split-lyrics' | 'split-trackInfo' | 'split-suggestions' | 'split-queue';

  function applyStoredView(key: ImmersiveViewKey) {
    if (key.startsWith('split-')) {
      viewMode = 'split';
      activeTab = key.replace('split-', '') as ImmersiveTab;
    } else {
      viewMode = 'focus';
      activeFocusTab = key as FocusTab;
    }
  }

  function getCurrentViewKey(): ImmersiveViewKey {
    if (viewMode === 'split') {
      return `split-${activeTab}` as ImmersiveViewKey;
    }
    return activeFocusTab as ImmersiveViewKey;
  }

  function saveLastUsedView() {
    const setting = localStorage.getItem('qbz-immersive-default-view') || 'remember';
    if (setting === 'remember') {
      localStorage.setItem('qbz-immersive-last-view', getCurrentViewKey());
    }
  }

  function restoreView() {
    const setting = localStorage.getItem('qbz-immersive-default-view') || 'remember';
    if (setting === 'remember') {
      const lastView = localStorage.getItem('qbz-immersive-last-view');
      if (lastView) {
        applyStoredView(lastView as ImmersiveViewKey);
      }
    } else {
      applyStoredView(setting as ImmersiveViewKey);
    }
  }

  // Fullscreen toggle
  async function toggleFullscreen() {
    const window = getCurrentWindow();
    const currentFullscreen = await window.isFullscreen();
    await window.setFullscreen(!currentFullscreen);
    isFullscreen = !currentFullscreen;
  }

  // Check fullscreen and maximized state on open
  async function checkWindowState() {
    const window = getCurrentWindow();
    isFullscreen = await window.isFullscreen();
    isMaximized = await window.isMaximized();
  }

  // Exit immersive and fullscreen
  async function handleExitImmersive() {
    if (isFullscreen) {
      const window = getCurrentWindow();
      await window.setFullscreen(false);
      isFullscreen = false;
    }
    onClose();
  }

  // Toggle maximize (not fullscreen)
  async function toggleMaximize() {
    const window = getCurrentWindow();
    if (isMaximized) {
      await window.unmaximize();
      isMaximized = false;
    } else {
      await window.maximize();
      isMaximized = true;
    }
  }

  // Minimize window
  async function minimizeWindow() {
    const window = getCurrentWindow();
    await window.minimize();
  }

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
      case 'F11':
        e.preventDefault();
        toggleFullscreen();
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
      // View mode toggle
      case 'v':
      case 'V':
        viewMode = viewMode === 'split' ? 'focus' : 'split';
        break;
      // Tab shortcuts (split mode)
      case 'l':
      case 'L':
        if (viewMode === 'split') activeTab = 'lyrics';
        break;
      case 't':
      case 'T':
        if (viewMode === 'split' && enableCredits) activeTab = 'trackInfo';
        break;
      case 's':
      case 'S':
        if (viewMode === 'split' && enableSuggestions) activeTab = 'suggestions';
        break;
      case 'q':
      case 'Q':
        if (viewMode === 'split') activeTab = 'queue';
        else if (viewMode === 'focus') activeFocusTab = 'queue-focus';
        break;
      // Focus mode tabs
      case '1':
        if (viewMode === 'focus') activeFocusTab = 'coverflow';
        break;
      case '2':
        if (viewMode === 'focus') activeFocusTab = 'static';
        break;
      case '3':
        if (viewMode === 'focus') activeFocusTab = 'lyrics-focus';
        break;
      case '4':
        if (viewMode === 'focus') activeFocusTab = 'queue-focus';
        break;
    }
    saveLastUsedView();
    resetHideTimer();
  }

  // Setup event listeners when open
  $effect(() => {
    if (isOpen) {
      restoreView();
      resetHideTimer();
      checkWindowState();
      document.addEventListener('keydown', handleKeydown);

      // Disable karaoke progress tracking in immersive mode (saves ~90% CPU on lyrics)
      setProgressTrackingEnabled(false);

      return () => {
        document.removeEventListener('keydown', handleKeydown);
        if (hideTimeout) clearTimeout(hideTimeout);
        // Re-enable progress tracking when leaving immersive
        setProgressTrackingEnabled(true);
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
    <!-- Background (skip entirely for visualizer - black background) -->
    {#if activeFocusTab !== 'visualizer'}
      <ImmersiveBackground {artwork} />
    {/if}

    <!-- Header with mode switcher -->
    <ImmersiveHeader
      {viewMode}
      {activeTab}
      {activeFocusTab}
      onViewModeChange={(mode) => { viewMode = mode; saveLastUsedView(); }}
      onTabChange={(tab) => { activeTab = tab; saveLastUsedView(); }}
      onFocusTabChange={(tab) => { activeFocusTab = tab; saveLastUsedView(); }}
      onClose={handleExitImmersive}
      visible={showUI}
      hasLyrics={true}
      hasTrackInfo={enableCredits}
      hasSuggestions={enableSuggestions}
      {isFullscreen}
      {isMaximized}
      onToggleFullscreen={toggleFullscreen}
      onToggleMaximize={toggleMaximize}
      onMinimize={minimizeWindow}
    />

    <!-- Content based on view mode -->
    {#if viewMode === 'focus'}
      <!-- Focus Mode Views -->
      {#if activeFocusTab === 'coverflow'}
        <!-- Coverflow: Animated carousel of album covers -->
        <CoverflowPanel
          {artwork}
          {trackTitle}
          {artist}
          {album}
          {isPlaying}
          {quality}
          {bitDepth}
          {samplingRate}
          {queueTracks}
          {queueCurrentIndex}
          onNavigate={(index) => onQueuePlayTrack?.(index)}
        />
      {:else if activeFocusTab === 'static'}
        <!-- Static: Single centered artwork -->
        <StaticPanel
          {artwork}
          {trackTitle}
          {artist}
          {album}
          {isPlaying}
          {quality}
          {bitDepth}
          {samplingRate}
        />
      {:else if activeFocusTab === 'vinyl'}
        <!-- Vinyl: Spinning record with album cover -->
        <VinylPanel
          {artwork}
          {trackTitle}
          {artist}
          {album}
          {isPlaying}
          {quality}
          {bitDepth}
          {samplingRate}
        />
      {:else if activeFocusTab === 'visualizer'}
        <!-- Visualizer: Audio spectrum with mirror mode -->
        <VisualizerPanel
          enabled={true}
          {artwork}
          {trackTitle}
          {artist}
          {album}
          {quality}
          {bitDepth}
          {samplingRate}
        />
      {:else if activeFocusTab === 'lyrics-focus'}
        <!-- Lyrics Focus: Single line, large, centered -->
        <LyricsFocusPanel
          lines={lyricsLines}
          activeIndex={lyricsActiveIndex}
          isLoading={lyricsLoading}
          error={lyricsError}
        />
      {:else if activeFocusTab === 'queue-focus'}
        <!-- Queue Focus: Full screen queue -->
        <div class="focus-panel">
          <div class="focus-panel-content queue-content">
            <QueuePanel
              tracks={queueTracks}
              currentIndex={queueCurrentIndex}
              onPlayTrack={(index) => onQueuePlayTrack?.(index)}
              onClear={onQueueClear}
              {historyTracks}
              onPlayHistoryTrack={(trackId) => onPlayHistoryTrack?.(trackId)}
            />
          </div>
        </div>
      {/if}
    {:else}
      <!-- Split: Artwork + Panel side by side -->
      <div class="immersive-main">
        <!-- Left: Artwork + Track Info -->
        <div class="artwork-section">
          <ImmersiveArtwork {artwork} {trackTitle} variant="floating" />
          <div class="split-track-info">
            <h2 class="split-track-title">{trackTitle}</h2>
            <p class="split-track-artist">{artist}</p>
            {#if album}
              <p class="split-track-album">{album}</p>
            {/if}
            <div class="split-quality-badge">
              <QualityBadge {quality} {bitDepth} {samplingRate} />
            </div>
          </div>
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
          {:else if activeTab === 'trackInfo'}
            <TrackInfoPanel {trackId} />
          {:else if activeTab === 'suggestions'}
            <SuggestionsPanel
              {trackId}
              {artistId}
              artistName={artist}
              trackName={trackTitle}
              currentArtwork={artwork}
            />
          {:else if activeTab === 'queue'}
            <QueuePanel
              tracks={queueTracks}
              currentIndex={queueCurrentIndex}
              onPlayTrack={(index) => onQueuePlayTrack?.(index)}
              onClear={onQueueClear}
              {historyTracks}
              onPlayHistoryTrack={(trackId) => onPlayHistoryTrack?.(trackId)}
            />
          {/if}
        </div>
      </div>
    {/if}

    <!-- Bottom Controls -->
    <PlayerControlsCompact
      visible={showUI}
      {isPlaying}
      {currentTime}
      {duration}
      {volume}
      {isShuffle}
      {repeatMode}
      {isFavorite}
      {onTogglePlay}
      {onSkipBack}
      {onSkipForward}
      {onSeek}
      {onToggleShuffle}
      {onToggleRepeat}
      {onToggleFavorite}
      {isInfinitePlay}
      {onToggleInfinitePlay}
      {onVolumeChange}
      {isFullscreen}
      {isMaximized}
      onClose={handleExitImmersive}
      onMinimize={minimizeWindow}
      onToggleFullscreen={toggleFullscreen}
      onToggleMaximize={toggleMaximize}
    />
  </div>
{/if}

<style>
  .immersive-player {
    position: fixed;
    inset: 0;
    z-index: 10001; /* Above all dropdowns/popups (z-index: 10000) */
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

  /* Split mode layout */
  .immersive-main {
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
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 20px;
  }

  .split-track-info {
    text-align: center;
    max-width: 380px;
  }

  .split-track-title {
    font-size: clamp(18px, 2.5vw, 24px);
    font-weight: 700;
    color: var(--text-primary, white);
    margin: 0 0 6px 0;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.3);
    /* Truncate long titles */
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .split-track-artist {
    font-size: clamp(14px, 1.8vw, 16px);
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    margin: 0 0 4px 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .split-track-album {
    font-size: clamp(12px, 1.5vw, 14px);
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    margin: 0 0 12px 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .split-quality-badge {
    display: flex;
    justify-content: center;
    margin-top: 8px;
  }

  .panel-section {
    flex: 1;
    min-width: 0;
    min-height: 0;
    max-width: 700px;
    height: 100%;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    align-self: center;
  }

  /* Focus mode panels (queue) */
  .focus-panel {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 80px 48px 140px;
    z-index: 5;
  }

  .focus-panel-content {
    width: 100%;
    max-width: 600px;
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .focus-panel-content.queue-content {
    max-width: 500px;
  }

  /* Responsive */
  @media (max-width: 1200px) {
    .immersive-main {
      padding: 70px 32px 130px;
      gap: 40px;
    }

    .panel-section {
      max-width: 620px;
    }

    .focus-panel {
      padding: 70px 32px 130px;
    }
  }

  @media (max-width: 900px) {
    .immersive-main {
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

    .focus-panel {
      padding: 70px 24px 140px;
    }

    .focus-panel-content {
      max-width: 100%;
    }
  }

  @media (max-width: 600px) {
    .immersive-main {
      padding: 60px 16px 130px;
      gap: 20px;
    }

    .focus-panel {
      padding: 60px 16px 130px;
    }
  }
</style>
