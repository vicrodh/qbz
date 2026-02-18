<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { Search, X, Download, Music, Disc3, ShoppingBag, ChevronDown } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import { getPurchases, searchPurchases } from '$lib/services/purchases';
  import { formatDuration, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import type { PurchasedAlbum, PurchasedTrack, PurchaseResponse } from '$lib/types/purchases';

  type PurchasesTab = 'albums' | 'tracks';
  type FilterOption = 'all' | 'hires';
  type SortOption = 'date' | 'artist' | 'album' | 'quality';

  interface Props {
    onAlbumClick?: (albumId: string) => void;
    onArtistClick?: (artistId: number) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
  }

  let {
    onAlbumClick,
    onArtistClick,
    activeTrackId = null,
    isPlaybackActive = false,
  }: Props = $props();

  let activeTab = $state<PurchasesTab>('albums');
  let searchQuery = $state('');
  let searchExpanded = $state(false);
  let filterOption = $state<FilterOption>('all');
  let sortOption = $state<SortOption>('date');

  // Sort/filter dropdown state
  let showSortMenu = $state(false);
  let showFilterMenu = $state(false);

  let albums = $state<PurchasedAlbum[]>([]);
  let tracks = $state<PurchasedTrack[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let searchTimeout: ReturnType<typeof setTimeout> | null = null;

  const FILTER_KEYS: FilterOption[] = ['all', 'hires'];
  const SORT_KEYS: SortOption[] = ['date', 'artist', 'album', 'quality'];

  function getFilterLabel(key: FilterOption): string {
    return get(t)(`purchases.filter.${key}`);
  }

  function getSortLabel(key: SortOption): string {
    return get(t)(`purchases.sort.${key}`);
  }

  function formatPurchaseDate(iso?: string): string {
    if (!iso) return '';
    try {
      return new Date(iso).toLocaleDateString(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
      });
    } catch {
      return '';
    }
  }

  function formatQualityLabel(bitDepth?: number, samplingRate?: number): string {
    if (!bitDepth || !samplingRate) return '';
    return `${bitDepth}/${samplingRate} kHz`;
  }

  function sortAlbums(list: PurchasedAlbum[], sort: SortOption): PurchasedAlbum[] {
    const sorted = [...list];
    switch (sort) {
      case 'date':
        return sorted.sort((a, b) =>
          new Date(b.purchased_at || '').getTime() - new Date(a.purchased_at || '').getTime()
        );
      case 'artist':
        return sorted.sort((a, b) => a.artist.name.localeCompare(b.artist.name));
      case 'album':
        return sorted.sort((a, b) => a.title.localeCompare(b.title));
      case 'quality':
        return sorted.sort((a, b) =>
          (b.maximum_sampling_rate || 0) - (a.maximum_sampling_rate || 0) ||
          (b.maximum_bit_depth || 0) - (a.maximum_bit_depth || 0)
        );
      default:
        return sorted;
    }
  }

  function sortTracks(list: PurchasedTrack[], sort: SortOption): PurchasedTrack[] {
    const sorted = [...list];
    switch (sort) {
      case 'date':
        return sorted.sort((a, b) =>
          new Date(b.purchased_at || '').getTime() - new Date(a.purchased_at || '').getTime()
        );
      case 'artist':
        return sorted.sort((a, b) => a.performer.name.localeCompare(b.performer.name));
      case 'album':
        return sorted.sort((a, b) => (a.album?.title || '').localeCompare(b.album?.title || ''));
      case 'quality':
        return sorted.sort((a, b) =>
          (b.maximum_sampling_rate || 0) - (a.maximum_sampling_rate || 0) ||
          (b.maximum_bit_depth || 0) - (a.maximum_bit_depth || 0)
        );
      default:
        return sorted;
    }
  }

  function filterAlbums(list: PurchasedAlbum[], filter: FilterOption): PurchasedAlbum[] {
    if (filter === 'hires') return list.filter((a) => a.hires);
    return list;
  }

  function filterTracks(list: PurchasedTrack[], filter: FilterOption): PurchasedTrack[] {
    if (filter === 'hires') return list.filter((track) => track.hires);
    return list;
  }

  const displayedAlbums = $derived(
    sortAlbums(filterAlbums(albums, filterOption), sortOption)
  );
  const displayedTracks = $derived(
    sortTracks(filterTracks(tracks, filterOption), sortOption)
  );

  async function loadPurchases() {
    loading = true;
    error = null;
    try {
      const response: PurchaseResponse = await getPurchases();
      albums = response.albums.items;
      tracks = response.tracks.items;
    } catch (err) {
      error = String(err);
    } finally {
      loading = false;
    }
  }

  function handleSearchInput() {
    if (searchTimeout) clearTimeout(searchTimeout);
    searchTimeout = setTimeout(async () => {
      if (!searchQuery.trim()) {
        await loadPurchases();
        return;
      }
      loading = true;
      try {
        const response = await searchPurchases(searchQuery.trim());
        albums = response.albums.items;
        tracks = response.tracks.items;
      } catch (err) {
        error = String(err);
      } finally {
        loading = false;
      }
    }, 300);
  }

  function clearSearch() {
    searchQuery = '';
    searchExpanded = false;
    loadPurchases();
  }

  onMount(() => {
    loadPurchases();
  });
</script>

<div class="purchases-view">
  <!-- Header (Favorites-style: icon + title) -->
  <div class="header">
    <div class="header-icon">
      <ShoppingBag size={32} color="var(--accent-primary)" />
    </div>
    <div class="header-content">
      <h1>{$t('purchases.title')}</h1>
    </div>
  </div>

  <!-- Navigation Bar (Favorites-style: sticky tabs + search toggle) -->
  <div class="purchases-nav">
    <div class="nav-left">
      <button
        class="nav-link"
        class:active={activeTab === 'albums'}
        onclick={() => (activeTab = 'albums')}
      >
        <Disc3 size={16} />
        <span>{$t('purchases.tabs.albums')}</span>
        <span class="nav-count">{displayedAlbums.length}</span>
      </button>
      <button
        class="nav-link"
        class:active={activeTab === 'tracks'}
        onclick={() => (activeTab = 'tracks')}
      >
        <Music size={16} />
        <span>{$t('purchases.tabs.tracks')}</span>
        <span class="nav-count">{displayedTracks.length}</span>
      </button>
    </div>
    <div class="nav-right">
      {#if !searchExpanded}
        <button class="search-icon-btn" onclick={() => searchExpanded = true} title={$t('nav.search')}>
          <Search size={16} />
        </button>
      {:else}
        <div class="search-expanded">
          <Search size={16} class="search-icon-inline" />
          <input
            type="text"
            placeholder={$t('purchases.search')}
            bind:value={searchQuery}
            oninput={handleSearchInput}
            class="search-input-inline"
          />
          {#if searchQuery}
            <button class="search-clear-btn" onclick={clearSearch} title={$t('actions.clear')}>
              <X size={14} />
            </button>
          {:else}
            <button class="search-clear-btn" onclick={() => searchExpanded = false} title={$t('actions.close')}>
              <X size={14} />
            </button>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <!-- Toolbar (Favorites-style: filter/sort controls) -->
  <div class="toolbar">
    <div class="toolbar-controls">
      <!-- Filter dropdown -->
      <div class="dropdown-container">
        <button class="control-btn" onclick={() => { showFilterMenu = !showFilterMenu; showSortMenu = false; }}>
          <span>{getFilterLabel(filterOption)}</span>
          <ChevronDown size={14} />
        </button>
        {#if showFilterMenu}
          <div class="dropdown-backdrop" onclick={() => showFilterMenu = false} role="presentation"></div>
          <div class="dropdown-menu">
            {#each FILTER_KEYS as key (key)}
              <button
                class="dropdown-item"
                class:selected={filterOption === key}
                onclick={() => { filterOption = key; showFilterMenu = false; }}
              >
                {getFilterLabel(key)}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <!-- Sort dropdown -->
      <div class="dropdown-container">
        <button class="control-btn" onclick={() => { showSortMenu = !showSortMenu; showFilterMenu = false; }}>
          <span>{getSortLabel(sortOption)}</span>
          <ChevronDown size={14} />
        </button>
        {#if showSortMenu}
          <div class="dropdown-backdrop" onclick={() => showSortMenu = false} role="presentation"></div>
          <div class="dropdown-menu">
            {#each SORT_KEYS as key (key)}
              <button
                class="dropdown-item"
                class:selected={sortOption === key}
                onclick={() => { sortOption = key; showSortMenu = false; }}
              >
                {getSortLabel(key)}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>

  <!-- Content -->
  <div class="content">
    {#if loading}
      <div class="loading">
        <div class="spinner"></div>
      </div>
    {:else if error}
      <div class="empty">
        <p>{error}</p>
      </div>
    {:else if activeTab === 'albums'}
      {#if displayedAlbums.length === 0}
        <div class="empty">
          <ShoppingBag size={48} />
          <p>{$t('purchases.empty')}</p>
        </div>
      {:else}
        <div class="albums-grid">
          {#each displayedAlbums as album (album.id)}
            <div class="album-card-wrapper" class:unavailable={!album.downloadable}>
              <AlbumCard
                albumId={album.id}
                artwork={getQobuzImage(album.image)}
                title={album.title}
                artist={album.artist.name}
                quality={formatQualityLabel(album.maximum_bit_depth, album.maximum_sampling_rate)}
                releaseDate={formatPurchaseDate(album.purchased_at)}
                onclick={() => album.downloadable && onAlbumClick?.(album.id)}
                showFavorite={false}
                showGenre={false}
              />
              {#if !album.downloadable}
                <div class="unavailable-overlay">
                  <span>{$t('purchases.unavailable')}</span>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {:else}
      {#if displayedTracks.length === 0}
        <div class="empty">
          <Music size={48} />
          <p>{$t('purchases.emptyTracks')}</p>
        </div>
      {:else}
        <div class="tracks-list">
          {#each displayedTracks as track (track.id)}
            <div class="track-row" class:active={activeTrackId === track.id}>
              <div class="track-artwork">
                {#if track.album?.image}
                  <img
                    src={getQobuzImage(track.album.image)}
                    alt={track.album.title}
                    class="track-thumb"
                  />
                {:else}
                  <div class="track-thumb-placeholder">
                    <Music size={16} />
                  </div>
                {/if}
              </div>
              <div class="track-info">
                <span class="track-title">{track.title}</span>
                <span class="track-meta">
                  <button
                    class="artist-link"
                    onclick={() => onArtistClick?.(track.performer.id)}
                  >
                    {track.performer.name}
                  </button>
                  {#if track.album}
                    <span class="separator">·</span>
                    <span class="album-name">{track.album.title}</span>
                  {/if}
                </span>
              </div>
              <div class="track-quality">
                {#if track.hires}
                  <QualityBadge bitDepth={track.maximum_bit_depth} samplingRate={track.maximum_sampling_rate} />
                {/if}
              </div>
              <div class="track-duration">
                {formatDuration(track.duration)}
              </div>
              <div class="track-date">
                {formatPurchaseDate(track.purchased_at)}
              </div>
              <button class="download-btn" title={$t('purchases.downloadTrack')}>
                <Download size={14} />
              </button>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .purchases-view {
    padding: 24px;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  .purchases-view::-webkit-scrollbar {
    width: 6px;
  }

  .purchases-view::-webkit-scrollbar-track {
    background: transparent;
  }

  .purchases-view::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .purchases-view::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  /* ── Header (Favorites style) ── */
  .header {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 16px;
  }

  .header-icon {
    width: 94px;
    height: 94px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--accent-primary) 0%, #6b8aff 100%);
    border-radius: 16px;
    flex-shrink: 0;
  }

  .header-content {
    flex: 1;
  }

  .header-content h1 {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
  }

  /* ── Sticky Navigation Bar (Favorites style) ── */
  .purchases-nav {
    position: sticky;
    top: -24px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 10px 24px;
    margin: 0 -8px 12px -18px;
    width: calc(100% + 26px);
    background: var(--bg-primary);
    border-bottom: 1px solid var(--alpha-6);
    box-shadow: 0 4px 8px -4px rgba(0, 0, 0, 0.5);
    z-index: 10;
  }

  .nav-left {
    display: flex;
    align-items: center;
    gap: 20px;
  }

  .nav-link {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: color 150ms ease, border-color 150ms ease;
  }

  .nav-link:hover {
    color: var(--text-secondary);
  }

  .nav-link.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-primary);
  }

  .nav-count {
    font-size: 11px;
    color: var(--text-muted);
    opacity: 0.7;
  }

  .nav-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .search-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    transition: all 150ms ease;
  }

  .search-icon-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .search-expanded {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    min-width: 240px;
  }

  .search-input-inline {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 13px;
  }

  .search-input-inline::placeholder {
    color: var(--text-muted);
  }

  .search-clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .search-clear-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  /* ── Toolbar (Favorites style) ── */
  .toolbar {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 24px;
  }

  .toolbar-controls {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .dropdown-container {
    position: relative;
  }

  .control-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 12px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .control-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .dropdown-backdrop {
    position: fixed;
    inset: 0;
    z-index: 99;
  }

  .dropdown-menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    min-width: 170px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    z-index: 100;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    width: 100%;
    text-align: left;
    padding: 8px 12px;
    border: none;
    background: none;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    border-radius: 6px;
    transition: all 100ms ease;
  }

  .dropdown-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .dropdown-item.selected {
    color: var(--accent-primary);
    font-weight: 600;
  }

  /* ── Content ── */
  .content {
    flex: 1;
  }

  /* Loading / Empty */
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 64px;
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

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px;
    color: var(--text-muted);
  }

  .empty p {
    margin-top: 12px;
    font-size: 14px;
  }

  /* ── Albums grid ── */
  .albums-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(162px, 1fr));
    gap: 16px;
    padding-bottom: 24px;
  }

  .album-card-wrapper {
    position: relative;
  }

  .album-card-wrapper.unavailable {
    opacity: 0.45;
    filter: grayscale(0.6);
  }

  .album-card-wrapper.unavailable:hover {
    opacity: 0.55;
  }

  .unavailable-overlay {
    position: absolute;
    bottom: 36px;
    left: 0;
    right: 0;
    text-align: center;
    padding: 4px 8px;
    background: rgba(0, 0, 0, 0.75);
    color: var(--text-muted);
    font-size: 11px;
    border-radius: 4px;
    margin: 0 8px;
  }

  /* ── Tracks list ── */
  .tracks-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .track-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 6px;
    transition: background 150ms ease;
  }

  .track-row:hover {
    background: var(--bg-hover);
  }

  .track-row.active {
    background: var(--bg-active, var(--bg-hover));
  }

  .track-artwork {
    flex-shrink: 0;
    width: 40px;
    height: 40px;
  }

  .track-thumb {
    width: 40px;
    height: 40px;
    border-radius: 4px;
    object-fit: cover;
  }

  .track-thumb-placeholder {
    width: 40px;
    height: 40px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .track-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .track-title {
    font-size: 14px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-meta {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .artist-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
    transition: color 150ms ease;
  }

  .artist-link:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .separator {
    color: var(--text-muted);
  }

  .album-name {
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-quality {
    flex-shrink: 0;
  }

  .track-duration {
    flex-shrink: 0;
    font-size: 13px;
    color: var(--text-muted);
    min-width: 45px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .track-date {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-muted);
    min-width: 80px;
    text-align: right;
  }

  .download-btn {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .download-btn:hover {
    background: var(--accent-primary);
    color: #fff;
    border-color: var(--accent-primary);
  }
</style>
