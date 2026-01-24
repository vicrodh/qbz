<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { ArrowLeft, Play, Shuffle, Heart, CloudDownload } from 'lucide-svelte';
  import TrackRow from '../TrackRow.svelte';
  import AlbumMenu from '../AlbumMenu.svelte';
  import { getOfflineCacheState, type OfflineCacheStatus, isAlbumFullyCached } from '$lib/stores/offlineCacheState';
  import { consumeContextTrackFocus } from '$lib/stores/playbackContextStore';
  import {
    subscribe as subscribeAlbumFavorites,
    isAlbumFavorite,
    loadAlbumFavorites,
    toggleAlbumFavorite
  } from '$lib/stores/albumFavoritesStore';

  interface Track {
    id: number;
    number: number;
    title: string;
    artist?: string;
    duration: string;
    durationSeconds: number;
    quality?: string;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
    isrc?: string;
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
      genre: string;
      quality: string;
      trackCount: number;
      duration: string;
      tracks: Track[];
    };
    onBack: () => void;
    onArtistClick?: () => void;
    onTrackPlay?: (track: Track) => void;
    onTrackPlayNext?: (track: Track) => void;
    onTrackPlayLater?: (track: Track) => void;
    onTrackAddFavorite?: (trackId: number) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: Track) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
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
  }

  let {
    album,
    onBack,
    onArtistClick,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddFavorite,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
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
    onReDownloadAlbum
  }: Props = $props();

  let isFavorite = $state(false);
  let isFavoriteLoading = $state(false);
  let scrollContainer: HTMLDivElement | null = $state(null);
  
  const albumFullyDownloaded = $derived(
    isAlbumFullyCached(album.tracks.map(t => t.id))
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

    return () => {
      unsubscribe?.();
    };
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

<div class="album-detail" bind:this={scrollContainer}>
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
      <div class="album-info">{formattedReleaseDate} • {album.label} • {album.genre}</div>
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
      {#each album.tracks as track, trackIndex (`${track.id}-${downloadStateVersion}`)}
        {@const downloadInfo = getTrackOfflineCacheStatus?.(track.id) ?? { status: 'none' as const, progress: 0 }}
        {@const isTrackDownloaded = downloadInfo.status === 'ready'}
        <TrackRow
          trackId={track.id}
          number={track.number}
          title={track.title}
          artist={track.artist}
          duration={track.duration}
          quality={track.quality}
          isPlaying={activeTrackId === track.id}
          downloadStatus={downloadInfo.status}
          downloadProgress={downloadInfo.progress}
          onPlay={() => {
            onTrackPlay?.(track);
          }}
          onDownload={onTrackDownload ? () => onTrackDownload(track) : undefined}
          onRemoveDownload={onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
          menuActions={{
            onPlayNow: () => {
              onTrackPlay?.(track);
            },
            onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
            onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined,
            onAddToPlaylist: onAddTrackToPlaylist ? () => onAddTrackToPlaylist(track.id) : undefined,
            onShareQobuz: onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
            onShareSonglink: onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined,
            onGoToAlbum: onTrackGoToAlbum ? () => onTrackGoToAlbum(album.id) : undefined,
            onGoToArtist: album.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(album.artistId!) : undefined,
            onDownload: onTrackDownload ? () => onTrackDownload(track) : undefined,
            isTrackDownloaded,
            onReDownload: isTrackDownloaded && onTrackReDownload ? () => onTrackReDownload(track) : undefined,
            onRemoveDownload: isTrackDownloaded && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined
          }}
        />
      {/each}
    </div>
  </div>
</div>

<style>
  .album-detail {
    width: 100%;
    height: 100%;
    padding: 24px;
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
    color: #666666;
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
    color: #666666;
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
</style>
