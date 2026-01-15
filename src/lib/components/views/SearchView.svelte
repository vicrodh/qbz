<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Search, Disc3, Music, Mic2, User, X } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackMenu from '../TrackMenu.svelte';
  import { getSearchState, setSearchState, type SearchResults, type SearchTab } from '$lib/stores/searchState';
  import { t } from '$lib/i18n';

  let searchInput: HTMLInputElement | null = null;

  onMount(async () => {
    console.log('SearchView mounted!');
    await tick();
    searchInput?.focus();
  });

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
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: Track) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
    onArtistClick?: (artistId: number) => void;
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
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onArtistClick
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
  let activeTab = $state<SearchTab>(cachedState.activeTab ?? 'albums');
  let isSearching = $state(false);
  let searchError = $state<string | null>(null);

  let albumResults = $state<SearchResults<Album> | null>(cachedState.albumResults ?? null);
  let trackResults = $state<SearchResults<Track> | null>(cachedState.trackResults ?? null);
  let artistResults = $state<SearchResults<Artist> | null>(cachedState.artistResults ?? null);

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
  }

  $effect(() => {
    setSearchState<Album, Track, Artist>({
      query,
      activeTab,
      albumResults,
      trackResults,
      artistResults
    });
  });

  // Download status tracking
  let albumDownloadStatuses = $state<Map<string, boolean>>(new Map());

  async function loadAlbumDownloadStatus(albumId: string) {
    if (!checkAlbumFullyDownloaded) return false;
    try {
      const isDownloaded = await checkAlbumFullyDownloaded(albumId);
      albumDownloadStatuses.set(albumId, isDownloaded);
      return isDownloaded;
    } catch {
      return false;
    }
  }

  function isAlbumDownloaded(albumId: string): boolean {
    void downloadStateVersion;
    return albumDownloadStatuses.get(albumId) || false;
  }

  async function performSearch() {
    if (!query.trim()) return;

    isSearching = true;
    searchError = null;

    try {
      // Search based on active tab - reset to first page
      if (activeTab === 'albums') {
        albumResults = await invoke<SearchResults<Album>>('search_albums', {
          query: query.trim(),
          limit: PAGE_SIZE,
          offset: 0
        });
        console.log('Album results:', albumResults);
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

  function getArtistImage(artist: Artist): string {
    return artist.image?.large || artist.image?.thumbnail || artist.image?.small || '';
  }
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
      class:active={activeTab === 'albums'}
      onclick={() => handleTabChange('albums')}
    >
      <Disc3 size={18} />
      <span>{$t('search.albums')}</span>
      {#if albumResults}
        <span class="count">{albumResults.total}</span>
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
              onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
              onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
              onDownload={onAlbumDownload ? () => onAlbumDownload(album.id) : undefined}
              isAlbumFullyDownloaded={isAlbumDownloaded(album.id)}
              onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(album.id) : undefined}
              onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(album.id) : undefined}
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
            <div
              class="track-row"
              role="button"
              tabindex="0"
              onclick={() => onTrackPlay?.(track)}
              onkeydown={(e) => e.key === 'Enter' && onTrackPlay?.(track)}
            >
              <div class="track-number">{index + 1}</div>
              {#if failedTrackImages.has(track.id) || !getTrackArtwork(track)}
                <div class="track-artwork-placeholder">
                  <Music size={20} />
                </div>
              {:else}
                <img src={getTrackArtwork(track)} alt={track.title} class="track-artwork" onerror={() => handleTrackImageError(track.id)} />
              {/if}
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
                  onPlayNow={() => onTrackPlay?.(track)}
                  onPlayNext={onTrackPlayNext ? () => onTrackPlayNext(track) : undefined}
                  onPlayLater={onTrackPlayLater ? () => onTrackPlayLater(track) : undefined}
                  onAddFavorite={onTrackAddFavorite ? () => onTrackAddFavorite(track.id) : undefined}
                  onAddToPlaylist={onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined}
                  onShareQobuz={onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined}
                  onShareSonglink={onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined}
                  onGoToAlbum={track.album?.id && onTrackGoToAlbum ? (() => { const albumId = track.album!.id!; return () => onTrackGoToAlbum(albumId); })() : undefined}
                  onGoToArtist={track.performer?.id && onTrackGoToArtist ? (() => { const artistId = track.performer!.id!; return () => onTrackGoToArtist(artistId); })() : undefined}
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
              {#if failedArtistImages.has(artist.id) || !getArtistImage(artist)}
                <div class="artist-image-placeholder">
                  <User size={40} />
                </div>
              {:else}
                <img src={getArtistImage(artist)} alt={artist.name} class="artist-image" onerror={() => handleArtistImageError(artist.id)} />
              {/if}
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
    padding-right: 8px;
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
    border-radius: 8px;
    color: var(--text-muted);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .tab:hover {
    color: var(--text-primary);
    background-color: var(--bg-tertiary);
  }

  .tab.active {
    color: var(--text-primary);
    background-color: var(--accent-primary);
  }

  .tab .count {
    padding: 2px 8px;
    background-color: rgba(255, 255, 255, 0.2);
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
    width: 48px;
    height: 48px;
    border-radius: 4px;
    object-fit: cover;
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
  }

  .artist-card:hover {
    background-color: var(--bg-tertiary);
  }

  .artist-image {
    width: 120px;
    height: 120px;
    border-radius: 50%;
    object-fit: cover;
    margin-bottom: 12px;
  }

  .artist-image-placeholder {
    width: 120px;
    height: 120px;
    border-radius: 50%;
    margin-bottom: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
  }

  .artist-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 4px;
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
</style>
