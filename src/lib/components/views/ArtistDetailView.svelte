<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, User, ChevronDown, ChevronUp, Play, Music, Heart } from 'lucide-svelte';
  import type { ArtistDetail, QobuzArtist } from '$lib/types';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackMenu from '../TrackMenu.svelte';

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
    artist: ArtistDetail;
    onBack: () => void;
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
    onLoadMore?: () => void;
    isLoadingMore?: boolean;
    onPlaylistClick?: (playlistId: number) => void;
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
    onLoadMore,
    isLoadingMore = false,
    onPlaylistClick,
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
  let similarArtists = $state<QobuzArtist[]>([]);
  let similarArtistsLoading = $state(false);
  let similarArtistImageErrors = $state<Set<number>>(new Set());
  let artistDetailEl = $state<HTMLDivElement | null>(null);
  let aboutSection = $state<HTMLDivElement | null>(null);
  let topTracksSection = $state<HTMLDivElement | null>(null);
  let discographySection = $state<HTMLDivElement | null>(null);
  let epsSinglesSection = $state<HTMLDivElement | null>(null);
  let liveAlbumsSection = $state<HTMLDivElement | null>(null);
  let compilationsSection = $state<HTMLDivElement | null>(null);
  let tributesSection = $state<HTMLDivElement | null>(null);
  let othersSection = $state<HTMLDivElement | null>(null);
  let playlistsSection = $state<HTMLDivElement | null>(null);
  let activeJumpSection = $state('about');
  let jumpObserver: IntersectionObserver | null = null;

  // Download status tracking
  let albumDownloadStatuses = $state<Map<string, boolean>>(new Map());

  async function loadAlbumDownloadStatus(albumId: string) {
    if (!checkAlbumFullyDownloaded) return false;
    try {
      const isDownloaded = await checkAlbumFullyDownloaded(albumId);
      albumDownloadStatuses.set(albumId, isDownloaded);
      albumDownloadStatuses = albumDownloadStatuses;
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
    return albumDownloadStatuses.get(albumId) || false;
  }

  $effect(() => {
    if (downloadStateVersion !== undefined) {
      const allAlbums = [
        ...artist.albums,
        ...artist.epsSingles,
        ...artist.liveAlbums,
        ...artist.compilations,
        ...artist.tributes,
        ...artist.others
      ];
      loadAllAlbumDownloadStatuses(allAlbums);
    }
  });

  interface SimilarArtistsPage {
    items: QobuzArtist[];
    total: number;
    offset: number;
    limit: number;
  }

  $effect(() => {
    const artistId = artist.id;
    const artistName = artist.name;
    if (!artistId || !artistName) return;

    bioExpanded = false;
    imageError = false;
    topTracks = [];
    similarArtists = [];
    similarArtistImageErrors = new Set();
    activeJumpSection = 'about';

    loadTopTracks();
    loadSimilarArtists();
    checkFavoriteStatus();
    loadArtistAlbumDownloadStatuses();
  });

  async function loadArtistAlbumDownloadStatuses() {
    const allAlbums = [
      ...artist.albums,
      ...artist.epsSingles,
      ...artist.liveAlbums,
      ...artist.compilations,
      ...artist.tributes,
      ...artist.others
    ];
    await loadAllAlbumDownloadStatuses(allAlbums);
  }

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
        limit: 30,
        offset: 0
      });
      // Filter to only include tracks by this artist
      topTracks = results.items.filter(track =>
        track.performer?.name?.toLowerCase() === artist.name.toLowerCase()
      ).slice(0, 20);
    } catch (err) {
      console.error('Failed to load top tracks:', err);
    } finally {
      tracksLoading = false;
    }
  }

  async function loadSimilarArtists() {
    similarArtistsLoading = true;
    try {
      const results = await invoke<SimilarArtistsPage>('get_similar_artists', {
        artistId: artist.id,
        limit: 5,
        offset: 0
      });
      similarArtists = results.items
        .filter(item => item.id !== artist.id)
        .slice(0, 5);
    } catch (err) {
      console.error('Failed to load similar artists:', err);
      similarArtists = [];
    } finally {
      similarArtistsLoading = false;
    }
  }

  function getSimilarArtistImage(similar: QobuzArtist): string {
    return (
      similar.image?.small ||
      similar.image?.thumbnail ||
      similar.image?.large ||
      ''
    );
  }

  function handleSimilarArtistImageError(artistId: number) {
    similarArtistImageErrors = new Set([...similarArtistImageErrors, artistId]);
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

  // Get biography text (prefer content for full text, fall back to summary)
  let bioText = $derived(
    artist.biography?.content || artist.biography?.summary || null
  );

  // Truncate bio for collapsed view
  let truncatedBio = $derived(
    bioText && bioText.length > 800 ? bioText.slice(0, 800) + '...' : bioText
  );

  let hasMoreAlbums = $derived(!!onLoadMore && artist.albumsFetched < artist.totalAlbums);
  let hasTopTracks = $derived(topTracks.length > 0 || tracksLoading);
  let hasEpsSingles = $derived(artist.epsSingles.length > 0);
  let hasLiveAlbums = $derived(artist.liveAlbums.length > 0);
  let hasCompilations = $derived(artist.compilations.length > 0);
  let hasTributes = $derived(artist.tributes.length > 0);
  let hasOthers = $derived(artist.others.length > 0);
  let hasPlaylists = $derived(artist.playlists.length > 0);
  let jumpSections = $derived.by(() => [
    { id: 'about', label: 'About', el: aboutSection, visible: true },
    { id: 'popular', label: 'Popular Tracks', el: topTracksSection, visible: hasTopTracks },
    { id: 'discography', label: 'Discography', el: discographySection, visible: true },
    { id: 'eps', label: 'EPs & Singles', el: epsSinglesSection, visible: hasEpsSingles },
    { id: 'live', label: 'Live Albums', el: liveAlbumsSection, visible: hasLiveAlbums },
    { id: 'compilations', label: 'Compilations', el: compilationsSection, visible: hasCompilations },
    { id: 'tributes', label: 'Tributes', el: tributesSection, visible: hasTributes },
    { id: 'others', label: 'Others', el: othersSection, visible: hasOthers },
    { id: 'playlists', label: 'Playlists', el: playlistsSection, visible: hasPlaylists },
  ].filter(section => section.visible));

  let showJumpNav = $derived(jumpSections.length > 1);

  function scrollToSection(target: HTMLDivElement | null, id: string) {
    activeJumpSection = id;
    target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  $effect(() => {
    if (!artistDetailEl) return;
    if (jumpObserver) {
      jumpObserver.disconnect();
      jumpObserver = null;
    }

    if (jumpSections.length === 0) return;

    const sectionByElement = new Map<HTMLDivElement, string>();
    for (const section of jumpSections) {
      if (section.el) {
        sectionByElement.set(section.el, section.id);
      }
    }

    const targets = [...sectionByElement.keys()];
    if (targets.length === 0) return;

    jumpObserver = new IntersectionObserver(
      (entries) => {
        const visible = entries.filter(entry => entry.isIntersecting);
        if (visible.length === 0) return;

        visible.sort((a, b) => b.intersectionRatio - a.intersectionRatio);
        const targetId = sectionByElement.get(visible[0].target as HTMLDivElement);
        if (targetId) {
          activeJumpSection = targetId;
        }
      },
      {
        root: artistDetailEl,
        rootMargin: '-20% 0px -60% 0px',
        threshold: [0.5]  // Single threshold for better performance
      }
    );

    targets.forEach(target => jumpObserver?.observe(target));

    return () => {
      jumpObserver?.disconnect();
      jumpObserver = null;
    };
  });
</script>

<div class="artist-detail" bind:this={artistDetailEl}>
  <!-- Back Navigation -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>Back</span>
  </button>

  <!-- Artist Header -->
  <div class="artist-header section-anchor" bind:this={aboutSection}>
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
          loading="lazy"
          decoding="async"
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
          <div class="bio-text">
            {@html bioExpanded ? bioText : truncatedBio}
          </div>
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

      {#if similarArtistsLoading}
        <div class="similar-loading">Loading similar artists...</div>
      {:else if similarArtists.length > 0}
        <div class="similar-artists">
          <div class="similar-title">Similar Artists</div>
          <div class="similar-list">
            {#each similarArtists as similar}
              <button
                class="similar-artist"
                onclick={() => onTrackGoToArtist?.(similar.id)}
                title={similar.name}
              >
                {#if similarArtistImageErrors.has(similar.id) || !getSimilarArtistImage(similar)}
                  <span class="similar-avatar placeholder">
                    <User size={12} />
                  </span>
                {:else}
                  <img
                    src={getSimilarArtistImage(similar)}
                    alt={similar.name}
                    class="similar-avatar"
                    loading="lazy"
                    decoding="async"
                    onerror={() => handleSimilarArtistImageError(similar.id)}
                  />
                {/if}
                <span class="similar-name">{similar.name}</span>
              </button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </div>

  {#if showJumpNav}
    <div class="jump-nav">
      <div class="jump-label">Jump to</div>
      <div class="jump-links">
        {#each jumpSections as section}
          <button
            class="jump-link"
            class:active={activeJumpSection === section.id}
            onclick={() => scrollToSection(section.el, section.id)}
          >
            {section.label}
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Divider -->
  <div class="divider"></div>

  <!-- Top Tracks Section -->
  {#if topTracks.length > 0 || tracksLoading}
    <div class="top-tracks-section section-anchor" bind:this={topTracksSection}>
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
                  <img src={track.album?.image?.thumbnail || track.album?.image?.small} alt={track.title} loading="lazy" decoding="async" />
                {:else}
                  <div class="track-artwork-placeholder">
                    <Music size={16} />
                  </div>
                {/if}
              </div>
              <div class="track-info">
                <div class="track-title">{track.title}</div>
                {#if track.album?.id && onTrackGoToAlbum}
                  <button
                    class="track-album track-link"
                    type="button"
                    onclick={(event) => {
                      event.stopPropagation();
                      onTrackGoToAlbum?.(track.album!.id);
                    }}
                  >
                    {track.album?.title || ''}
                  </button>
                {:else}
                  <div class="track-album">{track.album?.title || ''}</div>
                {/if}
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
  <div class="discography section-anchor" bind:this={discographySection}>
    <h2 class="section-title">Discography</h2>

    {#if artist.albums.length === 0}
      <div class="no-albums">No albums found</div>
    {:else}
        <div class="albums-grid">
          {#each artist.albums as album}
            <AlbumCard
              albumId={album.id}
              artwork={album.artwork}
              title={album.title}
              artist={album.year || ''}
              quality={album.quality}
              onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
              onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
              onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
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
          <button
            class="load-more-btn"
            onclick={onLoadMore}
            disabled={isLoadingMore}
          >
            {isLoadingMore ? 'Loading...' : `Load More (${artist.albumsFetched} of ${artist.totalAlbums})`}
          </button>
        </div>
      {/if}
    {/if}
  </div>

  {#if artist.epsSingles.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={epsSinglesSection}>
      <h2 class="section-title">EPs & Singles</h2>
      <div class="albums-grid">
        {#each artist.epsSingles as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
            onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
            onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
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
    </div>
  {/if}

  {#if artist.liveAlbums.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={liveAlbumsSection}>
      <h2 class="section-title">Live Albums</h2>
      <div class="albums-grid">
        {#each artist.liveAlbums as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
            onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
            onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
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
    </div>
  {/if}

  {#if artist.compilations.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={compilationsSection}>
      <h2 class="section-title">Compilations</h2>
      <div class="albums-grid">
        {#each artist.compilations as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
            onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
            onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
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
    </div>
  {/if}

  {#if artist.tributes.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={tributesSection}>
      <h2 class="section-title">Tributes & Covers</h2>
      <div class="albums-grid">
        {#each artist.tributes as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
            onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
            onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
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
    </div>
  {/if}

  {#if artist.others.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={othersSection}>
      <h2 class="section-title">Others</h2>
      <div class="albums-grid">
        {#each artist.others as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
            onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
            onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
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
    </div>
  {/if}

  {#if artist.playlists.length > 0}
    <div class="divider"></div>

    <div class="playlists-section section-anchor" bind:this={playlistsSection}>
      <h2 class="section-title">Playlists</h2>
      <div class="playlists-grid">
        {#each artist.playlists as playlist}
          <button
            class="playlist-card"
            onclick={() => onPlaylistClick?.(playlist.id)}
            disabled={!onPlaylistClick}
          >
            <div class="playlist-artwork">
              {#if playlist.artwork}
                <img src={playlist.artwork} alt={playlist.title} loading="lazy" decoding="async" />
              {:else}
                <div class="playlist-artwork-placeholder">
                  <Music size={18} />
                </div>
              {/if}
            </div>
            <div class="playlist-info">
              <div class="playlist-title">{playlist.title}</div>
              <div class="playlist-meta">
                {#if playlist.trackCount}
                  {playlist.trackCount} tracks
                {:else}
                  Playlist
                {/if}
                {#if playlist.owner}
                  · {playlist.owner}
                {/if}
              </div>
            </div>
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .artist-detail {
    width: 100%;
    height: 100%;
    padding: 24px;
    padding-top: 0;
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
    margin-top: 24px;
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
    font-size: 24px;
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

  .jump-nav {
    position: sticky;
    top: 0;
    z-index: 4;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    padding: 12px 24px;
    background-color: var(--bg-primary);
    border-bottom: 1px solid var(--bg-tertiary);
    margin: 0 -24px 16px;
  }

  .jump-label {
    font-size: 12px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .jump-links {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
  }

  .jump-link {
    padding: 4px 0;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: color 150ms ease, border-color 150ms ease;
  }

  .jump-link:hover {
    color: var(--text-secondary);
  }

  .jump-link.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-primary);
  }

  .similar-artists {
    margin-top: 16px;
  }

  .similar-title {
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .similar-loading {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 8px;
  }

  .similar-list {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .similar-artist {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
    background: none;
    border: none;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 16px;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .similar-artist:hover {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .similar-avatar {
    width: 25px;
    height: 25px;
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
  }

  .similar-avatar.placeholder {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background-color: var(--bg-tertiary);
    color: var(--text-muted);
  }

  .similar-name {
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .divider {
    height: 1px;
    background-color: var(--bg-tertiary);
    margin: 32px 0;
  }

  .section-anchor {
    scroll-margin-top: 140px;
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

  .playlists-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 16px;
  }

  .playlist-card {
    display: flex;
    gap: 12px;
    padding: 10px;
    border-radius: 10px;
    border: 1px solid var(--bg-tertiary);
    background-color: var(--bg-tertiary);
    cursor: pointer;
    text-align: left;
    transition: background-color 150ms ease, border-color 150ms ease;
  }

  .playlist-card:hover:not(:disabled) {
    background-color: var(--bg-hover);
    border-color: var(--bg-hover);
  }

  .playlist-card:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .playlist-artwork {
    width: 56px;
    height: 56px;
    border-radius: 8px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .playlist-artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .playlist-artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: var(--bg-secondary);
    color: var(--text-muted);
  }

  .playlist-info {
    min-width: 0;
  }

  .playlist-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playlist-meta {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
      font-size: 24px;
    }

    .biography {
      max-width: 100%;
    }
  }
</style>
