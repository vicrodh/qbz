<script lang="ts">
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import {
    Search, X, Download, Check, Loader2, Music, Disc3, ShoppingBag,
    ChevronDown, LayoutGrid, List
  } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import { getPurchasesByType, getPurchaseIds, searchPurchases, getDownloadedTrackIds, getFormats } from '$lib/services/purchases';
  import { allTrackStatuses, startTrackDownload } from '$lib/stores/purchaseDownloadStore';
  import { showToast } from '$lib/stores/toastStore';
  import { formatDuration, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import {
    getHideUnavailable, setHideUnavailable,
    getHideDownloaded, setHideDownloaded,
  } from '$lib/stores/purchasesStore';
  import { getUserItem, setUserItem } from '$lib/utils/userStorage';
  import type { PurchasedAlbum, PurchasedTrack, PurchaseFormatOption } from '$lib/types/purchases';
  import type { DisplayTrack } from '$lib/types';

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
    onTrackPlay?: (track: DisplayTrack) => void;
    onAlbumPlay?: (albumId: string) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
  }

  let {
    onAlbumClick,
    onArtistClick,
    onTrackPlay,
    onAlbumPlay,
    activeTrackId = null,
    isPlaybackActive = false,
  }: Props = $props();

  // Tab & search
  let activeTab = $state<PurchasesTab>('albums');
  let searchQuery = $state('');
  let searchExpanded = $state(false);
  let searchTimeout: ReturnType<typeof setTimeout> | null = null;
  let showRegionNotice = $state(getUserItem('qbz-purchases-region-notice-seen') !== 'true');

  // Data
  let albums = $state<PurchasedAlbum[]>([]);
  let tracks = $state<PurchasedTrack[]>([]);
  let downloadedTrackIds = $state<Set<number>>(new Set());
  let totalAlbumPurchases = $state(0);
  let totalTrackPurchases = $state(0);
  let metadataLoaded = $state(false);
  let albumsLoaded = $state(false);
  let tracksLoaded = $state(false);
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
  type QualityFilter = 'all' | 'hires' | 'cd' | 'lossy';
  let filterHideUnavailable = $state(getHideUnavailable());
  let filterQuality = $state<QualityFilter>((getUserItem('qbz-purchases-quality-filter') as QualityFilter) || 'all');
  let filterHideDownloaded = $state(getHideDownloaded());

  // Tracks: grouping
  let trackGroupingEnabled = $state(false);
  let trackGroupMode = $state<TrackGroupMode>('artist');
  let showTrackGroupMenu = $state(false);

  // Track format picker popup
  let formatPickerTrack = $state<PurchasedTrack | null>(null);
  let formatPickerFormats = $state<PurchaseFormatOption[]>([]);
  let formatPickerLoading = $state(false);
  let formatPickerAnchor = $state<{ top: number; right: number } | null>(null);

  // Persist filter changes
  $effect(() => { setHideUnavailable(filterHideUnavailable); });
  $effect(() => { setUserItem('qbz-purchases-quality-filter', filterQuality); });
  $effect(() => { setHideDownloaded(filterHideDownloaded); });

  /** Close all dropdown menus (mutual exclusion) */
  function closeAllMenus() {
    showAlbumGroupMenu = false;
    showAlbumSortMenu = false;
    showFilterPanel = false;
    showTrackGroupMenu = false;
    closeFormatPicker();
  }

  const albumSortOptions = [
    { value: 'date' as SortBy, labelKey: 'purchases.sort.date' },
    { value: 'artist' as SortBy, labelKey: 'purchases.sort.artist' },
    { value: 'album' as SortBy, labelKey: 'purchases.sort.album' },
    { value: 'quality' as SortBy, labelKey: 'purchases.sort.quality' },
  ];

  const hasActiveFilters = $derived(filterHideUnavailable || filterQuality !== 'all' || filterHideDownloaded);
  const activeFilterCount = $derived(
    (filterHideUnavailable ? 1 : 0) + (filterQuality !== 'all' ? 1 : 0) + (filterHideDownloaded ? 1 : 0)
  );

  function clearAllFilters() {
    filterHideUnavailable = false;
    filterQuality = 'all';
    filterHideDownloaded = false;
    setHideUnavailable(false);
    setUserItem('qbz-purchases-quality-filter', 'all');
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

  function formatPurchaseDate(ts?: number): string {
    if (!ts) return '';
    try {
      return new Date(ts * 1000).toLocaleDateString(undefined, {
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

  function matchesQualityFilter(hires: boolean, bitDepth?: number, samplingRate?: number): boolean {
    if (filterQuality === 'all') return true;
    if (filterQuality === 'hires') return hires;
    if (filterQuality === 'cd') return !hires && (bitDepth === 16 || (!bitDepth && !samplingRate));
    if (filterQuality === 'lossy') return !bitDepth || bitDepth < 16;
    return true;
  }

  function applyAlbumFilters(list: PurchasedAlbum[]): PurchasedAlbum[] {
    let result = list;
    if (filterHideUnavailable) result = result.filter((a) => a.downloadable);
    if (filterQuality !== 'all') result = result.filter((a) => matchesQualityFilter(a.hires, a.maximum_bit_depth, a.maximum_sampling_rate));
    if (filterHideDownloaded) result = result.filter((a) => !a.downloaded);
    return result;
  }

  function applyTrackFilters(list: PurchasedTrack[]): PurchasedTrack[] {
    let result = list;
    if (filterHideDownloaded) result = result.filter((track) => !track.downloaded);
    if (filterQuality !== 'all') result = result.filter((track) => matchesQualityFilter(track.hires, track.maximum_bit_depth, track.maximum_sampling_rate));
    return result;
  }

  function sortAlbums(list: PurchasedAlbum[]): PurchasedAlbum[] {
    const sorted = [...list];
    const dir = albumSortDirection === 'asc' ? 1 : -1;
    switch (albumSortBy) {
      case 'date':
        return sorted.sort((a, b) =>
          dir * ((a.purchased_at || 0) - (b.purchased_at || 0))
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
  const isSearchActive = $derived(searchQuery.trim().length > 0);
  const albumTabCount = $derived(
    isSearchActive || hasActiveFilters
      ? filteredAlbums.length
      : (totalAlbumPurchases || filteredAlbums.length)
  );
  const trackTabCount = $derived(
    isSearchActive || hasActiveFilters
      ? filteredTracks.length
      : (totalTrackPurchases || filteredTracks.length)
  );
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

  function enrichWithDownloadStatus(
    albumList: PurchasedAlbum[],
    trackList: PurchasedTrack[],
    dlIds: Set<number>,
  ): { albums: PurchasedAlbum[]; tracks: PurchasedTrack[] } {
    const enrichedAlbums = albumList.map((album) => {
      const albumTrackIds = album.tracks?.items?.map((track) => track.id) || [];
      const allDownloaded = albumTrackIds.length > 0 && albumTrackIds.every((id) => dlIds.has(id));
      return { ...album, downloaded: allDownloaded };
    });
    const enrichedTracks = trackList.map((track) => ({
      ...track,
      downloaded: dlIds.has(track.id),
    }));
    return { albums: enrichedAlbums, tracks: enrichedTracks };
  }

  async function loadPurchasesMetadata() {
    if (metadataLoaded) return;

    // Fetch totals per type separately — a single unfiltered call with limit=1
    // only returns the first type's total, leaving the other at 0.
    const [dlIds, albumIds, trackIds] = await Promise.all([
      getDownloadedTrackIds().catch(() => new Set<number>()),
      getPurchaseIds(1, 0, 'albums').catch(() => null),
      getPurchaseIds(1, 0, 'tracks').catch(() => null),
    ]);
    downloadedTrackIds = dlIds;
    totalAlbumPurchases = albumIds?.albums?.total ?? 0;
    totalTrackPurchases = trackIds?.tracks?.total ?? 0;
    metadataLoaded = true;
  }

  async function loadPurchasesByTab(tab: PurchasesTab, force = false) {
    if (!force) {
      if (tab === 'albums' && albumsLoaded) return;
      if (tab === 'tracks' && tracksLoaded) return;
    }

    loading = true;
    error = null;
    try {
      await loadPurchasesMetadata();
      const response = await getPurchasesByType(tab);
      const enriched = enrichWithDownloadStatus(
        response.albums.items,
        response.tracks.items,
        downloadedTrackIds,
      );

      if (tab === 'albums') {
        albums = enriched.albums;
        albumsLoaded = true;
        if (!totalAlbumPurchases) {
          totalAlbumPurchases = response.albums.total || enriched.albums.length;
        }
      } else {
        tracks = enriched.tracks;
        tracksLoaded = true;
        if (!totalTrackPurchases) {
          totalTrackPurchases = response.tracks.total || enriched.tracks.length;
        }
      }
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
        await loadPurchasesByTab(activeTab, true);
        return;
      }
      loading = true;
      try {
        await loadPurchasesMetadata();
        const response = await searchPurchases(searchQuery.trim());
        const enriched = enrichWithDownloadStatus(response.albums.items, response.tracks.items, downloadedTrackIds);
        albums = enriched.albums;
        tracks = enriched.tracks;
        albumsLoaded = true;
        tracksLoaded = true;
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
    void loadPurchasesByTab(activeTab, true);
  }

  function dismissRegionNotice() {
    showRegionNotice = false;
    setUserItem('qbz-purchases-region-notice-seen', 'true');
  }

  function toDisplayTrack(track: PurchasedTrack): DisplayTrack {
    return {
      id: track.id,
      title: track.title,
      artist: track.performer?.name,
      album: track.album?.title,
      albumArt: getQobuzImage(track.album?.image),
      albumId: track.album?.id,
      artistId: track.performer?.id,
      duration: formatDuration(track.duration),
      durationSeconds: track.duration,
      hires: track.hires,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate,
    };
  }

  function getTrackDownloadStatus(trackId: number): 'downloading' | 'complete' | 'failed' | null {
    return $allTrackStatuses[trackId] || null;
  }

  function closeFormatPicker() {
    formatPickerTrack = null;
    formatPickerFormats = [];
    formatPickerLoading = false;
    formatPickerAnchor = null;
  }

  async function handleTrackDownload(event: MouseEvent, track: PurchasedTrack) {
    event.stopPropagation();

    if (!track.album?.id) {
      showToast(get(t)('purchases.errors.noAlbum'), 'error');
      return;
    }

    try {
      const formats = await getFormats(track.album.id);
      if (formats.length === 0) {
        showToast(get(t)('purchases.errors.noFormats'), 'error');
        return;
      }

      // Single format: proceed directly
      if (formats.length === 1) {
        await executeTrackDownload(track, formats[0]);
        return;
      }

      // Multiple formats: show picker popup
      const btn = event.currentTarget as HTMLElement;
      const rect = btn.getBoundingClientRect();
      formatPickerTrack = track;
      formatPickerFormats = formats;
      formatPickerAnchor = { top: rect.bottom + 4, right: window.innerWidth - rect.right };
    } catch (err) {
      console.error('Track download error:', err);
    }
  }

  async function executeTrackDownload(track: PurchasedTrack, format: PurchaseFormatOption) {
    closeFormatPicker();
    if (!track.album?.id) return;

    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const { audioDir } = await import('@tauri-apps/api/path');
      const defaultPath = await audioDir();
      const dest = await open({
        directory: true,
        multiple: false,
        defaultPath,
        title: get(t)('purchases.chooseFolder'),
      });

      if (!dest || typeof dest !== 'string') return;

      const qualityDir = format.label.replace(/\//g, '-').trim();
      startTrackDownload(track.album.id, track.id, format.id, dest, qualityDir);
    } catch (err) {
      console.error('Track download error:', err);
    }
  }

  $effect(() => {
    if (searchQuery.trim()) return;
    void loadPurchasesByTab(activeTab);
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

  {#if showRegionNotice}
    <div class="region-notice" role="status" aria-live="polite">
      <span>{$t('purchases.regionNotice')}</span>
      <button type="button" class="region-notice-close" onclick={dismissRegionNotice}>
        {$t('actions.close')}
      </button>
    </div>
  {/if}

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
        <span class="nav-count">{albumTabCount}</span>
      </button>
      <button
        class="nav-link"
        class:active={activeTab === 'tracks'}
        onclick={() => (activeTab = 'tracks')}
      >
        <Music size={16} />
        <span>{$t('purchases.tabs.tracks')}</span>
        <span class="nav-count">{trackTabCount}</span>
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
          <button class="control-btn" onclick={() => { const wasOpen = showAlbumGroupMenu; closeAllMenus(); showAlbumGroupMenu = !wasOpen; }}>
            <span>{!albumGroupingEnabled
              ? $t('purchases.group.off')
              : albumGroupMode === 'alpha'
                ? $t('purchases.group.alpha')
                : $t('purchases.group.artist')}</span>
            <ChevronDown size={14} />
          </button>
          {#if showAlbumGroupMenu}
            <div class="dropdown-menu">
              <button
                class="dropdown-item"
                class:selected={!albumGroupingEnabled}
                onclick={() => { albumGroupingEnabled = false; showAlbumGroupMenu = false; }}
              >
                {$t('purchases.group.optionOff')}
              </button>
              <button
                class="dropdown-item"
                class:selected={albumGroupingEnabled && albumGroupMode === 'alpha'}
                onclick={() => { albumGroupMode = 'alpha'; albumGroupingEnabled = true; showAlbumGroupMenu = false; }}
              >
                {$t('purchases.group.optionAlpha')}
              </button>
              <button
                class="dropdown-item"
                class:selected={albumGroupingEnabled && albumGroupMode === 'artist'}
                onclick={() => { albumGroupMode = 'artist'; albumGroupingEnabled = true; showAlbumGroupMenu = false; }}
              >
                {$t('purchases.group.optionArtist')}
              </button>
            </div>
          {/if}
        </div>

        <!-- Sort dropdown -->
        <div class="dropdown-container">
          <button class="control-btn" onclick={() => { const wasOpen = showAlbumSortMenu; closeAllMenus(); showAlbumSortMenu = !wasOpen; }}>
            <span>{$t('purchases.sortLabel')}: {$t(albumSortOptions.find(option => option.value === albumSortBy)?.labelKey ?? 'purchases.sort.date')}</span>
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
                  <span>{$t(option.labelKey)}</span>
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
            onclick={() => { const wasOpen = showFilterPanel; closeAllMenus(); showFilterPanel = !wasOpen; }}
            title={$t('library.filters')}
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
                    <input type="radio" name="quality-filter" value="all" bind:group={filterQuality} />
                    <span class="checkmark radio"></span>
                    <span class="label-text">{$t('purchases.filter.all')}</span>
                  </label>
                  <label class="filter-checkbox">
                    <input type="radio" name="quality-filter" value="hires" bind:group={filterQuality} />
                    <span class="checkmark radio"></span>
                    <span class="label-text">{$t('purchases.filter.hires')}</span>
                    <span class="label-hint">24bit+</span>
                  </label>
                  <label class="filter-checkbox">
                    <input type="radio" name="quality-filter" value="cd" bind:group={filterQuality} />
                    <span class="checkmark radio"></span>
                    <span class="label-text">{$t('quality.cdQuality')}</span>
                    <span class="label-hint">16/44.1</span>
                  </label>
                  <label class="filter-checkbox">
                    <input type="radio" name="quality-filter" value="lossy" bind:group={filterQuality} />
                    <span class="checkmark radio"></span>
                    <span class="label-text">{$t('quality.mp3')}</span>
                    <span class="label-hint">MP3</span>
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
          title={albumViewMode === 'grid' ? $t('purchases.view.list') : $t('purchases.view.grid')}
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
          <button class="control-btn" onclick={() => { const wasOpen = showTrackGroupMenu; closeAllMenus(); showTrackGroupMenu = !wasOpen; }}>
            <span>
              {trackGroupingEnabled
                ? trackGroupMode === 'album'
                  ? $t('purchases.group.album')
                  : trackGroupMode === 'artist'
                    ? $t('purchases.group.artist')
                    : $t('purchases.group.name')
                : $t('purchases.group.off')}
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
                {$t('purchases.group.optionOff')}
              </button>
              <button
                class="dropdown-item"
                class:selected={trackGroupingEnabled && trackGroupMode === 'album'}
                onclick={() => { trackGroupMode = 'album'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
              >
                {$t('purchases.group.optionAlbum')}
              </button>
              <button
                class="dropdown-item"
                class:selected={trackGroupingEnabled && trackGroupMode === 'artist'}
                onclick={() => { trackGroupMode = 'artist'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
              >
                {$t('purchases.group.optionArtist')}
              </button>
              <button
                class="dropdown-item"
                class:selected={trackGroupingEnabled && trackGroupMode === 'name'}
                onclick={() => { trackGroupMode = 'name'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
              >
                {$t('purchases.group.optionName')}
              </button>
            </div>
          {/if}
        </div>

        <!-- Filter button (shared with albums) -->
        <div class="dropdown-container" bind:this={filterPanelRef}>
          <button
            class="control-btn icon-only"
            class:active={hasActiveFilters}
            onclick={() => { const wasOpen = showFilterPanel; closeAllMenus(); showFilterPanel = !wasOpen; }}
            title={$t('library.filters')}
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
                    <input type="radio" name="quality-filter" value="all" bind:group={filterQuality} />
                    <span class="checkmark radio"></span>
                    <span class="label-text">{$t('purchases.filter.all')}</span>
                  </label>
                  <label class="filter-checkbox">
                    <input type="radio" name="quality-filter" value="hires" bind:group={filterQuality} />
                    <span class="checkmark radio"></span>
                    <span class="label-text">{$t('purchases.filter.hires')}</span>
                    <span class="label-hint">24bit+</span>
                  </label>
                  <label class="filter-checkbox">
                    <input type="radio" name="quality-filter" value="cd" bind:group={filterQuality} />
                    <span class="checkmark radio"></span>
                    <span class="label-text">{$t('quality.cdQuality')}</span>
                    <span class="label-hint">16/44.1</span>
                  </label>
                  <label class="filter-checkbox">
                    <input type="radio" name="quality-filter" value="lossy" bind:group={filterQuality} />
                    <span class="checkmark radio"></span>
                    <span class="label-text">{$t('quality.mp3')}</span>
                    <span class="label-hint">MP3</span>
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
              {@const dlStatus = getTrackDownloadStatus(track.id)}
              {@const isDownloaded = track.downloaded || dlStatus === 'complete'}
              <div
                class="track-row"
                class:active={activeTrackId === track.id}
                class:playing={activeTrackId === track.id && isPlaybackActive}
                class:clickable={track.streamable && !!onTrackPlay}
                class:downloaded={isDownloaded}
                onclick={() => track.streamable && onTrackPlay?.(toDisplayTrack(track))}
                role={track.streamable && onTrackPlay ? 'button' : undefined}
                tabindex={track.streamable && onTrackPlay ? 0 : undefined}
              >
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
                      onclick={(e) => { e.stopPropagation(); onArtistClick?.(track.performer.id); }}
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
                {#if dlStatus === 'complete' || isDownloaded}
                  <button
                    class="download-btn redownload"
                    onclick={(e) => handleTrackDownload(e, track)}
                    title={$t('purchases.downloadTrack')}
                  >
                    <span class="redownload-check"><Check size={14} /></span>
                    <span class="redownload-icon"><Download size={14} /></span>
                  </button>
                {:else if dlStatus === 'downloading'}
                  <span class="download-active"><Loader2 size={14} class="spin" /></span>
                {:else if dlStatus === 'failed'}
                  <button class="download-btn failed" onclick={(e) => handleTrackDownload(e, track)} title={$t('purchases.failed')}>
                    <Download size={14} />
                  </button>
                {:else}
                  <button class="download-btn" onclick={(e) => handleTrackDownload(e, track)} title={$t('purchases.downloadTrack')}>
                    <Download size={14} />
                  </button>
                {/if}
              </div>
            {/each}
          </div>
        {/each}
      {/if}
    {/if}
  </div>

  <!-- Format picker popup for track downloads -->
  {#if formatPickerTrack && formatPickerAnchor}
    <div class="format-picker-backdrop" onclick={closeFormatPicker} role="presentation"></div>
    <div
      class="format-picker"
      style="top: {formatPickerAnchor.top}px; right: {formatPickerAnchor.right}px;"
    >
      <div class="format-picker-header">{$t('purchases.selectFormat')}</div>
      {#each formatPickerFormats as fmt (fmt.id)}
        <button
          class="format-picker-item"
          onclick={() => formatPickerTrack && executeTrackDownload(formatPickerTrack, fmt)}
        >
          <span class="format-label">{fmt.label}</span>
          {#if fmt.bit_depth && fmt.sampling_rate}
            <span class="format-detail">{fmt.bit_depth}/{fmt.sampling_rate}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
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

  .region-notice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    margin-bottom: 14px;
    border: 1px solid var(--alpha-12);
    border-radius: 10px;
    background: rgba(88, 165, 255, 0.08);
    color: var(--text-secondary);
    font-size: 12px;
  }

  .region-notice-close {
    border: 1px solid var(--alpha-12);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    border-radius: 8px;
    padding: 4px 8px;
    font-size: 11px;
    cursor: pointer;
  }

  .region-notice-close:hover {
    border-color: var(--alpha-18);
    color: var(--text-primary);
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

  .filter-checkbox .checkmark.radio {
    border-radius: 50%;
  }

  .filter-checkbox input:checked + .checkmark.radio::after {
    width: 6px;
    height: 6px;
    border: none;
    background: white;
    border-radius: 50%;
    transform: none;
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
    top: 0;
    left: 0;
    right: 0;
    aspect-ratio: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 16px;
    color: var(--text-muted);
    font-size: 11px;
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

  .track-row.clickable {
    cursor: pointer;
  }

  .track-row.active {
    background: var(--bg-active, var(--bg-hover));
  }

  .track-row.active .track-title {
    color: var(--accent-primary);
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

  .download-btn.failed {
    border-color: var(--error, #f44336);
    color: var(--error, #f44336);
  }

  .download-btn.redownload {
    border-color: transparent;
    background: transparent;
    color: var(--success, #4caf50);
  }

  .download-btn.redownload .redownload-check {
    display: flex;
  }

  .download-btn.redownload .redownload-icon {
    display: none;
  }

  .download-btn.redownload:hover {
    background: var(--bg-tertiary);
    border-color: var(--border-subtle);
    color: var(--text-secondary);
  }

  .download-btn.redownload:hover .redownload-check {
    display: none;
  }

  .download-btn.redownload:hover .redownload-icon {
    display: flex;
  }

  .download-active {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    color: var(--accent-primary);
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  .track-row.downloaded {
    opacity: 0.6;
  }

  /* Format picker popup */
  .format-picker-backdrop {
    position: fixed;
    inset: 0;
    z-index: 3000;
  }

  .format-picker {
    position: fixed;
    z-index: 3001;
    min-width: 180px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
    padding: 4px;
    overflow: hidden;
  }

  .format-picker-header {
    font-size: 11px;
    text-transform: uppercase;
    color: var(--text-muted);
    padding: 8px 12px 4px;
    letter-spacing: 0.5px;
  }

  .format-picker-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    width: 100%;
    padding: 8px 12px;
    border: none;
    background: none;
    color: var(--text-primary);
    cursor: pointer;
    border-radius: 6px;
    font-size: 13px;
    text-align: left;
    transition: background 100ms ease;
  }

  .format-picker-item:hover {
    background: var(--bg-hover);
  }

  .format-detail {
    font-size: 11px;
    color: var(--text-muted);
  }
</style>
