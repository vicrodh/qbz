<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import { ArrowLeft, Info, ListPlus, Play, RefreshCw, Search, Shuffle, X, CheckSquare } from 'lucide-svelte';
  import PlaylistModal from '$lib/components/PlaylistModal.svelte';
  import TrackRow from '$lib/components/TrackRow.svelte';
  import BulkActionBar from '$lib/components/BulkActionBar.svelte';
  import { t } from '$lib/i18n';
  import { formatDuration, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import { showToast } from '$lib/stores/toastStore';
  import { setPlaybackContext } from '$lib/stores/playbackContextStore';
  import { isBlacklisted as isArtistBlacklisted } from '$lib/stores/artistBlacklistStore';
  import { getUserItem, setUserItem } from '$lib/utils/userStorage';
  import type { OfflineCacheStatus } from '$lib/stores/offlineCacheState';
  import type { DisplayTrack } from '$lib/types';

  interface Props {
    onBack: () => void;
    onTrackPlay?: (track: DisplayTrack) => void;
    onTrackPlayNext?: (track: DisplayTrack) => void;
    onTrackPlayLater?: (track: DisplayTrack) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: DisplayTrack) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
    onTrackShowInfo?: (trackId: number) => void;
    onTrackDownload?: (track: DisplayTrack) => void;
    onTrackRemoveDownload?: (trackId: number) => void;
    onTrackReDownload?: (track: DisplayTrack) => void;
    getTrackOfflineCacheStatus?: (trackId: number) => { status: OfflineCacheStatus; progress: number };
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
  }

  let {
    onBack,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddToPlaylist,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onTrackShowInfo,
    onTrackDownload,
    onTrackRemoveDownload,
    onTrackReDownload,
    getTrackOfflineCacheStatus,
    activeTrackId = null,
    isPlaybackActive = false,
  }: Props = $props();

  interface FavTrack {
    id: number;
    title: string;
    duration: number;
    track_number: number;
    performer?: { id?: number; name: string };
    album?: {
      id: string;
      title: string;
      image: { small?: string; thumbnail?: string; large?: string };
    };
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    isrc?: string;
    streamable?: boolean;
  }

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
  }

  interface FavQCacheEntry {
    localDate: string;
    tracks: FavTrack[];
    cachedAtIso: string;
  }

  const FAVQ_CACHE_KEY = 'qbz-favq-cache-v1';
  const FAVQ_TARGET_SIZE = 50;

  let loading = $state(false);
  let showPlaylistModal = $state(false);
  let userPlaylists = $state<Playlist[]>([]);
  let playlistModalTrackIds = $state<number[]>([]);
  let error = $state<string | null>(null);
  let tracks = $state<FavTrack[]>([]);
  let emptyFavorites = $state(false);
  let autoRunDone = false;
  let searchQuery = $state('');

  // Multi-select
  let multiSelectMode = $state(false);
  let multiSelectedIds = $state(new Set<number>());

  function toggleMultiSelectMode() {
    multiSelectMode = !multiSelectMode;
    if (!multiSelectMode) multiSelectedIds = new Set();
  }

  function toggleMultiSelect(id: number) {
    const next = new Set(multiSelectedIds);
    if (next.has(id)) next.delete(id); else next.add(id);
    multiSelectedIds = next;
  }

  async function handleBulkPlayNext() {
    const selected = filteredTracks.filter(trk => multiSelectedIds.has(trk.id));
    await invoke('v2_add_tracks_to_queue_next', { tracks: buildQueueTracks(selected) });
    multiSelectMode = false; multiSelectedIds = new Set();
  }

  async function handleBulkPlayLater() {
    const selected = filteredTracks.filter(trk => multiSelectedIds.has(trk.id));
    await invoke('v2_add_tracks_to_queue', { tracks: buildQueueTracks(selected) });
    multiSelectMode = false; multiSelectedIds = new Set();
  }

  async function handleBulkAddToPlaylist() {
    const trackIds = filteredTracks.filter(trk => multiSelectedIds.has(trk.id)).map(trk => trk.id);
    if (trackIds.length === 0) return;
    try {
      userPlaylists = await invoke<Playlist[]>('v2_get_user_playlists');
      playlistModalTrackIds = trackIds;
      showPlaylistModal = true;
    } catch (err) {
      error = err instanceof Error ? err.message : $t('yourMixes.errors.playlistsLoadFailed');
    }
    multiSelectMode = false; multiSelectedIds = new Set();
  }

  function getLocalDateKey(date: Date = new Date()): string {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `${year}-${month}-${day}`;
  }

  function readCache(localDate: string): FavQCacheEntry | null {
    const raw = getUserItem(FAVQ_CACHE_KEY);
    if (!raw) return null;
    try {
      const parsed = JSON.parse(raw) as FavQCacheEntry;
      if (!parsed || parsed.localDate !== localDate || !parsed.tracks) return null;
      return parsed;
    } catch {
      return null;
    }
  }

  function writeCache(entry: FavQCacheEntry): void {
    setUserItem(FAVQ_CACHE_KEY, JSON.stringify(entry));
  }

  function qualityLabel(track: FavTrack): string {
    const bitDepth = track.maximum_bit_depth;
    const rate = track.maximum_sampling_rate;
    if (bitDepth && rate) {
      return `${bitDepth}bit/${rate}kHz`;
    }
    return '';
  }

  function shuffleArray<T>(arr: T[]): T[] {
    const shuffled = [...arr];
    for (let idx = shuffled.length - 1; idx > 0; idx--) {
      const swapIdx = Math.floor(Math.random() * (idx + 1));
      [shuffled[idx], shuffled[swapIdx]] = [shuffled[swapIdx], shuffled[idx]];
    }
    return shuffled;
  }

  async function generateFavQ(cacheMode: 'none' | 'read-write' = 'none'): Promise<void> {
    loading = true;
    error = null;
    emptyFavorites = false;

    try {
      const localDate = getLocalDateKey();
      if (cacheMode === 'read-write') {
        const cached = readCache(localDate);
        if (cached && cached.tracks.length > 0) {
          tracks = cached.tracks;
          return;
        }
      }

      const favoriteIds = await invoke<number[]>('v2_get_cached_favorite_tracks');
      if (!favoriteIds || favoriteIds.length === 0) {
        tracks = [];
        emptyFavorites = true;
        return;
      }

      const shuffledIds = shuffleArray(favoriteIds).slice(0, FAVQ_TARGET_SIZE);
      const batchTracks = await invoke<FavTrack[]>('v2_get_tracks_batch', {
        trackIds: shuffledIds
      });

      // Filter out unstreamable tracks
      tracks = batchTracks.filter(trk => trk.streamable !== false);

      if (cacheMode === 'read-write' && tracks.length > 0) {
        writeCache({
          localDate,
          tracks,
          cachedAtIso: new Date().toISOString()
        });
      }
    } catch (err) {
      error = err instanceof Error ? err.message : $t('yourMixes.errors.fetchFailed');
      tracks = [];
    } finally {
      loading = false;
    }
  }

  function getResultTrackIds(): number[] {
    return tracks
      .map((track) => Number(track.id))
      .filter((trackId) => Number.isFinite(trackId) && trackId > 0);
  }

  async function openSaveAsPlaylist(): Promise<void> {
    const trackIds = getResultTrackIds();
    if (trackIds.length === 0) {
      showToast($t('yourMixes.errors.emptyPlaylist'), 'info');
      return;
    }

    try {
      userPlaylists = await invoke<Playlist[]>('v2_get_user_playlists');
      playlistModalTrackIds = trackIds;
      showPlaylistModal = true;
    } catch (err) {
      error = err instanceof Error ? err.message : $t('yourMixes.errors.playlistsLoadFailed');
    }
  }

  function toDisplayTrack(track: FavTrack): DisplayTrack {
    return {
      id: track.id,
      title: track.title,
      artist: track.performer?.name || 'Unknown Artist',
      album: track.album?.title,
      albumArt: getQobuzImage(track.album?.image),
      albumId: track.album?.id,
      artistId: track.performer?.id,
      duration: formatDuration(track.duration || 0),
      durationSeconds: track.duration || 0,
      hires: track.hires,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate,
      isrc: track.isrc,
    };
  }

  function buildQueueTracks(queueItems: FavTrack[]) {
    return queueItems
      .filter(trk => {
        if (!trk.performer?.id) return true;
        return !isArtistBlacklisted(trk.performer.id);
      })
      .map(trk => ({
        id: trk.id,
        title: trk.title,
        artist: trk.performer?.name || 'Unknown Artist',
        album: trk.album?.title || 'FavQ',
        duration_secs: trk.duration || 0,
        artwork_url: getQobuzImage(trk.album?.image) || '',
        hires: trk.hires ?? false,
        bit_depth: trk.maximum_bit_depth ?? null,
        sample_rate: trk.maximum_sampling_rate ?? null,
        is_local: false,
        album_id: trk.album?.id || null,
        artist_id: trk.performer?.id ?? null,
      }));
  }

  async function handleTrackClick(track: FavTrack): Promise<void> {
    const queueTracks = buildQueueTracks(filteredTracks);
    const queueTrackIds = queueTracks.map(qt => qt.id);
    const queueIndex = queueTrackIds.indexOf(track.id);

    if (queueIndex < 0) return;

    try {
      await setPlaybackContext(
        'fav_q',
        'favq',
        'FavQ',
        'qobuz',
        queueTrackIds,
        queueIndex
      );
    } catch (err) {
      console.error('Failed to set playback context:', err);
    }

    try {
      await invoke('v2_set_queue', { tracks: queueTracks, startIndex: queueIndex });
    } catch (err) {
      console.error('Failed to set queue:', err);
    }

    onTrackPlay?.(toDisplayTrack(track));
  }

  async function handlePlayAll(): Promise<void> {
    if (filteredTracks.length === 0) return;
    await handleTrackClick(filteredTracks[0]);
  }

  async function handleShuffle(): Promise<void> {
    if (filteredTracks.length === 0) return;
    const randomIndex = Math.floor(Math.random() * filteredTracks.length);
    const track = filteredTracks[randomIndex];
    if (!track) return;
    await handleTrackClick(track);
  }

  const filteredTracks = $derived.by(() => {
    const items = tracks;
    const query = searchQuery.trim().toLowerCase();
    if (!query) return items;

    return items.filter((track) => {
      const title = (track.title ?? '').toLowerCase();
      const artist = (track.performer?.name ?? '').toLowerCase();
      const album = (track.album?.title ?? '').toLowerCase();
      return title.includes(query) || artist.includes(query) || album.includes(query);
    });
  });

  let showAlgoTooltip = $state(false);
  let algoTooltipTimer: ReturnType<typeof setTimeout> | null = null;

  function toggleAlgoTooltip(event: MouseEvent) {
    event.stopPropagation();
    if (showAlgoTooltip) {
      dismissAlgoTooltip();
      return;
    }
    showAlgoTooltip = true;
    algoTooltipTimer = setTimeout(dismissAlgoTooltip, 2000);
  }

  function dismissAlgoTooltip() {
    showAlgoTooltip = false;
    if (algoTooltipTimer) { clearTimeout(algoTooltipTimer); algoTooltipTimer = null; }
  }

  function handleGlobalClick() {
    if (showAlgoTooltip) dismissAlgoTooltip();
  }

  const totalDurationFormatted = $derived.by(() => {
    const totalSecs = filteredTracks.reduce((sum, track) => sum + (track.duration || 0), 0);
    const hours = Math.floor(totalSecs / 3600);
    const mins = Math.floor((totalSecs % 3600) / 60);
    if (hours > 0) return `${hours} hr ${mins} min`;
    return `${mins} min`;
  });

  onMount(() => {
    if (autoRunDone) return;
    autoRunDone = true;
    void generateFavQ('read-write');
    document.addEventListener('click', handleGlobalClick);
  });

  onDestroy(() => {
    dismissAlgoTooltip();
    document.removeEventListener('click', handleGlobalClick);
  });
</script>

<div class="favq-view">
  <div class="nav-row">
    <button class="back-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{$t('actions.back')}</span>
    </button>
  </div>

  <div class="playlist-header">
    <div class="artwork-container">
      <div class="artwork artwork-favq"></div>
    </div>

    <div class="metadata">
      <span class="playlist-label">{$t('home.yourMixes')}</span>
      <h1 class="playlist-title">
        {$t('favMixes.title')}
        <span class="info-wrapper">
          <button class="info-btn" onclick={toggleAlgoTooltip}>
            <Info size={16} />
          </button>
          {#if showAlgoTooltip}
            <span class="info-tooltip">{$t('favMixes.algorithmInfo')}</span>
          {/if}
        </span>
      </h1>
      <p class="playlist-description">{$t('favMixes.cardDesc')}</p>
      <div class="playlist-info">
        <span>{$t('yourMixes.result.count', { values: { count: filteredTracks.length } })}</span>
        {#if filteredTracks.length > 0}
          <span class="separator">•</span>
          <span>{totalDurationFormatted}</span>
        {/if}
      </div>

      <div class="actions">
        <button class="action-btn-circle primary" onclick={handlePlayAll} disabled={loading || filteredTracks.length === 0} title={$t('actions.playNow')}>
          <Play size={20} fill="currentColor" color="currentColor" />
        </button>
        <button class="action-btn-circle" onclick={handleShuffle} disabled={loading || filteredTracks.length === 0} title={$t('actions.shuffle')}>
          <Shuffle size={18} />
        </button>
        <button class="action-btn-circle" onclick={openSaveAsPlaylist} disabled={loading || tracks.length === 0} title={$t('yourMixes.actions.saveAsPlaylist')}>
          <ListPlus size={18} />
        </button>
        <button class="action-btn-circle" onclick={() => generateFavQ('none')} disabled={loading} title={$t('actions.refresh')}>
          <RefreshCw size={18} />
        </button>
        <button
          class="action-btn-circle"
          class:is-active={multiSelectMode}
          onclick={toggleMultiSelectMode}
          disabled={loading || filteredTracks.length === 0}
          title={multiSelectMode ? $t('actions.cancelSelection') : $t('actions.select')}
        >
          <CheckSquare size={18} />
        </button>
      </div>
    </div>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  {#if emptyFavorites && !loading}
    <div class="empty">{$t('favMixes.emptyFavorites')}</div>
  {:else}
    <div class="track-controls">
      <div class="search-container">
        <Search size={16} class="search-icon" />
        <input
          type="text"
          placeholder={$t('placeholders.searchInPlaylist')}
          bind:value={searchQuery}
          class="search-input"
        />
        {#if searchQuery}
          <button class="search-clear" onclick={() => searchQuery = ''}>
            <X size={14} />
          </button>
        {/if}
      </div>
    </div>

    {#if tracks.length === 0 && loading}
      <div class="empty">{$t('actions.loading')}</div>
    {:else if tracks.length > 0 && filteredTracks.length === 0}
      <div class="empty">{$t('yourMixes.result.empty')}</div>
    {:else if tracks.length > 0}
      <div class="track-list">
        <div class="track-list-header">
          <div class="col-number">#</div>
          <div class="col-artwork"></div>
          <div class="col-title">{$t('common.title')}</div>
          <div class="col-album">{$t('purchases.sort.album')}</div>
          <div class="col-duration">{$t('album.duration')}</div>
          <div class="col-quality">{$t('album.quality')}</div>
          <div class="col-spacer"></div>
        </div>

        {#each filteredTracks as track, index}
          {@const trackBlacklisted = track.performer?.id ? isArtistBlacklisted(track.performer.id) : false}
          {@const cacheStatus = getTrackOfflineCacheStatus?.(track.id) ?? { status: 'none' as const, progress: 0 }}
          {@const isTrackDownloaded = cacheStatus.status === 'ready'}
          {@const displayTrack = toDisplayTrack(track)}
          <TrackRow
            trackId={track.id}
            number={index + 1}
            title={track.title}
            artist={track.performer?.name}
            album={track.album?.title}
            duration={formatDuration(track.duration || 0)}
            quality={qualityLabel(track)}
            showArtwork={true}
            artworkUrl={getQobuzImage(track.album?.image)}
            isPlaying={isPlaybackActive && activeTrackId === track.id}
            isBlacklisted={trackBlacklisted}
            selectable={multiSelectMode}
            selected={multiSelectedIds.has(track.id)}
            onToggleSelect={() => toggleMultiSelect(track.id)}
            hideDownload={trackBlacklisted}
            hideFavorite={trackBlacklisted}
            downloadStatus={cacheStatus.status}
            downloadProgress={cacheStatus.progress}
            onPlay={!trackBlacklisted ? () => handleTrackClick(track) : undefined}
            onDownload={!trackBlacklisted && onTrackDownload ? () => onTrackDownload(displayTrack) : undefined}
            onRemoveDownload={isTrackDownloaded && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
            menuActions={trackBlacklisted ? {
              onGoToAlbum: track.album?.id && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.album!.id!) : undefined,
              onGoToArtist: track.performer?.id && onTrackGoToArtist ? () => onTrackGoToArtist(track.performer!.id!) : undefined,
              onShowInfo: onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined,
            } : {
              onPlayNow: () => handleTrackClick(track),
              onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(displayTrack) : undefined,
              onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(displayTrack) : undefined,
              onAddToPlaylist: onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined,
              onShareQobuz: onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
              onShareSonglink: onTrackShareSonglink ? () => onTrackShareSonglink(displayTrack) : undefined,
              onGoToAlbum: track.album?.id && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.album!.id!) : undefined,
              onGoToArtist: track.performer?.id && onTrackGoToArtist ? () => onTrackGoToArtist(track.performer!.id!) : undefined,
              onShowInfo: onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined,
              onDownload: onTrackDownload ? () => onTrackDownload(displayTrack) : undefined,
              isTrackDownloaded,
              onReDownload: isTrackDownloaded && onTrackReDownload ? () => onTrackReDownload(displayTrack) : undefined,
              onRemoveDownload: isTrackDownloaded && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined,
            }}
          />
        {/each}
        <BulkActionBar
          count={multiSelectedIds.size}
          onPlayNext={handleBulkPlayNext}
          onPlayLater={handleBulkPlayLater}
          onAddToPlaylist={handleBulkAddToPlaylist}
          onClearSelection={() => { multiSelectedIds = new Set(); }}
        />
      </div>
    {/if}
  {/if}
</div>

<PlaylistModal
  isOpen={showPlaylistModal}
  mode="addTrack"
  trackIds={playlistModalTrackIds}
  userPlaylists={userPlaylists}
  isLocalTracks={false}
  onClose={() => {
    showPlaylistModal = false;
  }}
  onSuccess={() => {
    showToast($t('toast.addedToPlaylist'), 'success');
    showPlaylistModal = false;
  }}
/>

<style>
  .favq-view {
    padding: 24px;
    padding-bottom: 100px;
    color: var(--text-primary);
    height: 100%;
    overflow-y: auto;
  }

  .nav-row {
    display: flex;
    align-items: center;
    margin-bottom: 24px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-primary);
  }

  .playlist-header {
    display: flex;
    gap: 32px;
    margin-bottom: 32px;
  }

  .artwork-container {
    flex-shrink: 0;
  }

  .artwork {
    width: 186px;
    height: 186px;
    position: relative;
    border-radius: 8px;
    overflow: hidden;
    background-color: var(--bg-tertiary);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .artwork-favq {
    position: relative;
    overflow: hidden;
  }

  .artwork-favq::before {
    content: '';
    position: absolute;
    inset: -40%;
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 200, 200, 0.45) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(80, 0, 0, 0.35) 58%, transparent 61%),
      radial-gradient(ellipse at 30% 20%, rgba(255, 180, 180, 0.25) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 60%, rgba(255, 50, 50, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 80%, rgba(200, 0, 0, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #e82020 0%, #c41818 30%, #a01010 60%, #800808 100%);
    will-change: transform;
    animation: silk-favq 28s ease-in-out infinite alternate;
  }

  @keyframes silk-favq {
    0%   { transform: translate(5%, 3%) rotate(0deg) scale(1); }
    25%  { transform: translate(-8%, 6%) rotate(6deg) scale(1.03); }
    50%  { transform: translate(3%, -5%) rotate(-4deg) scale(0.98); }
    75%  { transform: translate(-4%, 8%) rotate(8deg) scale(1.02); }
    100% { transform: translate(6%, -3%) rotate(-2deg) scale(1); }
  }

  .metadata {
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    min-width: 0;
  }

  .playlist-label {
    font-size: 12px;
    text-transform: uppercase;
    color: var(--text-muted);
    font-weight: 600;
    letter-spacing: 0.1em;
    margin-bottom: 8px;
  }

  .playlist-title {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 8px 0;
    line-height: 1.2;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .info-wrapper {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .info-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    transition: color 150ms ease;
  }

  .info-btn:hover {
    color: var(--text-primary);
  }

  .info-tooltip {
    position: absolute;
    top: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 400;
    line-height: 1.4;
    padding: 8px 12px;
    border-radius: 6px;
    white-space: normal;
    width: 260px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    z-index: 10;
    pointer-events: none;
    animation: tooltip-fade-in 150ms ease;
  }

  @keyframes tooltip-fade-in {
    from { opacity: 0; transform: translateX(-50%) translateY(-4px); }
    to { opacity: 1; transform: translateX(-50%) translateY(0); }
  }

  .playlist-description {
    font-size: 14px;
    color: var(--text-secondary);
    margin: 0 0 12px 0;
    line-height: 1.4;
  }

  .playlist-info {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-secondary);
    margin-bottom: 24px;
  }

  .separator {
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .error {
    margin: 0 0 10px;
    color: var(--error-color, #ef4444);
  }

  .track-controls {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 24px;
    margin-bottom: 16px;
  }

  .search-container {
    display: flex;
    align-items: center;
    gap: 8px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 8px 12px;
    flex: 1;
    max-width: 300px;
  }

  .search-container :global(.search-icon) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 14px;
    outline: none;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-clear {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .search-clear:hover {
    color: var(--text-primary);
  }

  .track-list {
    margin-top: 24px;
  }

  .track-list-header {
    width: 100%;
    height: 40px;
    padding: 0 16px;
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    font-size: 12px;
    text-transform: uppercase;
    color: #666666;
    font-weight: 400;
    box-sizing: border-box;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 8px;
  }

  .col-number {
    width: 48px;
    text-align: center;
  }

  .col-artwork {
    width: 36px;
    flex-shrink: 0;
  }

  .col-title {
    flex: 1;
    min-width: 0;
  }

  .col-album {
    flex: 1;
    min-width: 0;
  }

  .col-duration {
    width: 80px;
    text-align: center;
  }

  .col-quality {
    width: 80px;
    text-align: center;
  }

  .col-spacer {
    width: 28px;
  }

  .empty {
    border: 1px dashed var(--border-color);
    border-radius: 10px;
    padding: 18px;
    color: var(--text-muted);
    text-align: center;
  }

  @media (max-width: 760px) {
    .playlist-header {
      flex-direction: column;
      gap: 16px;
    }

    .artwork {
      width: 160px;
      height: 160px;
    }

    .playlist-title {
      font-size: 20px;
    }

    .track-list-header {
      display: none;
    }
  }
</style>
