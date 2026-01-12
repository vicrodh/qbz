<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { ArrowLeft, User, ChevronDown, ChevronUp, Play, Music, Heart } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackMenu from '../TrackMenu.svelte';

  interface Album {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    quality: string;
  }

  interface Biography {
    summary?: string;
    content?: string;
    source?: string;
  }

  interface Track {
    id: number;
    title: string;
    duration: number;
    album?: {
      id: string;
      title: string;
      image?: { small?: string; thumbnail?: string; large?: string };
    };
    performer?: { id?: number; name: string };
    hires_streamable?: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    isrc?: string;
  }

  interface SearchResults {
    items: Track[];
    total: number;
  }

  interface DisplayTrack {
    id: number;
    title: string;
    artist: string;
    album: string;
    albumArt: string;
    duration: string;
    durationSeconds: number;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
    albumId?: string;
    artistId?: number;
    isrc?: string;
  }

  interface Props {
    artist: {
      id: number;
      name: string;
      image?: string;
      albumsCount?: number;
      biography?: Biography;
      albums: Album[];
      totalAlbums: number;
    };
    onBack: () => void;
    onAlbumClick?: (albumId: string) => void;
    onLoadMore?: () => void;
    isLoadingMore?: boolean;
    onTrackPlay?: (track: DisplayTrack) => void;
    onTrackPlayNext?: (track: Track) => void;
    onTrackPlayLater?: (track: Track) => void;
    onTrackAddFavorite?: (trackId: number) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: Track) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
  }

  let {
    artist,
    onBack,
    onAlbumClick,
    onLoadMore,
    isLoadingMore = false,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddFavorite,
    onTrackAddToPlaylist,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist
  }: Props = $props();

  let bioExpanded = $state(false);
  let imageError = $state(false);
  let topTracks = $state<Track[]>([]);
  let tracksLoading = $state(false);
  let isFavorite = $state(false);
  let isFavoriteLoading = $state(false);

  onMount(() => {
    loadTopTracks();
    checkFavoriteStatus();
  });

  async function checkFavoriteStatus() {
    try {
      const response = await invoke<{ artists?: { items: Array<{ id: number }> } }>('get_favorites', {
        favType: 'artists',
        limit: 500,
        offset: 0
      });
      if (response.artists?.items) {
        isFavorite = response.artists.items.some(item => item.id === artist.id);
      }
    } catch (err) {
      console.error('Failed to check artist favorite status:', err);
    }
  }

  async function toggleFavorite() {
    if (isFavoriteLoading) return;

    isFavoriteLoading = true;
    const wasFavorite = isFavorite;

    try {
      if (wasFavorite) {
        await invoke('remove_favorite', { favType: 'artist', itemId: String(artist.id) });
        isFavorite = false;
      } else {
        await invoke('add_favorite', { favType: 'artist', itemId: String(artist.id) });
        isFavorite = true;
      }
    } catch (err) {
      console.error('Failed to toggle artist favorite:', err);
      isFavorite = wasFavorite; // Rollback on error
    } finally {
      isFavoriteLoading = false;
    }
  }

  async function loadTopTracks() {
    tracksLoading = true;
    try {
      // Search for tracks by artist name
      const results = await invoke<SearchResults>('search_tracks', {
        query: artist.name,
        limit: 10,
        offset: 0
      });
      // Filter to only include tracks by this artist
      topTracks = results.items.filter(track =>
        track.performer?.name?.toLowerCase() === artist.name.toLowerCase()
      ).slice(0, 5);
    } catch (err) {
      console.error('Failed to load top tracks:', err);
    } finally {
      tracksLoading = false;
    }
  }

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function handleTrackPlay(track: Track) {
    if (onTrackPlay) {
      onTrackPlay({
        id: track.id,
        title: track.title,
        artist: track.performer?.name || artist.name,
        album: track.album?.title || '',
        albumArt: track.album?.image?.large || track.album?.image?.thumbnail || '',
        duration: formatDuration(track.duration),
        durationSeconds: track.duration,
        hires: track.hires_streamable,
        bitDepth: track.maximum_bit_depth,
        samplingRate: track.maximum_sampling_rate,
        albumId: track.album?.id,
        artistId: track.performer?.id ?? artist.id,
        isrc: track.isrc,
      });
    }
  }

  async function handlePlayAllTracks() {
    if (topTracks.length === 0 || !onTrackPlay) return;

    const queueTracks = topTracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.performer?.name || artist.name,
      album: t.album?.title || '',
      duration_secs: t.duration,
      artwork_url: t.album?.image?.large || t.album?.image?.thumbnail || '',
      hires: t.hires_streamable ?? false,
      bit_depth: t.maximum_bit_depth ?? null,
      sample_rate: t.maximum_sampling_rate ?? null,
    }));

    try {
      await invoke('set_queue', { tracks: queueTracks, startIndex: 0 });
      handleTrackPlay(topTracks[0]);
    } catch (err) {
      console.error('Failed to set queue:', err);
    }
  }

  function handleImageError() {
    imageError = true;
  }

  // Get biography text (prefer summary, fall back to content)
  let bioText = $derived(
    artist.biography?.summary || artist.biography?.content || null
  );

  // Truncate bio for collapsed view
  let truncatedBio = $derived(
    bioText && bioText.length > 300 ? bioText.slice(0, 300) + '...' : bioText
  );

  let hasMoreAlbums = $derived(artist.albums.length < artist.totalAlbums);
</script>

<div class="artist-detail">
  <!-- Back Navigation -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>Back</span>
  </button>

  <!-- Artist Header -->
  <div class="artist-header">
    <!-- Artist Image -->
    <div class="artist-image-container">
      {#if imageError || !artist.image}
        <div class="artist-image-placeholder">
          <User size={60} />
        </div>
      {:else}
        <img
          src={artist.image}
          alt={artist.name}
          class="artist-image"
          onerror={handleImageError}
        />
      {/if}
    </div>

    <!-- Artist Info -->
    <div class="artist-info">
      <div class="artist-name-row">
        <h1 class="artist-name">{artist.name}</h1>
        <button
          class="favorite-btn"
          class:is-favorite={isFavorite}
          onclick={toggleFavorite}
          disabled={isFavoriteLoading}
          title={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
        >
          {#if isFavorite}
            <Heart size={24} fill="var(--accent-primary)" color="var(--accent-primary)" />
          {:else}
            <Heart size={24} />
          {/if}
        </button>
      </div>
      <div class="artist-stats">
        {artist.totalAlbums || artist.albumsCount || 0} albums
      </div>

      <!-- Biography -->
      {#if bioText}
        <div class="biography">
          <p class="bio-text">
            {bioExpanded ? bioText : truncatedBio}
          </p>
          {#if bioText.length > 300}
            <button class="bio-toggle" onclick={() => bioExpanded = !bioExpanded}>
              {#if bioExpanded}
                <ChevronUp size={16} />
                <span>Show less</span>
              {:else}
                <ChevronDown size={16} />
                <span>Read more</span>
              {/if}
            </button>
          {/if}
          {#if artist.biography?.source}
            <div class="bio-source">Source: {artist.biography.source}</div>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <!-- Divider -->
  <div class="divider"></div>

  <!-- Top Tracks Section -->
  {#if topTracks.length > 0 || tracksLoading}
    <div class="top-tracks-section">
      <div class="section-header-row">
        <h2 class="section-title">Popular Tracks</h2>
        {#if topTracks.length > 0}
          <button class="play-all-btn" onclick={handlePlayAllTracks}>
            <Play size={14} fill="white" color="white" />
            <span>Play All</span>
          </button>
        {/if}
      </div>

      {#if tracksLoading}
        <div class="tracks-loading">Loading tracks...</div>
      {:else}
        <div class="tracks-list">
          {#each topTracks as track, index}
            <div
              class="track-row"
              role="button"
              tabindex="0"
              onclick={() => handleTrackPlay(track)}
              onkeydown={(e) => e.key === 'Enter' && handleTrackPlay(track)}
            >
              <div class="track-number">{index + 1}</div>
              <div class="track-artwork">
                {#if track.album?.image?.thumbnail || track.album?.image?.small}
                  <img src={track.album?.image?.thumbnail || track.album?.image?.small} alt={track.title} />
                {:else}
                  <div class="track-artwork-placeholder">
                    <Music size={16} />
                  </div>
                {/if}
              </div>
              <div class="track-info">
                <div class="track-title">{track.title}</div>
                <div class="track-album">{track.album?.title || ''}</div>
              </div>
              <div class="track-duration">{formatDuration(track.duration)}</div>
              <div class="track-actions">
                <TrackMenu
                  onPlayNow={() => handleTrackPlay(track)}
                  onPlayNext={onTrackPlayNext ? () => onTrackPlayNext(track) : undefined}
                  onPlayLater={onTrackPlayLater ? () => onTrackPlayLater(track) : undefined}
                  onAddFavorite={onTrackAddFavorite ? () => onTrackAddFavorite(track.id) : undefined}
                  onAddToPlaylist={onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined}
                  onShareQobuz={onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined}
                  onShareSonglink={onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined}
                  onGoToAlbum={track.album?.id && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.album!.id) : undefined}
                  onGoToArtist={(track.performer?.id || artist.id) && onTrackGoToArtist ? () => onTrackGoToArtist(track.performer?.id ?? artist.id) : undefined}
                />
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <div class="divider"></div>
  {/if}

  <!-- Discography Section -->
  <div class="discography">
    <h2 class="section-title">Discography</h2>

    {#if artist.albums.length === 0}
      <div class="no-albums">No albums found</div>
    {:else}
      <div class="albums-grid">
        {#each artist.albums as album}
          <AlbumCard
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            onclick={() => onAlbumClick?.(album.id)}
          />
        {/each}
      </div>

      {#if hasMoreAlbums}
        <div class="load-more-container">
          <button
            class="load-more-btn"
            onclick={onLoadMore}
            disabled={isLoadingMore}
          >
            {isLoadingMore ? 'Loading...' : `Load More (${artist.albums.length} of ${artist.totalAlbums})`}
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .artist-detail {
    width: 100%;
    height: 100%;
    padding: 24px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
  }

  /* Custom scrollbar */
  .artist-detail::-webkit-scrollbar {
    width: 6px;
  }

  .artist-detail::-webkit-scrollbar-track {
    background: transparent;
  }

  .artist-detail::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .artist-detail::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
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
    margin-bottom: 24px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-secondary);
  }

  .artist-header {
    display: flex;
    gap: 32px;
    margin-bottom: 32px;
  }

  .artist-image-container {
    flex-shrink: 0;
  }

  .artist-image {
    width: 220px;
    height: 220px;
    border-radius: 50%;
    object-fit: cover;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .artist-image-placeholder {
    width: 220px;
    height: 220px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .artist-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .artist-name-row {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .artist-name {
    font-size: 36px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 8px;
  }

  .favorite-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 50%;
    cursor: pointer;
    color: var(--text-muted);
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .favorite-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--accent-primary);
  }

  .favorite-btn.is-favorite {
    background: rgba(var(--accent-primary-rgb, 139, 92, 246), 0.15);
  }

  .favorite-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .artist-stats {
    font-size: 16px;
    color: var(--text-muted);
    margin-bottom: 16px;
  }

  .biography {
    max-width: 600px;
  }

  .bio-text {
    font-size: 14px;
    line-height: 1.6;
    color: var(--text-secondary);
    margin-bottom: 8px;
  }

  .bio-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
    color: var(--accent-primary);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .bio-toggle:hover {
    text-decoration: underline;
  }

  .bio-source {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 8px;
  }

  .divider {
    height: 1px;
    background-color: var(--bg-tertiary);
    margin: 32px 0;
  }

  .section-title {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 24px;
  }

  .no-albums {
    color: var(--text-muted);
    font-size: 14px;
  }

  .albums-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 24px;
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

  /* Top Tracks */
  .top-tracks-section {
    margin-bottom: 0;
  }

  .section-header-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .section-header-row .section-title {
    margin-bottom: 0;
  }

  .play-all-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background-color: var(--accent-primary);
    border: none;
    border-radius: 8px;
    color: white;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .play-all-btn:hover {
    background-color: var(--accent-hover);
  }

  .tracks-loading {
    color: var(--text-muted);
    font-size: 14px;
    padding: 16px 0;
  }

  .tracks-list {
    display: flex;
    flex-direction: column;
  }

  .track-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    background: none;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: background-color 150ms ease;
  }

  .track-row:hover {
    background-color: var(--bg-tertiary);
  }

  .track-number {
    width: 24px;
    font-size: 14px;
    color: var(--text-muted);
    text-align: center;
  }

  .track-artwork {
    width: 40px;
    height: 40px;
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .track-artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .track-artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: var(--bg-tertiary);
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

  .track-album {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-duration {
    font-size: 13px;
    color: var(--text-muted);
    font-family: var(--font-mono);
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

  /* Responsive */
  @media (max-width: 768px) {
    .artist-header {
      flex-direction: column;
      align-items: center;
      text-align: center;
    }

    .artist-name {
      font-size: 28px;
    }

    .biography {
      max-width: 100%;
    }
  }
</style>
