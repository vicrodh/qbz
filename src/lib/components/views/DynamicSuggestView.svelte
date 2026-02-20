<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { ListPlus, Play, RefreshCw, Shuffle, Sparkles } from 'lucide-svelte';
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

  const DAILY_MIX_CACHE_KEY = 'qbz-dailyq-cache-v2';
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
      <div class="artwork">
        {#if result && result.tracks.items.length > 0}
          <img src={getQobuzImage(result.tracks.items[0].album?.image)} alt={$t('yourMixes.title')} />
        {:else}
          <div class="placeholder-art">
            <Sparkles size={34} />
          </div>
        {/if}
      </div>
    </div>

    <div class="metadata">
      <span class="playlist-label">{$t('favorites.playlists')}</span>
      <h1 class="playlist-title">{$t('yourMixes.title')}</h1>
      <p class="playlist-description">{$t('yourMixes.subtitle')}</p>
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
        <button class="action-btn-circle primary" onclick={() => generateDailyQ('none')} disabled={loading} title={$t('actions.refresh')}>
          <RefreshCw size={16} />
        </button>
        <button class="action-btn-circle" onclick={openSaveAsPlaylist} disabled={loading || !result || result.tracks.items.length === 0} title={$t('yourMixes.actions.saveAsPlaylist')}>
          <ListPlus size={16} />
        </button>
        <button class="action-btn-circle" onclick={handlePlayAll} disabled={loading || filteredTracks.length === 0} title={$t('actions.playNow')}>
          <Play size={16} />
        </button>
        <button class="action-btn-circle" onclick={handleShuffle} disabled={loading || filteredTracks.length === 0} title={$t('actions.shuffle')}>
          <Shuffle size={16} />
        </button>
      </div>
    </div>
  </div>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  <div class="track-controls">
    <div class="search-container">
      <input
        type="text"
        placeholder={$t('placeholders.searchInPlaylist')}
        bind:value={searchQuery}
        class="search-input"
      />
    </div>
  </div>

  {#if !result && loading}
    <div class="empty">{$t('actions.loading')}</div>
  {:else if result && filteredTracks.length === 0}
    <div class="empty">{$t('yourMixes.result.empty')}</div>
  {:else if result}
    <div class="track-list">
      <div class="track-list-header">
        <span class="col-number">#</span>
        <span class="col-title">{$t('common.title')}</span>
        <span class="col-album">{$t('purchases.sort.album')}</span>
        <span class="col-duration">{$t('album.duration')}</span>
        <span class="col-quality">{$t('album.quality')}</span>
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
    padding: 20px;
    color: var(--text-primary);
  }

  .playlist-header {
    display: grid;
    grid-template-columns: 200px 1fr;
    gap: 18px;
    margin-bottom: 16px;
    align-items: end;
  }

  .artwork-container {
    width: 200px;
    height: 200px;
  }

  .artwork {
    width: 100%;
    height: 100%;
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-secondary);
  }

  .artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .placeholder-art {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .playlist-label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
  }

  .playlist-title {
    margin: 4px 0;
    font-size: 2rem;
  }

  .playlist-description {
    margin: 0;
    color: var(--text-muted);
  }

  .playlist-info {
    margin-top: 8px;
    display: flex;
    gap: 10px;
    color: var(--text-secondary);
    font-size: 0.9rem;
  }

  .separator {
    opacity: 0.7;
  }

  .actions {
    margin-top: 12px;
    display: flex;
    gap: 8px;
  }

  .action-btn-circle {
    width: 34px;
    height: 34px;
    border-radius: 9999px;
    border: 1px solid var(--border-color);
    background: var(--surface-2);
    color: var(--text-primary);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
  }

  .action-btn-circle.primary {
    background: var(--accent-primary, #f5a524);
    border-color: var(--accent-primary, #f5a524);
    color: #000;
  }

  .action-btn-circle:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .error {
    margin: 0 0 10px;
    color: var(--error-color, #ef4444);
  }

  .track-controls {
    margin-bottom: 10px;
  }

  .search-input {
    width: 100%;
    max-width: 320px;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--text-primary);
    padding: 8px 10px;
    font: inherit;
  }

  .track-list {
    border-top: 1px solid var(--border-color);
  }

  .track-list-header {
    display: grid;
    grid-template-columns: 64px 1fr 1fr 92px 92px;
    gap: 12px;
    padding: 8px 16px;
    color: var(--text-muted);
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .col-number,
  .col-duration,
  .col-quality {
    text-align: center;
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
      grid-template-columns: 1fr;
      align-items: start;
    }

    .artwork-container {
      width: 140px;
      height: 140px;
    }

    .playlist-title {
      font-size: 1.6rem;
    }

    .track-list-header {
      display: none;
    }
  }
</style>
