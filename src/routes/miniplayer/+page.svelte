<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { t } from '$lib/i18n';
  import MiniPlayerShell from '$lib/components/miniplayer/MiniPlayerShell.svelte';
  import type { MiniPlayerQueueTrack, MiniPlayerSurface } from '$lib/components/miniplayer/types';
  import {
    subscribe as subscribePlayer,
    getPlayerState,
    togglePlay,
    seek as playerSeek,
    setVolume as playerSetVolume,
    startPolling,
    stopPolling,
    type PlayerState
  } from '$lib/stores/playerStore';
  import {
    subscribe as subscribeQueue,
    getQueueState,
    toggleShuffle,
    toggleRepeat,
    nextTrack,
    previousTrack,
    syncQueueState,
    startQueueEventListener,
    stopQueueEventListener,
    type QueueState
  } from '$lib/stores/queueStore';
  import {
    subscribe as subscribeLyrics,
    getLyricsState,
    startWatching,
    stopWatching,
    startActiveLineUpdates,
    stopActiveLineUpdates,
    type LyricsState
  } from '$lib/stores/lyricsStore';
  import {
    getMiniPlayerState,
    initMiniPlayerState,
    setMiniPlayerGeometry,
    setMiniPlayerOpen,
    setMiniPlayerSurface
  } from '$lib/stores/uiStore';
  import { exitMiniplayerMode, setMiniplayerAlwaysOnTop } from '$lib/services/miniplayerService';

  let playerState = $state<PlayerState>(getPlayerState());
  let queueState = $state<QueueState>(getQueueState());
  let lyricsState = $state<LyricsState>(getLyricsState());
  let miniState = $state(getMiniPlayerState());
  let showAlwaysOnTopWarning = $state(false);

  let unlistenMoved: (() => void) | null = null;
  let unlistenResized: (() => void) | null = null;
  let unlistenCloseRequested: (() => void) | null = null;

  let unsubscribePlayer: (() => void) | null = null;
  let unsubscribeQueue: (() => void) | null = null;
  let unsubscribeLyrics: (() => void) | null = null;
  let lastExpandedGeometry = $state({ width: 380, height: 540 });

  const COMPACT_GEOMETRY = {
    width: 380,
    height: 178
  };

  const MICRO_GEOMETRY = {
    width: 380,
    height: 57
  };

  const MICRO_MIN_HEIGHT = 57;
  const COMPACT_MIN_HEIGHT = 170;
  const EXPANDED_MIN_HEIGHT = 420;

  const EXPANDED_DEFAULT_GEOMETRY = {
    width: 380,
    height: 540
  };

  function isCondensedSurface(surface: MiniPlayerSurface): boolean {
    return surface === 'micro' || surface === 'compact';
  }

  function formatQualityLabel(track: PlayerState['currentTrack']): string | undefined {
    if (!track) return undefined;
    if (track.bitDepth && track.samplingRate) {
      return `${track.bitDepth}/${Math.round(track.samplingRate / 1000)}`;
    }
    return track.quality || undefined;
  }

  const activeSurface = $derived(miniState.surface);
  const currentTrackId = $derived(playerState.currentTrack ? String(playerState.currentTrack.id) : undefined);

  const queueTracks = $derived.by(() => {
    const rows: MiniPlayerQueueTrack[] = [];

    if (playerState.currentTrack) {
      rows.push({
        id: String(playerState.currentTrack.id),
        title: playerState.currentTrack.title,
        artist: playerState.currentTrack.artist,
        artwork: playerState.currentTrack.artwork,
        quality: formatQualityLabel(playerState.currentTrack)
      });
    }

    for (const queueTrack of queueState.queue) {
      if (rows.some(row => row.id === queueTrack.id)) {
        continue;
      }
      rows.push({
        id: queueTrack.id,
        title: queueTrack.title,
        artist: queueTrack.artist,
        artwork: queueTrack.artwork,
        quality: queueTrack.duration
      });
    }

    return rows;
  });

  function rememberExpandedGeometry(width: number, height: number): void {
    if (height < 320) return;
    lastExpandedGeometry = {
      width: Math.max(340, width),
      height: Math.max(420, height)
    };
  }

  async function applySurfaceGeometry(surface: MiniPlayerSurface): Promise<void> {
    const appWindow = getCurrentWindow();

    if (isCondensedSurface(surface)) {
      const condensedGeometry = surface === 'micro' ? MICRO_GEOMETRY : COMPACT_GEOMETRY;
      const condensedMinHeight = surface === 'micro' ? MICRO_MIN_HEIGHT : COMPACT_MIN_HEIGHT;

      await appWindow.setMinSize(new LogicalSize(340, condensedMinHeight));

      if (!isCondensedSurface(miniState.surface)) {
        const currentSize = await appWindow.innerSize();
        rememberExpandedGeometry(currentSize.width, currentSize.height);
      }

      await appWindow.setSize(new LogicalSize(condensedGeometry.width, condensedGeometry.height));
      setMiniPlayerGeometry({ width: condensedGeometry.width, height: condensedGeometry.height });
      miniState = getMiniPlayerState();
      return;
    }

    await appWindow.setMinSize(new LogicalSize(340, EXPANDED_MIN_HEIGHT));

    const target = {
      width: lastExpandedGeometry.width || EXPANDED_DEFAULT_GEOMETRY.width,
      height: lastExpandedGeometry.height || EXPANDED_DEFAULT_GEOMETRY.height
    };

    await appWindow.setSize(new LogicalSize(target.width, target.height));
    setMiniPlayerGeometry({ width: target.width, height: target.height });
    miniState = getMiniPlayerState();
  }

  $effect(() => {
    if (lyricsState.isSynced && lyricsState.lines.length > 0 && playerState.isPlaying) {
      startActiveLineUpdates();
    } else {
      stopActiveLineUpdates();
    }
  });

  onMount(async () => {
    initMiniPlayerState();
    miniState = getMiniPlayerState();
    setMiniPlayerOpen(true);

    if (isCondensedSurface(miniState.surface)) {
      await applySurfaceGeometry(miniState.surface);
    } else {
      rememberExpandedGeometry(miniState.geometry.width, miniState.geometry.height);
    }

    await startPolling();
    await syncQueueState();
    await startQueueEventListener();
    startWatching();

    unsubscribePlayer = subscribePlayer(() => {
      playerState = getPlayerState();
    });

    unsubscribeQueue = subscribeQueue(() => {
      queueState = getQueueState();
    });

    unsubscribeLyrics = subscribeLyrics(() => {
      lyricsState = getLyricsState();
    });

    const appWindow = getCurrentWindow();

    unlistenMoved = await appWindow.onMoved(({ payload }) => {
      setMiniPlayerGeometry({ x: payload.x, y: payload.y });
    });

    unlistenResized = await appWindow.onResized(({ payload }) => {
      setMiniPlayerGeometry({ width: payload.width, height: payload.height });
      if (!isCondensedSurface(miniState.surface)) {
        rememberExpandedGeometry(payload.width, payload.height);
      }
    });

    unlistenCloseRequested = await appWindow.onCloseRequested((event) => {
      event.preventDefault();
      void exitMiniplayerMode();
    });

    console.info('[MiniPlayer] Window ready');
  });

  onDestroy(() => {
    unsubscribePlayer?.();
    unsubscribeQueue?.();
    unsubscribeLyrics?.();

    unlistenMoved?.();
    unlistenResized?.();
    unlistenCloseRequested?.();

    stopActiveLineUpdates();
    stopWatching();
    stopQueueEventListener();
    stopPolling();
  });

  async function handleSurfaceChange(surface: MiniPlayerSurface): Promise<void> {
    if (surface === miniState.surface) return;

    const targetCondensed = isCondensedSurface(surface);
    const currentCondensed = isCondensedSurface(miniState.surface);
    if (targetCondensed || currentCondensed) {
      await applySurfaceGeometry(surface);
    }

    setMiniPlayerSurface(surface);
    miniState = getMiniPlayerState();
    console.info('[MiniPlayer] Surface changed:', surface);
  }

  async function handleToggleAlwaysOnTop(): Promise<void> {
    const targetValue = !miniState.alwaysOnTop;
    const result = await setMiniplayerAlwaysOnTop(targetValue);
    miniState = getMiniPlayerState();

    showAlwaysOnTopWarning = !result.applied && targetValue;
    if (!result.applied) {
      console.warn('[MiniPlayer] Always-on-top could not be fully applied:', result.reason);
    }
  }

  async function handleStartDrag(event: MouseEvent): Promise<void> {
    event.stopPropagation();

    try {
      await getCurrentWindow().startDragging();
    } catch (err) {
      console.error('[MiniPlayer] Drag start failed:', err);
    }
  }

  async function handleTogglePlay(): Promise<void> {
    try {
      await togglePlay();
    } catch (err) {
      console.error('[MiniPlayer] togglePlay failed:', err);
    }
  }

  async function handleSkipBack(): Promise<void> {
    try {
      await previousTrack();
    } catch (err) {
      console.error('[MiniPlayer] previousTrack failed:', err);
    }
  }

  async function handleSkipForward(): Promise<void> {
    try {
      await nextTrack();
    } catch (err) {
      console.error('[MiniPlayer] nextTrack failed:', err);
    }
  }

  async function handleToggleShuffle(): Promise<void> {
    try {
      await toggleShuffle();
    } catch (err) {
      console.error('[MiniPlayer] toggleShuffle failed:', err);
    }
  }

  async function handleToggleRepeat(): Promise<void> {
    try {
      await toggleRepeat();
    } catch (err) {
      console.error('[MiniPlayer] toggleRepeat failed:', err);
    }
  }
</script>

<div class="miniplayer-page">
  <MiniPlayerShell
    activeSurface={activeSurface}
    isPinned={miniState.alwaysOnTop}
    artwork={playerState.currentTrack?.artwork}
    title={playerState.currentTrack?.title}
    artist={playerState.currentTrack?.artist}
    album={playerState.currentTrack?.album}
    queueTracks={queueTracks}
    {currentTrackId}
    lyricsLines={lyricsState.lines}
    lyricsActiveIndex={lyricsState.activeIndex}
    lyricsActiveProgress={lyricsState.activeProgress}
    lyricsIsSynced={lyricsState.isSynced}
    isPlaying={playerState.isPlaying}
    currentTime={playerState.currentTime}
    duration={playerState.duration}
    volume={playerState.volume}
    isShuffle={queueState.isShuffle}
    repeatMode={queueState.repeatMode}
    onSurfaceChange={(surface) => {
      void handleSurfaceChange(surface);
    }}
    onTogglePin={handleToggleAlwaysOnTop}
    onExpand={exitMiniplayerMode}
    onClose={exitMiniplayerMode}
    onStartDrag={handleStartDrag}
    onTogglePlay={handleTogglePlay}
    onSkipBack={handleSkipBack}
    onSkipForward={handleSkipForward}
    onSeek={playerSeek}
    onVolumeChange={playerSetVolume}
    onToggleShuffle={handleToggleShuffle}
    onToggleRepeat={handleToggleRepeat}
  />

  {#if showAlwaysOnTopWarning}
    <div class="aot-warning">{$t('player.miniAlwaysOnTopLimited')}</div>
  {/if}
</div>

<style>
  .miniplayer-page {
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: transparent;
    padding: 6px;
    box-sizing: border-box;
    position: relative;
  }

  .aot-warning {
    position: absolute;
    bottom: 10px;
    left: 50%;
    transform: translateX(-50%);
    background: color-mix(in srgb, #d97706 20%, var(--bg-secondary));
    border: 1px solid color-mix(in srgb, #d97706 35%, transparent);
    color: var(--text-primary);
    border-radius: 999px;
    padding: 4px 10px;
    font-size: 11px;
    z-index: 90;
    white-space: nowrap;
    pointer-events: none;
  }
</style>
