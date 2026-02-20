<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { Disc3, Play, Music, MoreHorizontal, User, ChevronDown, ChevronUp } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import AlbumCard from '../AlbumCard.svelte';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import QobuzPlaylistCard from '../QobuzPlaylistCard.svelte';
  import TrackMenu from '../TrackMenu.svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import { setPlaybackContext } from '$lib/stores/playbackContextStore';
  import { togglePlay } from '$lib/stores/playerStore';
  import type { QobuzAlbum, LabelPageData, LabelExploreItem, DisplayTrack } from '$lib/types';

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

  interface Props {
    labelId: number;
    labelName?: string;
    onBack: () => void;
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    checkAlbumFullyDownloaded?: (albumId: string) => Promise<boolean>;
    downloadStateVersion?: number;
    onArtistClick?: (artistId: number) => void;
    onLabelClick?: (labelId: number, labelName?: string) => void;
    onNavigateReleases?: (labelId: number, labelName: string) => void;
    onPlaylistClick?: (playlistId: number) => void;
    onTrackPlay?: (track: DisplayTrack) => void;
    onTrackPlayNext?: (track: Track) => void;
    onTrackPlayLater?: (track: Track) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
  }

  let {
    labelId,
    labelName = '',
    onBack,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAddAlbumToPlaylist,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    checkAlbumFullyDownloaded,
    downloadStateVersion,
    onArtistClick,
    onLabelClick,
    onNavigateReleases,
    onPlaylistClick,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddToPlaylist,
    onTrackGoToAlbum,
    activeTrackId = null,
    isPlaybackActive = false,
  }: Props = $props();

  // State
  let loading = $state(true);
  let error = $state<string | null>(null);
  let pageData = $state<LabelPageData | null>(null);

  // Parsed sections
  let topTracks = $state<Track[]>([]);
  let releases = $state<QobuzAlbum[]>([]);
  let criticsPicks = $state<QobuzAlbum[]>([]);
  let playlists = $state<Record<string, unknown>[]>([]);
  let artists = $state<Record<string, unknown>[]>([]);
  let moreLabels = $state<LabelExploreItem[]>([]);

  // Track expand state (like ArtistDetailView: 5 → 10 → all)
  let visibleTracksCount = $state(5);
  let showTracksContextMenu = $state(false);

  // Description expand
  let descriptionExpanded = $state(false);

  // Failed images
  let failedArtistImages = $state(new Set<number>());
  let failedLabelImages = $state(new Set<number>());

  let visibleTracks = $derived(topTracks.slice(0, visibleTracksCount));
  let canLoadMoreTracks = $derived(topTracks.length > visibleTracksCount);

  function loadMoreTracks() {
    if (visibleTracksCount === 5) {
      visibleTracksCount = 10;
    } else if (visibleTracksCount === 10) {
      visibleTracksCount = topTracks.length;
    }
  }

  function showLessTracks() {
    visibleTracksCount = 5;
  }

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function getLabelImage(data: LabelPageData): string {
    if (!data.image) return '';
    if (typeof data.image === 'string') return data.image;
    const img = data.image as Record<string, string>;
    return img.mega || img.extralarge || img.large || img.thumbnail || img.small || '';
  }

  function parseLabelExploreImage(item: LabelExploreItem): string {
    if (!item.image) return '';
    if (typeof item.image === 'string') return item.image;
    const img = item.image as Record<string, string>;
    return img.large || img.thumbnail || img.small || '';
  }

  function parseTopTracks(rawTracks: Record<string, unknown>[]): Track[] {
    return rawTracks.map(raw => {
      const albumRaw = raw.album as Record<string, unknown> | undefined;
      const performerRaw = raw.performer as Record<string, unknown> | undefined;
      const artistRaw = raw.artist as Record<string, unknown> | undefined;
      const audioInfo = raw.audio_info as Record<string, unknown> | undefined;
      const rights = raw.rights as Record<string, unknown> | undefined;
      const physicalSupport = raw.physical_support as Record<string, unknown> | undefined;

      let album: Track['album'] | undefined;
      if (albumRaw) {
        const albumImage = albumRaw.image as Record<string, string> | undefined;
        album = {
          id: String(albumRaw.id || ''),
          title: String(albumRaw.title || ''),
          image: albumImage,
        };
      }

      const performer = performerRaw ?? artistRaw;
      let performerOut: Track['performer'] | undefined;
      if (performer) {
        const nameVal = performer.name;
        const displayName = typeof nameVal === 'object' && nameVal !== null
          ? (nameVal as Record<string, string>).display || ''
          : String(nameVal || '');
        performerOut = {
          id: performer.id as number | undefined,
          name: displayName,
        };
      }

      return {
        id: raw.id as number,
        title: String(raw.title || ''),
        duration: (raw.duration as number) || 0,
        album,
        performer: performerOut,
        hires_streamable: (rights?.hires_streamable as boolean) ?? (raw.hires_streamable as boolean) ?? false,
        maximum_bit_depth: (audioInfo?.maximum_bit_depth as number) ?? (raw.maximum_bit_depth as number),
        maximum_sampling_rate: (audioInfo?.maximum_sampling_rate as number) ?? (raw.maximum_sampling_rate as number),
        isrc: raw.isrc as string | undefined,
      };
    });
  }

  async function loadLabelPage() {
    loading = true;
    error = null;

    try {
      const result = await invoke<LabelPageData>('v2_get_label_page', { labelId });
      pageData = result;

      // Parse top tracks
      if (result.top_tracks && result.top_tracks.length > 0) {
        topTracks = parseTopTracks(result.top_tracks as Record<string, unknown>[]);
      }

      // Parse releases containers
      if (result.releases && result.releases.length > 0) {
        // First container = main releases
        const firstContainer = result.releases[0];
        if (firstContainer?.data?.items) {
          releases = firstContainer.data.items as unknown as QobuzAlbum[];
        }

        // Look for critics' picks / awarded container
        for (let i = 1; i < result.releases.length; i++) {
          const container = result.releases[i];
          const containerId = container?.id?.toLowerCase() || '';
          if (containerId.includes('award') || containerId.includes('critic') || containerId.includes('press')) {
            if (container?.data?.items) {
              criticsPicks = container.data.items as unknown as QobuzAlbum[];
            }
            break;
          }
        }

        // If no critics' picks found via ID, try the second container
        if (criticsPicks.length === 0 && result.releases.length > 1) {
          const secondContainer = result.releases[1];
          if (secondContainer?.data?.items && (secondContainer.data.items as unknown[]).length > 0) {
            criticsPicks = secondContainer.data.items as unknown as QobuzAlbum[];
          }
        }
      }

      // Parse playlists
      if (result.playlists?.items && result.playlists.items.length > 0) {
        playlists = result.playlists.items as Record<string, unknown>[];
      }

      // Parse artists
      if (result.top_artists?.items && result.top_artists.items.length > 0) {
        artists = result.top_artists.items as Record<string, unknown>[];
      }
    } catch (e) {
      console.error('Failed to load label page:', e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function loadMoreLabels() {
    try {
      const result = await invoke<{ has_more?: boolean; items?: LabelExploreItem[] }>(
        'v2_get_label_explore', { limit: 20, offset: 0 }
      );
      if (result?.items) {
        // Filter out current label
        moreLabels = (result.items as LabelExploreItem[]).filter(item => item.id !== labelId);
      }
    } catch (e) {
      console.error('Failed to load more labels:', e);
    }
  }

  // Track playback
  function buildTopTracksQueue(tracks: Track[]) {
    return tracks.map((track) => ({
      id: track.id,
      title: track.title,
      artist: track.performer?.name || pageData?.name || '',
      album: track.album?.title || '',
      duration_secs: track.duration,
      artwork_url: track.album?.image?.large || track.album?.image?.thumbnail || '',
      hires: track.hires_streamable ?? false,
      bit_depth: track.maximum_bit_depth ?? null,
      sample_rate: track.maximum_sampling_rate ?? null,
      is_local: false,
      album_id: track.album?.id || null,
      artist_id: track.performer?.id ?? null,
    }));
  }

  async function handleTrackPlay(track: Track, trackIndex?: number) {
    if (topTracks.length > 0) {
      const trackIds = topTracks.map((trk) => trk.id);
      const index = trackIndex !== undefined ? trackIndex : trackIds.indexOf(track.id);

      if (index >= 0) {
        await setPlaybackContext(
          'label_top',
          labelId.toString(),
          pageData?.name || labelName,
          'qobuz',
          trackIds,
          index
        );
        try {
          const queueTracks = buildTopTracksQueue(topTracks);
          await invoke('v2_set_queue', { tracks: queueTracks, startIndex: index });
        } catch (err) {
          console.error('Failed to set queue:', err);
        }
      }
    }

    if (onTrackPlay) {
      const displayTrack: DisplayTrack = {
        id: track.id,
        title: track.title,
        artist: track.performer?.name || '',
        album: track.album?.title || '',
        albumArt: track.album?.image?.large || track.album?.image?.thumbnail || '',
        duration: formatDuration(track.duration),
        durationSeconds: track.duration,
        hires: track.hires_streamable,
        bitDepth: track.maximum_bit_depth,
        samplingRate: track.maximum_sampling_rate,
        albumId: track.album?.id,
        artistId: track.performer?.id,
        isrc: track.isrc,
      };
      onTrackPlay(displayTrack);
    }
  }

  async function handlePlayAllTracks() {
    if (topTracks.length === 0 || !onTrackPlay) return;
    await handleTrackPlay(topTracks[0], 0);
  }

  function handlePlayAllTracksNext() {
    if (!onTrackPlayNext) return;
    for (let i = topTracks.length - 1; i >= 0; i--) {
      onTrackPlayNext(topTracks[i]);
    }
  }

  function handlePlayAllTracksLater() {
    if (!onTrackPlayLater) return;
    for (const track of topTracks) {
      onTrackPlayLater(track);
    }
  }

  async function handleShuffleAllTracks() {
    if (topTracks.length === 0 || !onTrackPlay) return;
    const randomIndex = Math.floor(Math.random() * topTracks.length);
    await handleTrackPlay(topTracks[randomIndex], randomIndex);
  }

  function handleAddAllTracksToPlaylist() {
    if (!onTrackAddToPlaylist || topTracks.length === 0) return;
    onTrackAddToPlaylist(topTracks[0].id);
  }

  function handlePausePlayback(event: MouseEvent) {
    event.stopPropagation();
    void togglePlay();
  }

  function handleArtistImageError(artistId: number) {
    failedArtistImages = new Set([...failedArtistImages, artistId]);
  }

  function handleLabelImageError(itemId: number) {
    failedLabelImages = new Set([...failedLabelImages, itemId]);
  }

  function getArtistImage(artist: Record<string, unknown>): string | null {
    const images = artist.images as Record<string, unknown> | undefined;
    if (!images) return null;
    const portrait = images.portrait as Record<string, string> | undefined;
    if (portrait?.hash && portrait?.format) {
      return `https://static.qobuz.com/images/artists/covers/medium/${portrait.hash}.${portrait.format}`;
    }
    // Fallback: direct image fields
    const image = artist.image as Record<string, string> | undefined;
    return image?.large || image?.thumbnail || image?.small || null;
  }

  function getArtistName(artist: Record<string, unknown>): string {
    const name = artist.name;
    if (typeof name === 'object' && name !== null) {
      return (name as Record<string, string>).display || '';
    }
    return String(name || '');
  }

  function getPlaylistImage(playlist: Record<string, unknown>): string {
    const images = playlist.images as Record<string, unknown> | undefined;
    if (images) {
      const rectangle = images.rectangle as string[] | string | undefined;
      if (Array.isArray(rectangle) && rectangle.length > 0) return rectangle[0];
      if (typeof rectangle === 'string') return rectangle;
      const covers = images.covers as string[] | undefined;
      if (covers && covers.length > 0) return covers[0];
    }
    const images300 = playlist.images300 as string[] | undefined;
    if (images300 && images300.length > 0) return images300[0];
    return '';
  }

  // Download status tracking
  let albumOfflineCacheStatuses = $state<Map<string, boolean>>(new Map());

  async function loadAlbumOfflineCacheStatus(albumId: string) {
    if (!checkAlbumFullyDownloaded) return false;
    try {
      const isDownloaded = await checkAlbumFullyDownloaded(albumId);
      albumOfflineCacheStatuses.set(albumId, isDownloaded);
      return isDownloaded;
    } catch {
      return false;
    }
  }

  async function loadAllAlbumDownloadStatuses(albumList: QobuzAlbum[]) {
    if (!checkAlbumFullyDownloaded || albumList.length === 0) return;
    await Promise.all(albumList.map(album => loadAlbumOfflineCacheStatus(album.id)));
  }

  function isAlbumDownloaded(albumId: string): boolean {
    void downloadStateVersion;
    return albumOfflineCacheStatuses.get(albumId) || false;
  }

  onMount(() => {
    loadLabelPage();
    loadMoreLabels();
  });

  // Reload when labelId changes
  $effect(() => {
    void labelId;
    loading = true;
    visibleTracksCount = 5;
    descriptionExpanded = false;
    topTracks = [];
    releases = [];
    criticsPicks = [];
    playlists = [];
    artists = [];
    moreLabels = [];
    loadLabelPage();
    loadMoreLabels();
  });

  // Load download statuses when releases change
  $effect(() => {
    if (releases.length > 0) {
      loadAllAlbumDownloadStatuses(releases);
    }
  });
</script>

<div class="label-detail-view">
  {#if loading}
    <div class="loading-state">
      <div class="spinner"></div>
      <p>{$t('actions.loading')}</p>
    </div>
  {:else if error}
    <div class="error-state">
      <Disc3 size={48} />
      <p>{error}</p>
      <button class="retry-btn" onclick={loadLabelPage}>{$t('actions.retry')}</button>
    </div>
  {:else if pageData}
    <!-- Header -->
    <header class="label-header">
      <div class="label-image-wrapper">
        {#if getLabelImage(pageData)}
          <img
            src={getLabelImage(pageData)}
            alt={pageData.name}
            class="label-image"
            loading="lazy"
            decoding="async"
          />
        {:else}
          <div class="label-image-placeholder">
            <Disc3 size={48} />
          </div>
        {/if}
      </div>
      <div class="label-header-info">
        <div class="label-subtitle">{$t('label.title')}</div>
        <h1 class="label-name">{pageData.name}</h1>
        {#if pageData.description}
          <div class="label-description" class:expanded={descriptionExpanded}>
            <p>{@html pageData.description}</p>
          </div>
          <button class="read-more-btn" onclick={() => descriptionExpanded = !descriptionExpanded}>
            {descriptionExpanded ? $t('label.readLess') : $t('label.readMore')}
          </button>
        {/if}
        {#if topTracks.length > 0}
          <button class="play-btn" onclick={handlePlayAllTracks}>
            <Play size={18} fill="currentColor" color="currentColor" />
            <span>{$t('actions.play')}</span>
          </button>
        {/if}
      </div>
    </header>

    <!-- Popular Tracks -->
    {#if topTracks.length > 0}
      <div class="section popular-tracks-section">
        <div class="section-header">
          <div class="section-header-left">
            <h2 class="section-title">{$t('label.popularTracks')}</h2>
          </div>
          <div class="section-header-actions">
            <button class="action-btn-circle primary" onclick={handlePlayAllTracks} title={$t('actions.play')}>
              <Play size={20} fill="currentColor" color="currentColor" />
            </button>
            <div class="context-menu-wrapper">
              <button
                class="action-btn-circle"
                onclick={() => showTracksContextMenu = !showTracksContextMenu}
                title={$t('actions.moreOptions')}
              >
                <MoreHorizontal size={18} />
              </button>
              {#if showTracksContextMenu}
                <div class="context-menu-backdrop" onclick={() => showTracksContextMenu = false} role="presentation"></div>
                <div class="context-menu">
                  <button class="context-menu-item" onclick={() => { handlePlayAllTracksNext(); showTracksContextMenu = false; }}>
                    {$t('player.playNext')}
                  </button>
                  <button class="context-menu-item" onclick={() => { handlePlayAllTracksLater(); showTracksContextMenu = false; }}>
                    {$t('player.addToQueue')}
                  </button>
                  <button class="context-menu-item" onclick={() => { handleShuffleAllTracks(); showTracksContextMenu = false; }}>
                    {$t('player.shuffle')}
                  </button>
                  <button class="context-menu-item" onclick={() => { handleAddAllTracksToPlaylist(); showTracksContextMenu = false; }}>
                    {$t('playlist.addToPlaylist')}
                  </button>
                </div>
              {/if}
            </div>
          </div>
        </div>

        <div class="tracks-list">
          {#each visibleTracks as track, index}
            {@const isActiveTrack = isPlaybackActive && activeTrackId === track.id}
            <div
              class="track-row"
              class:playing={isActiveTrack}
              role="button"
              tabindex="0"
              onclick={() => handleTrackPlay(track, index)}
              onkeydown={(e) => e.key === 'Enter' && handleTrackPlay(track, index)}
            >
              <div class="track-number">{index + 1}</div>
              <div class="track-artwork">
                <div class="track-artwork-placeholder">
                  <Music size={16} />
                </div>
                {#if track.album?.image?.thumbnail || track.album?.image?.small}
                  <img src={track.album?.image?.thumbnail || track.album?.image?.small} alt={track.title} loading="lazy" decoding="async" />
                {/if}
                <button
                  class="track-play-overlay"
                  class:is-playing={isActiveTrack}
                  onclick={(event) => {
                    if (isActiveTrack) {
                      handlePausePlayback(event);
                    } else {
                      event.stopPropagation();
                      handleTrackPlay(track, index);
                    }
                  }}
                  aria-label={isActiveTrack ? 'Pause' : 'Play'}
                >
                  <span class="play-icon" aria-hidden="true">
                    <Play size={18} />
                  </span>
                  <div class="playing-indicator" aria-hidden="true">
                    <div class="bar"></div>
                    <div class="bar"></div>
                    <div class="bar"></div>
                  </div>
                  <span class="pause-icon" aria-hidden="true">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="white">
                      <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z"/>
                    </svg>
                  </span>
                </button>
              </div>
              <div class="track-info">
                <div class="track-title">{track.title}</div>
                <div class="track-meta">
                  {#if track.performer?.name}
                    {#if track.performer?.id && onArtistClick}
                      <button class="track-link" type="button" onclick={(e) => { e.stopPropagation(); onArtistClick?.(track.performer!.id!); }}>
                        {track.performer.name}
                      </button>
                    {:else}
                      <span>{track.performer.name}</span>
                    {/if}
                  {/if}
                  {#if track.album?.title}
                    <span class="separator">·</span>
                    {#if track.album?.id && onTrackGoToAlbum}
                      <button class="track-link" type="button" onclick={(e) => { e.stopPropagation(); onTrackGoToAlbum?.(track.album!.id); }}>
                        {track.album.title}
                      </button>
                    {:else}
                      <span>{track.album.title}</span>
                    {/if}
                  {/if}
                </div>
              </div>
              <div class="track-quality">
                <QualityBadge
                  bitDepth={track.maximum_bit_depth}
                  samplingRate={track.maximum_sampling_rate}
                  compact
                />
              </div>
              <div class="track-duration">{formatDuration(track.duration)}</div>
              <div class="track-actions">
                <TrackMenu
                  onPlayNow={() => handleTrackPlay(track, index)}
                  onPlayNext={onTrackPlayNext ? () => onTrackPlayNext(track) : undefined}
                  onPlayLater={onTrackPlayLater ? () => onTrackPlayLater(track) : undefined}
                  onAddToPlaylist={onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined}
                  onGoToAlbum={track.album?.id && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.album!.id) : undefined}
                />
              </div>
            </div>
          {/each}
        </div>

        {#if canLoadMoreTracks}
          <button class="load-more-link" onclick={loadMoreTracks}>
            {$t('label.showMore')} <ChevronDown size={14} />
          </button>
        {:else if visibleTracksCount > 5 && topTracks.length > 5}
          <button class="load-more-link" onclick={showLessTracks}>
            {$t('label.showLess')} <ChevronUp size={14} />
          </button>
        {/if}
      </div>
    {/if}

    <!-- Releases -->
    {#if releases.length > 0}
      <div class="section">
        <HorizontalScrollRow>
          {#snippet header()}
            <h2 class="section-title">{$t('label.releases')}</h2>
            {#if onNavigateReleases}
              <button class="see-all-btn" onclick={() => onNavigateReleases?.(labelId, pageData?.name || labelName)}>
                {$t('label.seeAll')}
              </button>
            {/if}
          {/snippet}
          {#snippet children()}
            {#each releases.slice(0, 20) as album (album.id)}
              <AlbumCard
                albumId={album.id}
                artwork={album.image?.large || album.image?.thumbnail || ''}
                title={album.title}
                artist={album.artist?.name || ''}
                artistId={album.artist?.id}
                onArtistClick={onArtistClick}
                releaseDate={album.release_date_original}
                size="large"
                onclick={() => onAlbumClick?.(album.id)}
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
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      </div>
    {/if}

    <!-- Critics' Picks -->
    {#if criticsPicks.length > 0}
      <div class="section">
        <HorizontalScrollRow title={$t('label.criticsPicks')}>
          {#snippet children()}
            {#each criticsPicks.slice(0, 20) as album (album.id)}
              <AlbumCard
                albumId={album.id}
                artwork={album.image?.large || album.image?.thumbnail || ''}
                title={album.title}
                artist={album.artist?.name || ''}
                artistId={album.artist?.id}
                onArtistClick={onArtistClick}
                releaseDate={album.release_date_original}
                size="large"
                onclick={() => onAlbumClick?.(album.id)}
                onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
                onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
                onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
                onAddAlbumToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist(album.id) : undefined}
                onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
                onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
                {downloadStateVersion}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      </div>
    {/if}

    <!-- Playlists -->
    {#if playlists.length > 0}
      <div class="section">
        <HorizontalScrollRow title={$t('label.playlists')}>
          {#snippet children()}
            {#each playlists as playlist}
              {@const pid = (playlist.id as number) || 0}
              {@const pname = String(playlist.name || playlist.title || '')}
              {@const powner = (playlist.owner as Record<string, unknown>)?.name as string || ''}
              <QobuzPlaylistCard
                playlistId={pid}
                name={pname}
                owner={powner}
                image={getPlaylistImage(playlist)}
                trackCount={playlist.tracks_count as number}
                duration={playlist.duration as number}
                onclick={() => onPlaylistClick?.(pid)}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      </div>
    {/if}

    <!-- Artists -->
    {#if artists.length > 0}
      <div class="section">
        <HorizontalScrollRow title={$t('label.artists')}>
          {#snippet children()}
            {#each artists as artist}
              {@const artistId = artist.id as number}
              {@const artistName = getArtistName(artist)}
              {@const artistImage = getArtistImage(artist)}
              <button class="artist-card" onclick={() => onArtistClick?.(artistId)}>
                <div class="artist-image-wrapper">
                  <div class="artist-image-placeholder">
                    <User size={48} />
                  </div>
                  {#if !failedArtistImages.has(artistId) && artistImage}
                    <img
                      src={artistImage}
                      alt={artistName}
                      class="artist-image"
                      loading="lazy"
                      decoding="async"
                      onerror={() => handleArtistImageError(artistId)}
                    />
                  {/if}
                </div>
                <div class="artist-name">{artistName}</div>
              </button>
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      </div>
    {/if}

    <!-- More Labels -->
    {#if moreLabels.length > 0}
      <div class="section">
        <HorizontalScrollRow title={$t('label.moreLabels')}>
          {#snippet children()}
            {#each moreLabels as item}
              {@const itemImage = parseLabelExploreImage(item)}
              <button class="label-card" onclick={() => onLabelClick?.(item.id, item.name)}>
                <div class="label-card-image-wrapper">
                  <div class="label-card-image-placeholder">
                    <Disc3 size={36} />
                  </div>
                  {#if !failedLabelImages.has(item.id) && itemImage}
                    <img
                      src={itemImage}
                      alt={item.name}
                      class="label-card-image"
                      loading="lazy"
                      decoding="async"
                      onerror={() => handleLabelImageError(item.id)}
                    />
                  {/if}
                </div>
                <div class="label-card-name">{item.name}</div>
              </button>
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      </div>
    {/if}
  {/if}
</div>

<style>
  .label-detail-view {
    padding: 24px;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  .label-detail-view::-webkit-scrollbar {
    width: 6px;
  }

  .label-detail-view::-webkit-scrollbar-track {
    background: transparent;
  }

  .label-detail-view::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .label-detail-view::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  /* Loading / Error states */
  .loading-state,
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px 24px;
    color: var(--text-muted);
    text-align: center;
  }

  .loading-state p,
  .error-state p {
    margin: 16px 0 0 0;
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

  .retry-btn {
    margin-top: 16px;
    padding: 8px 24px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    cursor: pointer;
  }

  .retry-btn:hover {
    opacity: 0.9;
  }

  /* Header */
  .label-header {
    display: flex;
    gap: 24px;
    margin-bottom: 40px;
  }

  .label-image-wrapper {
    width: 180px;
    height: 180px;
    border-radius: 16px;
    overflow: hidden;
    flex-shrink: 0;
    background: var(--bg-tertiary);
  }

  .label-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .label-image-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%);
    color: white;
  }

  .label-header-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .label-subtitle {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.1em;
    margin-bottom: 4px;
  }

  .label-name {
    font-size: 32px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 8px 0;
    line-height: 1.2;
  }

  .label-description {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.6;
    max-height: 3.2em;
    overflow: hidden;
    margin-bottom: 4px;
  }

  .label-description.expanded {
    max-height: none;
  }

  .label-description p {
    margin: 0;
  }

  .read-more-btn {
    background: none;
    border: none;
    color: var(--accent-primary);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    padding: 0;
    margin-bottom: 12px;
    text-align: left;
    letter-spacing: 0.05em;
  }

  .read-more-btn:hover {
    text-decoration: underline;
  }

  .play-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px 24px;
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 24px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity 150ms ease;
    width: fit-content;
  }

  .play-btn:hover {
    opacity: 0.9;
  }

  /* Section layout */
  .section {
    margin-bottom: 8px;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 20px;
  }

  .section-header-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .section-title {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .section-header-actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .see-all-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    padding: 4px 8px;
    border-radius: 4px;
    transition: color 150ms ease;
  }

  .see-all-btn:hover {
    color: var(--text-primary);
  }

  /* Context menu */
  .context-menu-wrapper {
    position: relative;
  }

  .context-menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 99;
  }

  .context-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 8px;
    min-width: 160px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 2px 0;
    z-index: 100;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .context-menu-item {
    display: block;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    text-align: left;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .context-menu-item:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  /* Tracks */
  .popular-tracks-section {
    margin-bottom: 32px;
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
    position: relative;
  }

  .track-artwork img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    z-index: 1;
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

  .track-play-overlay {
    position: absolute;
    inset: 0;
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
    animation: label-equalize 1s ease-in-out infinite;
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

  @keyframes label-equalize {
    0%, 100% {
      transform: scaleY(0.5);
      opacity: 0.7;
    }
    50% {
      transform: scaleY(1);
      opacity: 1;
    }
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

  .track-meta {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .track-meta .separator {
    color: var(--text-muted);
    opacity: 0.5;
  }

  .track-link {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
    color: inherit;
    font-size: inherit;
  }

  .track-link:hover {
    color: var(--text-primary);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .track-quality {
    display: flex;
    align-items: center;
  }

  .track-duration {
    font-size: 13px;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .track-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-left: 8px;
  }

  .load-more-link {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    width: 100%;
    padding: 16px;
    background: none;
    border: none;
    text-align: center;
    font-size: 13px;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 150ms ease;
  }

  .load-more-link:hover {
    color: var(--text-primary);
  }

  /* Artist cards */
  .artist-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 120px;
    flex-shrink: 0;
    background: none;
    border: none;
    cursor: pointer;
    padding: 8px;
    border-radius: 8px;
    transition: background-color 150ms ease;
  }

  .artist-card:hover {
    background-color: var(--bg-tertiary);
  }

  .artist-image-wrapper {
    width: 100px;
    height: 100px;
    border-radius: 50%;
    overflow: hidden;
    position: relative;
  }

  .artist-image-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: var(--bg-tertiary);
    color: var(--text-muted);
  }

  .artist-image {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    z-index: 1;
  }

  .artist-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  /* Label cards */
  .label-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 140px;
    flex-shrink: 0;
    background: none;
    border: none;
    cursor: pointer;
    padding: 8px;
    border-radius: 8px;
    transition: background-color 150ms ease;
  }

  .label-card:hover {
    background-color: var(--bg-tertiary);
  }

  .label-card-image-wrapper {
    width: 120px;
    height: 120px;
    border-radius: 12px;
    overflow: hidden;
    position: relative;
    background: var(--bg-tertiary);
  }

  .label-card-image-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    background: linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%);
    color: white;
  }

  .label-card-image {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    z-index: 1;
  }

  .label-card-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .spacer {
    width: 8px;
    flex-shrink: 0;
  }
</style>
