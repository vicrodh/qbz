<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { Search, X, Download, Music, Disc3, Loader2, ShoppingBag } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import Dropdown from '../Dropdown.svelte';
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
  let searchInput = $state<HTMLInputElement | null>(null);
  let filterOption = $state<FilterOption>('all');
  let sortOption = $state<SortOption>('date');

  let albums = $state<PurchasedAlbum[]>([]);
  let tracks = $state<PurchasedTrack[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let searchTimeout: ReturnType<typeof setTimeout> | null = null;

  const FILTER_KEYS: FilterOption[] = ['all', 'hires'];
  const SORT_KEYS: SortOption[] = ['date', 'artist', 'album', 'quality'];

  function getFilterLabels(): string[] {
    const tr = get(t);
    return FILTER_KEYS.map((key) => tr(`purchases.filter.${key}`));
  }

  function getSortLabels(): string[] {
    const tr = get(t);
    return SORT_KEYS.map((key) => tr(`purchases.sort.${key}`));
  }

  function getFilterDisplayValue(): string {
    return get(t)(`purchases.filter.${filterOption}`);
  }

  function getSortDisplayValue(): string {
    return get(t)(`purchases.sort.${sortOption}`);
  }

  function handleFilterChange(label: string) {
    const labels = getFilterLabels();
    const idx = labels.indexOf(label);
    if (idx >= 0) filterOption = FILTER_KEYS[idx];
  }

  function handleSortChange(label: string) {
    const labels = getSortLabels();
    const idx = labels.indexOf(label);
    if (idx >= 0) sortOption = SORT_KEYS[idx];
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
        return sorted.sort(
          (a, b) =>
            new Date(b.purchased_at || '').getTime() -
            new Date(a.purchased_at || '').getTime()
        );
      case 'artist':
        return sorted.sort((a, b) =>
          a.artist.name.localeCompare(b.artist.name)
        );
      case 'album':
        return sorted.sort((a, b) => a.title.localeCompare(b.title));
      case 'quality':
        return sorted.sort(
          (a, b) =>
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
        return sorted.sort(
          (a, b) =>
            new Date(b.purchased_at || '').getTime() -
            new Date(a.purchased_at || '').getTime()
        );
      case 'artist':
        return sorted.sort((a, b) =>
          a.performer.name.localeCompare(b.performer.name)
        );
      case 'album':
        return sorted.sort((a, b) =>
          (a.album?.title || '').localeCompare(b.album?.title || '')
        );
      case 'quality':
        return sorted.sort(
          (a, b) =>
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
    loadPurchases();
  }

  onMount(() => {
    loadPurchases();
  });
</script>

<div class="purchases-view">
  <!-- Header -->
  <div class="purchases-header">
    <div class="title-row">
      <ShoppingBag size={20} />
      <h2>{$t('purchases.title')}</h2>
    </div>

    <!-- Search bar -->
    <div class="search-bar">
      <Search size={14} />
      <input
        type="text"
        bind:value={searchQuery}
        bind:this={searchInput}
        placeholder={$t('purchases.search')}
        oninput={handleSearchInput}
      />
      {#if searchQuery}
        <button class="clear-btn" onclick={clearSearch}>
          <X size={14} />
        </button>
      {/if}
    </div>

    <!-- Tabs and controls -->
    <div class="controls-row">
      <div class="tabs">
        <button
          class="tab"
          class:active={activeTab === 'albums'}
          onclick={() => (activeTab = 'albums')}
        >
          <Disc3 size={14} />
          <span>{$t('purchases.tabs.albums')}</span>
          <span class="count">{displayedAlbums.length}</span>
        </button>
        <button
          class="tab"
          class:active={activeTab === 'tracks'}
          onclick={() => (activeTab = 'tracks')}
        >
          <Music size={14} />
          <span>{$t('purchases.tabs.tracks')}</span>
          <span class="count">{displayedTracks.length}</span>
        </button>
      </div>

      <div class="filters">
        <Dropdown
          value={getFilterDisplayValue()}
          options={getFilterLabels()}
          onchange={handleFilterChange}
        />
        <Dropdown
          value={getSortDisplayValue()}
          options={getSortLabels()}
          onchange={handleSortChange}
        />
      </div>
    </div>
  </div>

  <!-- Content -->
  <div class="purchases-content">
    {#if loading}
      <div class="loading-state">
        <Loader2 size={24} class="spin" />
        <span>{$t('actions.loading')}</span>
      </div>
    {:else if error}
      <div class="error-state">
        <p>{error}</p>
        <button class="retry-btn" onclick={loadPurchases}>{$t('actions.retry')}</button>
      </div>
    {:else if activeTab === 'albums'}
      {#if displayedAlbums.length === 0}
        <div class="empty-state">
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
        <div class="empty-state">
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
                    <span class="separator">-</span>
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
    display: flex;
    flex-direction: column;
    gap: 16px;
    height: 100%;
  }

  .purchases-header {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-primary);
  }

  .title-row h2 {
    margin: 0;
    font-size: 1.4rem;
    font-weight: 600;
  }

  .search-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border-radius: 8px;
    border: 1px solid var(--border-primary);
    color: var(--text-secondary);
  }

  .search-bar input {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 0.875rem;
  }

  .search-bar input::placeholder {
    color: var(--text-tertiary);
  }

  .clear-btn {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-tertiary);
    padding: 2px;
    display: flex;
    align-items: center;
  }

  .clear-btn:hover {
    color: var(--text-primary);
  }

  .controls-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .tabs {
    display: flex;
    gap: 4px;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 0.8125rem;
    transition: all 0.15s ease;
  }

  .tab:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .tab.active {
    background: var(--accent-primary);
    color: var(--accent-on-primary, #fff);
    border-color: var(--accent-primary);
  }

  .count {
    font-size: 0.75rem;
    opacity: 0.7;
  }

  .filters {
    display: flex;
    gap: 8px;
  }

  .purchases-content {
    flex: 1;
    overflow-y: auto;
  }

  /* Loading / Error / Empty states */
  .loading-state,
  .error-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 60px 20px;
    color: var(--text-tertiary);
  }

  .error-state p {
    color: var(--error);
  }

  .retry-btn {
    padding: 6px 16px;
    border-radius: 6px;
    border: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
  }

  .empty-state p {
    font-size: 0.875rem;
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  /* Albums grid */
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
    opacity: 0.5;
    pointer-events: none;
  }

  .unavailable-overlay {
    position: absolute;
    bottom: 36px;
    left: 0;
    right: 0;
    text-align: center;
    padding: 4px 8px;
    background: rgba(0, 0, 0, 0.7);
    color: var(--text-tertiary);
    font-size: 0.7rem;
    border-radius: 4px;
    margin: 0 8px;
  }

  /* Tracks list */
  .tracks-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-bottom: 24px;
  }

  .track-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 6px;
    transition: background 0.15s ease;
  }

  .track-row:hover {
    background: var(--bg-hover);
  }

  .track-row.active {
    background: var(--bg-active);
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
    color: var(--text-tertiary);
  }

  .track-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .track-title {
    font-size: 0.875rem;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-meta {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 0.75rem;
    color: var(--text-tertiary);
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
    font-size: 0.75rem;
  }

  .artist-link:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .separator {
    color: var(--text-tertiary);
  }

  .album-name {
    color: var(--text-tertiary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-quality {
    flex-shrink: 0;
  }

  .track-duration {
    flex-shrink: 0;
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    min-width: 45px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .track-date {
    flex-shrink: 0;
    font-size: 0.75rem;
    color: var(--text-tertiary);
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
    border: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .download-btn:hover {
    background: var(--accent-primary);
    color: var(--accent-on-primary, #fff);
    border-color: var(--accent-primary);
  }
</style>
