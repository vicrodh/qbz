<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { ArrowLeft, Loader2, Music, Search, X, LayoutGrid, PanelLeftClose, Mic2, Disc3, ChevronDown, Filter, SlidersHorizontal, Globe, Share2 } from 'lucide-svelte';
  import VirtualizedFavoritesArtistGrid from '../VirtualizedFavoritesArtistGrid.svelte';
  import VirtualizedFavoritesArtistList from '../VirtualizedFavoritesArtistList.svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import { categorizeAlbum } from '$lib/adapters/qobuzAdapters';
  import { resolveArtistImage } from '$lib/stores/customArtistImageStore';
  import type { QobuzAlbum } from '$lib/types';

  interface LocationContext {
    sourceArtistMbid: string;
    sourceArtistName: string;
    sourceArtistType: 'Person' | 'Group' | 'Other';
    location: {
      city?: string;
      areaId?: string;
      country?: string;
      countryCode?: string;
      displayName: string;
      precision: 'city' | 'state' | 'country';
    };
    affinitySeeds: {
      genres: string[];
      tags: string[];
      normalizedSeeds: string[];
    };
  }

  interface LocationCandidate {
    mbid: string;
    mb_name: string;
    qobuz_id?: number;
    qobuz_name?: string;
    qobuz_image?: string;
    score: number;
    genres: string[];
    qobuz_albums_count?: number;
  }

  interface LocationDiscoveryResponse {
    artists: LocationCandidate[];
    scene_label: string;
    genre_summary: string;
    total_candidates: number;
    has_more: boolean;
    next_offset: number;
  }

  interface FavoriteArtist {
    id: number;
    name: string;
    image?: { small?: string; thumbnail?: string; large?: string };
    albums_count?: number;
  }

  interface ArtistGroup {
    key: string;
    id: string;
    artists: FavoriteArtist[];
  }

  interface Props {
    context: LocationContext;
    onBack: () => void;
    onArtistClick: (artistId: number, mbid?: string) => void;
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
  }

  let { context, onBack, onArtistClick, onAlbumClick, onAlbumPlay }: Props = $props();

  // Discovery state
  let loading = $state(true);
  let error = $state<string | null>(null);
  let artists = $state<LocationCandidate[]>([]);
  let sceneLabel = $state('');
  let genreSummary = $state('');
  let totalCandidates = $state(0);
  let hasMore = $state(false);
  let nextOffset = $state(0);
  let loadingMore = $state(false);

  // View state
  type ViewMode = 'grid' | 'sidepanel';
  let viewMode = $state<ViewMode>('grid');
  let searchQuery = $state('');
  let searchExpanded = $state(false);
  let groupingEnabled = $state(false);
  let showGroupMenu = $state(false);
  let activeGenreFilters = $state<Set<string>>(new Set());
  let showGenrePopup = $state(false);
  let genreSearchQuery = $state('');
  let genreFilterBtnEl = $state<HTMLButtonElement | null>(null);
  let heroScrolledPast = $state(false);
  let genrePopupEl = $state<HTMLDivElement | null>(null);

  // Sidepanel state
  let selectedArtist = $state<FavoriteArtist | null>(null);
  let selectedArtistAlbums = $state<QobuzAlbum[]>([]);
  let loadingAlbums = $state(false);
  let albumsError = $state<string | null>(null);

  // Loading progress state
  let loadingProgress = $state(0);
  let loadingPhase = $state('searching');
  let loadingDetail = $state('');
  let loadingIconIndex = $state(0);
  let iconCycleTimer: ReturnType<typeof setInterval> | null = null;
  let progressUnlisten: (() => void) | null = null;

  // Icon cycle: Music → Globe → Share2 (network-like)
  const LOADING_ICONS = ['music', 'globe', 'network'] as const;

  function startLoadingAnimation() {
    loadingIconIndex = 0;
    iconCycleTimer = setInterval(() => {
      loadingIconIndex = (loadingIconIndex + 1) % LOADING_ICONS.length;
    }, 1500);
  }

  function stopLoadingAnimation() {
    if (iconCycleTimer) {
      clearInterval(iconCycleTimer);
      iconCycleTimer = null;
    }
  }

  async function setupProgressListener() {
    progressUnlisten = await listen<{ phase: string; progress: number; detail: string }>('scene-discovery-progress', (event) => {
      loadingProgress = event.payload.progress;
      loadingPhase = event.payload.phase;
      loadingDetail = event.payload.detail;
    });
  }

  function cleanupProgressListener() {
    if (progressUnlisten) {
      progressUnlisten();
      progressUnlisten = null;
    }
  }

  function getPhaseLabel(): string {
    switch (loadingPhase) {
      case 'searching':
        return $t('artist.sceneStep1', { values: { genre: loadingDetail || 'music' } });
      case 'validating':
        return $t('artist.sceneStep3');
      case 'done':
        return $t('artist.sceneStep4');
      default:
        return $t('artist.sceneStep1', { values: { genre: 'music' } });
    }
  }

  // Flag URL from circle-flags CDN
  const flagUrl = $derived(
    context.location.countryCode
      ? `https://hatscripts.github.io/circle-flags/flags/${context.location.countryCode}.svg`
      : null
  );

  // Convert candidates to FavoriteArtist format
  function candidatesToFavoriteArtists(candidates: LocationCandidate[]): FavoriteArtist[] {
    return candidates
      .filter((candidate) => candidate.qobuz_id != null)
      .map((candidate) => {
        const name = candidate.qobuz_name || candidate.mb_name;
        const defaultUrl = candidate.qobuz_image || '';
        const resolved = resolveArtistImage(name, defaultUrl);
        return {
          id: candidate.qobuz_id!,
          name,
          image: resolved ? { small: resolved } : undefined,
          albums_count: candidate.qobuz_albums_count,
        };
      });
  }

  // Lookup: qobuz_id -> mbid for passing correct MBID on click
  let mbidByQobuzId = $derived.by(() => {
    const map = new Map<number, string>();
    for (const candidate of artists) {
      if (candidate.qobuz_id != null) {
        map.set(candidate.qobuz_id, candidate.mbid);
      }
    }
    return map;
  });

  // Lookup: qobuz_id -> genres for filtering
  let genresByQobuzId = $derived.by(() => {
    const map = new Map<number, string[]>();
    for (const candidate of artists) {
      if (candidate.qobuz_id != null) {
        map.set(candidate.qobuz_id, candidate.genres);
      }
    }
    return map;
  });

  // All unique genres across results, sorted by frequency (most common first)
  let availableGenres = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const candidate of artists) {
      for (const genre of candidate.genres) {
        counts.set(genre, (counts.get(genre) || 0) + 1);
      }
    }
    return [...counts.entries()]
      .sort((a, b) => b[1] - a[1])
      .map(([genre]) => genre);
  });

  function toggleGenreFilter(genre: string) {
    const next = new Set(activeGenreFilters);
    if (next.has(genre)) {
      next.delete(genre);
    } else {
      next.add(genre);
    }
    activeGenreFilters = next;
  }

  function clearGenreFilters() {
    activeGenreFilters = new Set();
  }

  // Genres filtered by search in popup
  let filteredGenres = $derived.by(() => {
    if (!genreSearchQuery.trim()) return availableGenres;
    const q = genreSearchQuery.toLowerCase();
    return availableGenres.filter((genre) => genre.toLowerCase().includes(q));
  });

  function handleGenrePopupClickOutside(e: MouseEvent) {
    if (!showGenrePopup) return;
    const target = e.target as HTMLElement;
    if (target.closest('.genre-popup') || target.closest('.genre-filter-trigger')) return;
    showGenrePopup = false;
    genreSearchQuery = '';
  }

  // Client-side search + genre filter
  let allArtists = $derived(candidatesToFavoriteArtists(artists));
  let filteredArtists = $derived.by(() => {
    let result = allArtists;

    // Genre filter: artist must have ALL selected genres
    if (activeGenreFilters.size > 0) {
      result = result.filter((artist) => {
        const artistGenres = genresByQobuzId.get(artist.id) || [];
        return [...activeGenreFilters].every((g) => artistGenres.includes(g));
      });
    }

    // Text search
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      result = result.filter((artist) => artist.name.toLowerCase().includes(query));
    }

    return result;
  });

  // Grouping
  const ALPHA_LETTERS = '#ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('');

  function alphaGroupKey(name: string): string {
    const first = name.charAt(0).toUpperCase();
    return /[A-Z]/.test(first) ? first : '#';
  }

  function groupArtists(items: FavoriteArtist[]): ArtistGroup[] {
    const sorted = [...items].sort((a, b) => a.name.localeCompare(b.name));
    const groups = new Map<string, FavoriteArtist[]>();
    for (const artist of sorted) {
      const key = alphaGroupKey(artist.name);
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key)?.push(artist);
    }
    const keys = [...groups.keys()].sort((a, b) => {
      if (a === '#') return -1;
      if (b === '#') return 1;
      return a.localeCompare(b);
    });
    return keys.map((key) => ({
      key,
      id: `scene-alpha-${key}`,
      artists: groups.get(key) ?? [],
    }));
  }

  let groups = $derived.by(() => {
    if (groupingEnabled) return groupArtists(filteredArtists);
    return [{ key: '', id: 'scene-all', artists: filteredArtists }];
  });

  // Alpha index for jump nav
  let alphaGroupKeys = $derived(new Set(groups.map((g) => g.key)));

  // Jump-nav scroll target
  let scrollToGroupId = $state<string | undefined>(undefined);

  function scrollToGroup(letter: string) {
    if (!alphaGroupKeys.has(letter)) return;
    scrollToGroupId = `scene-alpha-${letter}`;
    // Reset after a tick to allow re-triggering the same letter
    setTimeout(() => { scrollToGroupId = undefined; }, 100);
  }

  // Album categorization for sidepanel
  let groupedAlbums = $derived.by(() => {
    if (!selectedArtist) return { discography: [], epsSingles: [], liveAlbums: [] };
    const discography: QobuzAlbum[] = [];
    const epsSingles: QobuzAlbum[] = [];
    const liveAlbums: QobuzAlbum[] = [];
    for (const album of selectedArtistAlbums) {
      const category = categorizeAlbum(album, selectedArtist.id);
      switch (category) {
        case 'albums': discography.push(album); break;
        case 'eps': epsSingles.push(album); break;
        case 'live': liveAlbums.push(album); break;
      }
    }
    return { discography, epsSingles, liveAlbums };
  });

  let totalDisplayedAlbums = $derived(
    groupedAlbums.discography.length + groupedAlbums.epsSingles.length + groupedAlbums.liveAlbums.length
  );

  // Sidepanel artist select
  async function handleArtistSelect(artist: FavoriteArtist) {
    selectedArtist = artist;
    selectedArtistAlbums = [];
    loadingAlbums = true;
    albumsError = null;

    try {
      const result = await invoke<{ items: QobuzAlbum[] }>('get_artist_albums', {
        artistId: artist.id,
        limit: 500,
        offset: 0,
      });
      selectedArtistAlbums = result.items || [];
    } catch (err) {
      console.error('[SceneView] Failed to load artist albums:', err);
      albumsError = String(err);
    } finally {
      loadingAlbums = false;
    }
  }

  async function discoverArtists(offset: number = 0) {
    try {
      const response: LocationDiscoveryResponse = await invoke('v2_discover_artists_by_location', {
        sourceMbid: context.sourceArtistMbid,
        areaId: context.location.areaId,
        areaName: context.location.city || context.location.displayName,
        country: context.location.country || null,
        genres: context.affinitySeeds.genres,
        tags: context.affinitySeeds.tags,
        limit: 100,
        offset,
      });

      if (offset === 0) {
        artists = response.artists;
      } else {
        const existingIds = new Set(artists.map((a) => a.mbid));
        const newArtists = response.artists.filter((a) => !existingIds.has(a.mbid));
        artists = [...artists, ...newArtists];
      }
      sceneLabel = response.scene_label;
      genreSummary = response.genre_summary;
      totalCandidates = response.total_candidates;
      hasMore = response.has_more;
      nextOffset = response.next_offset;
    } catch (err) {
      console.error('[ArtistsByLocationView] Discovery failed:', err);
      error = String(err);
    }
  }

  async function loadMore() {
    if (loadingMore || !hasMore) return;
    loadingMore = true;
    loadingProgress = 0;
    loadingPhase = 'searching';
    await setupProgressListener();
    await discoverArtists(nextOffset);
    cleanupProgressListener();
    loadingMore = false;
  }

  onMount(async () => {
    loading = true;
    error = null;
    loadingProgress = 0;
    loadingPhase = 'searching';
    await setupProgressListener();
    startLoadingAnimation();
    await discoverArtists();
    stopLoadingAnimation();
    cleanupProgressListener();
    loading = false;
    document.addEventListener('click', handleGenrePopupClickOutside);
  });

  onDestroy(() => {
    stopLoadingAnimation();
    cleanupProgressListener();
    document.removeEventListener('click', handleGenrePopupClickOutside);
  });
</script>

<div class="scene-view">
  {#if !loading}
    <!-- Back button (always visible) -->
    <div class="top-bar">
      <button class="back-btn" onclick={onBack}>
        <ArrowLeft size={16} />
        <span>{$t('actions.back')}</span>
      </button>
    </div>

    <!-- Compact sticky header (appears when hero scrolls out) -->
    {#if heroScrolledPast}
      <div class="compact-header">
        {#if flagUrl}
          <img src={flagUrl} alt="" class="compact-flag" />
        {/if}
        <span class="compact-title">{sceneLabel || context.location.country || context.location.displayName}</span>
        {#if genreSummary}
          <span class="compact-subtitle">{genreSummary}</span>
        {/if}
      </div>
    {/if}
  {/if}

  <!-- Nav bar with search + controls -->
  {#if !loading && !error && allArtists.length > 0}
    <div class="favorites-nav">
      <div class="nav-left">
        <span class="results-count">
          {filteredArtists.length}{(searchQuery || activeGenreFilters.size > 0) ? ` / ${allArtists.length}` : ''} {$t('search.artists').toLowerCase()}
        </span>
      </div>
      <div class="nav-right">
        <!-- Search -->
        <div class="header-search">
          {#if !searchExpanded}
            <button class="search-icon-btn" onclick={() => (searchExpanded = true)} title={$t('nav.search')}>
              <Search size={16} />
            </button>
          {:else}
            <div class="search-expanded">
              <Search size={16} class="search-icon-inline" />
              <!-- svelte-ignore a11y_autofocus -->
              <input
                type="text"
                placeholder={$t('placeholders.search')}
                bind:value={searchQuery}
                class="search-input-inline"
                autofocus
              />
              <button class="search-clear-btn" onclick={() => { searchQuery = ''; searchExpanded = false; }}>
                <X size={14} />
              </button>
            </div>
          {/if}
        </div>

        <!-- Genre filter popup trigger -->
        {#if availableGenres.length > 1}
          <div class="genre-filter-container">
            <button
              class="control-btn icon-only genre-filter-trigger"
              class:active-filter={activeGenreFilters.size > 0}
              bind:this={genreFilterBtnEl}
              onclick={() => { showGenrePopup = !showGenrePopup; genreSearchQuery = ''; }}
              title={$t('genreFilter.title')}
            >
              <Filter size={16} />
              {#if activeGenreFilters.size > 0}
                <span class="filter-badge">{activeGenreFilters.size}</span>
              {/if}
            </button>

            {#if showGenrePopup}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <div class="genre-popup" bind:this={genrePopupEl} onclick={(e) => e.stopPropagation()}>
                <div class="genre-popup-header">
                  <div class="genre-popup-title">
                    <SlidersHorizontal size={16} />
                    <span>{$t('genreFilter.title')}</span>
                  </div>
                  <button class="genre-popup-close" onclick={() => { showGenrePopup = false; genreSearchQuery = ''; }}>
                    <X size={16} />
                  </button>
                </div>
                {#if availableGenres.length > 9}
                  <div class="genre-popup-search-row">
                    <Search size={14} />
                    <input
                      type="text"
                      placeholder={$t('placeholders.search')}
                      bind:value={genreSearchQuery}
                      class="genre-popup-search-input"
                    />
                    {#if genreSearchQuery}
                      <button class="genre-popup-search-clear" onclick={() => { genreSearchQuery = ''; }}>
                        <X size={12} />
                      </button>
                    {/if}
                  </div>
                {/if}
                <div class="genre-popup-grid">
                  {#each filteredGenres as genre}
                    <button
                      class="genre-card"
                      class:selected={activeGenreFilters.has(genre)}
                      onclick={() => toggleGenreFilter(genre)}
                    >
                      <span class="genre-name">{genre}</span>
                      <span class="check-circle" class:checked={activeGenreFilters.has(genre)}></span>
                    </button>
                  {/each}
                  {#if filteredGenres.length === 0}
                    <div class="genre-popup-empty">{$t('search.noResults')}</div>
                  {/if}
                </div>
                <div class="genre-popup-footer">
                  <button
                    class="genre-clear-btn"
                    onclick={() => { clearGenreFilters(); showGenrePopup = false; }}
                    disabled={activeGenreFilters.size === 0}
                  >
                    {$t('genreFilter.clearFilter')}
                  </button>
                </div>
              </div>
            {/if}
          </div>
        {/if}

        <!-- View toggle -->
        <button
          class="control-btn icon-only"
          onclick={() => {
            if (viewMode === 'grid') {
              viewMode = 'sidepanel';
            } else {
              viewMode = 'grid';
              selectedArtist = null;
            }
          }}
          title={viewMode === 'grid' ? 'Browse view' : 'Grid view'}
        >
          {#if viewMode === 'grid'}
            <PanelLeftClose size={16} />
          {:else}
            <LayoutGrid size={16} />
          {/if}
        </button>

        <!-- Group dropdown (grid mode only) -->
        {#if viewMode === 'grid'}
          <div class="dropdown-container">
            <button class="control-btn" onclick={() => (showGroupMenu = !showGroupMenu)}>
              <span>{groupingEnabled ? 'Group: A-Z' : 'Group: Off'}</span>
              <ChevronDown size={14} />
            </button>
            {#if showGroupMenu}
              <div class="dropdown-menu">
                <button
                  class="dropdown-item"
                  class:selected={!groupingEnabled}
                  onclick={() => { groupingEnabled = false; showGroupMenu = false; }}
                >
                  Off
                </button>
                <button
                  class="dropdown-item"
                  class:selected={groupingEnabled}
                  onclick={() => { groupingEnabled = true; showGroupMenu = false; }}
                >
                  Alphabetical (A-Z)
                </button>
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Content -->
  <div class="scene-content">
    {#if loading}
      <div class="scene-loading">
        <div class="loading-visual">
          <div class="loading-pulse">
            {#key loadingIconIndex}
              <span class="icon-fade">
                {#if LOADING_ICONS[loadingIconIndex] === 'music'}
                  <Music size={42} />
                {:else if LOADING_ICONS[loadingIconIndex] === 'globe'}
                  <Globe size={42} />
                {:else}
                  <Share2 size={42} />
                {/if}
              </span>
            {/key}
          </div>
        </div>

        <div class="loading-progress-bar">
          <div class="loading-progress-fill" style="width: {loadingProgress}%"></div>
        </div>

        <div class="loading-status">
          <span class="loading-text">{getPhaseLabel()}</span>
          <span class="loading-percent">{loadingProgress}%</span>
        </div>
      </div>
    {:else if error}
      <div class="scene-error">
        <p>{error}</p>
        <button class="retry-button" onclick={() => { loading = true; error = null; startLoadingAnimation(); discoverArtists().then(() => { stopLoadingAnimation(); loading = false; }); }}>
          {$t('actions.retry')}
        </button>
      </div>
    {:else if allArtists.length === 0}
      <div class="scene-empty">
        <Music size={48} />
        <p>{$t('artist.noSceneResults')}</p>
      </div>
    {:else if viewMode === 'grid'}
      <div class="grid-with-alpha">
        <div class="scene-grid-container">
          <VirtualizedFavoritesArtistGrid
            {groups}
            showGroupHeaders={groupingEnabled}
            onArtistClick={(id) => onArtistClick(id, mbidByQobuzId.get(id))}
            {scrollToGroupId}
            onScrollPastHeader={(isPast) => { heroScrolledPast = isPast; }}
          >
            {#snippet header()}
              <div class="hero-header">
                {#if flagUrl}
                  <div class="flag-wrapper">
                    <img src={flagUrl} alt="" class="flag-image" />
                  </div>
                {/if}
                <div class="hero-info">
                  <h1>{sceneLabel || context.location.country || context.location.displayName}</h1>
                  {#if genreSummary || context.affinitySeeds.genres.length > 0}
                    <p class="hero-subtitle">
                      {$t('artist.sceneBased', {
                        values: {
                          artist: context.sourceArtistName,
                          genres: genreSummary || context.affinitySeeds.genres.slice(0, 3).join(' / '),
                        },
                      })}
                    </p>
                  {/if}
                </div>
              </div>
            {/snippet}
            {#snippet footer()}
              {#if hasMore}
                <div class="load-more-inline">
                  {#if loadingMore}
                    <div class="load-more-progress">
                      <div class="load-more-bar">
                        <div class="load-more-bar-fill" style="width: {loadingProgress}%"></div>
                      </div>
                      <span class="load-more-status">{loadingProgress}%</span>
                    </div>
                  {:else}
                    <button class="load-more-button" onclick={loadMore}>
                      {$t('actions.loadMore')}
                    </button>
                  {/if}
                </div>
              {/if}
            {/snippet}
          </VirtualizedFavoritesArtistGrid>
        </div>

        <!-- Alpha jump-nav sidebar -->
        {#if groupingEnabled}
          <div class="alpha-index">
            {#each ALPHA_LETTERS as letter}
              <button
                class="alpha-letter"
                class:disabled={!alphaGroupKeys.has(letter)}
                onclick={() => scrollToGroup(letter)}
              >
                {letter}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <!-- Sidepanel mode -->
      <div class="artist-two-column-layout">
        <div class="artist-column">
          <VirtualizedFavoritesArtistList
            groups={groupArtists(filteredArtists)}
            showGroupHeaders={true}
            selectedArtistId={selectedArtist?.id ?? null}
            onArtistSelect={handleArtistSelect}
          />
        </div>

        <div class="artist-albums-column">
          {#if !selectedArtist}
            <div class="artist-albums-empty">
              <Mic2 size={48} />
              <p>{$t('favorites.selectArtistHint')}</p>
            </div>
          {:else if loadingAlbums}
            <div class="artist-albums-loading">
              <Loader2 size={32} class="spinner-icon" />
              <p>{$t('favorites.loadingAlbums')}</p>
            </div>
          {:else if albumsError}
            <div class="artist-albums-error">
              <p>{$t('favorites.failedLoadAlbums')}</p>
              <p class="error-detail">{albumsError}</p>
            </div>
          {:else if totalDisplayedAlbums === 0}
            <div class="artist-albums-empty">
              <Disc3 size={48} />
              <p>{$t('artist.noAlbumsFound')}</p>
            </div>
          {:else}
            <div class="artist-albums-scroll">
              {#if groupedAlbums.discography.length > 0}
                <div class="artist-albums-section">
                  <div class="artist-albums-section-header">
                    <span class="section-title">{$t('artist.discography')}</span>
                    <span class="section-count">{groupedAlbums.discography.length}</span>
                  </div>
                  <div class="artist-albums-grid">
                    {#each groupedAlbums.discography as album (album.id)}
                      <AlbumCard
                        albumId={album.id}
                        artwork={album.image?.small || album.image?.large || ''}
                        title={album.title}
                        artist={album.artist?.name || ''}
                        genre={album.genre?.name}
                        releaseDate={album.release_date_original}
                        onclick={() => onAlbumClick?.(album.id)}
                        onPlay={() => onAlbumPlay?.(album.id)}
                      />
                    {/each}
                  </div>
                </div>
              {/if}
              {#if groupedAlbums.epsSingles.length > 0}
                <div class="artist-albums-section">
                  <div class="artist-albums-section-header">
                    <span class="section-title">{$t('artist.epsSingles')}</span>
                    <span class="section-count">{groupedAlbums.epsSingles.length}</span>
                  </div>
                  <div class="artist-albums-grid">
                    {#each groupedAlbums.epsSingles as album (album.id)}
                      <AlbumCard
                        albumId={album.id}
                        artwork={album.image?.small || album.image?.large || ''}
                        title={album.title}
                        artist={album.artist?.name || ''}
                        genre={album.genre?.name}
                        releaseDate={album.release_date_original}
                        onclick={() => onAlbumClick?.(album.id)}
                        onPlay={() => onAlbumPlay?.(album.id)}
                      />
                    {/each}
                  </div>
                </div>
              {/if}
              {#if groupedAlbums.liveAlbums.length > 0}
                <div class="artist-albums-section">
                  <div class="artist-albums-section-header">
                    <span class="section-title">{$t('artist.liveAlbums')}</span>
                    <span class="section-count">{groupedAlbums.liveAlbums.length}</span>
                  </div>
                  <div class="artist-albums-grid">
                    {#each groupedAlbums.liveAlbums as album (album.id)}
                      <AlbumCard
                        albumId={album.id}
                        artwork={album.image?.small || album.image?.large || ''}
                        title={album.title}
                        artist={album.artist?.name || ''}
                        genre={album.genre?.name}
                        releaseDate={album.release_date_original}
                        onclick={() => onAlbumClick?.(album.id)}
                        onPlay={() => onAlbumPlay?.(album.id)}
                      />
                    {/each}
                  </div>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .scene-view {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    overflow: hidden;
    padding-top: 0;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 0;
  }

  /* Top bar */
  .top-bar {
    display: flex;
    align-items: center;
    flex-shrink: 0;
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
    margin-top: 8px;
    margin-bottom: 12px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-secondary);
  }

  /* Compact sticky header (on scroll past hero) */
  .compact-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-subtle);
    margin-bottom: 8px;
  }

  .compact-flag {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    flex-shrink: 0;
    object-fit: cover;
  }

  .compact-title {
    font-size: 16px;
    font-weight: 700;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .compact-subtitle {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Hero header with large flag (inside scroll content) */
  .hero-header {
    display: flex;
    align-items: center;
    gap: 20px;
    padding-bottom: 20px;
  }

  .flag-wrapper {
    width: 140px;
    height: 140px;
    border-radius: 50%;
    overflow: hidden;
    flex-shrink: 0;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .flag-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .hero-info {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .hero-info h1 {
    font-size: 28px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
    line-height: 1.2;
  }

  .hero-subtitle {
    font-size: 14px;
    color: var(--text-muted);
    line-height: 1.4;
    margin: 0;
  }

  /* Nav bar - matches FavoritesView */
  .favorites-nav {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
    flex-shrink: 0;
  }

  .nav-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .nav-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .results-count {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
  }

  /* Search */
  .search-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border-radius: 8px;
    border: none;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .search-icon-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .search-expanded {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-tertiary);
    border-radius: 8px;
    padding: 6px 12px;
    width: 240px;
  }

  :global(.search-expanded .search-icon-inline) {
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
    padding: 0;
    flex-shrink: 0;
  }

  .search-clear-btn:hover {
    color: var(--text-primary);
  }

  /* Control buttons - matches FavoritesView */
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
    position: relative;
    width: 36px;
    height: 36px;
    padding: 0;
    justify-content: center;
  }

  /* Dropdown - matches FavoritesView */
  .dropdown-container {
    position: relative;
  }

  .dropdown-menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 4px;
    min-width: 160px;
    z-index: 100;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    width: 100%;
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
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .dropdown-item.selected {
    color: var(--accent-primary);
    font-weight: 600;
  }

  /* Filter toggle active state */
  .control-btn.active-filter {
    color: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .filter-badge {
    position: absolute;
    top: -4px;
    right: -4px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--accent-primary);
    color: var(--bg-primary);
    font-size: 10px;
    font-weight: 700;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* Genre filter popup — matches GenreFilterPopup style */
  .genre-filter-container {
    position: relative;
  }

  .genre-popup {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    width: 530px;
    max-height: 440px;
    background: var(--bg-primary);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    z-index: 200;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .genre-popup-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-subtle);
    flex-shrink: 0;
  }

  .genre-popup-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .genre-popup-close {
    width: 28px;
    height: 28px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .genre-popup-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .genre-popup-search-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .genre-popup-search-input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    outline: none;
    min-width: 0;
  }

  .genre-popup-search-input::placeholder {
    color: var(--text-muted);
  }

  .genre-popup-search-clear {
    width: 20px;
    height: 20px;
    border: none;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    border-radius: 50%;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .genre-popup-search-clear:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .genre-popup-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
    padding: 12px;
    overflow-y: auto;
    flex: 1;
  }

  .genre-card {
    height: 36px;
    border-radius: 6px;
    border: 1px solid var(--border-subtle);
    cursor: pointer;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 10px;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease;
  }

  .genre-card:hover {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  .genre-card.selected {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .genre-card.selected:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  .genre-name {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.2;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .genre-name::first-letter {
    text-transform: capitalize;
  }

  .genre-card.selected .genre-name {
    color: white;
  }

  .check-circle {
    flex-shrink: 0;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1.5px solid var(--text-muted);
    background: transparent;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease;
    position: relative;
  }

  .check-circle.checked {
    border-color: white;
    background: white;
  }

  .check-circle.checked::after {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: 4px;
    height: 7px;
    border: solid var(--accent-primary);
    border-width: 0 1.5px 1.5px 0;
    transform: translate(-50%, -60%) rotate(45deg);
  }

  .genre-popup-footer {
    padding: 12px 16px;
    border-top: 1px solid var(--border-subtle);
    flex-shrink: 0;
  }

  .genre-clear-btn {
    width: 100%;
    padding: 8px 16px;
    border: none;
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease;
  }

  .genre-clear-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .genre-clear-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .genre-popup-empty {
    grid-column: 1 / -1;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
    padding: 16px 0;
  }

  /* Content area */
  .scene-content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  /* Grid with alpha sidebar */
  .grid-with-alpha {
    display: flex;
    flex: 1;
    min-height: 0;
    gap: 0;
  }

  .scene-grid-container {
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  /* Alpha jump-nav sidebar */
  .alpha-index {
    position: sticky;
    top: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 4px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.2);
    align-self: flex-start;
    margin-left: 4px;
    flex-shrink: 0;
  }

  .alpha-letter {
    width: 20px;
    height: 20px;
    padding: 0;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    opacity: 0.9;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    transition: all 100ms ease;
  }

  .alpha-letter:hover:not(.disabled) {
    background: var(--bg-tertiary);
    opacity: 1;
  }

  .alpha-letter.disabled {
    opacity: 0.2;
    cursor: default;
  }

  /* Two-column sidepanel layout - matches FavoritesView */
  .artist-two-column-layout {
    display: flex;
    gap: 0;
    flex: 1;
    min-height: 0;
    margin: 0 -8px 0 -18px;
    padding: 0;
  }

  .artist-column {
    width: 240px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: transparent;
    border-right: 1px solid var(--bg-tertiary);
    overflow: hidden;
    padding-left: 18px;
  }

  .artist-albums-column {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
    padding: 0 8px 0 24px;
  }

  .artist-albums-scroll {
    flex: 1;
    overflow-y: auto;
    padding-right: 8px;
    -webkit-overflow-scrolling: touch;
    overscroll-behavior: contain;
    contain: strict;
  }

  .artist-albums-section {
    margin-bottom: 32px;
  }

  .artist-albums-section:last-of-type {
    margin-bottom: 16px;
  }

  .artist-albums-section-header {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 16px;
  }

  .section-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .section-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .artist-albums-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
    gap: 22px;
    align-content: start;
  }

  .artist-albums-empty,
  .artist-albums-loading,
  .artist-albums-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 100%;
    color: var(--text-muted);
    font-size: 14px;
  }

  .error-detail {
    font-size: 12px;
    opacity: 0.7;
  }

  :global(.artist-albums-loading .spinner-icon) {
    animation: spin 1s linear infinite;
  }

  /* Loading state */
  .scene-loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 24px;
    flex: 1;
  }

  .loading-visual {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .loading-pulse {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 96px;
    height: 96px;
    border-radius: 50%;
    background: var(--bg-secondary);
    color: var(--accent-primary);
    animation: pulse 2s ease-in-out infinite;
  }

  .icon-fade {
    display: flex;
    align-items: center;
    justify-content: center;
    animation: iconFadeIn 400ms ease-out;
  }

  @keyframes iconFadeIn {
    from { opacity: 0; transform: scale(0.8); }
    to { opacity: 1; transform: scale(1); }
  }

  @keyframes pulse {
    0%, 100% { transform: scale(1); opacity: 0.8; }
    50% { transform: scale(1.06); opacity: 1; }
  }

  .loading-progress-bar {
    width: 280px;
    height: 4px;
    border-radius: 2px;
    background: var(--bg-tertiary);
    overflow: hidden;
  }

  .loading-progress-fill {
    height: 100%;
    border-radius: 2px;
    background: var(--accent-primary);
    transition: width 300ms ease-out;
  }

  .loading-status {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .loading-text {
    font-size: 14px;
    color: var(--text-secondary);
    text-align: center;
    line-height: 1.4;
  }

  .loading-percent {
    font-size: 12px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  /* Error and empty states */
  .scene-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 80px 0;
    color: var(--text-muted);
    font-size: 14px;
  }

  .retry-button {
    padding: 8px 20px;
    border-radius: 8px;
    border: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    transition: background-color 150ms ease;
  }

  .retry-button:hover {
    background: var(--bg-tertiary);
  }

  .scene-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    padding: 80px 0;
    color: var(--text-muted);
    font-size: 14px;
  }

  /* Load more (inline at bottom of scroll content) */
  .load-more-inline {
    display: flex;
    justify-content: center;
    padding: 16px 0 24px;
  }

  .load-more-button {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 24px;
    border-radius: 8px;
    border: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
    transition: background-color 150ms ease;
  }

  .load-more-button:hover {
    background: var(--bg-tertiary);
  }

  .load-more-progress {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .load-more-bar {
    width: 200px;
    height: 4px;
    border-radius: 2px;
    background: var(--bg-tertiary);
    overflow: hidden;
  }

  .load-more-bar-fill {
    height: 100%;
    border-radius: 2px;
    background: var(--accent-primary);
    transition: width 300ms ease-out;
  }

  .load-more-status {
    font-size: 12px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    min-width: 32px;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
