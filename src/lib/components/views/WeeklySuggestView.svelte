<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { ListPlus, Play, RefreshCw, Search, Shuffle, X } from 'lucide-svelte';
  import PlaylistModal from '$lib/components/PlaylistModal.svelte';
  import TrackRow from '$lib/components/TrackRow.svelte';
  import { t } from '$lib/i18n';
  import { formatDuration, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import { getDynamicSuggest } from '$lib/services/dynamicSuggest';
  import { getHomeSeeds, getHomeSeedsML } from '$lib/services/recoService';
  import { showToast } from '$lib/stores/toastStore';
  import { getUserItem, setUserItem } from '$lib/utils/userStorage';
  import type {
    DynamicSuggestResponse,
    DynamicTrackToAnalyseInput,
    DynamicSuggestTrack
  } from '$lib/types/dynamicSuggest';

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
  let mixSource = $state<'cache' | 'live' | null>(null);
  let showPlaylistModal = $state(false);
  let userPlaylists = $state<Playlist[]>([]);
  let playlistModalTrackIds = $state<number[]>([]);
  let error = $state<string | null>(null);
  let result = $state<DynamicSuggestResponse | null>(null);
  let autoRunDone = false;
  let searchQuery = $state('');

  function getLocalDateKey(date: Date = new Date()): string {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `${year}-${month}-${day}`;
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
    if (cacheMode !== 'read-write') mixSource = null;

    try {
      const localDate = getLocalDateKey();
      if (cacheMode === 'read-write') {
        const cached = readDailyCache(localDate);
        if (cached) {
          result = cached.result;
          mixSource = 'cache';
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
      mixSource = 'live';

      if (cacheMode === 'read-write') {
        writeDailyCache({
          localDate,
          result: response,
          cachedAtIso: new Date().toISOString()
        });
      }
    } catch (err) {
      error = err instanceof Error ? err.message : $t('yourMixes.errors.fetchFailed');
      result = null;
      mixSource = null;
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

  async function playTrack(track: DynamicSuggestTrack): Promise<void> {
    try {
      await invoke('v2_play_track', { trackId: track.id });
    } catch (err) {
      console.error('Failed to play DailyQ track:', err);
    }
  }

  async function handlePlayAll(): Promise<void> {
    if (filteredTracks.length === 0) return;
    await playTrack(filteredTracks[0]);
  }

  async function handleShuffle(): Promise<void> {
    if (filteredTracks.length === 0) return;
    const randomIndex = Math.floor(Math.random() * filteredTracks.length);
    const track = filteredTracks[randomIndex];
    if (!track) return;
    await playTrack(track);
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

  onMount(() => {
    if (autoRunDone) return;
    autoRunDone = true;
    void generateDailyQ('read-write');
  });
</script>

<div class="dailyq-view">
  <div class="playlist-header">
    <div class="artwork-container">
      <div class="artwork artwork-weekly"></div>
    </div>

    <div class="metadata">
      <span class="playlist-label">{$t('favorites.playlists')}</span>
      <h1 class="playlist-title">{$t('weeklyMixes.title')}</h1>
      <p class="playlist-description">{$t('weeklyMixes.subtitle')}</p>
      <div class="playlist-info">
        {#if mixSource === 'cache'}
          <span>{$t('yourMixes.result.sourceCached')}</span>
          <span class="separator">•</span>
        {:else if mixSource === 'live'}
          <span>{$t('yourMixes.result.sourceLive')}</span>
          <span class="separator">•</span>
        {/if}
        <span>{$t('yourMixes.result.count', { values: { count: filteredTracks.length } })}</span>
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
        <button class="action-btn-circle" onclick={() => generateDailyQ('none')} disabled={loading} title={$t('actions.refresh')}>
          <RefreshCw size={18} />
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
          hideDownload={true}
          onPlay={() => playTrack(track)}
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
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 220, 255, 0.5) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(30, 0, 50, 0.4) 58%, transparent 61%),
      radial-gradient(ellipse at 40% 20%, rgba(255, 200, 255, 0.35) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 50%, rgba(200, 150, 255, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 70%, rgba(130, 80, 200, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #b060d0 0%, #8040b0 30%, #6030a0 60%, #402080 100%);
    background-size: 200% 200%;
    animation: silk-shift 10s ease-in-out infinite alternate;
  }

  @keyframes silk-shift {
    0%   { background-position: 0% 0%; }
    50%  { background-position: 100% 50%; }
    100% { background-position: 30% 100%; }
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
