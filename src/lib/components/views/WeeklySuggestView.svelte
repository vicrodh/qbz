<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import { ArrowLeft, Info, ListPlus, Play, Search, Shuffle, X } from 'lucide-svelte';
  import PlaylistModal from '$lib/components/PlaylistModal.svelte';
  import TrackRow from '$lib/components/TrackRow.svelte';
  import { t } from '$lib/i18n';
  import { formatDuration, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import { getDynamicSuggest } from '$lib/services/dynamicSuggest';
  import { getHomeSeeds, getHomeSeedsML } from '$lib/services/recoService';
  import { showToast } from '$lib/stores/toastStore';
  import { setPlaybackContext } from '$lib/stores/playbackContextStore';
  import { isBlacklisted as isArtistBlacklisted } from '$lib/stores/artistBlacklistStore';
  import { getUserItem, setUserItem } from '$lib/utils/userStorage';
  import type { OfflineCacheStatus } from '$lib/stores/offlineCacheState';
  import type { DisplayTrack } from '$lib/types';
  import type {
    DynamicSuggestResponse,
    DynamicTrackToAnalyseInput,
    DynamicSuggestTrack
  } from '$lib/types/dynamicSuggest';

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

  interface RawSuggestTrack {
    performer?: { id?: number };
    composer?: { id?: number };
    album?: {
      label?: { id?: number };
      genre?: { id?: number };
    };
  }

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
  }

  interface DailyMixCacheEntry {
    localDate: string;
    result: DynamicSuggestResponse;
    cachedAtIso: string;
  }

  const DAILY_MIX_CACHE_KEY = 'qbz-weeklyq-cache-v1';
  const DAILY_MIX_TARGET_SIZE = 50;
  const DAILY_ANALYSE_TRACKS = 9;
  const DAILY_SEED_POOL = 120;

  let loading = $state(false);
  let showPlaylistModal = $state(false);
  let userPlaylists = $state<Playlist[]>([]);
  let playlistModalTrackIds = $state<number[]>([]);
  let error = $state<string | null>(null);
  let result = $state<DynamicSuggestResponse | null>(null);
  let autoRunDone = false;
  let searchQuery = $state('');

  function getMostRecentFriday(date: Date = new Date()): string {
    const d = new Date(date);
    const day = d.getDay(); // 0=Sun, 1=Mon, ..., 5=Fri, 6=Sat
    const daysSinceFriday = (day + 2) % 7;
    d.setDate(d.getDate() - daysSinceFriday);
    const year = d.getFullYear();
    const month = String(d.getMonth() + 1).padStart(2, '0');
    const dayStr = String(d.getDate()).padStart(2, '0');
    return `${year}-${month}-${dayStr}`;
  }

  function dedupeTrackIds(ids: number[]): number[] {
    const seen = new Set<number>();
    const out: number[] = [];
    for (const trackId of ids) {
      if (!Number.isFinite(trackId) || trackId <= 0) continue;
      if (seen.has(trackId)) continue;
      seen.add(trackId);
      out.push(trackId);
    }
    return out;
  }

  function pickSpread(ids: number[], count: number): number[] {
    if (ids.length <= count) return ids;
    const picked: number[] = [];
    const step = (ids.length - 1) / (count - 1);
    for (let index = 0; index < count; index += 1) {
      const position = Math.round(index * step);
      picked.push(ids[position]);
    }
    return dedupeTrackIds(picked).slice(0, count);
  }

  function readDailyCache(localDate: string): DailyMixCacheEntry | null {
    const raw = getUserItem(DAILY_MIX_CACHE_KEY);
    if (!raw) return null;
    try {
      const parsed = JSON.parse(raw) as DailyMixCacheEntry;
      if (!parsed || parsed.localDate !== localDate || !parsed.result) return null;
      return parsed;
    } catch {
      return null;
    }
  }

  function writeDailyCache(entry: DailyMixCacheEntry): void {
    setUserItem(DAILY_MIX_CACHE_KEY, JSON.stringify(entry));
  }

  function qualityLabel(track: DynamicSuggestTrack): string {
    const bitDepth = track.maximum_bit_depth;
    const rate = track.maximum_sampling_rate;
    if (bitDepth && rate) {
      return `${bitDepth}bit/${rate}kHz`;
    }
    return '';
  }

  async function buildSeeds(): Promise<{
    listenedTrackIds: number[];
    tracksToAnalyse: DynamicTrackToAnalyseInput[];
    limit: number;
  }> {
    const [seedsRecent, seedsMl] = await Promise.all([
      getHomeSeeds({ continueTracks: 90, favorites: 25 }),
      getHomeSeedsML({ continueTracks: 90, favorites: 25 })
    ]);

    const merged = dedupeTrackIds([
      ...seedsRecent.continueListeningTrackIds,
      ...seedsMl.continueListeningTrackIds,
      ...seedsRecent.favoriteTrackIds,
      ...seedsMl.favoriteTrackIds
    ]).slice(0, DAILY_SEED_POOL);

    const analysedSeedIds = pickSpread(merged, DAILY_ANALYSE_TRACKS);

    const analysed = await Promise.all(
      analysedSeedIds.map(async (trackId) => {
        const raw = await invoke<RawSuggestTrack>('v2_get_track', { trackId });
        const artistId = raw.performer?.id ?? raw.composer?.id ?? 0;
        return {
          trackId,
          artistId,
          genreId: raw.album?.genre?.id ?? 0,
          labelId: raw.album?.label?.id ?? 0
        };
      })
    );

    const tracksToAnalyse = analysed
      .filter((item) => item.artistId > 0)
      .slice(0, DAILY_ANALYSE_TRACKS);

    const limit = Math.max(1, DAILY_MIX_TARGET_SIZE - tracksToAnalyse.length);

    return {
      listenedTrackIds: merged,
      tracksToAnalyse,
      limit
    };
  }

  async function generateDailyQ(cacheMode: 'none' | 'read-write' = 'none'): Promise<void> {
    loading = true;
    error = null;

    try {
      const weekKey = getMostRecentFriday();
      if (cacheMode === 'read-write') {
        const cached = readDailyCache(weekKey);
        if (cached) {
          result = cached.result;
          return;
        }
      }

      const seedPayload = await buildSeeds();
      let response = await getDynamicSuggest(seedPayload);

      if (response.tracks.items.length === 0 && seedPayload.listenedTrackIds.length > 0) {
        response = await getDynamicSuggest({
          limit: DAILY_MIX_TARGET_SIZE,
          listenedTrackIds: seedPayload.listenedTrackIds,
          tracksToAnalyse: []
        });
      }

      result = response;

      if (cacheMode === 'read-write') {
        writeDailyCache({
          localDate: weekKey,
          result: response,
          cachedAtIso: new Date().toISOString()
        });
      }
    } catch (err) {
      error = err instanceof Error ? err.message : $t('yourMixes.errors.fetchFailed');
      result = null;
    } finally {
      loading = false;
    }
  }

  function getResultTrackIds(): number[] {
    if (!result) return [];
    return result.tracks.items
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

  function toDisplayTrack(track: DynamicSuggestTrack): DisplayTrack {
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
    };
  }

  function buildQueueTracks(tracks: DynamicSuggestTrack[]) {
    return tracks
      .filter(trk => {
        if (!trk.performer?.id) return true;
        return !isArtistBlacklisted(trk.performer.id);
      })
      .map(trk => ({
        id: trk.id,
        title: trk.title,
        artist: trk.performer?.name || 'Unknown Artist',
        album: trk.album?.title || 'WeeklyQ',
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

  async function handleTrackClick(track: DynamicSuggestTrack): Promise<void> {
    const queueTracks = buildQueueTracks(filteredTracks);
    const queueTrackIds = queueTracks.map(qt => qt.id);
    const queueIndex = queueTrackIds.indexOf(track.id);

    if (queueIndex < 0) return;

    await setPlaybackContext(
      'weekly_q',
      'weeklyq',
      'WeeklyQ',
      'qobuz',
      queueTrackIds,
      queueIndex
    );

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
    const items = result?.tracks.items ?? [];
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
    void generateDailyQ('read-write');
    document.addEventListener('click', handleGlobalClick);
  });

  onDestroy(() => {
    dismissAlgoTooltip();
    document.removeEventListener('click', handleGlobalClick);
  });
</script>

<div class="dailyq-view">
  <div class="nav-row">
    <button class="back-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{$t('actions.back')}</span>
    </button>
  </div>

  <div class="playlist-header">
    <div class="artwork-container">
      <div class="artwork artwork-weekly"></div>
    </div>

    <div class="metadata">
      <span class="playlist-label">{$t('home.yourMixes')}</span>
      <h1 class="playlist-title">
        {$t('weeklyMixes.title')}
        <span class="info-wrapper">
          <button class="info-btn" onclick={toggleAlgoTooltip}>
            <Info size={16} />
          </button>
          {#if showAlgoTooltip}
            <span class="info-tooltip">{$t('yourMixes.algorithmInfo')}</span>
          {/if}
        </span>
      </h1>
      <p class="playlist-description">{@html $t('weeklyMixes.cardDesc')}</p>
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
        <button class="action-btn-circle" onclick={openSaveAsPlaylist} disabled={loading || !result || result.tracks.items.length === 0} title={$t('yourMixes.actions.saveAsPlaylist')}>
          <ListPlus size={18} />
        </button>
      </div>
    </div>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}

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

  {#if !result && loading}
    <div class="empty">{$t('actions.loading')}</div>
  {:else if result && filteredTracks.length === 0}
    <div class="empty">{$t('yourMixes.result.empty')}</div>
  {:else if result}
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
    </div>
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
  .dailyq-view {
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

  .artwork-weekly {
    position: relative;
    overflow: hidden;
  }

  .artwork-weekly::before {
    content: '';
    position: absolute;
    inset: -40%;
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 220, 255, 0.5) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(30, 0, 50, 0.4) 58%, transparent 61%),
      radial-gradient(ellipse at 40% 20%, rgba(255, 200, 255, 0.35) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 50%, rgba(200, 150, 255, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 70%, rgba(130, 80, 200, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #b060d0 0%, #8040b0 30%, #6030a0 60%, #402080 100%);
    will-change: transform;
    animation: silk-weekly 34s ease-in-out infinite alternate;
  }

  @keyframes silk-weekly {
    0%   { transform: translate(-3%, 6%) rotate(2deg) scale(1.01); }
    20%  { transform: translate(7%, -4%) rotate(-5deg) scale(0.98); }
    45%  { transform: translate(-6%, -2%) rotate(7deg) scale(1.03); }
    70%  { transform: translate(4%, 7%) rotate(-3deg) scale(1); }
    100% { transform: translate(-5%, 3%) rotate(4deg) scale(0.99); }
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
