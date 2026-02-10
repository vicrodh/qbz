<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { ChevronLeft, Search, LayoutGrid, List, X } from 'lucide-svelte';
  import VirtualizedFavoritesAlbumGrid from '../VirtualizedFavoritesAlbumGrid.svelte';

  interface DiscoverAlbum {
    id: string;
    title: string;
    version?: string;
    track_count?: number;
    duration?: number;
    image: { small?: string; thumbnail?: string; large?: string };
    artists: { id: number; name: string; roles?: string[] }[];
    label?: { id: number; name: string };
    genre?: { id: number; name: string };
    dates?: { download?: string; original?: string; stream?: string };
    audio_info?: { maximum_sampling_rate?: number; maximum_bit_depth?: number; maximum_channel_count?: number };
  }

  interface DiscoverAlbumsResponse {
    has_more: boolean;
    items: DiscoverAlbum[];
  }

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

  interface GenreInfo {
    id: number;
    name: string;
  }

  interface Props {
    endpointType: 'newReleases' | 'idealDiscography' | 'mostStreamed';
    titleKey: string;
    showRanking?: boolean;
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
    endpointType,
    titleKey,
    showRanking = false,
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
    onArtistClick,
  }: Props = $props();

  const PAGE_SIZE = 50;

  // State
  let albums = $state<FavoriteAlbum[]>([]);
  let hasMore = $state(true);
  let offset = $state(0);
  let isLoading = $state(false);
  let isLoadingMore = $state(false);
  let searchQuery = $state('');
  let viewMode = $state<'grid' | 'list'>('grid');
  let genres = $state<GenreInfo[]>([]);
  let selectedGenreId = $state<number | null>(null);
  let albumDownloadStatuses = $state<Map<string, boolean>>(new Map());

  // Search filter (client-side on loaded items)
  function getFilteredAlbums(): FavoriteAlbum[] {
    if (!searchQuery.trim()) return albums;
    const q = searchQuery.toLowerCase();
    return albums.filter(
      (album) =>
        album.title.toLowerCase().includes(q) ||
        album.artist.name.toLowerCase().includes(q)
    );
  }

  function isAlbumDownloaded(albumId: string): boolean {
    return albumDownloadStatuses.get(albumId) ?? false;
  }

  async function loadAlbumDownloadStatuses(albumList: FavoriteAlbum[]) {
    if (!checkAlbumFullyDownloaded) return;
    for (const album of albumList) {
      if (!albumDownloadStatuses.has(album.id)) {
        try {
          const downloaded = await checkAlbumFullyDownloaded(album.id);
          albumDownloadStatuses.set(album.id, downloaded);
          albumDownloadStatuses = new Map(albumDownloadStatuses);
        } catch {
          // Ignore errors
        }
      }
    }
  }

  function discoverToFavorite(item: DiscoverAlbum): FavoriteAlbum {
    const bitDepth = item.audio_info?.maximum_bit_depth ?? 16;
    return {
      id: item.id,
      title: item.title,
      artist: {
        id: item.artists?.[0]?.id ?? 0,
        name: item.artists?.[0]?.name ?? 'Unknown Artist',
      },
      genre: item.genre ? { name: item.genre.name } : undefined,
      image: {
        small: item.image?.small,
        thumbnail: item.image?.thumbnail,
        large: item.image?.large,
      },
      release_date_original: item.dates?.original,
      hires: bitDepth > 16,
      maximum_bit_depth: item.audio_info?.maximum_bit_depth,
      maximum_sampling_rate: item.audio_info?.maximum_sampling_rate,
    };
  }

  async function fetchAlbums(resetData: boolean = false) {
    if (resetData) {
      offset = 0;
      albums = [];
      hasMore = true;
      isLoading = true;
    } else {
      isLoadingMore = true;
    }

    try {
      const genreIds = selectedGenreId ? [selectedGenreId] : undefined;
      const response = await invoke<DiscoverAlbumsResponse>('get_discover_albums', {
        endpointType,
        genreIds,
        offset,
        limit: PAGE_SIZE,
      });

      const newAlbums = response.items.map(discoverToFavorite);

      if (resetData) {
        albums = newAlbums;
      } else {
        albums = [...albums, ...newAlbums];
      }

      hasMore = response.has_more;
      offset += newAlbums.length;

      // Load download statuses for new albums (non-blocking)
      loadAlbumDownloadStatuses(newAlbums).catch(() => {});
    } catch (err) {
      console.error('Failed to fetch discover albums:', err);
    } finally {
      isLoading = false;
      isLoadingMore = false;
    }
  }

  function handleLoadMore() {
    if (hasMore && !isLoadingMore && !isLoading) {
      fetchAlbums(false);
    }
  }

  function handleGenreChange(e: Event) {
    const select = e.target as HTMLSelectElement;
    selectedGenreId = select.value ? Number(select.value) : null;
    fetchAlbums(true);
  }

  function clearSearch() {
    searchQuery = '';
  }

  async function loadGenres() {
    try {
      genres = await invoke<GenreInfo[]>('get_genres', {});
    } catch (err) {
      console.error('Failed to load genres:', err);
    }
  }

  function getQualityLabel(item: { hires?: boolean; maximum_bit_depth?: number; maximum_sampling_rate?: number }): string {
    if (!item.hires) return 'CD';
    const depth = item.maximum_bit_depth ?? 16;
    const rate = item.maximum_sampling_rate ?? 44.1;
    return `${depth}/${rate}kHz`;
  }

  function getAlbumYear(album: FavoriteAlbum): string | null {
    if (!album.release_date_original) return null;
    return album.release_date_original.substring(0, 4);
  }

  onMount(() => {
    fetchAlbums(true);
    loadGenres();
  });
</script>

<div class="discover-browse">
  <!-- Top bar -->
  <div class="top-bar">
    <div class="top-bar-left">
      <button class="back-btn" onclick={onBack}>
        <ChevronLeft size={20} />
      </button>
      <h1 class="page-title">{$t(titleKey)}</h1>
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
          <button class="clear-btn" onclick={clearSearch}>
            <X size={14} />
          </button>
        {/if}
      </div>
      <select class="genre-select" onchange={handleGenreChange} value={selectedGenreId ?? ''}>
        <option value="">{$t('discover.allGenres')}</option>
        {#each genres as genre (genre.id)}
          <option value={genre.id}>{genre.name}</option>
        {/each}
      </select>
      <div class="view-toggle">
        <button
          class="toggle-btn"
          class:active={viewMode === 'grid'}
          onclick={() => viewMode = 'grid'}
        >
          <LayoutGrid size={18} />
        </button>
        <button
          class="toggle-btn"
          class:active={viewMode === 'list'}
          onclick={() => viewMode = 'list'}
        >
          <List size={18} />
        </button>
      </div>
    </div>
  </div>

  <!-- Content -->
  <div class="browse-content">
    {#if isLoading}
      <div class="loading-state">
        <div class="skeleton-grid">
          {#each { length: 12 } as _}
            <div class="skeleton-card">
              <div class="skeleton-art"></div>
              <div class="skeleton-text"></div>
              <div class="skeleton-text short"></div>
            </div>
          {/each}
        </div>
      </div>
    {:else if getFilteredAlbums().length === 0}
      <div class="empty-state">
        <p>{$t('discover.noResults')}</p>
      </div>
    {:else}
      <VirtualizedFavoritesAlbumGrid
        groups={[{ key: '', id: 'all', albums: getFilteredAlbums() }]}
        showGroupHeaders={false}
        {viewMode}
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
        {showRanking}
        onLoadMore={!searchQuery.trim() ? handleLoadMore : undefined}
        {isLoadingMore}
        {getQualityLabel}
        {getAlbumYear}
      />
    {/if}
  </div>
</div>

<style>
  .discover-browse {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .top-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 0 0 16px;
    flex-shrink: 0;
  }

  .top-bar-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border-radius: 8px;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .back-btn:hover {
    background: var(--bg-tertiary);
  }

  .page-title {
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
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
    background: var(--bg-secondary);
    border-radius: 8px;
    padding: 6px 12px;
    color: var(--text-muted);
    min-width: 180px;
  }

  .search-input {
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 13px;
    outline: none;
    width: 100%;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    border-radius: 4px;
  }

  .clear-btn:hover {
    color: var(--text-primary);
  }

  .genre-select {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: none;
    border-radius: 8px;
    padding: 6px 12px;
    font-size: 13px;
    cursor: pointer;
    outline: none;
    max-width: 160px;
  }

  .genre-select option {
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .view-toggle {
    display: flex;
    background: var(--bg-secondary);
    border-radius: 8px;
    overflow: hidden;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .toggle-btn.active {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .toggle-btn:hover:not(.active) {
    color: var(--text-secondary);
  }

  .browse-content {
    flex: 1;
    min-height: 0;
  }

  .loading-state {
    padding: 16px 0;
  }

  .skeleton-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 24px 14px;
  }

  .skeleton-card {
    width: 180px;
  }

  .skeleton-art {
    width: 180px;
    height: 180px;
    border-radius: 8px;
    background: var(--bg-secondary);
    animation: pulse 1.5s ease-in-out infinite;
    margin-bottom: 8px;
  }

  .skeleton-text {
    height: 14px;
    background: var(--bg-secondary);
    border-radius: 4px;
    animation: pulse 1.5s ease-in-out infinite;
    margin-bottom: 6px;
  }

  .skeleton-text.short {
    width: 60%;
  }

  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }

  .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 200px;
    color: var(--text-muted);
    font-size: 14px;
  }
</style>
