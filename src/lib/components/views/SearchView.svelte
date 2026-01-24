<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Search, Disc3, Music, Mic2, User, X, ChevronLeft, ChevronRight, Crown } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackMenu from '../TrackMenu.svelte';
  import { getSearchState, setSearchState, type SearchResults, type SearchAllResults, type SearchTab } from '$lib/stores/searchState';
  import { setPlaybackContext } from '$lib/stores/playbackContextStore';
  import { togglePlay } from '$lib/stores/playerStore';
  import { t } from '$lib/i18n';

  let searchInput: HTMLInputElement | null = null;
  let albumsCarouselContainer: HTMLDivElement | null = null;
  let artistsCarouselContainer: HTMLDivElement | null = null;
  let currentAlbumPage = $state(0);
  let currentArtistPage = $state(0);
  let albumsPerPage = $state(5);
  let artistsPerPage = $state(5);
  let totalAlbumPages = $derived(allResults ? Math.ceil(allResults.albums.items.length / albumsPerPage) : 0);
  let totalArtistPages = $derived(allResults ? Math.ceil(allResults.artists.items.length / artistsPerPage) : 0);

  onMount(async () => {
    console.log('SearchView mounted!');
    await tick();
    searchInput?.focus();
    calculateAlbumsPerPage();
    calculateArtistsPerPage();
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  });

  function handleResize() {
    calculateAlbumsPerPage();
    calculateArtistsPerPage();
  }

  function calculateAlbumsPerPage() {
    if (!albumsCarouselContainer) return;
    const containerWidth = albumsCarouselContainer.clientWidth;
    const gap = 16;
    const cardWidth = 160;
    const cols = Math.floor((containerWidth + gap) / (cardWidth + gap));
    albumsPerPage = Math.max(2, cols);
    console.log(`Albums - Container width: ${containerWidth}px, Albums per page: ${albumsPerPage}`);
  }

  function calculateArtistsPerPage() {
    if (!artistsCarouselContainer) return;
    const containerWidth = artistsCarouselContainer.clientWidth;
    const gap = 16;
    const cardWidth = 160;
    const cols = Math.floor((containerWidth + gap) / (cardWidth + gap));
    artistsPerPage = Math.max(2, cols);
    console.log(`Artists - Container width: ${containerWidth}px, Artists per page: ${artistsPerPage}`);
  }

  // Track which images have failed to load
  let failedTrackImages = $state<Set<number>>(new Set());
  let failedArtistImages = $state<Set<number>>(new Set());

  function handleTrackImageError(trackId: number) {
    failedTrackImages = new Set([...failedTrackImages, trackId]);
  }

  function handleArtistImageError(artistId: number) {
    failedArtistImages = new Set([...failedArtistImages, artistId]);
  }

  interface Props {
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    checkAlbumFullyDownloaded?: (albumId: string) => Promise<boolean>;
    downloadStateVersion?: number;
    onTrackPlay?: (track: Track) => void;
    onTrackPlayNext?: (track: Track) => void;
    onTrackPlayLater?: (track: Track) => void;
    onTrackAddFavorite?: (trackId: number) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: Track) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
    onTrackDownload?: (track: Track) => void;
    onTrackRemoveDownload?: (trackId: number) => void;
    onTrackReDownload?: (track: Track) => void;
    checkTrackDownloaded?: (trackId: number) => boolean;
    onArtistClick?: (artistId: number) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
  }

  let {
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    checkAlbumFullyDownloaded,
    downloadStateVersion,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddFavorite,
    onTrackAddToPlaylist,
    onAddAlbumToPlaylist,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onTrackDownload,
    onTrackRemoveDownload,
    onTrackReDownload,
    checkTrackDownloaded,
    onArtistClick,
    activeTrackId = null,
    isPlaybackActive = false
  }: Props = $props();

  interface Album {
    id: string;
    title: string;
    artist: { name: string };
    image: { small?: string; thumbnail?: string; large?: string };
    release_date_original?: string;
    hires_streamable?: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
  }

  interface Track {
    id: number;
    title: string;
    duration: number;
    album?: {
      id?: string;
      title: string;
      image?: { small?: string; thumbnail?: string; large?: string };
    };
    performer?: { id?: number; name: string };
    hires_streamable?: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    isrc?: string;
  }

  interface Artist {
    id: number;
    name: string;
    image?: { small?: string; thumbnail?: string; large?: string };
    albums_count?: number;
  }

  const cachedState = getSearchState<Album, Track, Artist>();

  let query = $state(cachedState.query ?? '');
  let activeTab = $state<SearchTab>(cachedState.activeTab ?? 'all');
  let isSearching = $state(false);
  let searchError = $state<string | null>(null);

  let albumResults = $state<SearchResults<Album> | null>(cachedState.albumResults ?? null);
  let trackResults = $state<SearchResults<Track> | null>(cachedState.trackResults ?? null);
  let artistResults = $state<SearchResults<Artist> | null>(cachedState.artistResults ?? null);
  let allResults = $state<SearchAllResults<Album, Track, Artist> | null>(cachedState.allResults ?? null);

  let searchTimeout: ReturnType<typeof setTimeout> | null = null;
  let isLoadingMore = $state(false);
  const PAGE_SIZE = 20;

  // Check if there are more results to load
  let hasMoreAlbums = $derived(albumResults ? albumResults.offset + albumResults.items.length < albumResults.total : false);
  let hasMoreTracks = $derived(trackResults ? trackResults.offset + trackResults.items.length < trackResults.total : false);
  let hasMoreArtists = $derived(artistResults ? artistResults.offset + artistResults.items.length < artistResults.total : false);

  function debounceSearch() {
    if (searchTimeout) clearTimeout(searchTimeout);
    if (query.trim().length < 2) {
      albumResults = null;
      trackResults = null;
      artistResults = null;
      return;
    }
    searchTimeout = setTimeout(() => performSearch(), 300);
  }

  function clearSearch() {
    if (searchTimeout) {
      clearTimeout(searchTimeout);
      searchTimeout = null;
    }
    query = '';
    searchError = null;
    isSearching = false;
    albumResults = null;
    trackResults = null;
    artistResults = null;
    allResults = null;
  }

  $effect(() => {
    if (allResults) {
      currentAlbumPage = 0;
      currentArtistPage = 0;
      setTimeout(() => {
        calculateAlbumsPerPage();
        calculateArtistsPerPage();
      }, 100);
    }
  });

  $effect(() => {
    setSearchState<Album, Track, Artist>({
      query,
      activeTab,
      albumResults,
      trackResults,
      artistResults,
      allResults
    });
  });

  // Download status tracking
  let albumDownloadStatuses = $state<Map<string, boolean>>(new Map());
  let downloadStatusTick = $state(0);

  async function loadAlbumDownloadStatus(albumId: string) {
    if (!checkAlbumFullyDownloaded) return false;
    try {
      const isDownloaded = await checkAlbumFullyDownloaded(albumId);
      albumDownloadStatuses.set(albumId, isDownloaded);
      downloadStatusTick++;
      return isDownloaded;
    } catch {
      return false;
    }
  }

  async function loadAllAlbumDownloadStatuses(albums: { id: string }[]) {
    if (!checkAlbumFullyDownloaded || albums.length === 0) return;
    await Promise.all(albums.map(album => loadAlbumDownloadStatus(album.id)));
  }

  function isAlbumDownloaded(albumId: string): boolean {
    void downloadStateVersion;
    void downloadStatusTick;
    return albumDownloadStatuses.get(albumId) || false;
  }

  $effect(() => {
    if (downloadStateVersion !== undefined && albumResults) {
      loadAllAlbumDownloadStatuses(albumResults.items);
    }
  });

  async function performSearch() {
    if (!query.trim()) return;

    isSearching = true;
    searchError = null;

    try {
      // Search based on active tab - reset to first page
      if (activeTab === 'all') {
        allResults = await invoke<SearchAllResults<Album, Track, Artist>>('search_all', {
          query: query.trim()
        });
        console.log('All results:', allResults);
        if (allResults && allResults.albums.items) {
          await loadAllAlbumDownloadStatuses(allResults.albums.items);
        }
      } else if (activeTab === 'albums') {
        albumResults = await invoke<SearchResults<Album>>('search_albums', {
          query: query.trim(),
          limit: PAGE_SIZE,
          offset: 0
        });
        console.log('Album results:', albumResults);
        if (albumResults && albumResults.items) {
          await loadAllAlbumDownloadStatuses(albumResults.items);
        }
      } else if (activeTab === 'tracks') {
        trackResults = await invoke<SearchResults<Track>>('search_tracks', {
          query: query.trim(),
          limit: PAGE_SIZE,
          offset: 0
        });
        console.log('Track results:', trackResults);
      } else if (activeTab === 'artists') {
        artistResults = await invoke<SearchResults<Artist>>('search_artists', {
          query: query.trim(),
          limit: PAGE_SIZE,
          offset: 0
        });
        console.log('Artist results:', artistResults);
      }
    } catch (err) {
      console.error('Search error:', err);
      searchError = String(err);
    } finally {
      isSearching = false;
    }
  }

  async function loadMore() {
    if (!query.trim() || isLoadingMore) return;

    isLoadingMore = true;

    try {
      if (activeTab === 'albums' && albumResults && hasMoreAlbums) {
        const newOffset = albumResults.offset + albumResults.items.length;
        const moreResults = await invoke<SearchResults<Album>>('search_albums', {
          query: query.trim(),
          limit: PAGE_SIZE,
          offset: newOffset
        });
        await loadAllAlbumDownloadStatuses(moreResults.items);
        albumResults = {
          ...moreResults,
          items: [...albumResults.items, ...moreResults.items],
          offset: 0 // Keep offset at 0 since we're accumulating
        };
      } else if (activeTab === 'tracks' && trackResults && hasMoreTracks) {
        const newOffset = trackResults.offset + trackResults.items.length;
        const moreResults = await invoke<SearchResults<Track>>('search_tracks', {
          query: query.trim(),
          limit: PAGE_SIZE,
          offset: newOffset
        });
        trackResults = {
          ...moreResults,
          items: [...trackResults.items, ...moreResults.items],
          offset: 0
        };
      } else if (activeTab === 'artists' && artistResults && hasMoreArtists) {
        const newOffset = artistResults.offset + artistResults.items.length;
        const moreResults = await invoke<SearchResults<Artist>>('search_artists', {
          query: query.trim(),
          limit: PAGE_SIZE,
          offset: newOffset
        });
        artistResults = {
          ...moreResults,
          items: [...artistResults.items, ...moreResults.items],
          offset: 0
        };
      }
    } catch (err) {
      console.error('Load more error:', err);
    } finally {
      isLoadingMore = false;
    }
  }

  function handleTabChange(tab: typeof activeTab) {
    activeTab = tab;
    if (query.trim().length >= 2) {
      performSearch();
    }
  }

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function getQualityLabel(track: Track | Album): string {
    if (track.hires_streamable && track.maximum_bit_depth && track.maximum_sampling_rate) {
      return `${track.maximum_bit_depth}bit/${track.maximum_sampling_rate}kHz`;
    }
    return 'CD Quality';
  }

  function getAlbumArtwork(album: Album): string {
    return album.image?.large || album.image?.thumbnail || album.image?.small || '';
  }

  function getTrackArtwork(track: Track): string {
    return track.album?.image?.large || track.album?.image?.thumbnail || track.album?.image?.small || '';
  }

  function buildSearchQueueTracks(tracks: Track[]) {
    return tracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.performer?.name || 'Unknown Artist',
      album: t.album?.title || '',
      duration_secs: t.duration,
      artwork_url: t.album?.image?.large || t.album?.image?.thumbnail || t.album?.image?.small || '',
      hires: t.hires_streamable ?? false,
      bit_depth: t.maximum_bit_depth ?? null,
      sample_rate: t.maximum_sampling_rate ?? null,
      is_local: false,
      album_id: t.album?.id || null,
      artist_id: t.performer?.id ?? null,
    }));
  }

  async function handleSearchTrackPlay(track: Track, trackIndex: number) {
    // Create search results context
    if (trackResults && trackResults.items.length > 0) {
      const trackIds = trackResults.items.map(t => t.id);

      await setPlaybackContext(
        'home_list', // Using home_list for search results (search type doesn't exist yet)
        query,
        `Search: ${query}`,
        'qobuz',
        trackIds,
        trackIndex
      );
      console.log(`[Search] Context created: "${query}", ${trackIds.length} tracks, starting at ${trackIndex}`);
    }

    if (trackResults && trackResults.items.length > 0) {
      try {
        const queueTracks = buildSearchQueueTracks(trackResults.items);
        await invoke('set_queue', { tracks: queueTracks, startIndex: trackIndex });
      } catch (err) {
        console.error('Failed to set queue:', err);
      }
    }

    // Play track
    if (onTrackPlay) {
      onTrackPlay(track);
    }
  }

  function handlePausePlayback(event: MouseEvent) {
    event.stopPropagation();
    void togglePlay();
  }

  function getArtistImage(artist: Artist): string {
    return artist.image?.large || artist.image?.thumbnail || artist.image?.small || '';
  }

  function scrollAlbumsCarousel(direction: 'left' | 'right') {
    if (direction === 'left') {
      currentAlbumPage = Math.max(0, currentAlbumPage - 1);
    } else {
      currentAlbumPage = Math.min(totalAlbumPagesWithViewMore - 1, currentAlbumPage + 1);
    }
  }

  function scrollArtistsCarousel(direction: 'left' | 'right') {
    if (direction === 'left') {
      currentArtistPage = Math.max(0, currentArtistPage - 1);
    } else {
      currentArtistPage = Math.min(totalArtistPagesWithViewMore - 1, currentArtistPage + 1);
    }
  }

  let canScrollAlbumsLeft = $derived(currentAlbumPage > 0);
  let canScrollAlbumsRight = $derived(currentAlbumPage < totalAlbumPagesWithViewMore - 1);
  let canScrollArtistsLeft = $derived(currentArtistPage > 0);
  let canScrollArtistsRight = $derived(currentArtistPage < totalArtistPagesWithViewMore - 1);

  let showAlbumsViewMore = $derived(allResults ? allResults.albums.total > 30 : false);
  let showArtistsViewMore = $derived(allResults ? allResults.artists.total > 12 : false);
  
  let albumsWithViewMore = $derived(() => {
    if (!allResults) return [];
    const albums = [...allResults.albums.items];
    if (showAlbumsViewMore) {
      albums.push({ id: 'view-more', isViewMore: true } as any);
    }
    return albums;
  });

  let artistsWithViewMore = $derived(() => {
    if (!allResults) return [];
    const artists = [...allResults.artists.items];
    if (showArtistsViewMore) {
      artists.push({ id: 'view-more', isViewMore: true } as any);
    }
    return artists;
  });

  let totalAlbumPagesWithViewMore = $derived(albumsWithViewMore().length > 0 ? Math.ceil(albumsWithViewMore().length / albumsPerPage) : 0);
  let totalArtistPagesWithViewMore = $derived(artistsWithViewMore().length > 0 ? Math.ceil(artistsWithViewMore().length / artistsPerPage) : 0);
  
  let visibleAlbums = $derived(
    albumsWithViewMore().slice(currentAlbumPage * albumsPerPage, (currentAlbumPage + 1) * albumsPerPage)
  );

  let visibleArtists = $derived(
    artistsWithViewMore().slice(currentArtistPage * artistsPerPage, (currentArtistPage + 1) * artistsPerPage)
  );
</script>

<div class="search-view">
  <!-- Search Header -->
  <div class="search-header">
    <h1>{$t('search.title')}</h1>
    <div class="search-input-container">
      <Search size={20} class="search-icon" />
      <input
        type="text"
        placeholder={$t('search.placeholder')}
        bind:value={query}
        oninput={debounceSearch}
        class="search-input"
        bind:this={searchInput}
      />
      {#if query.trim()}
        <button class="search-clear" onclick={clearSearch} aria-label={$t('actions.clear')}>
          <X size={18} />
        </button>
      {/if}
    </div>
  </div>

  <!-- Tabs -->
  <div class="tabs">
    <button
      class="tab"
      class:active={activeTab === 'all'}
      onclick={() => handleTabChange('all')}
    >
      <Search size={18} />
      <span>All</span>
    </button>
    <button
      class="tab"
      class:active={activeTab === 'albums'}
      onclick={() => handleTabChange('albums')}
    >
      <Disc3 size={18} />
      <span>{$t('search.albums')}</span>
      {#if albumResults}
        <span class="count">{albumResults.total}</span>
      {:else if allResults}
        <span class="count">{allResults.albums.total}</span>
      {/if}
    </button>
    <button
      class="tab"
      class:active={activeTab === 'tracks'}
      onclick={() => handleTabChange('tracks')}
    >
      <Music size={18} />
      <span>{$t('search.tracks')}</span>
      {#if trackResults}
        <span class="count">{trackResults.total}</span>
      {:else if allResults}
        <span class="count">{allResults.tracks.total}</span>
      {/if}
    </button>
    <button
      class="tab"
      class:active={activeTab === 'artists'}
      onclick={() => handleTabChange('artists')}
    >
      <Mic2 size={18} />
      <span>{$t('search.artists')}</span>
      {#if artistResults}
        <span class="count">{artistResults.total}</span>
      {:else if allResults}
        <span class="count">{allResults.artists.total}</span>
      {/if}
    </button>
  </div>

  <!-- Results -->
  <div class="results">
    {#if isSearching}
      <div class="loading">
        <div class="spinner"></div>
        <p>{$t('search.searching')}</p>
      </div>
    {:else if searchError}
      <div class="error">
        <p>{$t('errors.loadFailed')}</p>
        <p class="error-detail">{searchError}</p>
      </div>
    {:else if !query.trim()}
      <div class="empty-state">
        <Search size={48} />
        <p>{$t('search.startTyping')}</p>
      </div>
    {:else if activeTab === 'all' && allResults}
      <!-- Unified Results View -->
      <div class="unified-results">
        <!-- Most Popular + Artists Section -->
        <div class="top-section">
          <div class="most-popular">
            <div class="section-header">
              <h3><Crown size={18} color="gold" /> Most Popular</h3>
            </div>
            <div class="most-popular-wrapper">
              {#if allResults.artists.items.length > 0}
              <button class="artist-card most-popular-card" onclick={() => onArtistClick?.(allResults.artists.items[0].id)}>
                <div class="artist-image-wrapper">
                  <!-- Placeholder always visible as background -->
                  <div class="artist-image-placeholder">
                    <User size={40} />
                  </div>
                  <!-- Image overlays placeholder when loaded -->
                  {#if !failedArtistImages.has(allResults.artists.items[0].id) && getArtistImage(allResults.artists.items[0])}
                    <img
                      src={getArtistImage(allResults.artists.items[0])}
                      alt={allResults.artists.items[0].name}
                      class="artist-image"
                      loading="lazy"
                      decoding="async"
                      onerror={() => handleArtistImageError(allResults.artists.items[0].id)}
                    />
                  {/if}
                </div>
                <div class="artist-name">{allResults.artists.items[0].name}</div>
                {#if allResults.artists.items[0].albums_count}
                  <div class="artist-albums">{$t('library.albumCount', { values: { count: allResults.artists.items[0].albums_count } })}</div>
                {/if}
              </button>
            {:else if allResults.albums.items.length > 0}
              <AlbumCard
                albumId={allResults.albums.items[0].id}
                artwork={getAlbumArtwork(allResults.albums.items[0])}
                title={allResults.albums.items[0].title}
                artist={allResults.albums.items[0].artist?.name || 'Unknown Artist'}
                quality={getQualityLabel(allResults.albums.items[0])}
                onPlay={onAlbumPlay ? () => onAlbumPlay(allResults.albums.items[0].id) : undefined}
                onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(allResults.albums.items[0].id) : undefined}
                onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(allResults.albums.items[0].id) : undefined}
                onAddAlbumToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist(album.id) : undefined}
                onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(allResults.albums.items[0].id) : undefined}
                onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(allResults.albums.items[0].id) : undefined}
                onDownload={onAlbumDownload ? () => onAlbumDownload(allResults.albums.items[0].id) : undefined}
                isAlbumFullyDownloaded={isAlbumDownloaded(allResults.albums.items[0].id)}
                onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(allResults.albums.items[0].id) : undefined}
                onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(allResults.albums.items[0].id) : undefined}
                {downloadStateVersion}
                onclick={() => { onAlbumClick?.(allResults.albums.items[0].id); loadAlbumDownloadStatus(allResults.albums.items[0].id); }}
              />
            {/if}
            </div>
          </div>

          <div class="artists-section">
            <div class="section-header">
              <h3>Artists</h3>
              <div class="carousel-controls">
                <button 
                  class="carousel-btn" 
                  onclick={() => scrollArtistsCarousel('left')} 
                  disabled={!canScrollArtistsLeft}
                  aria-label="Previous artists"
                >
                  <ChevronLeft size={20} />
                </button>
                <button 
                  class="carousel-btn" 
                  onclick={() => scrollArtistsCarousel('right')} 
                  disabled={!canScrollArtistsRight}
                  aria-label="Next artists"
                >
                  <ChevronRight size={20} />
                </button>
                <button class="view-all-link" onclick={() => handleTabChange('artists')}>
                  View all ({allResults.artists.total})
                </button>
              </div>
            </div>
            <div class="artists-carousel-wrapper" bind:this={artistsCarouselContainer}>
              <div class="artists-carousel">
                {#each visibleArtists as artist}
                  {#if artist.isViewMore}
                    <div class="view-more-card">
                      <button class="view-more-cover" onclick={() => handleTabChange('artists')}>
                        <div class="view-more-label">
                          <span>View more</span>
                          <ChevronRight size={20} />
                        </div>
                      </button>
                      <div class="view-more-info">
                        <span class="view-more-text">{allResults.artists.total - 12} more artists to discover</span>
                      </div>
                    </div>
                  {:else}
                    <button class="artist-card" onclick={() => onArtistClick?.(artist.id)}>
                      <div class="artist-image-wrapper">
                        <!-- Placeholder always visible as background -->
                        <div class="artist-image-placeholder">
                          <User size={40} />
                        </div>
                        <!-- Image overlays placeholder when loaded -->
                        {#if !failedArtistImages.has(artist.id) && getArtistImage(artist)}
                          <img src={getArtistImage(artist)} alt={artist.name} class="artist-image" loading="lazy" decoding="async" onerror={() => handleArtistImageError(artist.id)} />
                        {/if}
                      </div>
                      <div class="artist-name">{artist.name}</div>
                      {#if artist.albums_count}
                        <div class="artist-albums">{$t('library.albumCount', { values: { count: artist.albums_count } })}</div>
                      {/if}
                    </button>
                  {/if}
                {/each}
              </div>
            </div>
          </div>
        </div>

        <!-- Albums + Tracks Section (50/50) -->
        <div class="bottom-section">
          <!-- Albums Carousel with Navigation -->
          {#if allResults.albums.items.length > 0}
            <div class="albums-section">
              <div class="section-header">
                <h3>Albums</h3>
                <div class="carousel-controls">
                  <button 
                    class="carousel-btn" 
                    onclick={() => scrollAlbumsCarousel('left')} 
                    disabled={!canScrollAlbumsLeft}
                    aria-label="Previous albums"
                  >
                    <ChevronLeft size={20} />
                  </button>
                  <button 
                    class="carousel-btn" 
                    onclick={() => scrollAlbumsCarousel('right')} 
                    disabled={!canScrollAlbumsRight}
                    aria-label="Next albums"
                  >
                    <ChevronRight size={20} />
                  </button>
                  <button class="view-all-link" onclick={() => handleTabChange('albums')}>
                    View all ({allResults.albums.total})
                  </button>
                </div>
              </div>
              <div class="albums-carousel-wrapper" bind:this={albumsCarouselContainer}>
                <div class="albums-carousel">
                  {#each visibleAlbums as album}
                    {#if album.isViewMore}
                      <div class="album-card-wrapper">
                        <div class="view-more-card">
                          <button class="view-more-cover" onclick={() => handleTabChange('albums')}>
                            <div class="view-more-label">
                              <span>View more</span>
                              <ChevronRight size={20} />
                            </div>
                          </button>
                          <div class="view-more-info">
                            <span class="view-more-text">{allResults.albums.total - 30} more albums to discover</span>
                          </div>
                        </div>
                      </div>
                    {:else}
                      <div class="album-card-wrapper">
                        <AlbumCard
                          albumId={album.id}
                          artwork={getAlbumArtwork(album)}
                          title={album.title}
                          artist={album.artist?.name || 'Unknown Artist'}
                          quality={getQualityLabel(album)}
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
                          onclick={() => { onAlbumClick?.(album.id); loadAlbumDownloadStatus(album.id); }}
                        />
                      </div>
                    {/if}
                  {/each}
                </div>
              </div>
            </div>
          {/if}

          <!-- Tracks Section -->
          {#if allResults.tracks.items.length > 0}
            <div class="tracks-section">
              <div class="section-header">
                <h3>Tracks</h3>
                <button class="view-all-link" onclick={() => handleTabChange('tracks')}>
                  View all ({allResults.tracks.total})
                </button>
              </div>
              <div class="tracks-list-compact">
                {#each allResults.tracks.items.slice(0, 6) as track, index}
                  {@const isActiveTrack = isPlaybackActive && activeTrackId === track.id}
                  {@const isTrackDownloaded = checkTrackDownloaded?.(track.id) || false}
                  <div
                    class="track-row"
                    class:playing={isActiveTrack}
                    role="button"
                    tabindex="0"
                    onclick={() => handleSearchTrackPlay(track, index)}
                    onkeydown={(e) => e.key === 'Enter' && handleSearchTrackPlay(track, index)}
                  >
                    <div class="track-number">{index + 1}</div>
                    <div class="track-artwork-container">
                      <!-- Placeholder always visible as background -->
                      <div class="track-artwork-placeholder">
                        <Music size={20} />
                      </div>
                      <!-- Image overlays placeholder when loaded -->
                      {#if !failedTrackImages.has(track.id) && getTrackArtwork(track)}
                        <img src={getTrackArtwork(track)} alt={track.title} class="track-artwork" loading="lazy" decoding="async" onerror={() => handleTrackImageError(track.id)} />
                      {/if}
                      <button
                        class="track-play-overlay"
                        class:is-playing={isActiveTrack}
                        onclick={(e) => {
                          if (isActiveTrack) {
                            handlePausePlayback(e);
                          } else {
                            e.stopPropagation();
                            handleSearchTrackPlay(track, index);
                          }
                        }}
                        aria-label={isActiveTrack ? 'Pause track' : 'Play track'}
                      >
                        <span class="play-icon" aria-hidden="true">
                          <svg width="24" height="24" viewBox="0 0 24 24" fill="white">
                            <path d="M8 5v14l11-7z"/>
                          </svg>
                        </span>
                        <div class="playing-indicator" aria-hidden="true">
                          <div class="bar"></div>
                          <div class="bar"></div>
                          <div class="bar"></div>
                        </div>
                        <span class="pause-icon" aria-hidden="true">
                          <svg width="20" height="20" viewBox="0 0 24 24" fill="white">
                            <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/>
                          </svg>
                        </span>
                      </button>
                    </div>
                    <div class="track-info">
                      <div class="track-title">{track.title}</div>
                      {#if track.performer?.id && onTrackGoToArtist}
                        <button
                          class="track-artist track-link"
                          type="button"
                          onclick={(event) => {
                            event.stopPropagation();
                            onTrackGoToArtist?.(track.performer!.id!);
                          }}
                        >
                          {track.performer?.name || 'Unknown Artist'}
                        </button>
                      {:else}
                        <div class="track-artist">{track.performer?.name || 'Unknown Artist'}</div>
                      {/if}
                    </div>
                    <div class="track-quality">{getQualityLabel(track)}</div>
                    <div class="track-duration">{formatDuration(track.duration)}</div>
                    <div class="track-actions">
                      <TrackMenu
                        onPlayNow={() => handleSearchTrackPlay(track, index)}
                        onPlayNext={onTrackPlayNext ? () => onTrackPlayNext(track) : undefined}
                        onPlayLater={onTrackPlayLater ? () => onTrackPlayLater(track) : undefined}
                        onAddFavorite={onTrackAddFavorite ? () => onTrackAddFavorite(track.id) : undefined}
                        onAddToPlaylist={onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined}
                        onShareQobuz={onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined}
                        onShareSonglink={onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined}
                        onGoToAlbum={track.album?.id && onTrackGoToAlbum ? (() => { const albumId = track.album!.id!; return () => onTrackGoToAlbum(albumId); })() : undefined}
                        onGoToArtist={track.performer?.id && onTrackGoToArtist ? (() => { const artistId = track.performer!.id!; return () => onTrackGoToArtist(artistId); })() : undefined}
                        onDownload={onTrackDownload ? () => onTrackDownload(track) : undefined}
                        isTrackDownloaded={isTrackDownloaded}
                        onReDownload={isTrackDownloaded && onTrackReDownload ? () => onTrackReDownload(track) : undefined}
                        onRemoveDownload={isTrackDownloaded && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
                      />
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      </div>
    {:else if activeTab === 'albums' && albumResults}
      {#if albumResults.items.length === 0}
        <div class="no-results">{$t('search.noAlbumsFor', { values: { query } })}</div>
      {:else}
        <div class="albums-grid">
          {#each albumResults.items as album}
            <AlbumCard
              albumId={album.id}
              artwork={getAlbumArtwork(album)}
              title={album.title}
              artist={album.artist?.name || 'Unknown Artist'}
              quality={getQualityLabel(album)}
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
              onclick={() => { onAlbumClick?.(album.id); loadAlbumDownloadStatus(album.id); }}
            />
          {/each}
        </div>
        {#if hasMoreAlbums}
          <div class="load-more-container">
            <button class="load-more-btn" onclick={loadMore} disabled={isLoadingMore}>
              {isLoadingMore ? $t('actions.loading') : $t('artist.loadMore') + ` (${albumResults.items.length} / ${albumResults.total})`}
            </button>
          </div>
        {/if}
      {/if}
    {:else if activeTab === 'tracks' && trackResults}
      {#if trackResults.items.length === 0}
        <div class="no-results">{$t('search.noTracksFor', { values: { query } })}</div>
      {:else}
        <div class="tracks-list">
          {#each trackResults.items as track, index}
            {@const isActiveTrack = isPlaybackActive && activeTrackId === track.id}
            {@const isTrackDownloaded = checkTrackDownloaded?.(track.id) || false}
            <div
              class="track-row"
              class:playing={isActiveTrack}
              role="button"
              tabindex="0"
              onclick={() => handleSearchTrackPlay(track, index)}
              onkeydown={(e) => e.key === 'Enter' && handleSearchTrackPlay(track, index)}
            >
              <div class="track-number">{index + 1}</div>
              <div class="track-artwork-container">
                <!-- Placeholder always visible as background -->
                <div class="track-artwork-placeholder">
                  <Music size={20} />
                </div>
                <!-- Image overlays placeholder when loaded -->
                {#if !failedTrackImages.has(track.id) && getTrackArtwork(track)}
                  <img src={getTrackArtwork(track)} alt={track.title} class="track-artwork" loading="lazy" decoding="async" onerror={() => handleTrackImageError(track.id)} />
                {/if}
                <button
                  class="track-play-overlay"
                  class:is-playing={isActiveTrack}
                  onclick={(e) => {
                    if (isActiveTrack) {
                      handlePausePlayback(e);
                    } else {
                      e.stopPropagation();
                      handleSearchTrackPlay(track, index);
                    }
                  }}
                  aria-label={isActiveTrack ? 'Pause track' : 'Play track'}
                >
                  <span class="play-icon" aria-hidden="true">
                    <svg width="24" height="24" viewBox="0 0 24 24" fill="white">
                      <path d="M8 5v14l11-7z"/>
                    </svg>
                  </span>
                  <div class="playing-indicator" aria-hidden="true">
                    <div class="bar"></div>
                    <div class="bar"></div>
                    <div class="bar"></div>
                  </div>
                  <span class="pause-icon" aria-hidden="true">
                    <svg width="20" height="20" viewBox="0 0 24 24" fill="white">
                      <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/>
                    </svg>
                  </span>
                </button>
              </div>
              <div class="track-info">
                <div class="track-title">{track.title}</div>
                {#if track.performer?.id && onTrackGoToArtist}
                  <button
                    class="track-artist track-link"
                    type="button"
                    onclick={(event) => {
                      event.stopPropagation();
                      onTrackGoToArtist?.(track.performer!.id!);
                    }}
                  >
                    {track.performer?.name || 'Unknown Artist'}
                  </button>
                {:else}
                  <div class="track-artist">{track.performer?.name || 'Unknown Artist'}</div>
                {/if}
              </div>
              <div class="track-quality">{getQualityLabel(track)}</div>
              <div class="track-duration">{formatDuration(track.duration)}</div>
              <div class="track-actions">
                <TrackMenu
                  onPlayNow={() => handleSearchTrackPlay(track, index)}
                  onPlayNext={onTrackPlayNext ? () => onTrackPlayNext(track) : undefined}
                  onPlayLater={onTrackPlayLater ? () => onTrackPlayLater(track) : undefined}
                  onAddFavorite={onTrackAddFavorite ? () => onTrackAddFavorite(track.id) : undefined}
                  onAddToPlaylist={onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined}
                  onShareQobuz={onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined}
                  onShareSonglink={onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined}
                  onGoToAlbum={track.album?.id && onTrackGoToAlbum ? (() => { const albumId = track.album!.id!; return () => onTrackGoToAlbum(albumId); })() : undefined}
                  onGoToArtist={track.performer?.id && onTrackGoToArtist ? (() => { const artistId = track.performer!.id!; return () => onTrackGoToArtist(artistId); })() : undefined}
                  onDownload={onTrackDownload ? () => onTrackDownload(track) : undefined}
                  isTrackDownloaded={isTrackDownloaded}
                  onReDownload={isTrackDownloaded && onTrackReDownload ? () => onTrackReDownload(track) : undefined}
                  onRemoveDownload={isTrackDownloaded && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
                />
              </div>
            </div>
          {/each}
        </div>
        {#if hasMoreTracks}
          <div class="load-more-container">
            <button class="load-more-btn" onclick={loadMore} disabled={isLoadingMore}>
              {isLoadingMore ? $t('actions.loading') : $t('artist.loadMore') + ` (${trackResults.items.length} / ${trackResults.total})`}
            </button>
          </div>
        {/if}
      {/if}
    {:else if activeTab === 'artists' && artistResults}
      {#if artistResults.items.length === 0}
        <div class="no-results">{$t('search.noArtistsFor', { values: { query } })}</div>
      {:else}
        <div class="artists-grid">
          {#each artistResults.items as artist}
            <button class="artist-card" onclick={() => onArtistClick?.(artist.id)}>
              <div class="artist-image-wrapper">
                <!-- Placeholder always visible as background -->
                <div class="artist-image-placeholder">
                  <User size={40} />
                </div>
                <!-- Image overlays placeholder when loaded -->
                {#if !failedArtistImages.has(artist.id) && getArtistImage(artist)}
                  <img src={getArtistImage(artist)} alt={artist.name} class="artist-image" loading="lazy" decoding="async" onerror={() => handleArtistImageError(artist.id)} />
                {/if}
              </div>
              <div class="artist-name">{artist.name}</div>
              {#if artist.albums_count}
                <div class="artist-albums">{$t('library.albumCount', { values: { count: artist.albums_count } })}</div>
              {/if}
            </button>
          {/each}
        </div>
        {#if hasMoreArtists}
          <div class="load-more-container">
            <button class="load-more-btn" onclick={loadMore} disabled={isLoadingMore}>
              {isLoadingMore ? $t('actions.loading') : $t('artist.loadMore') + ` (${artistResults.items.length} / ${artistResults.total})`}
            </button>
          </div>
        {/if}
      {/if}
    {/if}
  </div>
</div>

<style>
  .search-view {
    width: 100%;
    height: 100%;
    padding: 24px;
    padding-right: 24px;
    padding-bottom: 100px;
    overflow-y: auto;
  }

  /* Custom scrollbar */
  .search-view::-webkit-scrollbar {
    width: 6px;
  }

  .search-view::-webkit-scrollbar-track {
    background: transparent;
  }

  .search-view::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .search-view::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .search-header {
    margin-bottom: 24px;
  }

  .search-header h1 {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 16px;
  }

  .search-input-container {
    position: relative;
  }

  .search-input-container :global(.search-icon) {
    position: absolute;
    left: 16px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--text-muted);
  }

  .search-input {
    width: 100%;
    height: 52px;
    padding: 0 48px 0 48px;
    background-color: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    font-size: 16px;
    color: var(--text-primary);
    outline: none;
    transition: border-color 150ms ease;
  }

  .search-input:focus {
    border-color: var(--accent-primary);
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-clear {
    position: absolute;
    right: 12px;
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .search-clear:hover {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .tabs {
    display: flex;
    gap: 8px;
    margin-bottom: 24px;
    border-bottom: 1px solid var(--bg-tertiary);
    padding-bottom: 16px;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    color: var(--text-muted);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .tab:hover {
    color: var(--text-primary);
  }

  .tab.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-primary);
  }

  .tab .count {
    padding: 2px 8px;
    background-color: var(--bg-tertiary);
    border-radius: 12px;
    font-size: 12px;
  }

  .results {
    min-height: 300px;
  }

  .loading, .empty-state, .error, .no-results {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px 0;
    color: var(--text-muted);
    gap: 16px;
  }

  .error {
    color: #ff6b6b;
  }

  .error-detail {
    font-size: 12px;
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

  .albums-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 24px;
  }

  .tracks-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .track-row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 16px;
    background: none;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
    width: 100%;
    text-align: left;
  }

  .track-row:hover {
    background-color: var(--bg-tertiary);
  }

  .track-number {
    width: 32px;
    font-size: 14px;
    color: var(--text-muted);
    text-align: center;
  }

  .track-artwork {
    position: absolute;
    inset: 0;
    width: 48px;
    height: 48px;
    border-radius: 4px;
    object-fit: cover;
    z-index: 1;
  }

  .track-artwork-placeholder {
    width: 48px;
    height: 48px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
  }

  .track-info {
    flex: 1;
    min-width: 0;
  }

  .track-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-artist {
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-link {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
  }

  .track-link:hover {
    color: var(--text-primary);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .track-quality {
    font-size: 12px;
    color: var(--text-muted);
    padding: 4px 8px;
    background-color: var(--bg-tertiary);
    border-radius: 4px;
  }

  .track-duration {
    font-size: 13px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  .track-actions {
    display: flex;
    align-items: center;
    margin-left: 8px;
    opacity: 0.7;
    transition: opacity 150ms ease;
  }

  .track-row:hover .track-actions {
    opacity: 1;
  }

  .artists-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 24px;
  }

  .artist-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 16px;
    background-color: var(--bg-secondary);
    border: none;
    border-radius: 12px;
    cursor: pointer;
    transition: background-color 150ms ease;
    width: 160px;
    height: 220px;
  }

  .artist-card:hover {
    background-color: var(--bg-tertiary);
  }

  .artist-image-wrapper {
    position: relative;
    width: 120px;
    height: 120px;
    min-height: 120px;
    border-radius: 50%;
    margin-bottom: 12px;
    flex-shrink: 0;
    overflow: hidden;
  }

  .artist-image {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border-radius: 50%;
    object-fit: cover;
    z-index: 1;
  }

  .artist-image-placeholder {
    width: 100%;
    height: 100%;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .artist-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 4px;
    width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    line-height: 1.3;
  }

  .artist-albums {
    font-size: 12px;
    color: var(--text-muted);
  }

  .load-more-container {
    display: flex;
    justify-content: center;
    padding: 32px 0;
  }

  .load-more-btn {
    padding: 12px 32px;
    background-color: var(--bg-tertiary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .load-more-btn:hover:not(:disabled) {
    background-color: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .load-more-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  /* Unified Results View Styles */
  .unified-results {
    display: flex;
    flex-direction: column;
    gap: 32px;
  }

  .top-section {
    display: grid;
    grid-template-columns: 176px minmax(0, 1fr);
    gap: 18px;
    align-items: start;
  }

  .most-popular {
    display: flex;
    flex-direction: column;
  }

  .most-popular-wrapper {
    display: flex;
    justify-content: flex-start;
    width: 100%;
  }

  .most-popular .section-header {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
    min-height: 32px;
  }

  .most-popular h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    gap: 8px;
  }



  .artist-card {
    width: 160px;
    height: 220px;
  }

  .most-popular-card {
    width: 160px;
    height: 220px;
  }

  .most-popular-card .artist-image-wrapper {
    width: 120px;
    height: 120px;
    min-height: 120px;
  }

  .artists-section h3, .section-header h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .section-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  .carousel-controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .carousel-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    border-radius: 6px;
    background-color: transparent;
    color: var(--text-primary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .carousel-btn:hover:not(:disabled) {
    background-color: var(--bg-tertiary);
  }

  .carousel-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .view-all-link {
    background: none;
    border: none;
    color: var(--accent-primary);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 4px;
    transition: all 150ms ease;
  }

  .view-all-link:hover {
    background-color: var(--bg-tertiary);
    text-decoration: underline;
  }

  .artists-grid-compact {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 16px;
    overflow-y: auto;
    max-height: 400px;
    padding-right: 8px;
  }

  .artists-carousel-wrapper {
    position: relative;
    overflow: hidden;
  }

  .artists-carousel {
    display: flex;
    gap: 15px;
  }

  .artists-grid-compact .artist-card {
    width: 160px;
    height: 220px;
  }

  .artists-grid-compact .artist-image-wrapper {
    width: 120px;
    height: 120px;
    min-height: 120px;
  }

  .artists-grid-compact::-webkit-scrollbar {
    width: 6px;
  }

  .artists-grid-compact::-webkit-scrollbar-track {
    background: transparent;
  }

  .artists-grid-compact::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .artists-grid-compact::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .bottom-section {
    display: flex;
    flex-direction: column;
    gap: 32px;
  }

  .albums-section {
    width: 100%;
    display: flex;
    flex-direction: column;
  }

  .albums-carousel-wrapper {
    position: relative;
    overflow-x: hidden;
  }

  .albums-carousel {
    display: flex;
    gap: 16px;
  }

  .album-card-wrapper {
    min-width: 160px;
    flex-shrink: 0;
  }

  .view-more-card {
    width: 160px;
    min-width: 160px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .view-more-cover {
    width: 160px;
    height: 160px;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: var(--bg-secondary);
    border: 2px solid var(--bg-tertiary);
    border-radius: 8px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .view-more-cover:hover {
    background-color: var(--bg-tertiary);
    border-color: var(--accent-primary);
  }

  .view-more-label {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    color: var(--text-muted);
    font-size: 14px;
    font-weight: 500;
  }

  .view-more-cover:hover .view-more-label {
    color: var(--accent-primary);
  }

  .view-more-info {
    width: 160px;
    padding: 0 4px;
    text-align: center;
  }

  .view-more-text {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-muted);
    line-height: 1.3;
    display: block;
  }

  .tracks-section {
    width: 100%;
  }

  .tracks-list-compact {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .track-artwork-container {
    position: relative;
    width: 48px;
    height: 48px;
    border-radius: 4px;
    overflow: hidden;
  }

  .track-play-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    display: none;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.6);
    border: none;
    cursor: pointer;
    transition: background 150ms ease;
    z-index: 2;
  }

  .track-row:hover .track-play-overlay {
    display: flex;
  }

  .track-row.playing .track-play-overlay {
    display: flex;
  }

  .track-play-overlay:hover {
    background: rgba(0, 0, 0, 0.75);
  }

  .track-play-overlay .playing-indicator,
  .track-play-overlay .pause-icon {
    display: none;
  }

  .track-row.playing .track-play-overlay .play-icon {
    display: none;
  }

  .track-row.playing .track-play-overlay .playing-indicator {
    display: flex;
  }

  .track-row.playing:hover .track-play-overlay .playing-indicator {
    display: none;
  }

  .track-row.playing:hover .track-play-overlay .pause-icon {
    display: inline-flex;
  }

  .playing-indicator {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .playing-indicator .bar {
    width: 3px;
    background-color: var(--accent-primary);
    border-radius: 9999px;
    transform-origin: bottom;
    animation: search-equalize 1s ease-in-out infinite;
  }

  .playing-indicator .bar:nth-child(1) {
    height: 10px;
  }

  .playing-indicator .bar:nth-child(2) {
    height: 14px;
    animation-delay: 0.15s;
  }

  .playing-indicator .bar:nth-child(3) {
    height: 8px;
    animation-delay: 0.3s;
  }

  @keyframes search-equalize {
    0%, 100% {
      transform: scaleY(0.5);
      opacity: 0.7;
    }
    50% {
      transform: scaleY(1);
      opacity: 1;
    }
  }

  @media (max-width: 1024px) {
    .top-section {
      grid-template-columns: 1fr;
      gap: 24px;
    }

    .bottom-section {
      gap: 24px;
    }

    .artists-grid-compact {
      grid-template-columns: repeat(auto-fill, minmax(120px, 1fr));
      max-height: 300px;
    }
  }
</style>
