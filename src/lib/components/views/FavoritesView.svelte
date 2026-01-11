<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { Heart, Play, Plus, Disc3, Mic2, Music, Search, X } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import { type DownloadStatus } from '$lib/stores/downloadState';

  interface FavoriteAlbum {
    id: string;
    title: string;
    artist: { id: number; name: string };
    image: { small?: string; thumbnail?: string; large?: string };
    release_date_original?: string;
    hires: boolean;
  }

  interface FavoriteTrack {
    id: number;
    title: string;
    duration: number;
    track_number: number;
    performer?: { id?: number; name: string };
    album?: { id: string; title: string; image: { small?: string; thumbnail?: string; large?: string } };
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    isrc?: string;
  }

  interface FavoriteArtist {
    id: number;
    name: string;
    image?: { small?: string; thumbnail?: string; large?: string };
    albums_count?: number;
  }

  interface Props {
    onAlbumClick?: (albumId: string) => void;
    onTrackPlay?: (track: DisplayTrack) => void;
    onArtistClick?: (artistId: number) => void;
    onTrackPlayNext?: (track: DisplayTrack) => void;
    onTrackPlayLater?: (track: DisplayTrack) => void;
    onTrackAddFavorite?: (trackId: number) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: DisplayTrack) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
    onTrackDownload?: (track: DisplayTrack) => void;
    onTrackRemoveDownload?: (trackId: number) => void;
    getTrackDownloadStatus?: (trackId: number) => { status: DownloadStatus; progress: number };
    downloadStateVersion?: number;
  }

  interface DisplayTrack {
    id: number;
    number: number;
    title: string;
    artist?: string;
    album?: string;
    albumArt?: string;
    albumId?: string;
    artistId?: number;
    duration: string;
    durationSeconds: number;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
    isrc?: string;
  }

  let {
    onAlbumClick,
    onTrackPlay,
    onArtistClick,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddFavorite,
    onTrackAddToPlaylist,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onTrackDownload,
    onTrackRemoveDownload,
    getTrackDownloadStatus,
    downloadStateVersion
  }: Props = $props();

  type TabType = 'tracks' | 'albums' | 'artists';
  let activeTab = $state<TabType>('tracks');

  let favoriteAlbums = $state<FavoriteAlbum[]>([]);
  let favoriteTracks = $state<FavoriteTrack[]>([]);
  let favoriteArtists = $state<FavoriteArtist[]>([]);

  let loading = $state(false);
  let error = $state<string | null>(null);

  // Search state for each tab
  let trackSearch = $state('');
  let albumSearch = $state('');
  let artistSearch = $state('');

  // Filtered lists based on search
  let filteredTracks = $derived.by(() => {
    if (!trackSearch.trim()) return favoriteTracks;
    const query = trackSearch.toLowerCase();
    return favoriteTracks.filter(t =>
      t.title.toLowerCase().includes(query) ||
      t.performer?.name?.toLowerCase().includes(query) ||
      t.album?.title?.toLowerCase().includes(query)
    );
  });

  let filteredAlbums = $derived.by(() => {
    if (!albumSearch.trim()) return favoriteAlbums;
    const query = albumSearch.toLowerCase();
    return favoriteAlbums.filter(a =>
      a.title.toLowerCase().includes(query) ||
      a.artist.name.toLowerCase().includes(query)
    );
  });

  let filteredArtists = $derived.by(() => {
    if (!artistSearch.trim()) return favoriteArtists;
    const query = artistSearch.toLowerCase();
    return favoriteArtists.filter(a =>
      a.name.toLowerCase().includes(query)
    );
  });

  onMount(() => {
    loadFavorites('tracks');
  });

  async function loadFavorites(type: string) {
    loading = true;
    error = null;
    try {
      const result = await invoke<any>('get_favorites', {
        favType: type,
        limit: 50,
        offset: 0
      });

      if (type === 'tracks' && result.tracks?.items) {
        favoriteTracks = result.tracks.items;
      } else if (type === 'albums' && result.albums?.items) {
        favoriteAlbums = result.albums.items;
      } else if (type === 'artists' && result.artists?.items) {
        favoriteArtists = result.artists.items;
      }
    } catch (err) {
      console.error(`Failed to load ${type} favorites:`, err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  function handleTabChange(tab: TabType) {
    activeTab = tab;
    loadFavorites(tab);
  }

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function buildDisplayTrack(track: FavoriteTrack, index: number): DisplayTrack {
    return {
      id: track.id,
      number: index + 1,
      title: track.title,
      artist: track.performer?.name,
      album: track.album?.title,
      albumArt: track.album?.image?.thumbnail || track.album?.image?.small,
      albumId: track.album?.id,
      artistId: track.performer?.id,
      duration: formatDuration(track.duration),
      durationSeconds: track.duration,
      hires: track.hires,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate,
      isrc: track.isrc,
    };
  }

  function handleTrackClick(track: FavoriteTrack, index: number) {
    if (onTrackPlay) {
      onTrackPlay(buildDisplayTrack(track, index));
    }
  }

  async function handlePlayAllTracks() {
    if (filteredTracks.length === 0 || !onTrackPlay) return;

    const queueTracks = filteredTracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.performer?.name || 'Unknown Artist',
      album: t.album?.title || 'Favorites',
      duration_secs: t.duration,
      artwork_url: t.album?.image?.thumbnail || t.album?.image?.small || '',
    }));

    try {
      await invoke('set_queue', { tracks: queueTracks, startIndex: 0 });
      handleTrackClick(filteredTracks[0], 0);
    } catch (err) {
      console.error('Failed to set queue:', err);
    }
  }

  async function handleAddAllToQueue() {
    if (filteredTracks.length === 0) return;

    const queueTracks = filteredTracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.performer?.name || 'Unknown Artist',
      album: t.album?.title || 'Favorites',
      duration_secs: t.duration,
      artwork_url: t.album?.image?.thumbnail || t.album?.image?.small || '',
    }));

    try {
      await invoke('add_tracks_to_queue', { tracks: queueTracks });
    } catch (err) {
      console.error('Failed to add to queue:', err);
    }
  }
</script>

<div class="favorites-view">
  <!-- Header -->
  <div class="header">
    <div class="header-icon">
      <Heart size={32} fill="var(--accent-primary)" color="var(--accent-primary)" />
    </div>
    <div class="header-content">
      <h1>Favorites</h1>
      <p class="subtitle">Your liked tracks, albums, and artists</p>
    </div>
  </div>

  <!-- Tabs -->
  <div class="tabs">
    <button
      class="tab"
      class:active={activeTab === 'tracks'}
      onclick={() => handleTabChange('tracks')}
    >
      <Music size={16} />
      <span>Tracks</span>
    </button>
    <button
      class="tab"
      class:active={activeTab === 'albums'}
      onclick={() => handleTabChange('albums')}
    >
      <Disc3 size={16} />
      <span>Albums</span>
    </button>
    <button
      class="tab"
      class:active={activeTab === 'artists'}
      onclick={() => handleTabChange('artists')}
    >
      <Mic2 size={16} />
      <span>Artists</span>
    </button>
  </div>

  <!-- Toolbar with search and actions -->
  <div class="toolbar">
    <!-- Search input -->
    <div class="search-container">
      <Search size={16} class="search-icon" />
      {#if activeTab === 'tracks'}
        <input
          type="text"
          placeholder="Search tracks..."
          bind:value={trackSearch}
          class="search-input"
        />
        {#if trackSearch}
          <button class="search-clear" onclick={() => trackSearch = ''}>
            <X size={14} />
          </button>
        {/if}
      {:else if activeTab === 'albums'}
        <input
          type="text"
          placeholder="Search albums..."
          bind:value={albumSearch}
          class="search-input"
        />
        {#if albumSearch}
          <button class="search-clear" onclick={() => albumSearch = ''}>
            <X size={14} />
          </button>
        {/if}
      {:else}
        <input
          type="text"
          placeholder="Search artists..."
          bind:value={artistSearch}
          class="search-input"
        />
        {#if artistSearch}
          <button class="search-clear" onclick={() => artistSearch = ''}>
            <X size={14} />
          </button>
        {/if}
      {/if}
    </div>

    <!-- Actions (for tracks tab) -->
    {#if activeTab === 'tracks' && filteredTracks.length > 0}
      <div class="actions">
        <button class="play-btn" onclick={handlePlayAllTracks}>
          <Play size={16} fill="white" />
          <span>Play All</span>
        </button>
        <button class="add-btn" onclick={handleAddAllToQueue}>
          <Plus size={16} />
          <span>Add to Queue</span>
        </button>
      </div>
    {/if}

    <!-- Results count -->
    <span class="results-count">
      {#if activeTab === 'tracks'}
        {filteredTracks.length}{trackSearch ? ` / ${favoriteTracks.length}` : ''} tracks
      {:else if activeTab === 'albums'}
        {filteredAlbums.length}{albumSearch ? ` / ${favoriteAlbums.length}` : ''} albums
      {:else}
        {filteredArtists.length}{artistSearch ? ` / ${favoriteArtists.length}` : ''} artists
      {/if}
    </span>
  </div>

  <!-- Content -->
  <div class="content">
    {#if loading}
      <div class="loading">
        <div class="spinner"></div>
        <p>Loading favorites...</p>
      </div>
    {:else if error}
      <div class="error">
        <p>Failed to load favorites</p>
        <p class="error-detail">{error}</p>
        <button class="retry-btn" onclick={() => loadFavorites(activeTab)}>Retry</button>
      </div>
    {:else if activeTab === 'tracks'}
      {#if favoriteTracks.length === 0}
        <div class="empty">
          <Music size={48} />
          <p>No favorite tracks yet</p>
          <p class="empty-hint">Like tracks to see them here</p>
        </div>
      {:else if filteredTracks.length === 0}
        <div class="empty">
          <Search size={48} />
          <p>No tracks match "{trackSearch}"</p>
        </div>
      {:else}
        <div class="track-list">
          {#each filteredTracks as track, index (`${track.id}-${downloadStateVersion}`)}
            {@const displayTrack = buildDisplayTrack(track, index)}
            {@const downloadInfo = getTrackDownloadStatus?.(track.id) ?? { status: 'none' as const, progress: 0 }}
            <TrackRow
              trackId={track.id}
              number={index + 1}
              title={track.title}
              artist={track.performer?.name}
              duration={formatDuration(track.duration)}
              quality={track.hires ? 'Hi-Res' : undefined}
              isFavoriteOverride={true}
              downloadStatus={downloadInfo.status}
              downloadProgress={downloadInfo.progress}
              onPlay={() => handleTrackClick(track, index)}
              onDownload={onTrackDownload ? () => onTrackDownload(displayTrack) : undefined}
              onRemoveDownload={onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
              menuActions={{
                onPlayNow: () => handleTrackClick(track, index),
                onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(displayTrack) : undefined,
                onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(displayTrack) : undefined,
                onAddToPlaylist: onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined,
                onShareQobuz: onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
                onShareSonglink: onTrackShareSonglink ? () => onTrackShareSonglink(displayTrack) : undefined,
                onGoToAlbum: track.album?.id && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.album!.id) : undefined,
                onGoToArtist: track.performer?.id && onTrackGoToArtist ? () => onTrackGoToArtist(track.performer!.id!) : undefined
              }}
            />
          {/each}
        </div>
      {/if}
    {:else if activeTab === 'albums'}
      {#if favoriteAlbums.length === 0}
        <div class="empty">
          <Disc3 size={48} />
          <p>No favorite albums yet</p>
          <p class="empty-hint">Like albums to see them here</p>
        </div>
      {:else if filteredAlbums.length === 0}
        <div class="empty">
          <Search size={48} />
          <p>No albums match "{albumSearch}"</p>
        </div>
      {:else}
        <div class="album-grid">
          {#each filteredAlbums as album (album.id)}
            <AlbumCard
              artwork={album.image?.large || album.image?.thumbnail || ''}
              title={album.title}
              artist={album.artist.name}
              quality={album.hires ? 'Hi-Res' : undefined}
              onclick={() => onAlbumClick?.(album.id)}
            />
          {/each}
        </div>
      {/if}
    {:else if activeTab === 'artists'}
      {#if favoriteArtists.length === 0}
        <div class="empty">
          <Mic2 size={48} />
          <p>No favorite artists yet</p>
          <p class="empty-hint">Like artists to see them here</p>
        </div>
      {:else if filteredArtists.length === 0}
        <div class="empty">
          <Search size={48} />
          <p>No artists match "{artistSearch}"</p>
        </div>
      {:else}
        <div class="artist-grid">
          {#each filteredArtists as artist (artist.id)}
            <button class="artist-card" onclick={() => onArtistClick?.(artist.id)}>
              <div class="artist-image">
                {#if artist.image?.large || artist.image?.thumbnail}
                  <img src={artist.image?.large || artist.image?.thumbnail} alt={artist.name} />
                {:else}
                  <div class="artist-placeholder">
                    <Mic2 size={32} />
                  </div>
                {/if}
              </div>
              <div class="artist-name">{artist.name}</div>
              {#if artist.albums_count}
                <div class="artist-albums">{artist.albums_count} albums</div>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .favorites-view {
    padding: 24px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  .header {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 32px;
  }

  .header-icon {
    width: 80px;
    height: 80px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--accent-primary) 0%, #ff6b9d 100%);
    border-radius: 16px;
  }

  .header-content h1 {
    font-size: 32px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 4px 0;
  }

  .subtitle {
    font-size: 14px;
    color: var(--text-muted);
    margin: 0;
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
    padding: 10px 20px;
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
    background-color: var(--bg-tertiary);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 24px;
  }

  .search-container {
    display: flex;
    align-items: center;
    gap: 8px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 8px 12px;
    flex: 1;
    max-width: 300px;
  }

  .search-container :global(.search-icon) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 14px;
    outline: none;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-clear {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .search-clear:hover {
    color: var(--text-primary);
  }

  .results-count {
    margin-left: auto;
    font-size: 13px;
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    gap: 12px;
  }

  .play-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 24px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .play-btn:hover {
    background-color: var(--accent-hover);
  }

  .add-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 24px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--text-muted);
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: border-color 150ms ease;
  }

  .add-btn:hover {
    border-color: var(--text-primary);
  }

  .content {
    min-height: 200px;
  }

  .loading,
  .error,
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

  .empty-hint {
    font-size: 13px;
    margin-top: 8px;
  }

  .track-list {
    display: flex;
    flex-direction: column;
  }

  .album-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 24px;
  }

  .artist-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 24px;
  }

  .artist-card {
    display: flex;
    flex-direction: column;
    align-items: center;
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
    overflow: hidden;
    margin-bottom: 12px;
    background-color: var(--bg-tertiary);
  }

  .artist-image img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artist-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .artist-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
  }

  .artist-albums {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
  }
</style>
