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
    Minus,
    Volume2,
    VolumeX,
    Volume1,
    Monitor,
    Cast,
    MicVocal,
    Maximize2,
    PictureInPicture2,
    TriangleAlert,
    MoreHorizontal
  } from 'lucide-svelte';
  import { t } from 'svelte-i18n';
  import { isRemoteMode, playbackTarget } from '$lib/stores/playbackTargetStore';
  import { disconnectFromRemote } from '$lib/stores/playbackTargetStore';
  import QualityBadge from './QualityBadge.svelte';
  import AudioOutputBadges from './AudioOutputBadges.svelte';
  import QconnectBadge from './QconnectBadge.svelte';
  import StackIcon from './StackIcon.svelte';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { t as translateStore } from '$lib/i18n';
  import {
    subscribe as subscribeOffline,
    isOffline as checkIsOffline,
    getOfflineReason,
    refreshStatus,
    type OfflineReason
  } from '$lib/stores/offlineStore';
  import {
    toggleMute,
    getBitPerfectMode,
    subscribe as subscribePlayer,
    type BitPerfectMode,
  } from '$lib/stores/playerStore';
  import {
    subscribe as subscribeDegraded,
    isDegraded
  } from '$lib/stores/degradedStore';
  import type { QconnectSessionSnapshot } from '$lib/services/qconnectRuntime';

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
    onQobuzConnect?: () => void;
    onToggleLyrics?: () => void;
    lyricsActive?: boolean;
    isCastConnected?: boolean;
    isQobuzConnectConnected?: boolean;
    onArtistClick?: () => void;
    onAlbumClick?: () => void;
    onTrackClick?: () => void;
    onContextClick?: () => void;
    queueOpen?: boolean;
    normalizationEnabled?: boolean;
    normalizationGain?: number | null;
    onToggleNormalization?: () => void;
    controlsDisabled?: boolean;
    explicit?: boolean;
    qconnectSessionSnapshot?: QconnectSessionSnapshot | null;
    onToggleQconnectConnection?: () => void | Promise<void>;
    qconnectBusy?: boolean;
    showQconnectDevButton?: boolean;
    volumeLocked?: boolean;
    bufferProgress?: number | null;
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
    onQobuzConnect,
    onToggleLyrics,
    lyricsActive = false,
    isCastConnected = false,
    isQobuzConnectConnected = false,
    onArtistClick,
    onAlbumClick,
    onTrackClick,
    onContextClick,
    queueOpen = false,
    normalizationEnabled = false,
    normalizationGain = null,
    onToggleNormalization,
    controlsDisabled = false,
    explicit = false,
    qconnectSessionSnapshot = null,
    onToggleQconnectConnection,
    qconnectBusy = false,
    showQconnectDevButton = false,
    volumeLocked = false,
    bufferProgress = null,
  }: Props = $props();

  let progressRef = $state<HTMLDivElement | null>(null);
  let volumeRef = $state<HTMLDivElement | null>(null);
  let barRef = $state<HTMLElement | null>(null);
  let isDraggingProgress = $state(false);
  let isDraggingVolume = $state(false);
  let showArtworkPreview = $state(false);
  let dragPreviewTime = $state<number | null>(null);

  // Narrow-layout detection (issue #303): below this width the right-section
  // collapses into a hamburger popup and the quality/qconnect badges switch
  // to compact variants so nothing overlaps. Uses ResizeObserver on the bar
  // itself instead of a viewport media query so the breakpoint reacts to
  // the actual space the bar gets (sidebar state, window chrome, etc.).
  const NARROW_LAYOUT_PX = 1100;
  let isNarrowBar = $state(false);
  let isOverflowMenuOpen = $state(false);
  let isVolumePopupOpen = $state(false);

  $effect(() => {
    if (!barRef) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const width = entry.contentRect.width;
        const narrow = width > 0 && width < NARROW_LAYOUT_PX;
        if (narrow !== isNarrowBar) {
          isNarrowBar = narrow;
          if (!narrow) {
            isOverflowMenuOpen = false;
            isVolumePopupOpen = false;
          }
        }
      }
    });
    ro.observe(barRef);
    return () => ro.disconnect();
  });

  function toggleOverflowMenu(): void {
    isOverflowMenuOpen = !isOverflowMenuOpen;
    if (isOverflowMenuOpen) isVolumePopupOpen = false;
  }

  function closeOverflowMenu(): void {
    isOverflowMenuOpen = false;
  }

  function toggleVolumePopup(): void {
    isVolumePopupOpen = !isVolumePopupOpen;
    if (isVolumePopupOpen) isOverflowMenuOpen = false;
  }

  function closeVolumePopup(): void {
    isVolumePopupOpen = false;
  }

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

  // Degraded service state
  let isDegradedState = $state(isDegraded());

  $effect(() => {
    const unsubDegraded = subscribeDegraded(() => {
      isDegradedState = isDegraded();
    });
    return unsubDegraded;
  });

  // Bit-perfect mode reported by the audio backend (null until first stream).
  let bitPerfectMode = $state<BitPerfectMode | null>(getBitPerfectMode());

  $effect(() => {
    const unsubPlayer = subscribePlayer(() => {
      bitPerfectMode = getBitPerfectMode();
    });
    return unsubPlayer;
  });

  // Get human-readable offline reason
  function getOfflineReasonText(reason: OfflineReason | null): string {
    switch (reason) {
      case 'no_network':
        return $translateStore('offline.noNetwork');
      case 'not_logged_in':
        return $translateStore('offline.notLoggedIn');
      case 'manual_override':
        return $translateStore('offline.manualMode');
      default:
        return $translateStore('offline.indicator');
    }
  }

  // Force an immediate network check when the offline indicator is clicked
  async function handleCheckNetwork() {
    await refreshStatus();
  }

  const effectiveTime = $derived(dragPreviewTime ?? currentTime);
  const progress = $derived(duration > 0 ? (effectiveTime / duration) * 100 : 0);
  const hasTrack = $derived(trackTitle !== '');
  const remainingTime = $derived(Math.max(0, duration - effectiveTime));

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
      dragPreviewTime = newTime;
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
    if (isDraggingProgress && dragPreviewTime !== null) {
      onSeek?.(dragPreviewTime);
    }
    isDraggingProgress = false;
    isDraggingVolume = false;
    dragPreviewTime = null;
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

  // Set CSS variable for player bar height so page layout adjusts
  $effect(() => {
    const barHeight = $isRemoteMode ? 128 : 104;
    document.documentElement.style.setProperty('--player-bar-height', `${barHeight}px`);
  });
</script>

<div class="now-playing-bar" class:has-remote-banner={$isRemoteMode} class:narrow={isNarrowBar} bind:this={barRef}>
  {#if $isRemoteMode}
    <div class="remote-indicator">
      <Monitor size={14} />
      <span>{$t('player.controllingRemote', { values: { name: $playbackTarget.name || 'Remote' } })}</span>
      <button class="remote-disconnect" onclick={disconnectFromRemote}>{$t('settings.integrations.disconnect')}</button>
    </div>
  {/if}
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
        {#if bufferProgress != null && bufferProgress < 1}
          <div class="seekbar-buffer" style="width: {bufferProgress * 100}%"></div>
        {/if}
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
        class:active={isShuffle && !controlsDisabled}
        class:disabled={controlsDisabled}
        disabled={controlsDisabled}
        onclick={controlsDisabled ? undefined : onToggleShuffle}
        title={$translateStore('player.shuffle')}
      >
        <Shuffle size={16} />
      </button>

      <button
        class="control-btn"
        class:disabled={controlsDisabled}
        disabled={controlsDisabled}
        onclick={controlsDisabled ? undefined : onSkipBack}
        title={$translateStore('player.previous')}
      >
        <SkipBack size={18} />
      </button>

      <button
        class="control-btn play-btn"
        class:disabled={controlsDisabled}
        disabled={controlsDisabled}
        onclick={controlsDisabled ? undefined : onTogglePlay}
        title={isPlaying ? $translateStore('player.pause') : $translateStore('player.play')}
      >
        {#if isPlaying}
          <Pause size={20} />
        {:else}
          <Play size={20} />
        {/if}
      </button>

      <button
        class="control-btn"
        class:disabled={controlsDisabled}
        disabled={controlsDisabled}
        onclick={controlsDisabled ? undefined : onSkipForward}
        title={$translateStore('player.next')}
      >
        <SkipForward size={18} />
      </button>

      <button
        class="control-btn"
        class:active={repeatMode !== 'off' && !controlsDisabled}
        class:disabled={controlsDisabled}
        disabled={controlsDisabled}
        onclick={controlsDisabled ? undefined : onToggleRepeat}
        title={repeatMode === 'off' ? $translateStore('player.repeat') : repeatMode === 'all' ? $translateStore('player.repeatAll') : $translateStore('player.repeatOne')}
      >
        {#if repeatMode === 'one'}
          <Repeat1 size={16} />
        {:else}
          <Repeat size={16} />
        {/if}
      </button>

      <button
        class="control-btn"
        class:disabled={controlsDisabled}
        disabled={controlsDisabled}
        onclick={controlsDisabled ? undefined : onAddToPlaylist}
        title={$translateStore('actions.addToPlaylist')}
      >
        <Plus size={16} />
      </button>

      <button
        class="control-btn"
        class:active={isFavorite && !controlsDisabled}
        class:disabled={controlsDisabled}
        disabled={controlsDisabled}
        onclick={controlsDisabled ? undefined : onToggleFavorite}
        title={isFavorite ? $translateStore('actions.removeFromFavorites') : $translateStore('actions.addToFavorites')}
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
              <img use:cachedSrc={artwork} alt={trackTitle} class="artwork" />
            {:else}
              <div class="artwork-placeholder"></div>
            {/if}

            <!-- Artwork Preview on Hover -->
            {#if showArtworkPreview && artwork}
              <div class="artwork-preview">
                <img use:cachedSrc={artwork} alt={trackTitle} />
              </div>
            {/if}
          </button>

          <div class="song-info">
            <div class="song-title-row">
              <button class="song-title" title={$translateStore('actions.trackInfo')} onclick={onTrackClick}>{trackTitle}</button>
              {#if explicit}
                <span class="explicit-badge" title={ $t('library.explicit') }></span>
              {/if}
            </div>
            <div class="song-meta">
              <StackIcon size={12} class="stack-icon" onClick={onContextClick} />
              {#if artist}
                <button class="meta-link" onclick={onArtistClick} title={$translateStore('actions.goToArtist')}>
                  {artist}
                </button>
              {/if}
              {#if artist && album}
                <span class="meta-separator">·</span>
              {/if}
              {#if album}
                <button class="meta-link" onclick={onAlbumClick} title={$translateStore('actions.goToAlbum')}>
                  {album}
                </button>
              {/if}
            </div>
          </div>

          <div class="badges-group" class:narrow={isNarrowBar}>
            <div class="quality-indicator">
              <QualityBadge
                {quality}
                {bitDepth}
                {samplingRate}
                {originalBitDepth}
                {originalSamplingRate}
                {format}
                {bitPerfectMode}
                compact={isNarrowBar}
              />
              <div class="audio-badges-row">
                <AudioOutputBadges {samplingRate} />
              </div>
            </div>
            <QconnectBadge
              connected={isQobuzConnectConnected}
              sessionSnapshot={qconnectSessionSnapshot}
              onToggleConnection={onToggleQconnectConnection ?? (() => {})}
              busy={qconnectBusy}
              compact={isNarrowBar}
            />
          </div>
        </div>
      {:else}
        <div class="empty-state">
          <span>{$translateStore('player.noTrackPlaying')}</span>
        </div>
      {/if}
    </div>

    <!-- Right: Actions & Volume -->
    <div class="right-section">
      {#if isOffline}
        <button
          class="offline-indicator"
          title={$translateStore('offline.checkNow')}
          onclick={handleCheckNetwork}
          aria-label={$translateStore('offline.checkNow')}
        >
          <img src="/offline-small.svg" alt="" class="offline-icon" aria-hidden="true" />
        </button>
      {/if}

      {#if !isOffline && isDegradedState}
        <div
          class="degraded-indicator"
          title={$translateStore('degraded.title')}
          role="status"
        >
          <TriangleAlert size={16} />
        </div>
      {/if}

      {#if !isNarrowBar}
        <!-- Wide layout: each secondary control visible inline -->
        <button
          class="control-btn"
          class:cast-active={isCastConnected}
          onclick={onCast}
          title={isCastConnected ? $translateStore('player.castingManage') : $translateStore('player.castToDevice')}
        >
          <Cast size={16} />
        </button>

        {#if showQconnectDevButton}
        <button
          class="control-btn"
          class:qconnect-active={isQobuzConnectConnected}
          onclick={onQobuzConnect}
          title={isQobuzConnectConnected ? $translateStore('player.qobuzConnectManage') : $translateStore('player.qobuzConnect')}
        >
          <span class="qconnect-icon" aria-hidden="true"></span>
        </button>
        {/if}

        <button
          class="control-btn"
          class:active={lyricsActive && !isOffline}
          class:disabled={isOffline}
          onclick={isOffline ? undefined : onToggleLyrics}
          disabled={isOffline}
          title={isOffline ? $translateStore('offline.featureDisabled') : $translateStore('player.lyrics')}
          aria-label={isOffline ? $translateStore('offline.featureDisabled') : $translateStore('player.lyrics')}
        >
          <MicVocal size={16} aria-hidden="true" />
        </button>

        {#if onOpenMiniPlayer}
          <button class="control-btn" onclick={onOpenMiniPlayer} title={$translateStore('player.miniPlayer')}>
            <PictureInPicture2 size={16} />
          </button>
        {/if}

        <button class="control-btn" onclick={onOpenFullScreen} title={$translateStore('player.fullScreen')}>
          <Maximize2 size={16} />
        </button>

        <button
          class="control-btn"
          class:active={normalizationEnabled && normalizationGain !== null && normalizationGain !== 1.0}
          class:norm-enabled={normalizationEnabled && (normalizationGain === null || normalizationGain === 1.0)}
          onclick={onToggleNormalization}
          title={!normalizationEnabled
            ? $translateStore('player.normalizationOff')
            : normalizationGain !== null && normalizationGain !== 1.0
              ? $translateStore('player.normalizationApplied')
              : $translateStore('player.normalizationOn')}
        >
          <span
            class="norm-icon"
            class:norm-on={normalizationEnabled}
            aria-hidden="true"
          ></span>
        </button>
      {:else}
        <!-- Narrow layout: collapse secondary controls into a hamburger menu -->
        <div class="overflow-wrapper">
          <button
            class="control-btn overflow-btn"
            class:active={isOverflowMenuOpen}
            onclick={toggleOverflowMenu}
            title={$translateStore('player.moreControls')}
            aria-label={$translateStore('player.moreControls')}
            aria-expanded={isOverflowMenuOpen}
          >
            <MoreHorizontal size={16} />
          </button>

          {#if isOverflowMenuOpen}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="popup-backdrop" onclick={closeOverflowMenu}></div>
            <div class="overflow-menu" role="menu">
              <button
                class="overflow-item"
                class:active={isCastConnected}
                onclick={() => { closeOverflowMenu(); onCast?.(); }}
              >
                <Cast size={16} />
                <span>{isCastConnected ? $translateStore('player.castingManage') : $translateStore('player.castToDevice')}</span>
              </button>

              {#if showQconnectDevButton}
                <button
                  class="overflow-item"
                  class:active={isQobuzConnectConnected}
                  onclick={() => { closeOverflowMenu(); onQobuzConnect?.(); }}
                >
                  <span class="qconnect-icon" aria-hidden="true"></span>
                  <span>{isQobuzConnectConnected ? $translateStore('player.qobuzConnectManage') : $translateStore('player.qobuzConnect')}</span>
                </button>
              {/if}

              <button
                class="overflow-item"
                class:active={lyricsActive && !isOffline}
                disabled={isOffline}
                onclick={() => { closeOverflowMenu(); if (!isOffline) onToggleLyrics?.(); }}
              >
                <MicVocal size={16} />
                <span>{isOffline ? $translateStore('offline.featureDisabled') : $translateStore('player.lyrics')}</span>
              </button>

              {#if onOpenMiniPlayer}
                <button
                  class="overflow-item"
                  onclick={() => { closeOverflowMenu(); onOpenMiniPlayer?.(); }}
                >
                  <PictureInPicture2 size={16} />
                  <span>{$translateStore('player.miniPlayer')}</span>
                </button>
              {/if}

              <button
                class="overflow-item"
                onclick={() => { closeOverflowMenu(); onOpenFullScreen?.(); }}
              >
                <Maximize2 size={16} />
                <span>{$translateStore('player.fullScreen')}</span>
              </button>

              <button
                class="overflow-item"
                class:active={normalizationEnabled && normalizationGain !== null && normalizationGain !== 1.0}
                onclick={() => { closeOverflowMenu(); onToggleNormalization?.(); }}
              >
                <span class="norm-icon" class:norm-on={normalizationEnabled} aria-hidden="true"></span>
                <span>{!normalizationEnabled
                  ? $translateStore('player.normalizationOff')
                  : normalizationGain !== null && normalizationGain !== 1.0
                    ? $translateStore('player.normalizationApplied')
                    : $translateStore('player.normalizationOn')}</span>
              </button>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Volume Control: inline at wide widths, popup button at narrow widths.
           The popup variant keeps only the mute/unmute button on the bar and
           opens a vertical slider upward on click (XP-style). Locked volume
           (ALSA hw:) always shows the disabled inline variant so users can see
           the 100% indicator without clicking. -->
      {#if isNarrowBar && !volumeLocked}
        <div class="volume-wrapper" class:volume-locked={volumeLocked}>
          <button
            class="control-btn volume-btn"
            class:active={isVolumePopupOpen}
            onclick={toggleVolumePopup}
            title={volume === 0 ? $translateStore('player.unmute') : $translateStore('player.volume')}
            aria-label={$translateStore('player.volume')}
            aria-expanded={isVolumePopupOpen}
          >
            {#if volume === 0}
              <VolumeX size={16} />
            {:else if volume < 50}
              <Volume1 size={16} />
            {:else}
              <Volume2 size={16} />
            {/if}
          </button>

          {#if isVolumePopupOpen}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="popup-backdrop" onclick={closeVolumePopup}></div>
            <div class="volume-popup">
              <span class="volume-popup-value">{volume}</span>
              <input
                type="range"
                min="0"
                max="100"
                value={volume}
                class="volume-popup-slider"
                oninput={(e) => onVolumeChange?.(Number((e.target as HTMLInputElement).value))}
                aria-label={$translateStore('player.volume')}
              />
              <button
                class="control-btn volume-popup-mute"
                onclick={() => toggleMute()}
                title={volume === 0 ? $translateStore('player.unmute') : $translateStore('player.mute')}
              >
                {#if volume === 0}
                  <VolumeX size={14} />
                {:else}
                  <Volume2 size={14} />
                {/if}
              </button>
            </div>
          {/if}
        </div>
      {:else}
      <div class="volume-control" class:volume-locked={volumeLocked}>
        {#if volumeLocked}
          <button
            class="control-btn volume-btn"
            title={$translateStore('player.volumeLockedHw')}
            disabled
          >
            <Volume2 size={16} />
          </button>

          <div
            class="volume-slider"
            role="slider"
            tabindex="-1"
            aria-valuenow={100}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-disabled="true"
          >
            <div class="volume-track">
              <div class="volume-fill" style="width: 100%"></div>
            </div>
            <div class="volume-thumb" style="left: 100%"></div>
          </div>
        {:else}
          <div class="volume-value" class:visible={isDraggingVolume}>{volume}</div>
          <button
            class="control-btn volume-btn"
            onclick={() => toggleMute()}
            title={volume === 0 ? $translateStore('player.unmute') : $translateStore('player.mute')}
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

          <button
            class="control-btn volume-step-btn"
            onclick={() => onVolumeChange?.(Math.max(0, volume - 5))}
            title={$translateStore('player.volumeDown')}
          >
            <Minus size={14} />
          </button>

          <button
            class="control-btn volume-step-btn"
            onclick={() => onVolumeChange?.(Math.min(100, volume + 5))}
            title={$translateStore('player.volumeUp')}
          >
            <Plus size={14} />
          </button>
        {/if}
      </div>
      {/if}

      <!-- Queue Button (far right) -->
      <button
        class="control-btn queue-btn"
        class:active={queueOpen}
        onclick={onOpenQueue}
        title={$translateStore('player.queue')}
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
    z-index: 2001;
    display: flex;
    flex-direction: column;
  }

  .now-playing-bar.has-remote-banner {
    height: 128px;
  }

  .remote-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 4px 12px;
    background: var(--accent-primary);
    color: white;
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.02em;
  }

  .remote-disconnect {
    background: rgba(255, 255, 255, 0.2);
    border: none;
    color: white;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 4px;
    cursor: pointer;
    margin-left: 4px;
  }

  .remote-disconnect:hover {
    background: rgba(255, 255, 255, 0.35);
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
    position: relative;
    width: 100%;
    height: 3px;
    background: var(--border-subtle);
    border-radius: 999px;
    overflow: hidden;
  }

  .seekbar-buffer {
    position: absolute;
    height: 100%;
    background: var(--text-muted, #666);
    opacity: 0.3;
    border-radius: 999px;
    transition: width 250ms linear;
  }

  .seekbar-fill {
    position: relative;
    height: 100%;
    background: var(--accent-primary, #6366f1);
    border-radius: 999px;
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
    /* keep both layers stable on hover */
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
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease, transform 150ms ease;
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

  .control-btn.qconnect-active {
    color: var(--accent-primary, #6366f1);
    animation: qconnect-pulse 2s ease-in-out infinite;
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

  @keyframes qconnect-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.7; }
  }

  .qconnect-icon {
    display: block;
    width: 16px;
    height: 16px;
    background-color: currentColor;
    mask-image: url('/qobuz-logo.svg');
    mask-size: contain;
    mask-repeat: no-repeat;
    mask-position: center;
    -webkit-mask-image: url('/qobuz-logo.svg');
    -webkit-mask-size: contain;
    -webkit-mask-repeat: no-repeat;
    -webkit-mask-position: center;
  }

  /* Section Separator */
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
    border: none;
    padding: 0;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .offline-indicator:hover {
    background: rgba(234, 179, 8, 0.3);
  }

  /* Degraded Service Indicator */
  .degraded-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 6px;
    background: rgba(251, 146, 60, 0.15);
    color: #fb923c;
    cursor: help;
  }

  .offline-icon {
    width: 16px;
    height: 16px;
    opacity: 0.9;
  }

  /* Normalization Icon (mask-image for currentColor inheritance) */
  .norm-icon {
    display: block;
    width: 16px;
    height: 16px;
    background-color: currentColor;
    mask-image: url('/bars-disorder-outlined.svg');
    mask-size: contain;
    mask-repeat: no-repeat;
    mask-position: center;
    -webkit-mask-image: url('/bars-disorder-outlined.svg');
    -webkit-mask-size: contain;
    -webkit-mask-repeat: no-repeat;
    -webkit-mask-position: center;
  }

  .norm-icon.norm-on {
    mask-image: url('/bars-normal.svg');
    -webkit-mask-image: url('/bars-normal.svg');
  }

  .control-btn.norm-enabled {
    color: var(--text-primary);
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

  .play-btn.disabled {
    color: var(--text-disabled);
  }

  .play-btn.disabled:hover {
    color: var(--text-disabled);
  }

  /* ===== Song Card ===== */
  .song-card {
    display: flex;
    align-items: stretch;
    gap: 12px;
    padding: 2px;
    background: var(--bg-tertiary);
    border-radius: 8px;
    min-width: 354px;
    flex: 1;
    max-width: 718px;
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

  .song-title-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
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

  .explicit-badge {
    display: inline-block;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    opacity: 0.45;
    background-color: var(--text-secondary);
    -webkit-mask: url('/explicit.svg') center / contain no-repeat;
    mask: url('/explicit.svg') center / contain no-repeat;
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

  .badges-group {
    display: flex;
    align-items: stretch;
    gap: 3px;
    flex-shrink: 0;
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

  .volume-step-btn {
    width: 24px;
    height: 24px;
    flex-shrink: 0;
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

  .volume-locked {
    opacity: 0.5;
    pointer-events: none;
  }

  /* ===== Narrow layout: hamburger + volume popup (issue #303) ===== */
  .overflow-wrapper {
    position: relative;
    display: flex;
    align-items: center;
  }

  .overflow-btn.active {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .popup-backdrop {
    position: fixed;
    inset: 0;
    z-index: 9998;
  }

  .overflow-menu {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    z-index: 9999;
    min-width: 200px;
    max-width: 260px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .overflow-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .overflow-item:hover:not(:disabled) {
    background: var(--alpha-6);
    color: var(--text-primary);
  }

  .overflow-item:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .overflow-item.active {
    color: var(--text-primary);
    background: var(--alpha-6);
  }

  .overflow-item > span:last-child {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* XP-style volume popup */
  .volume-wrapper {
    position: relative;
    display: flex;
    align-items: center;
  }

  .volume-popup {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 50%;
    transform: translateX(50%);
    z-index: 9999;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    padding: 12px 10px 8px 10px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 54px;
  }

  .volume-popup-value {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
    font-variant-numeric: tabular-nums;
  }

  .volume-popup-slider {
    -webkit-appearance: slider-vertical;
    appearance: slider-vertical;
    writing-mode: vertical-lr;
    direction: rtl;
    width: 22px;
    height: 140px;
    padding: 0;
    margin: 0;
    cursor: pointer;
    background: transparent;
  }

  .volume-popup-slider::-webkit-slider-runnable-track {
    width: 4px;
    background: var(--alpha-15);
    border-radius: 2px;
  }

  .volume-popup-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--text-primary);
    border: 2px solid var(--bg-secondary);
    cursor: pointer;
  }

  .volume-popup-slider::-moz-range-track {
    width: 4px;
    background: var(--alpha-15);
    border-radius: 2px;
  }

  .volume-popup-slider::-moz-range-thumb {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--text-primary);
    border: 2px solid var(--bg-secondary);
    cursor: pointer;
  }

  .volume-popup-mute {
    width: 28px;
    height: 28px;
  }
</style>
