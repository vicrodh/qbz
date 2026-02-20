<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import { ArrowLeft, Disc3, Play, Music, MoreHorizontal, Heart, User, ChevronDown, ChevronUp } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import AlbumCard from '../AlbumCard.svelte';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import QobuzPlaylistCard from '../QobuzPlaylistCard.svelte';
  import TrackMenu from '../TrackMenu.svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import { setPlaybackContext } from '$lib/stores/playbackContextStore';
  import { togglePlay } from '$lib/stores/playerStore';
  import {
    subscribe as subscribeFavorites,
    isTrackFavorite,
    isTrackToggling,
    toggleTrackFavorite
  } from '$lib/stores/favoritesStore';
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
    onTrackAddFavorite?: (trackId: number) => void;
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
    onTrackAddFavorite,
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

  // Artist image cache (fetched via v2_get_artist)
  let artistImageMap = $state<Map<number, string>>(new Map());

  // Track expand state (like ArtistDetailView: 5 → 20 → 50)
  let visibleTracksCount = $state(5);
  let showTracksContextMenu = $state(false);

  // Description expand
  let descriptionExpanded = $state(false);
  let labelDescription = $state<string | null>(null);

  // Favorites reactivity
  let trackFavoritesVersion = $state(0);
  let unsubFavorites: (() => void) | null = null;

  function checkTrackFav(trackId: number): boolean {
    return trackFavoritesVersion >= 0 && isTrackFavorite(trackId);
  }
  function checkTrackToggling(trackId: number): boolean {
    return trackFavoritesVersion >= 0 && isTrackToggling(trackId);
  }

  // Failed images
  let failedArtistImages = $state(new Set<number>());
  let failedLabelImages = $state(new Set<number>());

  // Jump-nav
  let labelDetailEl = $state<HTMLDivElement | null>(null);
  let headerSection = $state<HTMLElement | null>(null);
  let popularTracksSection = $state<HTMLDivElement | null>(null);
  let releasesSection = $state<HTMLDivElement | null>(null);
  let criticsPicksSection = $state<HTMLDivElement | null>(null);
  let playlistsSection = $state<HTMLDivElement | null>(null);
  let artistsSection = $state<HTMLDivElement | null>(null);
  let moreLabelsSection = $state<HTMLDivElement | null>(null);
  let activeJumpSection = $state('about');
  let jumpObserver: IntersectionObserver | null = null;

  let hasTopTracks = $derived(topTracks.length > 0);
  let hasReleases = $derived(releases.length > 0);
  let hasCriticsPicks = $derived(criticsPicks.length > 0);
  let hasPlaylists = $derived(playlists.length > 0);
  let hasArtists = $derived(artists.length > 0);
  let hasMoreLabels = $derived(moreLabels.length > 0);

  let jumpSections = $derived.by(() => [
    { id: 'about', labelKey: 'label.about', el: headerSection, visible: true },
    { id: 'popular', labelKey: 'label.popularTracks', el: popularTracksSection, visible: hasTopTracks },
    { id: 'releases', labelKey: 'label.releases', el: releasesSection, visible: hasReleases },
    { id: 'critics', labelKey: 'label.criticsPicks', el: criticsPicksSection, visible: hasCriticsPicks },
    { id: 'playlists', labelKey: 'label.playlists', el: playlistsSection, visible: hasPlaylists },
    { id: 'artists', labelKey: 'label.artists', el: artistsSection, visible: hasArtists },
    { id: 'labels', labelKey: 'label.otherLabels', el: moreLabelsSection, visible: hasMoreLabels },
  ].filter(section => section.visible));

  let showJumpNav = $derived(jumpSections.length > 1);

  function scrollToSection(target: HTMLElement | null, id: string) {
    activeJumpSection = id;
    target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  let visibleTracks = $derived(topTracks.slice(0, visibleTracksCount));
  let canLoadMoreTracks = $derived(visibleTracksCount < 50 && topTracks.length > visibleTracksCount);

  function loadMoreTracks() {
    if (visibleTracksCount === 5) {
      visibleTracksCount = 20;
    } else if (visibleTracksCount === 20) {
      visibleTracksCount = 50;
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

      console.log('[LabelView] Raw label page data:', JSON.stringify(result).slice(0, 500));

      // Set description from page data
      if (result.description) {
        labelDescription = result.description;
      }

      // Parse top tracks
      if (result.top_tracks && result.top_tracks.length > 0) {
        topTracks = parseTopTracks(result.top_tracks as Record<string, unknown>[]);
        console.log(`[LabelView] Parsed ${topTracks.length} top tracks`);
      }

      // Parse critics' picks from label/page release containers
      if (result.releases && result.releases.length > 0) {
        for (const container of result.releases) {
          const containerId = container?.id?.toLowerCase() || '';
          if (containerId.includes('award') || containerId.includes('critic') || containerId.includes('press')) {
            if (container?.data?.items) {
              criticsPicks = container.data.items as unknown as QobuzAlbum[];
              console.log(`[LabelView] Critics' picks: ${criticsPicks.length} albums from container '${container.id}'`);
            }
            break;
          }
        }
      }

      // Parse playlists
      if (result.playlists?.items && result.playlists.items.length > 0) {
        playlists = result.playlists.items as Record<string, unknown>[];
        console.log(`[LabelView] Parsed ${playlists.length} playlists, first:`, JSON.stringify(playlists[0]).slice(0, 300));
      }

      // Parse artists
      if (result.top_artists?.items && result.top_artists.items.length > 0) {
        artists = result.top_artists.items as Record<string, unknown>[];
        console.log(`[LabelView] Parsed ${artists.length} artists, first:`, JSON.stringify(artists[0]).slice(0, 300));
      }
    } catch (e) {
      console.error('Failed to load label page:', e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function loadLabelAlbumsAndDescription() {
    // Fetch albums + description from v2_get_label (same endpoint as old LabelView)
    try {
      const result = await invoke<{
        description?: string;
        albums?: { items?: QobuzAlbum[]; total?: number };
        albums_count?: number;
      }>('v2_get_label', { labelId, limit: 20, offset: 0 });

      // Set releases from the albums
      if (result?.albums?.items && result.albums.items.length > 0) {
        releases = result.albums.items;
        console.log(`[LabelView] Loaded ${releases.length} releases from v2_get_label`);
      }

      // Set description if not already set from label/page
      if (!labelDescription && result?.description) {
        labelDescription = result.description;
        console.log('[LabelView] Description loaded from v2_get_label');
      }
    } catch (err) {
      console.error('[LabelView] Failed to load label albums/description:', err);
    }
  }

  async function loadMoreLabels() {
    try {
      const result = await invoke<{ has_more?: boolean; items?: LabelExploreItem[] }>(
        'v2_get_label_explore', { limit: 20, offset: 0 }
      );
      if (result?.items) {
        moreLabels = (result.items as LabelExploreItem[]).filter(item => item.id !== labelId);
      }
    } catch (e) {
      console.error('Failed to load more labels:', e);
    }
  }

  async function loadArtistImages(artistList: Record<string, unknown>[]) {
    // Fetch images for artists that don't have them from the label page data
    const fetches = artistList.map(async (artist) => {
      const id = artist.id as number;
      // Skip if we already have a local image from the label page data
      if (getArtistImageUrl(artist)) return;
      try {
        const detail = await invoke<{ image?: { small?: string; thumbnail?: string; large?: string } }>(
          'v2_get_artist', { artistId: id }
        );
        const url = detail?.image?.large || detail?.image?.thumbnail || detail?.image?.small;
        if (url) {
          artistImageMap = new Map([...artistImageMap, [id, url]]);
        }
      } catch {
        // Silently skip — placeholder will show
      }
    });
    await Promise.allSettled(fetches);
  }

  // Track playback — mirrors ArtistDetailView exactly
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
    console.log('[LabelView] handleTrackPlay called:', { trackId: track.id, title: track.title, trackIndex });

    // Set context and queue — wrapped in try-catch so playback still starts even if context fails
    try {
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
          console.log(`[LabelView] Context set: "${pageData?.name}" top tracks, ${trackIds.length} tracks, index ${index}`);

          const queueTracks = buildTopTracksQueue(topTracks);
          await invoke('v2_set_queue', { tracks: queueTracks, startIndex: index });
          console.log('[LabelView] Queue set successfully');
        }
      }
    } catch (err) {
      console.error('[LabelView] Failed to set context/queue (continuing to play):', err);
    }

    // Always fire play — even if context/queue setup failed
    if (onTrackPlay) {
      console.log('[LabelView] Calling onTrackPlay with track:', track.id);
      onTrackPlay({
        id: track.id,
        title: track.title,
        artist: track.performer?.name || pageData?.name || '',
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
      });
    } else {
      console.warn('[LabelView] onTrackPlay is not defined!');
    }
  }

  async function handlePlayAllTracks() {
    if (topTracks.length === 0) return;
    if (!onTrackPlay) {
      console.warn('[LabelView] handlePlayAllTracks: onTrackPlay is not defined!');
      return;
    }
    try {
      await handleTrackPlay(topTracks[0], 0);
    } catch (err) {
      console.error('[LabelView] handlePlayAllTracks failed:', err);
    }
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

  async function createTrackRadio(track: Track) {
    try {
      const trackArtistId = track.performer?.id || 0;
      await invoke<string>('v2_create_track_radio', {
        trackId: track.id,
        trackName: track.title,
        artistId: trackArtistId
      });
    } catch (err) {
      console.error('Failed to create track radio:', err);
    }
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

  function getArtistImageUrl(artist: Record<string, unknown>): string | null {
    // 0. Check fetched image cache
    const cached = artistImageMap.get(artist.id as number);
    if (cached) return cached;
    // 1. image object (most common in search results)
    const image = artist.image as Record<string, string> | null | undefined;
    if (image && typeof image === 'object') {
      const url = image.large || image.extralarge || image.medium || image.thumbnail || image.small;
      if (url) return url;
    }
    // 2. picture field (string URL, some endpoints)
    if (typeof artist.picture === 'string' && artist.picture) {
      return artist.picture;
    }
    // 3. images.portrait hash (artist page format)
    const images = artist.images as Record<string, unknown> | undefined;
    if (images) {
      const portrait = images.portrait as Record<string, string> | undefined;
      if (portrait?.hash && portrait?.format) {
        return `https://static.qobuz.com/images/artists/covers/medium/${portrait.hash}.${portrait.format}`;
      }
    }
    return null;
  }

  function getPlaylistImage(playlist: Record<string, unknown>): string {
    // 1. image.rectangle (discover/label page format)
    const image = playlist.image as Record<string, unknown> | null | undefined;
    if (image && typeof image === 'object') {
      if (typeof image.rectangle === 'string' && image.rectangle) return image.rectangle;
      const covers = image.covers as string[] | undefined;
      if (covers?.length) return covers[0];
      // Try size-based keys
      if (typeof image.large === 'string' && image.large) return image.large as string;
      if (typeof image.thumbnail === 'string' && image.thumbnail) return image.thumbnail as string;
      if (typeof image.small === 'string' && image.small) return image.small as string;
    }
    // 2. images300/images150/images arrays (search format)
    const images300 = playlist.images300 as string[] | undefined;
    if (images300?.length) return images300[0];
    const images150 = playlist.images150 as string[] | undefined;
    if (images150?.length) return images150[0];
    const imagesArr = playlist.images as string[] | undefined;
    if (imagesArr?.length) return imagesArr[0];
    return '';
  }

  function getArtistName(artist: Record<string, unknown>): string {
    const name = artist.name;
    if (typeof name === 'object' && name !== null) {
      return (name as Record<string, string>).display || '';
    }
    return String(name || '');
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
    loadLabelAlbumsAndDescription();
    loadMoreLabels();
    unsubFavorites = subscribeFavorites(() => {
      trackFavoritesVersion++;
    });
  });

  onDestroy(() => {
    unsubFavorites?.();
    jumpObserver?.disconnect();
  });

  // Reload when labelId changes
  $effect(() => {
    void labelId;
    loading = true;
    visibleTracksCount = 5;
    descriptionExpanded = false;
    activeJumpSection = 'about';
    topTracks = [];
    releases = [];
    criticsPicks = [];
    playlists = [];
    artists = [];
    moreLabels = [];
    labelDescription = null;
    artistImageMap = new Map();
    loadLabelPage();
    loadLabelAlbumsAndDescription();
    loadMoreLabels();
  });

  // Load download statuses when releases change
  $effect(() => {
    if (releases.length > 0) {
      loadAllAlbumDownloadStatuses(releases);
    }
  });

  // Fetch artist images when artists are loaded
  $effect(() => {
    if (artists.length > 0) {
      loadArtistImages(artists);
    }
  });

  // Jump-nav IntersectionObserver
  $effect(() => {
    if (!labelDetailEl) return;
    if (jumpObserver) {
      jumpObserver.disconnect();
      jumpObserver = null;
    }

    if (jumpSections.length === 0) return;

    const sectionByElement = new Map<HTMLElement, string>();
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
        root: labelDetailEl,
        rootMargin: '-20% 0px -60% 0px',
        threshold: [0.5]
      }
    );

    targets.forEach(target => jumpObserver?.observe(target));

    return () => {
      jumpObserver?.disconnect();
      jumpObserver = null;
    };
  });
</script>

<div class="label-detail-view" bind:this={labelDetailEl}>
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
    <!-- Back button -->
    <button class="back-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{$t('actions.back')}</span>
    </button>

    <!-- Header -->
    <header class="label-header section-anchor" bind:this={headerSection}>
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
        {#if labelDescription}
          <div class="label-description" class:expanded={descriptionExpanded}>
            {@html labelDescription}
          </div>
          <button class="read-more-btn" onclick={() => descriptionExpanded = !descriptionExpanded}>
            {descriptionExpanded ? $t('label.readLess') : $t('label.readMore')}
          </button>
        {/if}
      </div>
    </header>

    <!-- Jump Nav (below header, sticky on scroll) -->
    {#if showJumpNav}
      <div class="jump-nav">
        <div class="jump-nav-left">
          <div class="jump-label">Jump to</div>
          <div class="jump-links">
            {#each jumpSections as section}
              <button
                class="jump-link"
                class:active={activeJumpSection === section.id}
                onclick={() => scrollToSection(section.el, section.id)}
              >
                {$t(section.labelKey)}
              </button>
            {/each}
          </div>
        </div>
      </div>
    {/if}

    <!-- Popular Tracks (mirrors ArtistDetailView exactly) -->
    {#if topTracks.length > 0}
      <div class="top-tracks-section section-anchor" bind:this={popularTracksSection}>
        <div class="section-header">
          <div class="section-header-left">
            <h2 class="section-title">{$t('label.popularTracks')}</h2>
          </div>
          <div class="section-header-actions">
            <button class="action-btn-circle primary" onclick={handlePlayAllTracks} title="Play All">
              <Play size={20} fill="currentColor" color="currentColor" />
            </button>
            <div class="context-menu-wrapper">
              <button
                class="action-btn-circle"
                onclick={() => showTracksContextMenu = !showTracksContextMenu}
                title="More options"
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
              data-track-id={track.id}
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
                  aria-label={isActiveTrack ? 'Pause track' : 'Play track'}
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
              <div class="track-quality">
                <QualityBadge
                  bitDepth={track.maximum_bit_depth}
                  samplingRate={track.maximum_sampling_rate}
                  compact
                />
              </div>
              <div class="track-duration">{formatDuration(track.duration)}</div>
              <div class="track-actions">
                {#if onTrackAddFavorite}
                  {@const trackIsFav = checkTrackFav(track.id)}
                  {@const trackIsToggling = checkTrackToggling(track.id)}
                  <button
                    class="track-favorite-btn"
                    class:is-favorite={trackIsFav}
                    class:is-toggling={trackIsToggling}
                    onclick={async (e) => {
                      e.stopPropagation();
                      await toggleTrackFavorite(track.id);
                    }}
                    disabled={trackIsToggling}
                    title={trackIsFav ? 'Remove from favorites' : 'Add to favorites'}
                  >
                    {#if trackIsFav}
                      <Heart size={16} fill="var(--accent-primary)" color="var(--accent-primary)" />
                    {:else}
                      <Heart size={16} />
                    {/if}
                  </button>
                {/if}
                <TrackMenu
                  onPlayNow={() => handleTrackPlay(track, index)}
                  onPlayNext={onTrackPlayNext ? () => onTrackPlayNext(track) : undefined}
                  onPlayLater={onTrackPlayLater ? () => onTrackPlayLater(track) : undefined}
                  onCreateRadio={() => createTrackRadio(track)}
                  onAddFavorite={onTrackAddFavorite ? () => onTrackAddFavorite(track.id) : undefined}
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
      <div class="section section-anchor" bind:this={releasesSection}>
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
      <div class="section section-anchor" bind:this={criticsPicksSection}>
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
      <div class="section section-anchor" bind:this={playlistsSection}>
        <HorizontalScrollRow title={$t('label.playlists')}>
          {#snippet children()}
            {#each playlists as playlist (playlist.id)}
              <QobuzPlaylistCard
                playlistId={playlist.id as number}
                name={String(playlist.name || '')}
                owner={(playlist.owner as Record<string, unknown>)?.name as string || 'Qobuz'}
                image={getPlaylistImage(playlist)}
                trackCount={playlist.tracks_count as number | undefined}
                duration={playlist.duration as number | undefined}
                genre={(playlist.genres as { name: string }[])?.[0]?.name}
                onclick={() => onPlaylistClick?.(playlist.id as number)}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      </div>
    {/if}

    <!-- Artists -->
    {#if artists.length > 0}
      <div class="section section-anchor" bind:this={artistsSection}>
        <HorizontalScrollRow title={$t('label.artists')}>
          {#snippet children()}
            {#each artists as artist}
              {@const artistId = artist.id as number}
              {@const artistName = getArtistName(artist)}
              {@const artistImage = getArtistImageUrl(artist)}
              <button class="artist-card" onclick={() => onArtistClick?.(artistId)}>
                <div class="artist-image-wrapper">
                  <div class="artist-image-placeholder">
                    <User size={40} />
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
      <div class="section section-anchor" bind:this={moreLabelsSection}>
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
    padding-top: 0;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  .label-detail-view::-webkit-scrollbar { width: 6px; }
  .label-detail-view::-webkit-scrollbar-track { background: transparent; }
  .label-detail-view::-webkit-scrollbar-thumb { background: var(--bg-tertiary); border-radius: 3px; }
  .label-detail-view::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }

  /* Loading / Error */
  .loading-state, .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px 24px;
    color: var(--text-muted);
    text-align: center;
  }
  .loading-state p, .error-state p { margin: 16px 0 0 0; }
  .spinner {
    width: 32px; height: 32px;
    border: 3px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .retry-btn {
    margin-top: 16px; padding: 8px 24px;
    background-color: var(--accent-primary); color: white;
    border: none; border-radius: 8px; cursor: pointer;
  }
  .retry-btn:hover { opacity: 0.9; }

  /* Back button */
  .back-btn {
    display: flex; align-items: center; gap: 8px;
    font-size: 14px; color: var(--text-muted);
    background: none; border: none; cursor: pointer;
    margin-top: 24px; margin-bottom: 24px; transition: color 150ms ease;
  }
  .back-btn:hover { color: var(--text-secondary); }

  /* Jump Nav */
  .jump-nav {
    position: sticky;
    top: 0;
    z-index: 50;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 10px 24px;
    background: var(--bg-primary);
    border-bottom: 1px solid var(--alpha-6);
    box-shadow: 0 4px 8px -4px rgba(0, 0, 0, 0.5);
    margin: 0 -8px 24px -24px;
    width: calc(100% + 32px);
  }
  .jump-nav-left { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; }
  .jump-label { font-size: 12px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.08em; }
  .jump-links { display: flex; flex-wrap: wrap; gap: 14px; }
  .jump-link {
    padding: 4px 0; border: none; background: none;
    color: var(--text-muted); font-size: 13px; cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: color 150ms ease, border-color 150ms ease;
  }
  .jump-link:hover { color: var(--text-secondary); }
  .jump-link.active { color: var(--text-primary); border-bottom-color: var(--accent-primary); }

  /* Header */
  .label-header { display: flex; gap: 24px; margin-bottom: 40px; }
  .label-image-wrapper {
    width: 180px; height: 180px; border-radius: 50%;
    overflow: hidden; flex-shrink: 0; background: var(--bg-tertiary);
  }
  .label-image { width: 100%; height: 100%; object-fit: cover; }
  .label-image-placeholder {
    width: 100%; height: 100%;
    display: flex; align-items: center; justify-content: center;
    background: linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%); color: white;
  }
  .label-header-info {
    flex: 1; min-width: 0; display: flex; flex-direction: column; justify-content: center;
  }
  .label-subtitle {
    font-size: 12px; font-weight: 600; color: var(--text-muted);
    text-transform: uppercase; letter-spacing: 0.1em; margin-bottom: 4px;
  }
  .label-name {
    font-size: 32px; font-weight: 700; color: var(--text-primary);
    margin: 0 0 8px 0; line-height: 1.2;
  }
  .label-description {
    font-size: 14px; color: var(--text-secondary); line-height: 1.6;
    max-height: 3.2em; overflow: hidden; margin-bottom: 4px;
    border: none; outline: none;
  }
  .label-description.expanded { max-height: none; }
  :global(.label-description p) { margin: 0; border: none; }
  :global(.label-description *) { border: none !important; outline: none !important; }
  .read-more-btn {
    background: none; border: none; color: var(--accent-primary);
    font-size: 12px; font-weight: 600; cursor: pointer;
    padding: 0; margin-bottom: 12px; text-align: left; letter-spacing: 0.05em;
  }
  .read-more-btn:hover { text-decoration: underline; }

  /* Sections */
  .section-anchor { scroll-margin-top: 56px; }
  .section { margin-bottom: 28px; }
  .section-header {
    display: flex; align-items: center; justify-content: space-between;
    gap: 12px; margin-bottom: 20px;
  }
  .section-header-left { display: flex; align-items: center; gap: 12px; }
  .section-title { font-size: 20px; font-weight: 600; color: var(--text-primary); margin: 0; }
  .section-header-actions { display: flex; align-items: center; gap: 12px; }
  .action-btn-circle {
    display: flex; align-items: center; justify-content: center;
    width: 36px; height: 36px; border-radius: 50%;
    background-color: var(--bg-tertiary); border: none;
    color: var(--text-secondary); cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }
  .action-btn-circle:hover { background-color: var(--bg-hover); color: var(--text-primary); }
  .action-btn-circle.primary { background-color: var(--accent-primary); color: white; }
  .action-btn-circle.primary:hover { opacity: 0.9; }
  .see-all-btn {
    background: none; border: none; color: var(--text-muted);
    font-size: 13px; font-weight: 500; cursor: pointer;
    padding: 4px 8px; border-radius: 4px; transition: color 150ms ease;
  }
  .see-all-btn:hover { color: var(--text-primary); }

  /* Context menu */
  .context-menu-wrapper { position: relative; }
  .context-menu-backdrop { position: fixed; inset: 0; z-index: 99; }
  .context-menu {
    position: absolute; top: 100%; right: 0; margin-top: 8px;
    min-width: 160px; background-color: var(--bg-tertiary);
    border-radius: 8px; padding: 2px 0; z-index: 100;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }
  .context-menu-item {
    display: block; width: 100%; padding: 8px 12px;
    background: none; border: none; text-align: left;
    font-size: 12px; color: var(--text-secondary); cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }
  .context-menu-item:hover { background-color: var(--bg-hover); color: var(--text-primary); }

  /* Tracks */
  .top-tracks-section { margin-bottom: 48px; }
  .tracks-list { display: flex; flex-direction: column; }
  .track-row {
    display: flex; align-items: center; gap: 12px;
    padding: 8px 12px; background: none; border: none;
    border-radius: 8px; cursor: pointer; text-align: left;
    width: 100%; transition: background-color 150ms ease;
  }
  .track-row:hover { background-color: var(--bg-tertiary); }
  .track-number { width: 24px; font-size: 14px; color: var(--text-muted); text-align: center; }
  .track-artwork {
    width: 40px; height: 40px; border-radius: 4px;
    overflow: hidden; flex-shrink: 0; position: relative;
  }
  .track-artwork img {
    position: absolute; inset: 0; width: 100%; height: 100%;
    object-fit: cover; z-index: 1;
  }
  .track-artwork-placeholder {
    width: 100%; height: 100%; display: flex;
    align-items: center; justify-content: center;
    background-color: var(--bg-tertiary); color: var(--text-muted);
  }
  .track-play-overlay {
    position: absolute; inset: 0; display: none;
    align-items: center; justify-content: center;
    background: rgba(0, 0, 0, 0.6); border: none;
    cursor: pointer; transition: background 150ms ease; z-index: 2;
  }
  .track-row:hover .track-play-overlay { display: flex; }
  .track-row.playing .track-play-overlay { display: flex; }
  .track-play-overlay:hover { background: rgba(0, 0, 0, 0.75); }
  .track-play-overlay .playing-indicator, .track-play-overlay .pause-icon { display: none; }
  .track-row.playing .track-play-overlay .play-icon { display: none; }
  .track-row.playing .track-play-overlay .playing-indicator { display: flex; }
  .track-row.playing:hover .track-play-overlay .playing-indicator { display: none; }
  .track-row.playing:hover .track-play-overlay .pause-icon { display: inline-flex; }

  .playing-indicator { display: flex; align-items: center; gap: 2px; }
  .playing-indicator .bar {
    width: 3px; background-color: var(--accent-primary);
    border-radius: 9999px; transform-origin: bottom;
    animation: label-equalize 1s ease-in-out infinite;
  }
  .playing-indicator .bar:nth-child(1) { height: 10px; }
  .playing-indicator .bar:nth-child(2) { height: 14px; animation-delay: 0.15s; }
  .playing-indicator .bar:nth-child(3) { height: 8px; animation-delay: 0.3s; }
  @keyframes label-equalize {
    0%, 100% { transform: scaleY(0.5); opacity: 0.7; }
    50% { transform: scaleY(1); opacity: 1; }
  }

  .track-info { flex: 1; min-width: 0; }
  .track-title {
    font-size: 14px; font-weight: 500; color: var(--text-primary);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .track-album {
    font-size: 12px; color: var(--text-muted);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .track-link {
    background: none; border: none; padding: 0; text-align: left;
    cursor: pointer; color: var(--text-muted); font-size: 12px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    display: block; max-width: 100%;
  }
  .track-link:hover { color: var(--text-primary); text-decoration: underline; text-underline-offset: 2px; }
  .track-quality { display: flex; align-items: center; }
  .track-duration { font-size: 13px; color: var(--text-muted); font-family: var(--font-mono); }
  .track-actions { display: flex; align-items: center; gap: 4px; margin-left: 8px; }
  .track-favorite-btn {
    display: flex; align-items: center; justify-content: center;
    width: 28px; height: 28px; border: none; background: none;
    color: var(--text-muted); cursor: pointer; border-radius: 50%;
    transition: color 150ms ease;
  }
  .track-favorite-btn:hover { color: var(--text-primary); }
  .track-favorite-btn.is-favorite { color: var(--accent-primary); }
  .track-favorite-btn.is-toggling { opacity: 0.5; pointer-events: none; }
  .load-more-link {
    display: flex; align-items: center; justify-content: center;
    gap: 4px; width: 100%; padding: 16px;
    background: none; border: none; text-align: center;
    font-size: 13px; color: var(--text-muted); cursor: pointer;
    transition: color 150ms ease;
  }
  .load-more-link:hover { color: var(--text-primary); }

  /* Artist cards (SearchView style) */
  .artist-card {
    display: flex; flex-direction: column; align-items: center;
    text-align: center; padding: 16px;
    background-color: var(--bg-secondary); border: none;
    border-radius: 12px; cursor: pointer;
    transition: background-color 150ms ease;
    width: 160px; height: 220px; flex-shrink: 0;
  }
  .artist-card:hover { background-color: var(--bg-tertiary); }
  .artist-image-wrapper {
    position: relative; width: 120px; height: 120px; min-height: 120px;
    border-radius: 50%; margin-bottom: 12px; flex-shrink: 0; overflow: hidden;
  }
  .artist-image-placeholder {
    width: 100%; height: 100%; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted); flex-shrink: 0;
  }
  .artist-image {
    position: absolute; inset: 0; width: 100%; height: 100%;
    border-radius: 50%; object-fit: cover; z-index: 1;
  }
  .artist-name {
    font-size: 14px; font-weight: 500; color: var(--text-primary);
    margin-bottom: 4px; width: 100%; overflow: hidden;
    text-overflow: ellipsis; display: -webkit-box;
    -webkit-line-clamp: 2; -webkit-box-orient: vertical; line-height: 1.3;
  }

  /* Label cards (round) */
  .label-card {
    display: flex; flex-direction: column; align-items: center;
    gap: 8px; width: 140px; flex-shrink: 0;
    background: none; border: none; cursor: pointer;
    padding: 8px; border-radius: 8px;
    transition: background-color 150ms ease;
  }
  .label-card:hover { background-color: var(--bg-tertiary); }
  .label-card-image-wrapper {
    width: 120px; height: 120px; border-radius: 50%;
    overflow: hidden; position: relative; background: var(--bg-tertiary);
  }
  .label-card-image-placeholder {
    width: 100%; height: 100%;
    display: flex; align-items: center; justify-content: center;
    background: linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%); color: white;
  }
  .label-card-image {
    position: absolute; inset: 0; width: 100%; height: 100%;
    object-fit: cover; z-index: 1;
  }
  .label-card-name {
    font-size: 13px; font-weight: 500; color: var(--text-primary);
    text-align: center; overflow: hidden; text-overflow: ellipsis;
    white-space: nowrap; width: 100%;
  }
  .spacer { width: 8px; flex-shrink: 0; }
</style>
