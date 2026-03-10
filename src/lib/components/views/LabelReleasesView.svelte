<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { ArrowLeft, Search, X, Disc3, Loader2, ArrowUpDown, Filter, ChevronDown, Users } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import type { QobuzAlbum, LabelDetail, LabelPageData } from '$lib/types';
  import type { OfflineCacheStatus } from '$lib/stores/offlineCacheState';

  interface Props {
    labelId: number;
    labelName?: string;
    onBack: () => void;
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    checkAlbumFullyDownloaded?: (albumId: string) => Promise<boolean>;
    downloadStateVersion?: number;
    onArtistClick?: (artistId: number) => void;
  }

  let {
    labelId,
    labelName = '',
    onBack,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAddAlbumToPlaylist,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    checkAlbumFullyDownloaded,
    downloadStateVersion,
    onArtistClick
  }: Props = $props();

  // State
  let label = $state<LabelDetail | null>(null);
  let labelPageImage = $state<string>('');
  let albums = $state<QobuzAlbum[]>([]);
  let loading = $state(false);
  let loadingMore = $state(false);
  let error = $state<string | null>(null);
  let searchQuery = $state('');
  let searchExpanded = $state(false);
  let totalAlbums = $state(0);
  let albumsFetched = $state(0);

  // Sort/filter state
  type SortOption = 'newest' | 'oldest' | 'title-az' | 'title-za' | 'artist-az';
  let sortBy = $state<SortOption>('newest');
  let filterHiRes = $state(false);
  let showSortMenu = $state(false);
  let groupByArtist = $state(false);

  // API search state
  let apiSearchResults = $state<QobuzAlbum[] | null>(null);
  let apiSearching = $state(false);
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;
  let searchVersion = 0;

  // Download status tracking
  let albumOfflineCacheStatuses = $state<Map<string, boolean>>(new Map());

  // Sorted + filtered albums
  let processedAlbums = $derived.by(() => {
    let source = apiSearchResults !== null ? apiSearchResults : albums;

    // Filter
    if (filterHiRes) {
      source = source.filter(album => album.hires_streamable);
    }

    // Local text filter (when search is open but < 2 chars for API search)
    if (searchQuery.trim().length > 0 && searchQuery.trim().length < 2 && apiSearchResults === null) {
      const q = searchQuery.trim().toLowerCase();
      source = source.filter(album =>
        album.title?.toLowerCase().includes(q) ||
        album.artist?.name?.toLowerCase().includes(q)
      );
    }

    // Sort
    const sorted = [...source];
    switch (sortBy) {
      case 'newest':
        sorted.sort((a, b) => (b.release_date_original || '').localeCompare(a.release_date_original || ''));
        break;
      case 'oldest':
        sorted.sort((a, b) => (a.release_date_original || '').localeCompare(b.release_date_original || ''));
        break;
      case 'title-az':
        sorted.sort((a, b) => (a.title || '').localeCompare(b.title || ''));
        break;
      case 'title-za':
        sorted.sort((a, b) => (b.title || '').localeCompare(a.title || ''));
        break;
      case 'artist-az':
        sorted.sort((a, b) => (a.artist?.name || '').localeCompare(b.artist?.name || ''));
        break;
    }
    return sorted;
  });

  // Grouped by artist
  let artistGroups = $derived.by(() => {
    if (!groupByArtist) return [];
    const groups = new Map<string, QobuzAlbum[]>();
    for (const album of processedAlbums) {
      const artistName = album.artist?.name || 'Unknown Artist';
      if (!groups.has(artistName)) {
        groups.set(artistName, []);
      }
      groups.get(artistName)?.push(album);
    }
    const keys = [...groups.keys()].sort((a, b) => a.localeCompare(b));
    return keys.map(key => ({
      key,
      albums: groups.get(key) ?? []
    }));
  });

  const sortLabels: Record<SortOption, string> = {
    'newest': 'labelReleases.sortNewest',
    'oldest': 'labelReleases.sortOldest',
    'title-az': 'labelReleases.sortTitleAZ',
    'title-za': 'labelReleases.sortTitleZA',
    'artist-az': 'labelReleases.sortArtistAZ'
  };

  async function loadLabel() {
    loading = true;
    error = null;

    try {
      // Fetch both label data (for albums) and label page (for image) in parallel
      const [result, pageResult] = await Promise.all([
        invoke<{
          id: number;
          name: string;
          description?: string;
          image?: { small?: string; thumbnail?: string; large?: string };
          albums?: { items: QobuzAlbum[]; total: number; offset: number; limit: number };
          albums_count?: number;
        }>('v2_get_label', { labelId, limit: 500, offset: 0 }),
        invoke<LabelPageData>('v2_get_label_page', { labelId }).catch(() => null)
      ]);

      label = {
        id: result.id,
        name: result.name,
        description: result.description,
        image: result.image,
        albums: result.albums?.items ?? [],
        totalAlbums: result.albums?.total ?? result.albums_count ?? 0,
        albumsFetched: result.albums?.items?.length ?? 0
      };

      // Extract richer image from label page data
      if (pageResult?.image) {
        if (typeof pageResult.image === 'string') {
          labelPageImage = pageResult.image;
        } else {
          const img = pageResult.image as Record<string, string>;
          labelPageImage = img.mega || img.extralarge || img.large || img.thumbnail || img.small || '';
        }
      }

      albums = label.albums;
      totalAlbums = label.totalAlbums;
      albumsFetched = label.albumsFetched;

      await loadAllAlbumOfflineCacheStatuses(albums);
    } catch (e) {
      console.error('Failed to load label:', e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function loadMore() {
    if (loadingMore || albumsFetched >= totalAlbums) return;

    loadingMore = true;

    try {
      const result = await invoke<{
        id: number;
        name: string;
        albums?: { items: QobuzAlbum[]; total: number; offset: number; limit: number };
      }>('v2_get_label', { labelId, limit: 500, offset: albumsFetched });

      const newAlbums = result.albums?.items ?? [];
      albums = [...albums, ...newAlbums];
      albumsFetched += newAlbums.length;

      await loadAllAlbumOfflineCacheStatuses(newAlbums);
    } catch (e) {
      console.error('Failed to load more albums:', e);
    } finally {
      loadingMore = false;
    }
  }

  async function loadAlbumOfflineCacheStatus(albumId: string) {
    if (!checkAlbumFullyDownloaded) return false;
    try {
      const isDownloaded = await checkAlbumFullyDownloaded(albumId);
      albumOfflineCacheStatuses.set(albumId, isDownloaded);
      return isDownloaded;
    } catch {
      return false;
    }
  }

  async function loadAllAlbumOfflineCacheStatuses(albumList: QobuzAlbum[]) {
    if (!checkAlbumFullyDownloaded || albumList.length === 0) return;
    await Promise.all(albumList.map(album => loadAlbumOfflineCacheStatus(album.id)));
  }

  function isAlbumDownloaded(albumId: string): boolean {
    void downloadStateVersion;
    return albumOfflineCacheStatuses.get(albumId) || false;
  }

  function getQualityLabel(album: QobuzAlbum): string {
    if (album.hires_streamable) {
      const bitDepth = album.maximum_bit_depth || 24;
      const sampleRate = album.maximum_sampling_rate || 96;
      return `${bitDepth}-bit/${sampleRate}kHz`;
    }
    return '';
  }

  function getGenreLabel(album: QobuzAlbum): string {
    return album.genre?.name || '';
  }

  function getLabelImage(): string {
    // Prefer richer image from label page endpoint
    if (labelPageImage) return labelPageImage;
    if (!label?.image) return '';
    return label.image.large || label.image.thumbnail || label.image.small || '';
  }

  function closeSearch() {
    searchQuery = '';
    searchExpanded = false;
    apiSearchResults = null;
    apiSearching = false;
    if (searchTimeout) clearTimeout(searchTimeout);
  }

  function handleSearchInput() {
    if (searchTimeout) clearTimeout(searchTimeout);
    const query = searchQuery.trim();

    if (!query) {
      apiSearchResults = null;
      apiSearching = false;
      return;
    }

    if (query.length < 2) {
      apiSearchResults = null;
      apiSearching = false;
      return;
    }

    apiSearching = true;
    searchTimeout = setTimeout(() => performLabelSearch(query), 300);
  }

  async function performLabelSearch(query: string) {
    searchVersion++;
    const thisVersion = searchVersion;
    try {
      const results = await invoke<{ items: QobuzAlbum[]; total: number; offset: number; limit: number }>(
        'v2_search_albums', { query, limit: 200, offset: 0, searchType: null }
      );
      if (thisVersion !== searchVersion) return;
      apiSearchResults = results.items.filter(
        album => album.label?.id === labelId
      );
    } catch (e) {
      console.error('Label search failed:', e);
      if (thisVersion === searchVersion) apiSearchResults = [];
    } finally {
      if (thisVersion === searchVersion) apiSearching = false;
    }
  }

  function handleScroll(e: Event) {
    if (apiSearchResults !== null) return;
    const target = e.target as HTMLElement;
    const scrollBottom = target.scrollHeight - target.scrollTop - target.clientHeight;

    if (scrollBottom < 400 && !loadingMore && albumsFetched < totalAlbums) {
      loadMore();
    }
  }

  function handleSortSelect(option: SortOption) {
    sortBy = option;
    showSortMenu = false;
  }

  function toggleHiResFilter() {
    filterHiRes = !filterHiRes;
  }

  function toggleGroupByArtist() {
    groupByArtist = !groupByArtist;
  }

  onMount(() => {
    loadLabel();
  });
</script>

<div class="label-view" onscroll={handleScroll}>
  <!-- Back button -->
  <div class="top-bar">
    <button class="back-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{$t('actions.back')}</span>
    </button>
  </div>

  <!-- Header -->
  <header class="label-header">
    <div class="label-image-wrapper">
      {#if getLabelImage()}
        <img src={getLabelImage()} alt={label?.name || labelName} class="label-image" loading="lazy" decoding="async" />
      {:else}
        <div class="label-image-placeholder">
          <Disc3 size={48} />
        </div>
      {/if}
    </div>
    <div class="label-header-info">
      <div class="label-subtitle">{$t('label.title')}</div>
      <h1 class="label-name">{label?.name || labelName || 'Label'}</h1>
      <p class="subtitle">
        {#if loading}
          {$t('actions.loading')}
        {:else if totalAlbums > 0}
          {$t('labelReleases.albumCount', { values: { count: totalAlbums } })}
        {/if}
      </p>
    </div>
  </header>

  <!-- Toolbar: Search + Sort + Filter -->
  <nav class="label-nav">
    <div class="nav-left">
      <span class="nav-title">{$t('label.releases')}</span>
      {#if apiSearchResults !== null}
        <span class="nav-count">{processedAlbums.length} {$t('labelReleases.results')}</span>
      {:else if filterHiRes || groupByArtist || sortBy !== 'newest'}
        <span class="nav-count">
          {processedAlbums.length}
          {#if filterHiRes} Hi-Res{/if}
          {#if albumsFetched < totalAlbums}
            — {$t('labelReleases.showingOf', { values: { shown: albumsFetched, total: totalAlbums } })}
          {/if}
        </span>
      {:else if albumsFetched > 0 && albumsFetched < totalAlbums}
        <span class="nav-count">{$t('labelReleases.showingOf', { values: { shown: albumsFetched, total: totalAlbums } })}</span>
      {/if}
    </div>
    <div class="nav-right">
      {#if searchExpanded}
        <div class="search-bar">
          <Search size={15} class="search-icon-inline" />
          <!-- svelte-ignore a11y_autofocus -->
          <input
            type="text"
            class="search-input-inline"
            placeholder={$t('labelReleases.searchPlaceholder')}
            bind:value={searchQuery}
            oninput={handleSearchInput}
            autofocus
          />
          <button class="search-clear-btn" onclick={closeSearch} title={$t('actions.close')}>
            <X size={15} />
          </button>
        </div>
      {:else}
        <button class="toolbar-btn" onclick={() => searchExpanded = true} title={$t('labelReleases.searchAlbums')}>
          <Search size={16} />
        </button>
      {/if}

      <button
        class="toolbar-btn"
        class:active={groupByArtist}
        onclick={toggleGroupByArtist}
        title={$t('labelReleases.groupByArtist')}
      >
        <Users size={16} />
        <span class="toolbar-label">{$t('labelReleases.groupByArtist')}</span>
      </button>

      <button
        class="toolbar-btn"
        class:active={filterHiRes}
        onclick={toggleHiResFilter}
        title={$t('labelReleases.filterHiRes')}
      >
        <Filter size={16} />
        <span class="toolbar-label">Hi-Res</span>
      </button>

      <div class="sort-dropdown">
        <button class="toolbar-btn" onclick={() => showSortMenu = !showSortMenu} title={$t('labelReleases.sort')}>
          <ArrowUpDown size={16} />
          <span class="toolbar-label">{$t(sortLabels[sortBy])}</span>
          <ChevronDown size={12} />
        </button>
        {#if showSortMenu}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="sort-menu" onmouseleave={() => showSortMenu = false}>
            {#each Object.entries(sortLabels) as [key, labelKey]}
              <button
                class="sort-option"
                class:active={sortBy === key}
                onclick={() => handleSortSelect(key as SortOption)}
              >
                {$t(labelKey)}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </nav>

  <!-- Content -->
  <div class="content">
    {#if loading}
      <div class="status-message">
        <div class="spinner"></div>
        <p>{$t('actions.loading')}</p>
      </div>
    {:else if error}
      <div class="status-message">
        <p>{$t('errors.generic')}</p>
        <p class="error-detail">{error}</p>
        <button class="retry-btn" onclick={loadLabel}>{$t('actions.retry')}</button>
      </div>
    {:else if albums.length === 0}
      <div class="status-message">
        <Disc3 size={48} />
        <p>{$t('labelReleases.noAlbums')}</p>
      </div>
    {:else if apiSearching}
      <div class="status-message">
        <div class="spinner"></div>
        <p>{$t('actions.loading')}</p>
      </div>
    {:else if processedAlbums.length === 0}
      <div class="status-message">
        <Search size={48} />
        <p>{$t('labelReleases.noResults')}</p>
      </div>
    {:else if groupByArtist && artistGroups.length > 0}
      {#each artistGroups as group (group.key)}
        <div class="artist-group">
          <h2 class="artist-group-header">{group.key} <span class="artist-group-count">({group.albums.length})</span></h2>
          <div class="album-grid">
            {#each group.albums as album (album.id)}
              <AlbumCard
                albumId={album.id}
                artwork={album.image?.small || album.image?.thumbnail || ''}
                title={album.title}
                artist={album.artist?.name || 'Unknown Artist'}
                artistId={album.artist?.id}
                onArtistClick={onArtistClick}
                genre={getGenreLabel(album)}
                releaseDate={album.release_date_original}
                quality={getQualityLabel(album)}
                size="large"
                onclick={() => onAlbumClick?.(album.id)}
                onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
                onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
                onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
                onAddAlbumToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist(album.id) : undefined}
                onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
                onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
                onDownload={onAlbumDownload ? () => onAlbumDownload(album.id) : undefined}
                isAlbumFullyDownloaded={isAlbumDownloaded(album.id)}
                onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(album.id) : undefined}
                onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(album.id) : undefined}
                {downloadStateVersion}
              />
            {/each}
          </div>
        </div>
      {/each}
    {:else}
      <div class="album-grid">
        {#each processedAlbums as album (album.id)}
          <AlbumCard
            albumId={album.id}
            artwork={album.image?.small || album.image?.thumbnail || ''}
            title={album.title}
            artist={album.artist?.name || 'Unknown Artist'}
            artistId={album.artist?.id}
            onArtistClick={onArtistClick}
            genre={getGenreLabel(album)}
            releaseDate={album.release_date_original}
            quality={getQualityLabel(album)}
            size="large"
            onclick={() => onAlbumClick?.(album.id)}
            onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
            onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
            onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
            onAddAlbumToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist(album.id) : undefined}
            onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
            onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
            onDownload={onAlbumDownload ? () => onAlbumDownload(album.id) : undefined}
            isAlbumFullyDownloaded={isAlbumDownloaded(album.id)}
            onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(album.id) : undefined}
            onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(album.id) : undefined}
            {downloadStateVersion}
          />
        {/each}
      </div>
    {/if}

    {#if loadingMore}
      <div class="loading-more">
        <Loader2 size={20} class="spinner-icon" />
        <span>{$t('labelReleases.loadingMore')}</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .label-view {
    padding: 8px 8px 100px 18px;
    overflow-y: auto;
    height: 100%;
  }

  .label-view::-webkit-scrollbar { width: 6px; }
  .label-view::-webkit-scrollbar-track { background: transparent; }
  .label-view::-webkit-scrollbar-thumb { background: var(--bg-tertiary); border-radius: 3px; }
  .label-view::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }

  /* Header — matches LabelView layout */
  .label-header {
    display: flex;
    gap: 24px;
    margin-bottom: 40px;
    align-items: flex-start;
  }

  .top-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    margin-top: 24px;
    margin-bottom: 24px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-secondary);
  }

  .label-image-wrapper {
    width: 180px;
    height: 180px;
    border-radius: 50%;
    overflow: hidden;
    flex-shrink: 0;
    background: var(--bg-tertiary);
  }

  .label-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .label-image-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%);
    color: white;
  }

  .label-header-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-self: center;
  }

  .label-subtitle {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.1em;
    margin-bottom: 4px;
  }

  .label-name {
    font-size: 28px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 4px 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .subtitle {
    font-size: 13px;
    color: var(--text-muted);
    margin: 0;
  }

  /* Toolbar */
  .label-nav {
    position: sticky;
    top: -24px;
    z-index: 4;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 10px 24px;
    margin: 0 -8px 16px -24px;
    width: calc(100% + 32px);
    height: 50px;
    box-sizing: border-box;
    background: var(--bg-primary);
    border-bottom: 1px solid var(--alpha-6);
    box-shadow: 0 4px 8px -4px rgba(0, 0, 0, 0.5);
  }

  .nav-left {
    display: flex;
    align-items: center;
    gap: 12px;
    flex: 1;
    min-width: 0;
  }

  .nav-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    flex-shrink: 0;
  }

  .nav-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .nav-right {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  /* Search bar (replaces nav-left content when expanded) */
  .search-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    flex: 1;
    min-width: 0;
    max-width: 400px;
  }

  :global(.search-icon-inline) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input-inline {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 13px;
    min-width: 0;
  }

  .search-input-inline::placeholder {
    color: var(--text-muted);
  }

  .search-clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
    flex-shrink: 0;
  }

  .search-clear-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  /* Toolbar buttons */
  .toolbar-btn {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 30px;
    padding: 0 8px;
    border: 1px solid transparent;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    font-size: 12px;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
    white-space: nowrap;
  }

  .toolbar-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .toolbar-btn.active {
    color: var(--accent-primary);
    background: color-mix(in srgb, var(--accent-primary) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent-primary) 30%, transparent);
  }

  .toolbar-label {
    display: none;
  }

  @media (min-width: 600px) {
    .toolbar-label {
      display: inline;
    }
  }

  /* Sort dropdown */
  .sort-dropdown {
    position: relative;
  }

  .sort-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 4px;
    min-width: 160px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10;
  }

  .sort-option {
    display: block;
    width: 100%;
    padding: 7px 12px;
    border: none;
    background: none;
    color: var(--text-secondary);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    border-radius: 5px;
    transition: color 100ms ease, background-color 100ms ease, border-color 100ms ease, opacity 100ms ease;
  }

  .sort-option:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .sort-option.active {
    color: var(--accent-primary);
    font-weight: 600;
  }

  /* Content */
  .content {
    min-height: 200px;
  }

  .status-message {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px 24px;
    color: var(--text-muted);
    text-align: center;
  }

  .status-message p {
    margin: 16px 0 0 0;
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .error-detail {
    font-size: 12px;
    margin-top: 8px;
  }

  .retry-btn {
    margin-top: 16px;
    padding: 8px 24px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    cursor: pointer;
  }

  .retry-btn:hover {
    opacity: 0.9;
  }

  .album-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 24px 22px;
  }

  .loading-more {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
    color: var(--text-muted);
    font-size: 13px;
  }

  :global(.spinner-icon) {
    animation: spin 1s linear infinite;
  }

  /* Artist groups */
  .artist-group {
    margin-bottom: 32px;
  }

  .artist-group-header {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 12px 0;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .artist-group-count {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-muted);
  }
</style>
