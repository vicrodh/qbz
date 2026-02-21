<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { t } from 'svelte-i18n';
  import { ArrowLeft, Play, Shuffle, Heart, Radio, CloudDownload, ChevronLeft, ChevronRight } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import AlbumMenu from '../AlbumMenu.svelte';
  import ViewTransition from '../ViewTransition.svelte';
  import { getOfflineCacheState, type OfflineCacheStatus, isAlbumFullyCached } from '$lib/stores/offlineCacheState';
  import { consumeContextTrackFocus } from '$lib/stores/playbackContextStore';
  import { saveScrollPosition, getSavedScrollPosition } from '$lib/stores/navigationStore';
  import {
    subscribe as subscribeAlbumFavorites,
    isAlbumFavorite,
    loadAlbumFavorites,
    toggleAlbumFavorite
  } from '$lib/stores/albumFavoritesStore';
  import { isBlacklisted as isArtistBlacklisted } from '$lib/stores/artistBlacklistStore';

  interface Track {
    id: number;
    number: number;
    title: string;
    artist?: string;
    artistId?: number;
    duration: string;
    durationSeconds: number;
    quality?: string;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
    isrc?: string;
  }

  interface ArtistAlbum {
    id: string;
    title: string;
    artwork: string;
    quality: string;
    genre: string;
    releaseDate?: string;
  }

  interface Props {
    album: {
      id: string;
      artwork: string;
      title: string;
      artist: string;
      artistId?: number;
      year: string;
      releaseDate?: string;
      label: string;
      labelId?: number;
      genre: string;
      quality: string;
      trackCount: number;
      duration: string;
      tracks: Track[];
    };
    onBack: () => void;
    onArtistClick?: () => void;
    onLabelClick?: (labelId: number, labelName: string) => void;
    onTrackPlay?: (track: Track) => void;
    onTrackPlayNext?: (track: Track) => void;
    onTrackPlayLater?: (track: Track) => void;
    onTrackAddFavorite?: (trackId: number) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: Track) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
    onTrackShowInfo?: (trackId: number) => void;
    onPlayAll?: () => void;
    onShuffleAll?: () => void;
    onPlayAllNext?: () => void;
    onPlayAllLater?: () => void;
    onAddTrackToPlaylist?: (trackId: number) => void;
    onAddAlbumToPlaylist?: () => void;
    onTrackDownload?: (track: Track) => void;
    onTrackRemoveDownload?: (trackId: number) => void;
    onTrackReDownload?: (track: Track) => void;
    getTrackOfflineCacheStatus?: (trackId: number) => { status: OfflineCacheStatus; progress: number };
    onDownloadAlbum?: () => void;
    onShareAlbumQobuz?: () => void;
    onShareAlbumSonglink?: () => void;
    downloadStateVersion?: number;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
    onOpenAlbumFolder?: () => void;
    onReDownloadAlbum?: () => void;
    // By the same artist section
    artistAlbums?: ArtistAlbum[];
    onRelatedAlbumClick?: (albumId: string) => void;
    onRelatedAlbumPlay?: (albumId: string) => void;
    onRelatedAlbumPlayNext?: (albumId: string) => void;
    onRelatedAlbumPlayLater?: (albumId: string) => void;
    onRelatedAlbumDownload?: (albumId: string) => void;
    onRelatedAlbumShareQobuz?: (albumId: string) => void;
    onRelatedAlbumShareSonglink?: (albumId: string) => void;
    onViewArtistDiscography?: () => void;
    checkRelatedAlbumDownloaded?: (albumId: string) => Promise<boolean>;
    onShowAlbumCredits?: () => void;
    onCreateAlbumRadio?: () => void;
  }

  let {
    album,
    onBack,
    onArtistClick,
    onLabelClick,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddFavorite,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onTrackShowInfo,
    onPlayAll,
    onShuffleAll,
    onPlayAllNext,
    onPlayAllLater,
    onAddTrackToPlaylist,
    onAddAlbumToPlaylist,
    onTrackDownload,
    onTrackRemoveDownload,
    onTrackReDownload,
    getTrackOfflineCacheStatus,
    onDownloadAlbum,
    onShareAlbumQobuz,
    onShareAlbumSonglink,
    downloadStateVersion,
    activeTrackId = null,
    isPlaybackActive = false,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    artistAlbums = [],
    onRelatedAlbumClick,
    onRelatedAlbumPlay,
    onRelatedAlbumPlayNext,
    onRelatedAlbumPlayLater,
    onRelatedAlbumDownload,
    onRelatedAlbumShareQobuz,
    onRelatedAlbumShareSonglink,
    onViewArtistDiscography,
    checkRelatedAlbumDownloaded,
    onShowAlbumCredits,
    onCreateAlbumRadio
  }: Props = $props();

  let isFavorite = $state(false);
  let isFavoriteLoading = $state(false);
  let scrollContainer: HTMLDivElement | null = $state(null);

  // Carousel state for "By the same artist" section
  let carouselContainer: HTMLDivElement | null = $state(null);
  let albumsPerPage = $state(4);
  let currentPage = $state(0);

  // Filter out current album from artist albums
  const filteredArtistAlbums = $derived(
    artistAlbums.filter(a => a.id !== album.id).slice(0, 16)
  );

  const totalPages = $derived(Math.ceil(filteredArtistAlbums.length / albumsPerPage));
  const visibleAlbums = $derived(
    filteredArtistAlbums.slice(currentPage * albumsPerPage, (currentPage + 1) * albumsPerPage)
  );
  const canScrollLeft = $derived(currentPage > 0);
  const canScrollRight = $derived(currentPage < totalPages - 1);
  const hasMoreThanVisible = $derived(filteredArtistAlbums.length > albumsPerPage);

  // Download status tracking for "By the same artist" albums
  let relatedAlbumDownloadStatuses = $state<Map<string, boolean>>(new Map());

  async function loadRelatedAlbumDownloadStatus(albumId: string) {
    if (!checkRelatedAlbumDownloaded) return false;
    try {
      const isDownloaded = await checkRelatedAlbumDownloaded(albumId);
      relatedAlbumDownloadStatuses.set(albumId, isDownloaded);
      relatedAlbumDownloadStatuses = relatedAlbumDownloadStatuses;
      return isDownloaded;
    } catch {
      return false;
    }
  }

  async function loadAllRelatedAlbumDownloadStatuses() {
    if (!checkRelatedAlbumDownloaded || filteredArtistAlbums.length === 0) return;
    await Promise.all(filteredArtistAlbums.map(album => loadRelatedAlbumDownloadStatus(album.id)));
  }

  function isRelatedAlbumDownloaded(albumId: string): boolean {
    return relatedAlbumDownloadStatuses.get(albumId) ?? false;
  }

  // Load download statuses when artist albums change
  $effect(() => {
    if (filteredArtistAlbums.length > 0) {
      loadAllRelatedAlbumDownloadStatuses();
    }
  });

  function calculateAlbumsPerPage() {
    if (!carouselContainer) return;
    const containerWidth = carouselContainer.clientWidth;
    const gap = 16;
    const cardWidth = 162;
    const cols = Math.floor((containerWidth + gap) / (cardWidth + gap));
    albumsPerPage = Math.max(2, cols);
  }

  function scrollCarousel(direction: 'left' | 'right') {
    if (direction === 'left') {
      currentPage = Math.max(0, currentPage - 1);
    } else {
      currentPage = Math.min(totalPages - 1, currentPage + 1);
    }
  }
  
  const albumFullyDownloaded = $derived(
    isAlbumFullyCached(album.tracks.map(track => track.id))
  );
  
  const isVariousArtists = $derived(
    album.artist?.trim().toLowerCase() === 'various artists'
  );

  // Format release date nicely, fallback to year
  const formattedReleaseDate = $derived.by(() => {
    if (album.releaseDate) {
      const date = new Date(album.releaseDate);
      if (!isNaN(date.getTime())) {
        return date.toLocaleDateString('en-US', {
          year: 'numeric',
          month: 'long',
          day: 'numeric'
        });
      }
    }
    return album.year;
  });

  async function scrollToTrack(trackId: number) {
    await tick();
    const target = scrollContainer?.querySelector<HTMLElement>(`[data-track-id="${trackId}"]`);
    target?.scrollIntoView({ block: 'center' });
  }

  // Check if album is in favorites on mount
  onMount(() => {
    let unsubscribe: (() => void) | null = null;
    (async () => {
      try {
        await loadAlbumFavorites();
        isFavorite = isAlbumFavorite(album.id);
        unsubscribe = subscribeAlbumFavorites(() => {
          isFavorite = isAlbumFavorite(album.id);
        });
      } catch (err) {
        console.error('Failed to check album favorite status:', err);
      }
    })();

    // Restore scroll position
    requestAnimationFrame(() => {
      const saved = getSavedScrollPosition('album');
      if (scrollContainer && saved > 0) {
        scrollContainer.scrollTop = saved;
      }
    });

    return () => {
      unsubscribe?.();
    };
  });

  // Set up resize observer for carousel when container is available
  $effect(() => {
    if (!carouselContainer) return;
    calculateAlbumsPerPage();
    const resizeObserver = new ResizeObserver(() => {
      calculateAlbumsPerPage();
    });
    resizeObserver.observe(carouselContainer);
    return () => resizeObserver.disconnect();
  });

  $effect(() => {
    if (!album.tracks?.length) return;
    const targetId = consumeContextTrackFocus('album', album.id);
    if (targetId !== null) {
      void scrollToTrack(targetId);
    }
  });

  async function toggleFavorite() {
    if (isFavoriteLoading) return;

    isFavoriteLoading = true;
    try {
      isFavorite = await toggleAlbumFavorite(album.id);
    } catch (err) {
      console.error('Failed to toggle favorite:', err);
    } finally {
      isFavoriteLoading = false;
    }
  }

  function handleAddAlbumToPlaylist() {
    if (!album?.tracks?.length) return;
    onAddAlbumToPlaylist?.();
  }
</script>

<ViewTransition duration={200} distance={12} direction="up">
<div class="album-detail" bind:this={scrollContainer} onscroll={(e) => saveScrollPosition('album', (e.target as HTMLElement).scrollTop)}>
  <!-- Back Navigation -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>Back</span>
  </button>

  <!-- Album Header -->
  <div class="album-header">
    <!-- Album Artwork -->
    <div class="artwork">
      <img src={album.artwork} alt={album.title} />
    </div>

    <!-- Album Metadata -->
    <div class="metadata">
      <h1 class="album-title">{album.title}</h1>
      {#if onArtistClick && !isVariousArtists}
        <button class="artist-link" onclick={onArtistClick}>
          {album.artist}
        </button>
      {:else}
        <div class="artist-name">{album.artist}</div>
      {/if}
      <div class="album-info">
        {formattedReleaseDate} •
        {#if album.labelId && onLabelClick}
          <button class="label-link" onclick={() => onLabelClick!(album.labelId!, album.label)}>
            {album.label}
          </button>
        {:else}
          {album.label}
        {/if}
         • {album.genre}
      </div>
      <div class="album-quality">{album.quality}</div>
      <div class="album-stats">{album.trackCount} tracks • {album.duration}</div>

      <!-- Action Buttons -->
      <div class="actions">
        <button
          class="action-btn-circle primary"
          onclick={onPlayAll}
          title="Play"
        >
          <Play size={20} fill="currentColor" color="currentColor" />
        </button>
        <button
          class="action-btn-circle"
          onclick={onShuffleAll}
          title="Shuffle"
        >
          <Shuffle size={18} />
        </button>
        <button
          class="action-btn-circle"
          class:is-active={isFavorite}
          onclick={toggleFavorite}
          disabled={isFavoriteLoading}
          title={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
        >
          <Heart
            size={18}
            color={isFavorite ? 'var(--accent-primary)' : 'currentColor'}
            fill={isFavorite ? 'var(--accent-primary)' : 'none'}
          />
        </button>
        {#if onCreateAlbumRadio}
          <button
            class="action-btn-circle"
            onclick={onCreateAlbumRadio}
            title={$t('radio.albumRadio')}
          >
            <Radio size={18} />
          </button>
        {/if}
        {#if onShowAlbumCredits}
          <button
            class="action-btn-circle"
            onclick={onShowAlbumCredits}
            title="Album credits"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
              <path d="M13.839 17.525c-.006.002-.559.186-1.039.186-.265 0-.372-.055-.406-.079-.168-.117-.48-.336.054-1.4l1-1.994c.593-1.184.681-2.329.245-3.225-.356-.733-1.039-1.236-1.92-1.416-.317-.065-.639-.097-.958-.097-1.849 0-3.094 1.08-3.146 1.126-.179.158-.221.42-.102.626.12.206.367.3.595.222.005-.002.559-.187 1.039-.187.263 0 .369.055.402.078.169.118.482.34-.051 1.402l-1 1.995c-.594 1.185-.681 2.33-.245 3.225.356.733 1.038 1.236 1.921 1.416.314.063.636.097.954.097 1.85 0 3.096-1.08 3.148-1.126.179-.157.221-.42.102-.626-.12-.205-.369-.297-.593-.223z"/>
              <circle cx="13" cy="6.001" r="2.5"/>
            </svg>
          </button>
        {/if}
        <AlbumMenu
          onPlayNext={onPlayAllNext}
          onPlayLater={onPlayAllLater}
          onAddToPlaylist={onAddAlbumToPlaylist ? handleAddAlbumToPlaylist : undefined}
          onShareQobuz={onShareAlbumQobuz}
          onShareSonglink={onShareAlbumSonglink}
          onDownload={onDownloadAlbum}
          isAlbumFullyDownloaded={albumFullyDownloaded}
          onOpenContainingFolder={onOpenAlbumFolder}
          onReDownloadAlbum={onReDownloadAlbum}
        />
      </div>
    </div>
  </div>

  <!-- Divider -->
  <div class="divider"></div>

  <!-- Track List -->
  <div class="track-list">
    <!-- Table Header -->
    <div class="table-header">
      <div class="col-number">#</div>
      <div class="col-title">Title</div>
      <div class="col-duration">Duration</div>
      <div class="col-quality">Quality</div>
      <div class="col-icon"><Heart size={14} /></div>
      <div class="col-icon"><CloudDownload size={14} /></div>
      <div class="col-spacer"></div>
    </div>

    <!-- Track Rows -->
    <div class="tracks">
      {#if !album.tracks || album.tracks.length === 0}
        <div class="empty-tracks-message">
          <p>{$t('album.loadError')}</p>
          <button class="retry-btn" onclick={onBack}>{$t('actions.back')}</button>
        </div>
      {:else}
      {#each album.tracks as track, trackIndex (track.id)}
        {@const downloadInfo = getTrackOfflineCacheStatus?.(track.id) ?? { status: 'none' as const, progress: 0 }}
        {@const isTrackDownloaded = downloadInfo.status === 'ready'}
        {@const trackArtistId = track.artistId ?? album.artistId}
        {@const trackBlacklisted = trackArtistId ? isArtistBlacklisted(trackArtistId) : false}
        <TrackRow
          trackId={track.id}
          number={track.number}
          title={track.title}
          artist={track.artist}
          duration={track.duration}
          quality={track.quality}
          isPlaying={activeTrackId === track.id}
          isBlacklisted={trackBlacklisted}
          downloadStatus={downloadInfo.status}
          downloadProgress={downloadInfo.progress}
          hideFavorite={trackBlacklisted}
          hideDownload={trackBlacklisted}
          onPlay={trackBlacklisted ? undefined : () => {
            onTrackPlay?.(track);
          }}
          onDownload={!trackBlacklisted && onTrackDownload ? () => onTrackDownload(track) : undefined}
          onRemoveDownload={!trackBlacklisted && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
          menuActions={trackBlacklisted ? {
            // Blacklisted: only navigation and info
            onGoToArtist: album.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(album.artistId!) : undefined,
            onShowInfo: onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined
          } : {
            onPlayNow: () => {
              onTrackPlay?.(track);
            },
            onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
            onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined,
            onAddToPlaylist: onAddTrackToPlaylist ? () => onAddTrackToPlaylist(track.id) : undefined,
            onShareQobuz: onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
            onShareSonglink: onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined,
            onGoToArtist: album.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(album.artistId!) : undefined,
            onShowInfo: onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined,
            onDownload: onTrackDownload ? () => onTrackDownload(track) : undefined,
            isTrackDownloaded,
            onReDownload: isTrackDownloaded && onTrackReDownload ? () => onTrackReDownload(track) : undefined,
            onRemoveDownload: isTrackDownloaded && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined
          }}
        />
      {/each}
      {/if}
    </div>
  </div>

  <!-- By the same artist Section -->
  {#if filteredArtistAlbums.length > 0 && !isVariousArtists}
    <div class="same-artist-section">
      <div class="section-header">
        <h2 class="section-title">By the same artist</h2>
        {#if hasMoreThanVisible}
          <div class="carousel-controls">
            <button
              class="carousel-btn"
              onclick={() => scrollCarousel('left')}
              disabled={!canScrollLeft}
              aria-label="Previous albums"
            >
              <ChevronLeft size={20} />
            </button>
            <button
              class="carousel-btn"
              onclick={() => scrollCarousel('right')}
              disabled={!canScrollRight}
              aria-label="Next albums"
            >
              <ChevronRight size={20} />
            </button>
          </div>
        {/if}
      </div>
      <div class="albums-carousel-wrapper" bind:this={carouselContainer}>
        <div class="albums-carousel">
          {#each visibleAlbums as relatedAlbum}
            <div class="album-card-wrapper">
              <AlbumCard
                albumId={relatedAlbum.id}
                artwork={relatedAlbum.artwork}
                title={relatedAlbum.title}
                artist={album.artist}
                artistId={album.artistId}
                onArtistClick={onTrackGoToArtist}
                genre={relatedAlbum.genre}
                releaseDate={relatedAlbum.releaseDate}
                size="large"
                quality={relatedAlbum.quality}
                onclick={() => onRelatedAlbumClick?.(relatedAlbum.id)}
                onPlay={onRelatedAlbumPlay ? () => onRelatedAlbumPlay(relatedAlbum.id) : undefined}
                onPlayNext={onRelatedAlbumPlayNext ? () => onRelatedAlbumPlayNext(relatedAlbum.id) : undefined}
                onPlayLater={onRelatedAlbumPlayLater ? () => onRelatedAlbumPlayLater(relatedAlbum.id) : undefined}
                onDownload={onRelatedAlbumDownload ? () => onRelatedAlbumDownload(relatedAlbum.id) : undefined}
                onShareQobuz={onRelatedAlbumShareQobuz ? () => onRelatedAlbumShareQobuz(relatedAlbum.id) : undefined}
                onShareSonglink={onRelatedAlbumShareSonglink ? () => onRelatedAlbumShareSonglink(relatedAlbum.id) : undefined}
                isAlbumFullyDownloaded={isRelatedAlbumDownloaded(relatedAlbum.id)}
              />
            </div>
          {/each}
          {#if onViewArtistDiscography && filteredArtistAlbums.length >= albumsPerPage && currentPage === totalPages - 1}
            <div class="album-card-wrapper">
              <div class="view-more-card">
                <button class="view-more-cover" onclick={onViewArtistDiscography}>
                  <div class="view-more-label">
                    <span>View more</span>
                    <ChevronRight size={20} />
                  </div>
                </button>
                <div class="view-more-info">
                  <span class="view-more-text">See full discography</span>
                </div>
              </div>
            </div>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>
</ViewTransition>

<style>
  .album-detail {
    width: 100%;
    height: 100%;
    padding: 24px;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
  }

  /* Custom scrollbar */
  .album-detail::-webkit-scrollbar {
    width: 6px;
  }

  .album-detail::-webkit-scrollbar-track {
    background: transparent;
  }

  .album-detail::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .album-detail::-webkit-scrollbar-thumb:hover {
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

  .album-header {
    display: flex;
    gap: 32px;
    margin-bottom: 32px;
  }

  .artwork {
    flex-shrink: 0;
    width: 224px;
    height: 224px;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .metadata {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .album-title {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .artist-link {
    font-size: 18px;
    font-weight: 500;
    color: var(--accent-primary);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    width: fit-content;
    margin-bottom: 8px;
  }

  .artist-name {
    font-size: 18px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 8px;
  }

  .artist-link:hover {
    text-decoration: underline;
  }

  .label-link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: inherit;
    cursor: pointer;
    transition: color 150ms ease;
  }

  .label-link:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .album-info {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .album-quality {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .album-stats {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 24px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  /* Style AlbumMenu trigger to match action buttons */
  .actions :global(.album-menu .menu-trigger) {
    width: 36px;
    height: 36px;
    border: 1px solid var(--border-strong);
    color: var(--text-muted);
  }

  .actions :global(.album-menu .menu-trigger:hover) {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: var(--text-primary);
  }

  .divider {
    height: 1px;
    background-color: var(--bg-tertiary);
    margin: 32px 0;
  }

  .table-header {
    width: 100%;
    height: 40px;
    padding: 0 16px;
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    font-size: 12px;
    text-transform: uppercase;
    color: var(--text-muted);
    font-weight: 400;
    box-sizing: border-box;
  }

  .col-number {
    width: 48px;
    text-align: center;
  }

  .col-title {
    flex: 1;
    min-width: 0;
  }

  .col-duration {
    width: 80px;
    text-align: center;
  }

  .col-quality {
    width: 80px;
    text-align: center;
  }

  .col-icon {
    width: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    opacity: 0.5;
  }

  .col-spacer {
    width: 28px;
  }

  .track-list {
    display: flex;
    flex-direction: column;
    width: 100%;
  }

  .tracks {
    display: flex;
    flex-direction: column;
    width: 100%;
  }

  /* By the same artist section */
  .same-artist-section {
    margin-top: 48px;
    padding-top: 32px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .same-artist-section .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .same-artist-section .section-title {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
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

  .albums-carousel-wrapper {
    position: relative;
    overflow-x: hidden;
  }

  .albums-carousel {
    display: flex;
    gap: 16px;
  }

  .album-card-wrapper {
    min-width: 162px;
    flex-shrink: 0;
  }

  .view-more-card {
    width: 162px;
    min-width: 162px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .view-more-cover {
    width: 162px;
    height: 162px;
    border-radius: 8px;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    border: 1px dashed var(--border-strong);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .view-more-cover:hover {
    background: linear-gradient(135deg, var(--bg-secondary) 0%, var(--bg-tertiary) 100%);
    border-color: var(--accent-primary);
  }

  .view-more-label {
    display: flex;
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
    padding: 0 4px;
  }

  .view-more-text {
    font-size: 13px;
    color: var(--text-muted);
  }

  .empty-tracks-message {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 32px 16px;
    color: var(--text-muted);
  }

  .empty-tracks-message p {
    margin: 0;
    font-size: 14px;
  }

  .retry-btn {
    padding: 8px 16px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 13px;
  }

  .retry-btn:hover {
    background: var(--bg-hover);
  }
</style>
