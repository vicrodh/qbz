<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import {
    Search, X, Download, Music, Disc3, ShoppingBag,
    ChevronDown, LayoutGrid, List
  } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import { getPurchases, searchPurchases } from '$lib/services/purchases';
  import { formatDuration, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import {
    getHideUnavailable, setHideUnavailable,
    getHiResOnly, setHiResOnly,
    getHideDownloaded, setHideDownloaded,
  } from '$lib/stores/purchasesStore';
  import type { PurchasedAlbum, PurchasedTrack, PurchaseResponse } from '$lib/types/purchases';

  type PurchasesTab = 'albums' | 'tracks';
  type AlbumGroupMode = 'alpha' | 'artist';
  type SortBy = 'date' | 'artist' | 'album' | 'quality';
  type SortDirection = 'asc' | 'desc';
  type TrackGroupMode = 'artist' | 'album' | 'name';

  interface AlbumGroup {
    key: string;
    title: string;
    items: PurchasedAlbum[];
  }

  interface TrackGroup {
    key: string;
    title: string;
    items: PurchasedTrack[];
  }

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

  // Tab & search
  let activeTab = $state<PurchasesTab>('albums');
  let searchQuery = $state('');
  let searchExpanded = $state(false);
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;

  // Data
  let albums = $state<PurchasedAlbum[]>([]);
  let tracks = $state<PurchasedTrack[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // Albums: view mode, grouping, sorting
  let albumViewMode = $state<'grid' | 'list'>('grid');
  let albumGroupingEnabled = $state(false);
  let albumGroupMode = $state<AlbumGroupMode>('alpha');
  let showAlbumGroupMenu = $state(false);
  let albumSortBy = $state<SortBy>('date');
  let albumSortDirection = $state<SortDirection>('desc');
  let showAlbumSortMenu = $state(false);

  // Albums: filter panel (LocalLibraryView pattern)
  let showFilterPanel = $state(false);
  let filterPanelRef = $state<HTMLDivElement | null>(null);
  let filterHideUnavailable = $state(getHideUnavailable());
  let filterHiResOnly = $state(getHiResOnly());
  let filterHideDownloaded = $state(getHideDownloaded());

  // Tracks: grouping
  let trackGroupingEnabled = $state(false);
  let trackGroupMode = $state<TrackGroupMode>('artist');
  let showTrackGroupMenu = $state(false);

  // Persist filter changes
  $effect(() => { setHideUnavailable(filterHideUnavailable); });
  $effect(() => { setHiResOnly(filterHiResOnly); });
  $effect(() => { setHideDownloaded(filterHideDownloaded); });

  const albumSortOptions = [
    { value: 'date' as SortBy, label: 'Purchase date' },
    { value: 'artist' as SortBy, label: 'Artist' },
    { value: 'album' as SortBy, label: 'Album' },
    { value: 'quality' as SortBy, label: 'Quality' },
  ];

  const hasActiveFilters = $derived(filterHideUnavailable || filterHiResOnly || filterHideDownloaded);
  const activeFilterCount = $derived(
    (filterHideUnavailable ? 1 : 0) + (filterHiResOnly ? 1 : 0) + (filterHideDownloaded ? 1 : 0)
  );

  function clearAllFilters() {
    filterHideUnavailable = false;
    filterHiResOnly = false;
    filterHideDownloaded = false;
    setHideUnavailable(false);
    setHiResOnly(false);
    setHideDownloaded(false);
  }

  function selectAlbumSort(value: SortBy) {
    if (albumSortBy === value) {
      albumSortDirection = albumSortDirection === 'asc' ? 'desc' : 'asc';
    } else {
      albumSortBy = value;
      albumSortDirection = value === 'date' ? 'desc' : 'asc';
    }
    showAlbumSortMenu = false;
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

  // ── Album filtering & sorting ──

  function applyAlbumFilters(list: PurchasedAlbum[]): PurchasedAlbum[] {
    let result = list;
    if (filterHideUnavailable) result = result.filter((a) => a.downloadable);
    if (filterHiResOnly) result = result.filter((a) => a.hires);
    if (filterHideDownloaded) result = result.filter((a) => !a.downloaded);
    return result;
  }

  function applyTrackFilters(list: PurchasedTrack[]): PurchasedTrack[] {
    let result = list;
    if (filterHideDownloaded) result = result.filter((track) => !track.downloaded);
    if (filterHiResOnly) result = result.filter((track) => track.hires);
    return result;
  }

  function sortAlbums(list: PurchasedAlbum[]): PurchasedAlbum[] {
    const sorted = [...list];
    const dir = albumSortDirection === 'asc' ? 1 : -1;
    switch (albumSortBy) {
      case 'date':
        return sorted.sort((a, b) =>
          dir * (new Date(a.purchased_at || '').getTime() - new Date(b.purchased_at || '').getTime())
        );
      case 'artist':
        return sorted.sort((a, b) => dir * a.artist.name.localeCompare(b.artist.name));
      case 'album':
        return sorted.sort((a, b) => dir * a.title.localeCompare(b.title));
      case 'quality':
        return sorted.sort((a, b) =>
          dir * ((a.maximum_sampling_rate || 0) - (b.maximum_sampling_rate || 0) ||
          (a.maximum_bit_depth || 0) - (b.maximum_bit_depth || 0))
        );
      default:
        return sorted;
    }
  }

  function alphaGroupKey(str: string): string {
    const ch = str.charAt(0).toUpperCase();
    return /[A-Z]/.test(ch) ? ch : '#';
  }

  function groupAlbums(list: PurchasedAlbum[]): AlbumGroup[] {
    const groups = new Map<string, PurchasedAlbum[]>();
    for (const album of list) {
      const key = albumGroupMode === 'alpha'
        ? alphaGroupKey(album.title)
        : album.artist.name;
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key)!.push(album);
    }
    return [...groups.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, items]) => ({ key, title: key, items }));
  }

  const filteredAlbums = $derived(sortAlbums(applyAlbumFilters(albums)));
  const filteredTracks = $derived(applyTrackFilters(tracks));
  const groupedAlbums = $derived(
    albumGroupingEnabled ? groupAlbums(filteredAlbums) : [{ key: 'all', title: '', items: filteredAlbums }]
  );

  // ── Track grouping ──

  function groupTracks(list: PurchasedTrack[]): TrackGroup[] {
    const groups = new Map<string, PurchasedTrack[]>();
    for (const track of list) {
      let key: string;
      if (trackGroupMode === 'name') {
        key = alphaGroupKey(track.title);
      } else if (trackGroupMode === 'artist') {
        key = track.performer.name;
      } else {
        key = track.album?.title || 'Unknown';
      }
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key)!.push(track);
    }
    return [...groups.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, items]) => ({ key, title: key, items }));
  }

  const groupedTracks = $derived(
    trackGroupingEnabled ? groupTracks(filteredTracks) : [{ key: 'all', title: '', items: filteredTracks }]
  );

  // ── Data loading ──

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
  <!-- Header -->
  <div class="header">
    <div class="header-icon">
      <ShoppingBag size={32} color="var(--accent-primary)" />
    </div>
    <div class="header-content">
      <h1>{$t('purchases.title')}</h1>
    </div>
  </div>

  <!-- Sticky Navigation Bar -->
  <div class="purchases-nav">
    <div class="nav-left">
      <button
        class="nav-link"
        class:active={activeTab === 'albums'}
        onclick={() => (activeTab = 'albums')}
      >
        <Disc3 size={16} />
        <span>{$t('purchases.tabs.albums')}</span>
        <span class="nav-count">{filteredAlbums.length}</span>
      </button>
      <button
        class="nav-link"
        class:active={activeTab === 'tracks'}
        onclick={() => (activeTab = 'tracks')}
      >
        <Music size={16} />
        <span>{$t('purchases.tabs.tracks')}</span>
        <span class="nav-count">{filteredTracks.length}</span>
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

  <!-- Toolbar (per-tab, matches FavoritesView) -->
  <div class="toolbar">
    {#if activeTab === 'albums'}
      <div class="toolbar-controls">
        <!-- Group dropdown -->
        <div class="dropdown-container">
          <button class="control-btn" onclick={() => (showAlbumGroupMenu = !showAlbumGroupMenu)}>
            <span>{!albumGroupingEnabled
              ? 'Group: Off'
              : albumGroupMode === 'alpha'
                ? 'Group: A-Z'
                : 'Group: Artist'}</span>
            <ChevronDown size={14} />
          </button>
          {#if showAlbumGroupMenu}
            <div class="dropdown-menu">
              <button
                class="dropdown-item"
                class:selected={!albumGroupingEnabled}
                onclick={() => { albumGroupingEnabled = false; showAlbumGroupMenu = false; }}
              >
                Off
              </button>
              <button
                class="dropdown-item"
                class:selected={albumGroupingEnabled && albumGroupMode === 'alpha'}
                onclick={() => { albumGroupMode = 'alpha'; albumGroupingEnabled = true; showAlbumGroupMenu = false; }}
              >
                Alphabetical (A-Z)
              </button>
              <button
                class="dropdown-item"
                class:selected={albumGroupingEnabled && albumGroupMode === 'artist'}
                onclick={() => { albumGroupMode = 'artist'; albumGroupingEnabled = true; showAlbumGroupMenu = false; }}
              >
                Artist
              </button>
            </div>
          {/if}
        </div>

        <!-- Sort dropdown -->
        <div class="dropdown-container">
          <button class="control-btn" onclick={() => (showAlbumSortMenu = !showAlbumSortMenu)}>
            <span>Sort: {albumSortOptions.find(o => o.value === albumSortBy)?.label}</span>
            <ChevronDown size={14} />
          </button>
          {#if showAlbumSortMenu}
            <div class="dropdown-menu sort-menu">
              {#each albumSortOptions as option}
                <button
                  class="dropdown-item"
                  class:selected={albumSortBy === option.value}
                  onclick={() => selectAlbumSort(option.value)}
                >
                  <span>{option.label}</span>
                  {#if albumSortBy === option.value}
                    <span class="sort-indicator">{albumSortDirection === 'asc' ? '↑' : '↓'}</span>
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>

        <!-- Filter button (LocalLibraryView pattern) -->
        <div class="dropdown-container" bind:this={filterPanelRef}>
          <button
            class="control-btn icon-only"
            class:active={hasActiveFilters}
            onclick={() => (showFilterPanel = !showFilterPanel)}
            title="Filter"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
              <path d="M4.22657 2C2.50087 2 1.58526 4.03892 2.73175 5.32873L8.99972 12.3802V19C8.99972 19.3788 9.21373 19.725 9.55251 19.8944L13.5525 21.8944C13.8625 22.0494 14.2306 22.0329 14.5255 21.8507C14.8203 21.6684 14.9997 21.3466 14.9997 21V12.3802L21.2677 5.32873C22.4142 4.03893 21.4986 2 19.7729 2H4.22657Z"/>
            </svg>
            {#if activeFilterCount > 0}
              <span class="filter-badge">{activeFilterCount}</span>
            {/if}
          </button>
          {#if showFilterPanel}
            <div class="filter-backdrop" onclick={() => showFilterPanel = false} role="presentation"></div>
            <div class="filter-panel">
              <div class="filter-panel-header">
                <span>{$t('library.filters')}</span>
                {#if hasActiveFilters}
                  <button class="clear-filters-btn" onclick={clearAllFilters}>{$t('library.clearAllFilters')}</button>
                {/if}
              </div>
              <div class="filter-section">
                <div class="filter-section-label">{$t('purchases.filter.availability')}</div>
                <div class="filter-checkboxes">
                  <label class="filter-checkbox">
                    <input type="checkbox" bind:checked={filterHideUnavailable} />
                    <span class="checkmark"></span>
                    <span class="label-text">{$t('purchases.filter.hideUnavailable')}</span>
                  </label>
                </div>
              </div>
              <div class="filter-section">
                <div class="filter-section-label">{$t('library.quality')}</div>
                <div class="filter-checkboxes">
                  <label class="filter-checkbox">
                    <input type="checkbox" bind:checked={filterHiResOnly} />
                    <span class="checkmark"></span>
                    <span class="label-text">Hi-Res</span>
                    <span class="label-hint">24bit+</span>
                  </label>
                </div>
              </div>
              <div class="filter-section">
                <div class="filter-section-label">{$t('purchases.filter.downloads')}</div>
                <div class="filter-checkboxes">
                  <label class="filter-checkbox">
                    <input type="checkbox" bind:checked={filterHideDownloaded} />
                    <span class="checkmark"></span>
                    <span class="label-text">{$t('purchases.filter.hideDownloaded')}</span>
                  </label>
                </div>
              </div>
            </div>
          {/if}
        </div>

        <!-- Grid/List toggle -->
        <button
          class="icon-btn"
          onclick={() => (albumViewMode = albumViewMode === 'grid' ? 'list' : 'grid')}
          title={albumViewMode === 'grid' ? 'List view' : 'Grid view'}
        >
          {#if albumViewMode === 'grid'}
            <List size={16} />
          {:else}
            <LayoutGrid size={16} />
          {/if}
        </button>
      </div>
    {:else if activeTab === 'tracks'}
      <div class="toolbar-controls">
        <!-- Track group dropdown -->
        <div class="dropdown-container">
          <button class="control-btn" onclick={() => (showTrackGroupMenu = !showTrackGroupMenu)}>
            <span>
              {trackGroupingEnabled
                ? trackGroupMode === 'album'
                  ? 'Group: Album'
                  : trackGroupMode === 'artist'
                    ? 'Group: Artist'
                    : 'Group: Name'
                : 'Group: Off'}
            </span>
            <ChevronDown size={14} />
          </button>
          {#if showTrackGroupMenu}
            <div class="dropdown-menu">
              <button
                class="dropdown-item"
                class:selected={!trackGroupingEnabled}
                onclick={() => { trackGroupingEnabled = false; showTrackGroupMenu = false; }}
              >
                Off
              </button>
              <button
                class="dropdown-item"
                class:selected={trackGroupingEnabled && trackGroupMode === 'album'}
                onclick={() => { trackGroupMode = 'album'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
              >
                Album
              </button>
              <button
                class="dropdown-item"
                class:selected={trackGroupingEnabled && trackGroupMode === 'artist'}
                onclick={() => { trackGroupMode = 'artist'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
              >
                Artist
              </button>
              <button
                class="dropdown-item"
                class:selected={trackGroupingEnabled && trackGroupMode === 'name'}
                onclick={() => { trackGroupMode = 'name'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
              >
                Name (A-Z)
              </button>
            </div>
          {/if}
        </div>

        <!-- Filter button (shared with albums) -->
        <div class="dropdown-container" bind:this={filterPanelRef}>
          <button
            class="control-btn icon-only"
            class:active={hasActiveFilters}
            onclick={() => (showFilterPanel = !showFilterPanel)}
            title="Filter"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
              <path d="M4.22657 2C2.50087 2 1.58526 4.03892 2.73175 5.32873L8.99972 12.3802V19C8.99972 19.3788 9.21373 19.725 9.55251 19.8944L13.5525 21.8944C13.8625 22.0494 14.2306 22.0329 14.5255 21.8507C14.8203 21.6684 14.9997 21.3466 14.9997 21V12.3802L21.2677 5.32873C22.4142 4.03893 21.4986 2 19.7729 2H4.22657Z"/>
            </svg>
            {#if activeFilterCount > 0}
              <span class="filter-badge">{activeFilterCount}</span>
            {/if}
          </button>
          {#if showFilterPanel}
            <div class="filter-backdrop" onclick={() => showFilterPanel = false} role="presentation"></div>
            <div class="filter-panel">
              <div class="filter-panel-header">
                <span>{$t('library.filters')}</span>
                {#if hasActiveFilters}
                  <button class="clear-filters-btn" onclick={clearAllFilters}>{$t('library.clearAllFilters')}</button>
                {/if}
              </div>
              <div class="filter-section">
                <div class="filter-section-label">{$t('library.quality')}</div>
                <div class="filter-checkboxes">
                  <label class="filter-checkbox">
                    <input type="checkbox" bind:checked={filterHiResOnly} />
                    <span class="checkmark"></span>
                    <span class="label-text">Hi-Res</span>
                    <span class="label-hint">24bit+</span>
                  </label>
                </div>
              </div>
              <div class="filter-section">
                <div class="filter-section-label">{$t('purchases.filter.downloads')}</div>
                <div class="filter-checkboxes">
                  <label class="filter-checkbox">
                    <input type="checkbox" bind:checked={filterHideDownloaded} />
                    <span class="checkmark"></span>
                    <span class="label-text">{$t('purchases.filter.hideDownloaded')}</span>
                  </label>
                </div>
              </div>
            </div>
          {/if}
        </div>
      </div>
    {/if}
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
      {#if filteredAlbums.length === 0}
        <div class="empty">
          <ShoppingBag size={48} />
          <p>{$t('purchases.empty')}</p>
        </div>
      {:else}
        {#each groupedAlbums as group (group.key)}
          {#if albumGroupingEnabled && group.title}
            <div class="group-header">{group.title}</div>
          {/if}

          {#if albumViewMode === 'grid'}
            <div class="albums-grid">
              {#each group.items as album (album.id)}
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
          {:else}
            <div class="albums-list">
              {#each group.items as album (album.id)}
                <button
                  class="album-list-row"
                  class:unavailable={!album.downloadable}
                  onclick={() => album.downloadable && onAlbumClick?.(album.id)}
                >
                  <img
                    src={getQobuzImage(album.image)}
                    alt={album.title}
                    class="album-list-art"
                  />
                  <div class="album-list-info">
                    <span class="album-list-title">{album.title}</span>
                    <span class="album-list-artist">{album.artist.name}</span>
                  </div>
                  <div class="album-list-quality">
                    <QualityBadge
                      bitDepth={album.maximum_bit_depth}
                      samplingRate={album.maximum_sampling_rate}
                      compact={true}
                    />
                  </div>
                  <span class="album-list-date">{formatPurchaseDate(album.purchased_at)}</span>
                  {#if !album.downloadable}
                    <span class="album-list-unavailable">{$t('purchases.unavailable')}</span>
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        {/each}
      {/if}
    {:else}
      {#if filteredTracks.length === 0}
        <div class="empty">
          <Music size={48} />
          <p>{$t('purchases.emptyTracks')}</p>
        </div>
      {:else}
        {#each groupedTracks as group (group.key)}
          {#if trackGroupingEnabled && group.title}
            <div class="group-header">{group.title}</div>
          {/if}
          <div class="tracks-list">
            {#each group.items as track (track.id)}
              <div class="track-row" class:active={activeTrackId === track.id}>
                <div class="track-artwork">
                  {#if track.album?.image}
                    <img
                      src={getQobuzImage(track.album.image)}
                      alt={track.album?.title}
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
                      <span class="separator">&middot;</span>
                      <span class="album-name">{track.album.title}</span>
                    {/if}
                  </span>
                </div>
                <div class="track-quality">
                  {#if track.maximum_bit_depth && track.maximum_sampling_rate}
                    {track.maximum_bit_depth}/{track.maximum_sampling_rate}
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
        {/each}
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

  /* ── Header ── */
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

  .header-content h1 {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
  }

  /* ── Sticky Navigation Bar ── */
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

  /* ── Toolbar (FavoritesView pattern) ── */
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

  .control-btn.icon-only {
    width: 36px;
    height: 36px;
    justify-content: center;
    padding: 0;
  }

  .control-btn.active {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: white;
  }

  .control-btn.active:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: 8px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
  }

  .icon-btn:hover {
    color: var(--text-primary);
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
    max-height: 260px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--text-muted) transparent;
  }

  .dropdown-menu::-webkit-scrollbar {
    width: 8px;
  }

  .dropdown-menu::-webkit-scrollbar-track {
    background: transparent;
  }

  .dropdown-menu::-webkit-scrollbar-thumb {
    background: var(--text-muted);
    border-radius: 9999px;
  }

  .dropdown-menu::-webkit-scrollbar-thumb:hover {
    background: var(--text-secondary);
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    text-align: left;
    padding: 8px 10px;
    background: none;
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
  }

  .dropdown-item:hover,
  .dropdown-item.selected {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .sort-indicator {
    font-size: 11px;
    color: var(--accent-primary);
    font-weight: 600;
  }

  /* ── Filter Panel (LocalLibraryView pattern) ── */
  .filter-backdrop {
    position: fixed;
    inset: 0;
    z-index: 19;
  }

  .filter-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    background: var(--accent-primary);
    color: white;
    font-size: 11px;
    font-weight: 600;
    border-radius: 9px;
    margin-left: 4px;
  }

  .filter-panel {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 12px;
    min-width: 280px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
    z-index: 20;
  }

  .filter-panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .filter-panel-header span {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .clear-filters-btn {
    background: none;
    border: none;
    padding: 4px 8px;
    font-size: 12px;
    color: var(--accent-primary);
    cursor: pointer;
    border-radius: 4px;
    transition: background 150ms ease;
  }

  .clear-filters-btn:hover {
    background: var(--bg-tertiary);
  }

  .filter-section {
    margin-bottom: 12px;
  }

  .filter-section:last-child {
    margin-bottom: 0;
  }

  .filter-section-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .filter-checkboxes {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .filter-checkbox {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    padding: 6px 8px;
    border-radius: 6px;
    transition: background 150ms ease;
  }

  .filter-checkbox:hover {
    background: var(--bg-tertiary);
  }

  .filter-checkbox input {
    display: none;
  }

  .filter-checkbox .checkmark {
    width: 16px;
    height: 16px;
    border: 2px solid var(--text-muted);
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .filter-checkbox input:checked + .checkmark {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .filter-checkbox input:checked + .checkmark::after {
    content: '';
    width: 4px;
    height: 8px;
    border: solid white;
    border-width: 0 2px 2px 0;
    transform: rotate(45deg) translateY(-1px);
  }

  .filter-checkbox .label-text {
    font-size: 12px;
    color: var(--text-primary);
  }

  .filter-checkbox .label-hint {
    font-size: 11px;
    color: var(--text-muted);
    margin-left: auto;
  }

  /* ── Content ── */
  .content {
    min-height: 200px;
  }

  .loading,
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px;
    color: var(--text-muted);
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

  .empty p {
    margin-top: 12px;
    font-size: 14px;
  }

  /* ── Group header ── */
  .group-header {
    padding: 16px 0 8px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 12px;
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

  /* ── Albums list ── */
  .albums-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding-bottom: 24px;
  }

  .album-list-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 6px;
    background: none;
    border: none;
    width: 100%;
    cursor: pointer;
    text-align: left;
    transition: background 150ms ease;
  }

  .album-list-row:hover {
    background: var(--bg-hover);
  }

  .album-list-row.unavailable {
    opacity: 0.45;
    filter: grayscale(0.6);
    cursor: default;
  }

  .album-list-art {
    width: 48px;
    height: 48px;
    border-radius: 4px;
    object-fit: cover;
    flex-shrink: 0;
  }

  .album-list-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .album-list-title {
    font-size: 14px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .album-list-artist {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .album-list-quality {
    flex-shrink: 0;
  }

  .album-list-date {
    flex-shrink: 0;
    font-size: 12px;
    color: var(--text-muted);
    min-width: 80px;
    text-align: right;
  }

  .album-list-unavailable {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
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
    font-size: 12px;
    color: #666666;
    width: 80px;
    text-align: center;
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
