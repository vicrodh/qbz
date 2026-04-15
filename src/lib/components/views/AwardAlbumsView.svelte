<script lang="ts">
  /**
   * AwardAlbumsView — dedicated paginated listing of every album that
   * has received a given award. Opened from AwardView's "See all".
   * Mirrors LabelReleasesView's structure — header + search +
   * virtualized grid with load-more.
   */
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { ChevronLeft, Search, X, LoaderCircle } from 'lucide-svelte';
  import VirtualizedFavoritesAlbumGrid from '../VirtualizedFavoritesAlbumGrid.svelte';
  import type { QobuzAlbum } from '$lib/types';

  interface FavoriteAlbum {
    id: string;
    title: string;
    artist: { id: number; name: string };
    genre?: { name: string };
    image: { small?: string; thumbnail?: string; large?: string };
    release_date_original?: string;
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
  }

  interface Props {
    awardId: string;
    awardName: string;
    onBack: () => void;
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    checkAlbumFullyDownloaded?: (albumId: string) => Promise<boolean>;
    downloadStateVersion?: number;
    onArtistClick?: (artistId: number) => void;
  }

  let {
    awardId,
    awardName,
    onBack,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    onAddAlbumToPlaylist,
    checkAlbumFullyDownloaded,
    downloadStateVersion,
  }: Props = $props();

  const PAGE_SIZE = 50;

  let albums = $state<FavoriteAlbum[]>([]);
  let total = $state<number>(0);
  let offset = $state(0);
  let hasMore = $state(true);
  let loading = $state(true);
  let loadingMore = $state(false);
  let error = $state<string | null>(null);
  let searchQuery = $state('');
  let albumDownloadStatuses = $state<Map<string, boolean>>(new Map());

  function qobuzToFavorite(a: QobuzAlbum): FavoriteAlbum {
    const bitDepth = a.maximum_bit_depth ?? 16;
    return {
      id: a.id,
      title: a.title,
      artist: { id: a.artist?.id ?? 0, name: a.artist?.name ?? 'Unknown Artist' },
      genre: a.genre,
      image: a.image as { small?: string; thumbnail?: string; large?: string },
      release_date_original: a.release_date_original,
      hires: bitDepth > 16,
      maximum_bit_depth: a.maximum_bit_depth,
      maximum_sampling_rate: a.maximum_sampling_rate,
    };
  }

  function filteredAlbums(): FavoriteAlbum[] {
    if (!searchQuery.trim()) return albums;
    const q = searchQuery.toLowerCase();
    return albums.filter(a => a.title.toLowerCase().includes(q) || a.artist.name.toLowerCase().includes(q));
  }

  async function loadDownloadStatuses(list: FavoriteAlbum[]) {
    if (!checkAlbumFullyDownloaded) return;
    for (const album of list) {
      if (albumDownloadStatuses.has(album.id)) continue;
      try {
        const downloaded = await checkAlbumFullyDownloaded(album.id);
        albumDownloadStatuses.set(album.id, downloaded);
      } catch {
        // ignore
      }
    }
    albumDownloadStatuses = new Map(albumDownloadStatuses);
  }

  function isAlbumDownloaded(id: string): boolean {
    void downloadStateVersion;
    return albumDownloadStatuses.get(id) ?? false;
  }

  async function fetchPage(reset: boolean) {
    if (reset) {
      offset = 0;
      albums = [];
      hasMore = true;
      loading = true;
    } else {
      loadingMore = true;
    }
    try {
      const result = await invoke<{ items: QobuzAlbum[]; total: number }>(
        'v2_get_award_albums',
        { awardId, limit: PAGE_SIZE, offset }
      );
      const mapped = (result.items ?? []).map(qobuzToFavorite);
      albums = reset ? mapped : [...albums, ...mapped];
      offset = albums.length;
      total = result.total;
      hasMore = mapped.length >= PAGE_SIZE && offset < result.total;
      loadDownloadStatuses(mapped).catch(() => {});
    } catch (err) {
      console.error('[AwardAlbumsView] failed:', err);
      error = String(err);
    } finally {
      loading = false;
      loadingMore = false;
    }
  }

  function handleLoadMore() {
    if (hasMore && !loadingMore && !loading && !searchQuery.trim()) fetchPage(false);
  }

  function getQualityLabel(item: { hires?: boolean; maximum_bit_depth?: number; maximum_sampling_rate?: number }): string {
    if (!item.hires) return $t('quality.cdQuality');
    const depth = item.maximum_bit_depth ?? 16;
    const rate = item.maximum_sampling_rate ?? 44.1;
    return `${depth}/${rate}kHz`;
  }

  function getAlbumYear(album: FavoriteAlbum): string | null {
    return album.release_date_original?.substring(0, 4) ?? null;
  }

  onMount(() => {
    fetchPage(true);
  });
</script>

<div class="award-albums-view">
  <div class="top-bar">
    <div class="top-bar-left">
      <button class="back-btn" onclick={onBack} title={$t('actions.back')}>
        <ChevronLeft size={20} />
      </button>
      <div class="title-block">
        <div class="page-kicker">{awardName}</div>
        <h1 class="page-title">{$t('award.section.releases')}</h1>
      </div>
    </div>
    <div class="top-bar-right">
      <div class="search-wrapper">
        <Search size={16} />
        <input
          type="text"
          class="search-input"
          placeholder={$t('discover.searchPlaceholder')}
          bind:value={searchQuery}
        />
        {#if searchQuery}
          <button class="clear-btn" onclick={() => (searchQuery = '')}>
            <X size={14} />
          </button>
        {/if}
      </div>
    </div>
  </div>

  <div class="content">
    {#if loading}
      <div class="loading">
        <LoaderCircle size={28} class="spinner" />
        <p>{$t('album.loading')}</p>
      </div>
    {:else if error}
      <div class="error">
        <p>{$t('favorites.failedLoadFavorites')}</p>
        <p class="error-detail">{error}</p>
        <button class="retry-btn" onclick={() => fetchPage(true)}>{$t('actions.retry')}</button>
      </div>
    {:else if filteredAlbums().length === 0}
      <div class="empty">
        <p>{$t('award.empty')}</p>
      </div>
    {:else}
      <VirtualizedFavoritesAlbumGrid
        groups={[{ key: '', id: 'all', albums: filteredAlbums() }]}
        showGroupHeaders={false}
        viewMode="grid"
        onAlbumClick={onAlbumClick}
        onAlbumPlay={onAlbumPlay}
        onAlbumPlayNext={onAlbumPlayNext}
        onAlbumPlayLater={onAlbumPlayLater}
        onAlbumShareQobuz={onAlbumShareQobuz}
        onAlbumShareSonglink={onAlbumShareSonglink}
        onAlbumDownload={onAlbumDownload}
        onOpenAlbumFolder={onOpenAlbumFolder}
        onReDownloadAlbum={onReDownloadAlbum}
        onAddAlbumToPlaylist={onAddAlbumToPlaylist}
        {downloadStateVersion}
        {isAlbumDownloaded}
        onLoadMore={!searchQuery.trim() ? handleLoadMore : undefined}
        {loadingMore}
        {getQualityLabel}
        {getAlbumYear}
      />
    {/if}
  </div>
</div>

<style>
  .award-albums-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .top-bar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
    padding: 20px 24px 16px;
    flex-shrink: 0;
  }

  .top-bar-left {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
    flex: 1;
  }

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }
  .back-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .title-block {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .page-kicker {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .page-title {
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .top-bar-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .search-wrapper {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-muted);
  }
  .search-input {
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 13px;
    width: 200px;
  }
  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
  }
  .clear-btn:hover { color: var(--text-primary); }

  .content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    padding: 0 24px 24px;
  }

  .loading,
  .error,
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px 24px;
    color: var(--text-secondary);
    text-align: center;
  }
  .error-detail { font-size: 12px; color: var(--text-muted); }
  .retry-btn {
    margin-top: 8px;
    padding: 8px 16px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }
  .retry-btn:hover { background: var(--bg-secondary); }
</style>
