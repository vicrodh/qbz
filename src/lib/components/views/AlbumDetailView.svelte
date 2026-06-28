<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { cmdAddTracksToQueue, cmdAddTracksToQueueNext } from '$lib/services/commandRouter';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { t, locale } from 'svelte-i18n';
  import { showToast } from '$lib/stores/toastStore';
  import {
    hasCustomAlbumCover,
    setCustomAlbumCover,
    removeCustomAlbumCover as removeCustomCoverFromStore
  } from '$lib/stores/customAlbumCoverStore';
  import { ArrowLeft, Play, Shuffle, Heart, Radio, CloudDownload, ChevronLeft, ChevronRight, LoaderCircle, SquareCheckBig, BookOpen, Disc3, CassetteTape, Search, X } from 'lucide-svelte';
  import QualityBadgeStatic from '../QualityBadgeStatic.svelte';
  import { openAddToMixtape } from '$lib/stores/addToMixtapeModalStore';
  import { formatTrackTitle } from '$lib/utils/trackTitle';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import AlbumCard from '$lib/discovery-v2/AlbumCardLite.svelte';
  import TrackRow from '../TrackRow.svelte';
  import AlbumMenu from '../AlbumMenu.svelte';
  import BulkActionBar from '../BulkActionBar.svelte';
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
  import {
    isTrackRemovedFromQobuz,
    subscribe as subscribeUnavailable
  } from '$lib/stores/unavailableTracksStore';
  import ImageLightbox from '../ImageLightbox.svelte';
  import BookletViewer from '../BookletViewer.svelte';
  import type { QobuzGoody } from '$lib/types';
  import { applyShiftRange, isSelectAllShortcut } from '$lib/utils/multiSelect';
  import { extractPalette, pickHeaderColor, type ArtworkPalette } from '$lib/utils/artworkPalette';
  import { sanitizeHtml } from '$lib/utils/sanitize';
  import {
    subscribe as subscribeAppearance,
    isAlbumHeaderGradientEnabled,
  } from '$lib/stores/appearancePreferencesStore';
  import { getCachedImageUrl } from '$lib/services/imageCacheService';

  interface Track {
    id: number;
    number: number;
    title: string;
    /** Qobuz subtitle/edition (e.g. "Player's Ball Mix") (#360). */
    version?: string | null;
    work?: string | null;
    artist?: string;
    artistId?: number;
    composer?: string;
    composerId?: number;
    duration: string;
    durationSeconds: number;
    quality?: string;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
    isrc?: string;
    parental_warning?: boolean;
    /** API streamable flag — false when the track has been removed from Qobuz. */
    streamable?: boolean;
  }

  interface ArtistAlbum {
    id: string;
    title: string;
    artwork: string;
    quality: string;
    genre: string;
    releaseDate?: string;
  }

  interface Award {
    /** String id — normalized from int or string by the backend. */
    id?: string;
    name: string;
    awardedAt?: string;
  }

  interface TrackSection {
    work: string | null;
    composer: string | null;
    tracks: Track[];
  }

  interface Props {
    album: {
      id: string;
      artwork: string;
      title: string;
      artist: string;
      artistId?: number;
      featuredArtists?: { id: number; name: string }[];
      parentalWarning?: boolean;
      year: string;
      releaseDate?: string;
      label: string;
      labelId?: number;
      genre: string;
      quality: string;
      bitDepth?: number;
      samplingRate?: number;
      trackCount: number;
      duration: string;
      durationSeconds?: number;
      description?: string;
      tracks: Track[];
      goodies?: QobuzGoody[];
      awards?: Award[];
    };
    onBack: () => void;
    onArtistClick?: () => void;
    onFeaturedArtistClick?: (artistId: number) => void;
    onLabelClick?: (labelId: number, labelName: string) => void;
    onAwardClick?: (awardId: string, awardName: string) => void;
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
    onBulkAddToPlaylist?: (trackIds: number[]) => void;
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
    radioLoading?: boolean;
  }

  let {
    album,
    onBack,
    onArtistClick,
    onFeaturedArtistClick,
    onLabelClick,
    onAwardClick,
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
    onBulkAddToPlaylist,
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
    onCreateAlbumRadio,
    radioLoading = false
  }: Props = $props();

  let isFavorite = $state(false);
  let isFavoriteLoading = $state(false);
  // Counter incremented when the unavailable-tracks store mutates so reactive
  // reads in {@const} blocks re-evaluate.
  let unavailableVersion = $state(0);
  // Reading `unavailableVersion` here registers the dependency so the {@const}
  // calling this re-evaluates when the store changes.
  function checkTrackUnavailable(track: { id: number; streamable?: boolean }): boolean {
    void unavailableVersion;
    return isTrackRemovedFromQobuz(track);
  }
  let lightboxOpen = $state(false);
  let bookletOpen = $state(false);
  let descriptionExpanded = $state(false);
  let trackSearch = $state('');
  const filteredTracks = $derived.by(() => {
    if (!album.tracks) return [];
    const q = trackSearch.trim().toLowerCase();
    if (!q) return album.tracks;
    return album.tracks.filter((track) =>
      (track.title?.toLowerCase().includes(q)) ||
      (track.artist?.toLowerCase().includes(q))
    );
  });

  const filteredTrackIndexById = $derived.by(() =>
    new Map(filteredTracks.map((track, index) => [track.id, index]))
  );

  const trackSections = $derived.by(() => {
    const sections: TrackSection[] = [];
    let currentTracks: Track[] = [];
    let currentWork = null;
    let currentComposer = null;

    for (const track of filteredTracks) {
      const work = track.work || null;

      if (work === currentWork) {
        currentTracks.push(track);
      } else {
        if (currentTracks.length !== 0) {
          sections.push({ work: currentWork, composer: currentComposer, tracks: currentTracks });
        }

        currentWork = work;
        currentComposer = track.composer || null;
        currentTracks = [track];
      }
    }

    if (currentTracks.length !== 0) {
      sections.push({ work: currentWork, composer: currentComposer, tracks: currentTracks });
    }

    return sections;
  });

  // Booklet: find first PDF goody
  const bookletGoody = $derived(
    album.goodies?.find((goody: QobuzGoody) => goody.url && goody.url.endsWith('.pdf')) ?? null
  );

  // Cover context menu
  let showCoverMenu = $state(false);
  let coverMenuPos = $state({ x: 0, y: 0 });
  let hasCustomCover = $state(false);
  let coverOverride = $state<string | null>(null);
  let scrollContainer: HTMLDivElement | null = $state(null);

  // Header gradient driven by extracted artwork palette.
  let gradientEnabled = $state(isAlbumHeaderGradientEnabled());
  let artworkPalette = $state<ArtworkPalette | null>(null);
  let resolvedHeaderImageUrl = $state<string | null>(null);
  $effect(() => {
    const url = coverOverride ?? album.artwork ?? null;
    artworkPalette = null;
    resolvedHeaderImageUrl = null;
    if (!url || !gradientEnabled) return;
    const stillCurrent = () => (coverOverride ?? album.artwork ?? null) === url;
    getCachedImageUrl(url).then((resolved) => {
      if (stillCurrent()) resolvedHeaderImageUrl = resolved;
    }).catch(() => {});
    extractPalette(url).then((p) => {
      if (stillCurrent()) artworkPalette = p;
    });
  });
  const headerColor = $derived(gradientEnabled ? pickHeaderColor(artworkPalette) : null);
  const headerStyle = $derived.by(() => {
    if (!headerColor) return '';
    const needsScrim = headerColor.luminance > 0.6;
    const imageRule = resolvedHeaderImageUrl
      ? `--art-image-url: url("${resolvedHeaderImageUrl.replace(/"/g, '\\"')}");`
      : '';
    return `--art-bg: ${headerColor.hex}; --art-scrim: ${needsScrim ? '0.55' : '0.3'}; ${imageRule}`;
  });

  // Multi-select
  let multiSelectMode = $state(false);
  let multiSelectedIds = $state(new Set<number>());
  let lastSelectedIndex = $state<number | null>(null);

  function toggleMultiSelectMode() {
    multiSelectMode = !multiSelectMode;
    if (!multiSelectMode) {
      multiSelectedIds = new Set();
      lastSelectedIndex = null;
    }
  }

  function toggleMultiSelect(id: number, index: number, event?: MouseEvent | KeyboardEvent) {
    if (event?.shiftKey && lastSelectedIndex !== null) {
      const ids = filteredTracks.map(track => track.id);
      multiSelectedIds = applyShiftRange({
        current: multiSelectedIds,
        ids,
        lastIndex: lastSelectedIndex,
        currentIndex: index,
      });
      lastSelectedIndex = index;
      return;
    }
    const next = new Set(multiSelectedIds);
    if (next.has(id)) next.delete(id); else next.add(id);
    multiSelectedIds = next;
    lastSelectedIndex = index;
  }

  function getSelectStateForTracks(tracks: Track[]) {
    if (tracks.length === 0) return 'none' as const;

    const selectedCount = tracks.filter((track) => multiSelectedIds.has(track.id)).length;
    if (selectedCount === 0) return 'none' as const;
    if (selectedCount === tracks.length) return 'all' as const;
    return 'partial' as const;
  }

  function toggleSelectTracks(tracks: Track[]) {
    const allSelected = tracks.every((track) => multiSelectedIds.has(track.id));
    const next = new Set(multiSelectedIds);

    for (const track of tracks) {
      if (allSelected) {
        next.delete(track.id);
      } else {
        next.add(track.id);
      }
    }

    multiSelectedIds = next;
  }

  $effect(() => {
    if (!multiSelectMode) return;
    const handler = (e: KeyboardEvent) => {
      if (!isSelectAllShortcut(e)) return;
      e.preventDefault();
      multiSelectedIds = new Set(filteredTracks.map(track => track.id));
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  });

  function buildAlbumQueueTracks(tracks: Track[]) {
    return tracks.map(trk => ({
      id: trk.id,
      title: trk.title,
      version: trk.version ?? null,
      artist: trk.artist || album.artist,
      album: album.title,
      duration_secs: trk.durationSeconds,
      artwork_url: album.artwork || null,
      hires: trk.hires ?? false,
      bit_depth: trk.bitDepth ?? null,
      sample_rate: trk.samplingRate ?? null,
      is_local: false,
      album_id: album.id || null,
      artist_id: album.artistId ?? null,
    }));
  }

  async function handleBulkPlayNext() {
    const selected = album.tracks.filter(track => multiSelectedIds.has(track.id));
    await cmdAddTracksToQueueNext(buildAlbumQueueTracks(selected));
    multiSelectMode = false; multiSelectedIds = new Set();
  }

  async function handleBulkPlayLater() {
    const selected = album.tracks.filter(track => multiSelectedIds.has(track.id));
    await cmdAddTracksToQueue(buildAlbumQueueTracks(selected));
    multiSelectMode = false; multiSelectedIds = new Set();
  }

  async function handleBulkAddToPlaylist() {
    onBulkAddToPlaylist?.([...multiSelectedIds]);
    multiSelectMode = false; multiSelectedIds = new Set();
  }

  async function handleBulkAddFavorites() {
    for (const id of multiSelectedIds) { onTrackAddFavorite?.(id); }
    multiSelectMode = false; multiSelectedIds = new Set();
  }

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

  /** Render a track-count duration as `Xh Ym Zs`, dropping leading zero
   *  units (so `1h 4m 12s`, not `01h 04m 12s`). Empty input returns `0s`
   *  as the safe fallback. */
  function formatAlbumDuration(seconds: number): string {
    const total = Math.max(0, Math.floor(seconds || 0));
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const s = total % 60;
    if (h > 0) return `${h}h ${m}m ${s}s`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  }

  // Format release date nicely, fallback to year
  const formattedReleaseDate = $derived.by(() => {
    if (album.releaseDate) {
      const date = new Date(album.releaseDate);
      if (!isNaN(date.getTime())) {
        return date.toLocaleDateString($locale ? $locale : 'en-us', {
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
  // --- Custom album cover handlers ---

  function loadCustomCoverStatus() {
    hasCustomCover = hasCustomAlbumCover(album.id);
  }

  async function handleAddCustomCover() {
    showCoverMenu = false;
    const selected = await open({
      filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
      multiple: false
    });
    if (!selected) return;

    try {
      const result = await invoke<{ image_path: string; thumbnail_path: string }>(
        'v2_library_set_custom_album_cover',
        { albumId: album.id, customImagePath: selected }
      );
      coverOverride = convertFileSrc(result.image_path);
      hasCustomCover = true;
      setCustomAlbumCover(album.id, convertFileSrc(result.image_path));
      showToast($t('album.customCoverSet'), 'success');
    } catch (err) {
      showToast(`${$t('album.customCoverError')}: ${err}`, 'error');
    }
  }

  async function handleRemoveCustomCover() {
    showCoverMenu = false;
    try {
      await invoke('v2_library_remove_custom_album_cover', { albumId: album.id });
      coverOverride = null;
      hasCustomCover = false;
      removeCustomCoverFromStore(album.id);
      showToast($t('album.customCoverRemoved'), 'success');
    } catch (err) {
      showToast(`${$t('album.customCoverError')}: ${err}`, 'error');
    }
  }

  async function handleOpenCoverInBrowser() {
    showCoverMenu = false;
    const url = coverOverride ?? album.artwork;
    if (url && !url.startsWith('asset://')) {
      await openUrl(url).catch(err => console.error('Failed to open URL:', err));
    }
  }

  async function handleSaveCoverAs() {
    showCoverMenu = false;
    const artworkUrl = coverOverride ?? album.artwork;
    if (!artworkUrl) return;

    const dest = await save({
      filters: [{ name: 'Images', extensions: ['jpg', 'jpeg', 'png'] }],
      defaultPath: `${album.title} - Cover.jpg`
    });
    if (!dest) return;

    try {
      if (artworkUrl.startsWith('asset://') || artworkUrl.startsWith('http://asset.localhost')) {
        showToast($t('album.customCoverError'), 'error');
        return;
      }
      await invoke('v2_save_image_url_to_file', { url: artworkUrl, destPath: dest });
      showToast($t('album.customCoverSet'), 'success');
    } catch (err) {
      showToast(`${$t('album.customCoverError')}: ${err}`, 'error');
    }
  }

  onMount(() => {
    let unsubscribe: (() => void) | null = null;
    (async () => {
      try {
        await loadAlbumFavorites();
        isFavorite = isAlbumFavorite(album.id);
        loadCustomCoverStatus();
        unsubscribe = subscribeAlbumFavorites(() => {
          isFavorite = isAlbumFavorite(album.id);
        });
      } catch (err) {
        console.error('Failed to check album favorite status:', err);
      }
    })();

    // Restore scroll position
    requestAnimationFrame(() => {
      const saved = getSavedScrollPosition('album', album.id);
      if (scrollContainer && saved > 0) {
        scrollContainer.scrollTop = saved;
      }
    });

    const unsubscribeAppearance = subscribeAppearance(() => {
      gradientEnabled = isAlbumHeaderGradientEnabled();
    });

    const unsubscribeUnavailable = subscribeUnavailable(() => {
      unavailableVersion++;
    });

    return () => {
      unsubscribe?.();
      unsubscribeAppearance();
      unsubscribeUnavailable();
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

  function handleAddToMixtape() {
    const parsedYear = album.year ? parseInt(album.year, 10) : undefined;
    openAddToMixtape({
      item_type: 'album',
      source: 'qobuz',
      source_item_id: album.id,
      title: album.title,
      subtitle: album.artist,
      artwork_url: album.artwork,
      year: parsedYear && !isNaN(parsedYear) ? parsedYear : undefined,
      track_count: album.trackCount,
    });
  }
</script>

<ViewTransition duration={200} distance={12} direction="up">
<div class="album-detail" class:has-art-bg={!!headerColor} style={headerStyle} bind:this={scrollContainer} onscroll={(e) => saveScrollPosition('album', (e.target as HTMLElement).scrollTop, album.id)}>
  <!-- Back Navigation -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>{$t('actions.back')}</span>
  </button>

  <!-- Album Header -->
  <div class="album-header">
    <!-- Album Artwork -->
    <div
      class="artwork"
      onclick={() => lightboxOpen = true}
      onkeydown={(e) => { if (e.key === 'Enter') lightboxOpen = true; }}
      oncontextmenu={(e) => { e.preventDefault(); coverMenuPos = { x: e.clientX, y: e.clientY }; showCoverMenu = true; }}
      role="button"
      tabindex="0"
    >
      <img use:cachedSrc={coverOverride ?? album.artwork} alt={album.title} />
    </div>

    <!-- Album Metadata -->
    <div class="metadata selectable" class:no-description={!album.description}>
      <h1 class="album-title">{album.title}</h1>
      <div class="artist-line">
        {#if album.parentalWarning}
          <span class="explicit-badge" title={$t('library.explicit')}></span>
        {/if}
        {#if onArtistClick && !isVariousArtists}
          <button class="artist-link" onclick={onArtistClick}>
            {album.artist}
          </button>
        {:else}
          <span class="artist-name">{album.artist}</span>
        {/if}
        {#if album.featuredArtists && album.featuredArtists.length > 0}
          {#each album.featuredArtists as featured (featured.id)}
            <span class="featured-sep">•</span>
            {#if onFeaturedArtistClick}
              <button class="artist-link featured" onclick={() => onFeaturedArtistClick!(featured.id)}>
                {featured.name}
              </button>
            {:else}
              <span class="artist-name featured">{featured.name}</span>
            {/if}
          {/each}
        {/if}
      </div>
      <div class="album-info">
        {formattedReleaseDate} •
        {#if album.labelId && onLabelClick}
          <button class="label-link" onclick={() => onLabelClick!(album.labelId!, album.label)}>
            {album.label}
          </button>
        {:else}
          {album.label}
        {/if}
         • {album.genre} • {album.trackCount} {$t('album.tracks')} • {formatAlbumDuration(album.durationSeconds ?? 0)}
      </div>

      {#if album.description}
        <div class="album-description" class:expanded={descriptionExpanded}>
          <div class="album-description-text">{@html sanitizeHtml(album.description)}</div>
          <button
            type="button"
            class="description-toggle"
            onclick={() => descriptionExpanded = !descriptionExpanded}
          >
            {descriptionExpanded ? $t('album.readLess') : $t('album.readMore')}
          </button>
        </div>
      {/if}

      <!-- Action Buttons -->
      <div class="actions">
        <button
          class="action-btn-circle primary"
          onclick={onPlayAll}
          title={$t('actions.play')}
        >
          <Play size={20} fill="currentColor" color="currentColor" />
        </button>
        <button
          class="action-btn-circle"
          onclick={onShuffleAll}
          title={$t('actions.shuffle')}
        >
          <Shuffle size={18} />
        </button>
        <button
          class="action-btn-circle"
          class:is-active={isFavorite}
          onclick={toggleFavorite}
          disabled={isFavoriteLoading}
          title={isFavorite ? $t('actions.removeFromFavorites') : $t('actions.addToFavorites')}
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
            disabled={radioLoading}
          >
            {#if radioLoading}
              <LoaderCircle size={18} class="spin" />
            {:else}
              <Radio size={18} />
            {/if}
          </button>
        {/if}
        {#if onShowAlbumCredits}
          <button
            class="action-btn-circle"
            onclick={onShowAlbumCredits}
            title={$t('actions.albumCredits')}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
              <path d="M13.839 17.525c-.006.002-.559.186-1.039.186-.265 0-.372-.055-.406-.079-.168-.117-.48-.336.054-1.4l1-1.994c.593-1.184.681-2.329.245-3.225-.356-.733-1.039-1.236-1.92-1.416-.317-.065-.639-.097-.958-.097-1.849 0-3.094 1.08-3.146 1.126-.179.158-.221.42-.102.626.12.206.367.3.595.222.005-.002.559-.187 1.039-.187.263 0 .369.055.402.078.169.118.482.34-.051 1.402l-1 1.995c-.594 1.185-.681 2.33-.245 3.225.356.733 1.038 1.236 1.921 1.416.314.063.636.097.954.097 1.85 0 3.096-1.08 3.148-1.126.179-.157.221-.42.102-.626-.12-.205-.369-.297-.593-.223z"/>
              <circle cx="13" cy="6.001" r="2.5"/>
            </svg>
          </button>
        {/if}
        {#if bookletGoody}
          <button
            class="action-btn-circle"
            onclick={() => bookletOpen = true}
            title={$t('album.viewBooklet')}
          >
            <BookOpen size={18} />
          </button>
        {/if}
        <button
          class="action-btn-circle"
          onclick={handleAddToMixtape}
          title={$t('common.addToMixtapeOrCollection')}
        >
          <CassetteTape size={18} />
        </button>
        <button
          class="action-btn-circle"
          class:is-active={multiSelectMode}
          onclick={toggleMultiSelectMode}
          title={multiSelectMode ? $t('actions.cancelSelection') : $t('actions.select')}
        >
          <SquareCheckBig size={18} />
        </button>
        <AlbumMenu
          onPlayNext={onPlayAllNext}
          onPlayLater={onPlayAllLater}
          onAddToPlaylist={onAddAlbumToPlaylist ? handleAddAlbumToPlaylist : undefined}
          onAddToMixtape={handleAddToMixtape}
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

  <!-- Header / track list divider — hidden when the artwork gradient
       provides its own visual demarcation. -->
  <div class="divider"></div>

  <!-- Track list + right-side metadata sidebar (label + awards) -->
  <div class="track-sidebar-layout">
  <!-- Track List -->
  <div class="track-list">
    <!-- Table Header -->
    <div class="tracklist-toolbar" data-tauri-drag-region="deep">
      <div class="tracklist-toolbar-left" data-tauri-drag-region="false">
        <QualityBadgeStatic
          bare
          quality={album.quality}
          bitDepth={album.bitDepth}
          samplingRate={album.samplingRate}
        />
      </div>
      <div class="tracklist-toolbar-search" data-tauri-drag-region="false">
        <Search size={14} />
        <input
          type="text"
          placeholder={$t('tracklist.searchPlaceholder')}
          bind:value={trackSearch}
          aria-label={$t('tracklist.searchPlaceholder')}
        />
        {#if trackSearch}
          <button
            type="button"
            class="tracklist-search-clear"
            onclick={() => trackSearch = ''}
            aria-label={$t('actions.clear')}
          >
            <X size={14} />
          </button>
        {/if}
      </div>
    </div>

    {#snippet tableHeader(sectionTracks: Track[])}
      {@const sectionSelectState = getSelectStateForTracks(sectionTracks)}
      <div class="table-header">
        <div class="col-select-all" class:active={multiSelectMode}>
          <input
            type="checkbox"
            checked={sectionSelectState === 'all'}
            indeterminate={sectionSelectState === 'partial'}
            onchange={() => toggleSelectTracks(sectionTracks)}
            title={$t('actions.selectAll')}
            tabindex={multiSelectMode ? 0 : -1}
          />
        </div>
        <div class="col-number">#</div>
        <div class="col-title">{$t('tracklist.title')}</div>
        <div class="col-duration">{$t('tracklist.duration')}</div>
        <div class="col-quality">{$t('tracklist.quality')}</div>
        <div class="col-icon"><Heart size={14} /></div>
        <div class="col-icon"><CloudDownload size={14} /></div>
        <div class="col-spacer"></div>
      </div>
    {/snippet}

    <!-- Track Rows -->
    <div class="tracks">
      {#if !album.tracks || album.tracks.length === 0}
        <div class="empty-tracks-message">
          <p>{$t('album.loadError')}</p>
          <button class="retry-btn" onclick={onBack}>{$t('actions.back')}</button>
        </div>
      {:else if filteredTracks.length === 0}
        <div class="empty-tracks-message">
          <p>{$t('tracklist.noMatches')}</p>
        </div>
      {:else}
      {#each trackSections as section, sectionIndex}
        {#if section.work || sectionIndex > 0}
          <div class="work-header">
            {#if section.work}
              {section.work} {section.composer ? `(${section.composer})` : ""}
            {/if}
          </div>
        {/if}

        {@render tableHeader(section.tracks)}

        {#each section.tracks as track (track.id)}
          {@const downloadInfo = getTrackOfflineCacheStatus?.(track.id) ?? { status: 'none' as const, progress: 0 }}
          {@const isTrackDownloaded = downloadInfo.status === 'ready'}
          {@const trackArtistId = track.artistId ?? album.artistId}
          {@const trackBlacklisted = trackArtistId ? isArtistBlacklisted(trackArtistId) : false}
          {@const trackUnavailable = checkTrackUnavailable(track)}
          {@const visibleTrackIndex = filteredTrackIndexById.get(track.id) ?? 0}

          <TrackRow
            trackId={track.id}
            number={track.number}
            title={formatTrackTitle(track)}
            artist={track.artist}
            duration={track.duration}
            quality={track.quality}
            explicit={track.parental_warning === true}
            isPlaying={isPlaybackActive && activeTrackId === track.id}
            isActiveTrack={activeTrackId === track.id}
            isBlacklisted={trackBlacklisted}
            isUnavailable={trackUnavailable}
            unavailableTooltip={trackUnavailable ? $t('player.trackUnavailable') : undefined}
            selectable={multiSelectMode}
            selected={multiSelectedIds.has(track.id)}
            dragTrackIds={multiSelectMode && multiSelectedIds.has(track.id) ? [...multiSelectedIds] : undefined}
            onToggleSelect={(e) => toggleMultiSelect(track.id, visibleTrackIndex, e)}
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
      {/each}
      {/if}
    </div>
    <BulkActionBar
      count={multiSelectedIds.size}
      onPlayNext={handleBulkPlayNext}
      onPlayLater={handleBulkPlayLater}
      onAddToPlaylist={handleBulkAddToPlaylist}
      onAddFavorites={onTrackAddFavorite ? handleBulkAddFavorites : undefined}
      onClearSelection={() => { multiSelectedIds = new Set(); }}
    />
  </div>

  <!-- Album metadata sidebar (label + awards stack). Matches Qobuz web
       where the label and press accolades sit to the right of the track
       list; collapses under the tracks below 1100px. -->
  {#if (album.labelId && album.label) || (album.awards && album.awards.length > 0)}
    <aside class="album-sidebar">
      {#if album.labelId && album.label}
        <section class="sidebar-section">
          <h3 class="sidebar-section-title">{$t('album.sidebar.label')}</h3>
          <button
            class="sidebar-entity-card"
            type="button"
            onclick={() => onLabelClick?.(album.labelId!, album.label)}
            disabled={!onLabelClick}
          >
            <div class="sidebar-entity-avatar label-avatar">
              <Disc3 size={20} />
            </div>
            <div class="sidebar-entity-name">{album.label}</div>
          </button>
        </section>
      {/if}
      {#if album.awards && album.awards.length > 0}
        <section class="sidebar-section">
          <h3 class="sidebar-section-title">{$t('album.sidebar.awards')}</h3>
          <div class="sidebar-awards-list">
            {#each album.awards as award (award.name)}
              <button
                class="sidebar-entity-card"
                type="button"
                onclick={() => onAwardClick?.(award.id ?? '', award.name)}
                disabled={!onAwardClick}
              >
                <div class="sidebar-entity-avatar award-avatar">
                  <img src="/laurels.svg" alt="" class="laurel-icon-xs" />
                </div>
                <div class="sidebar-entity-name">{award.name}</div>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    </aside>
  {/if}
  </div>

  <!-- By the same artist Section -->
  {#if filteredArtistAlbums.length > 0 && !isVariousArtists}
    <div class="same-artist-section">
      <div class="section-header">
        <h2 class="section-title">{$t('album.sameArtist')}</h2>
        {#if hasMoreThanVisible}
          <div class="carousel-controls">
            <button
              class="carousel-btn"
              onclick={() => scrollCarousel('left')}
              disabled={!canScrollLeft}
              aria-label={$t('actions.previousAlbums')}
            >
              <ChevronLeft size={20} />
            </button>
            <button
              class="carousel-btn"
              onclick={() => scrollCarousel('right')}
              disabled={!canScrollRight}
              aria-label={$t('actions.nextAlbums')}
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
                genre={relatedAlbum.genre}
                releaseYear={Number(relatedAlbum.releaseDate?.slice(0, 4)) || undefined}
                quality={relatedAlbum.quality}
                isAlbumFullyDownloaded={isRelatedAlbumDownloaded(relatedAlbum.id)}
                onArtistClick={album.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(album.artistId!) : undefined}
                onClick={() => onRelatedAlbumClick?.(relatedAlbum.id)}
                onPlay={onRelatedAlbumPlay ? () => onRelatedAlbumPlay(relatedAlbum.id) : undefined}
                onPlayNext={onRelatedAlbumPlayNext ? () => onRelatedAlbumPlayNext(relatedAlbum.id) : undefined}
                onPlayLater={onRelatedAlbumPlayLater ? () => onRelatedAlbumPlayLater(relatedAlbum.id) : undefined}
                onDownload={onRelatedAlbumDownload ? () => onRelatedAlbumDownload(relatedAlbum.id) : undefined}
                onShareQobuz={onRelatedAlbumShareQobuz ? () => onRelatedAlbumShareQobuz(relatedAlbum.id) : undefined}
                onShareSonglink={onRelatedAlbumShareSonglink ? () => onRelatedAlbumShareSonglink(relatedAlbum.id) : undefined}
              />
            </div>
          {/each}
          {#if onViewArtistDiscography && filteredArtistAlbums.length >= albumsPerPage && currentPage === totalPages - 1}
            <div class="album-card-wrapper">
              <div class="view-more-card">
                <button class="view-more-cover" onclick={onViewArtistDiscography}>
                  <div class="view-more-label">
                    <span>{$t('search.viewMore')}</span>
                    <ChevronRight size={20} />
                  </div>
                </button>
                <div class="view-more-info">
                  <span class="view-more-text">{$t('album.seeFullDiscography')}</span>
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

<ImageLightbox
  isOpen={lightboxOpen}
  onClose={() => lightboxOpen = false}
  src={coverOverride ?? album.artwork}
  alt={album.title}
/>

{#if bookletGoody}
  <BookletViewer
    isOpen={bookletOpen}
    onClose={() => bookletOpen = false}
    url={bookletGoody.original_url || bookletGoody.url}
    title={bookletGoody.name || $t('album.booklet')}
  />
{/if}

{#if showCoverMenu}
  <div
    class="cover-context-backdrop"
    onclick={() => showCoverMenu = false}
    onkeydown={(e) => { if (e.key === 'Escape') showCoverMenu = false; }}
    role="button"
    tabindex="-1"
  ></div>
  <div
    class="cover-context-menu"
    style="left: {coverMenuPos.x}px; top: {coverMenuPos.y}px;"
  >
    {#if hasCustomCover}
      <button class="cover-context-item" onclick={handleAddCustomCover}>
        {$t('album.changeCover')}
      </button>
      <button class="cover-context-item danger" onclick={handleRemoveCustomCover}>
        {$t('album.removeCover')}
      </button>
    {:else}
      <button class="cover-context-item" onclick={handleAddCustomCover}>
        {$t('album.addCover')}
      </button>
    {/if}
    <div class="cover-context-divider"></div>
    <button class="cover-context-item" onclick={handleOpenCoverInBrowser}>
      {$t('album.openInBrowser')}
    </button>
    <button class="cover-context-item" onclick={handleSaveCoverAs}>
      {$t('album.saveAs')}
    </button>
  </div>
{/if}

<style>
  .album-detail {
    width: 100%;
    height: 100%;
    padding: 8px 8px 100px 18px;
    overflow-x: hidden;
    overflow-y: auto;
    position: relative;
  }

  /* Artwork-derived backdrop. ::before renders the actual cover heavily
     blurred (Qobuz/Feishin-style) with a mask that fades it into the
     theme bg toward the bottom; ::after lays a scrim on top for text
     contrast on light artwork. The cost is one-shot — once the image
     is rasterized into a layer the GPU just composites it. */
  .album-detail.has-art-bg::before {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 280px;
    background-color: var(--art-bg, transparent);
    background-image: var(--art-image-url, none);
    background-size: cover;
    background-position: center;
    background-repeat: no-repeat;
    filter: blur(70px) saturate(1.3);
    transform: scale(1.15);
    transform-origin: center top;
    z-index: 0;
    pointer-events: none;
    -webkit-mask-image: linear-gradient(180deg, #000 0%, #000 55%, transparent 100%);
            mask-image: linear-gradient(180deg, #000 0%, #000 55%, transparent 100%);
    transition: background-color 320ms ease;
    will-change: transform, filter;
  }

  .album-detail.has-art-bg::after {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 280px;
    background: linear-gradient(180deg, rgba(0, 0, 0, var(--art-scrim, 0.3)) 0%, rgba(0, 0, 0, 0) 80%);
    z-index: 0;
    pointer-events: none;
  }

  .album-detail > * {
    position: relative;
    z-index: 1;
  }

  /* Lift secondary text contrast over the colored backdrop. */
  .album-detail.has-art-bg .back-btn,
  .album-detail.has-art-bg .album-info {
    color: rgba(255, 255, 255, 0.78);
  }

  .album-detail.has-art-bg .back-btn:hover {
    color: #fff;
  }

  .album-detail.has-art-bg .artist-link {
    color: #fff;
  }

  .album-detail.has-art-bg .artist-link:hover {
    text-decoration: underline;
  }

  .album-detail.has-art-bg .album-description {
    color: rgba(255, 255, 255, 0.78);
  }

  .album-detail.has-art-bg .description-toggle {
    color: rgba(255, 255, 255, 0.78);
  }

  .album-detail.has-art-bg .description-toggle:hover {
    color: #fff;
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
    margin-top: 8px;
    margin-bottom: 24px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-secondary);
  }

  .album-header {
    display: flex;
    gap: 32px;
    margin-bottom: 8px;
  }

  .artwork {
    flex-shrink: 0;
    width: 224px;
    height: 224px;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    cursor: pointer;
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
    justify-content: flex-start;
    padding-top: 4px;
  }

  .metadata.no-description {
    justify-content: center;
    padding-top: 0;
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

  .artist-line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 8px;
  }

  .artist-line .artist-link,
  .artist-line .artist-name {
    margin-bottom: 0;
  }

  .artist-link.featured,
  .artist-name.featured {
    font-weight: 400;
    color: var(--text-secondary);
    font-size: 16px;
  }

  .album-detail.has-art-bg .artist-link.featured,
  .album-detail.has-art-bg .artist-name.featured {
    color: rgba(255, 255, 255, 0.78);
  }

  .featured-sep {
    color: var(--text-muted);
    font-size: 14px;
  }

  .album-detail.has-art-bg .featured-sep {
    color: rgba(255, 255, 255, 0.6);
  }

  /* Action buttons reactive to gradient backdrop. */
  .album-detail.has-art-bg :global(.action-btn-circle) {
    color: rgba(255, 255, 255, 0.78);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.35);
  }

  .album-detail.has-art-bg :global(.action-btn-circle:hover:not(:disabled)) {
    color: #fff;
    background: rgba(255, 255, 255, 0.12);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.7);
  }

  .album-detail.has-art-bg :global(.action-btn-circle.is-active) {
    color: #fff;
    background: rgba(255, 255, 255, 0.18);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.6);
  }

  .explicit-badge {
    display: inline-block;
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    opacity: 0.55;
    background-color: var(--text-secondary);
    -webkit-mask: url('/explicit.svg') center / contain no-repeat;
    mask: url('/explicit.svg') center / contain no-repeat;
  }

  .album-detail.has-art-bg .explicit-badge {
    background-color: rgba(255, 255, 255, 0.85);
    opacity: 0.85;
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
    margin-bottom: 12px;
  }

  .album-description {
    margin: 0 0 16px 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-secondary);
    max-width: 720px;
  }

  .album-description-text {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .album-description-text :global(p) {
    margin: 0;
  }

  .album-description-text :global(p + p) {
    margin-top: 8px;
  }

  .album-description.expanded .album-description-text {
    -webkit-line-clamp: unset;
    line-clamp: unset;
    overflow: visible;
  }

  .tracklist-toolbar {
    display: flex;
    align-items: center;
    gap: 28px;
    /* Asymmetric padding compensates for the table-header below: it has
       40px height with text vertically centered, so visually the content
       sits ~20px below the toolbar's bottom edge. We add the same offset
       on top so the badge/search row reads as equidistant between the
       divider above and the first track row below. */
    padding: 20px 0 8px 0;
    margin: 0;
  }

  .divider {
    height: 1px;
    background-color: var(--border-subtle);
    margin: 14px 0 0 0;
  }

  /* Hide divider when the artwork gradient is active — the gradient
     itself acts as the demarcation between header and tracklist. */
  .album-detail.has-art-bg .divider {
    background-color: transparent;
  }

  .tracklist-toolbar-left {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    min-height: 32px;
    padding-left: 4px;
  }

  .tracklist-toolbar-search {
    flex: 0 1 320px;
    width: 320px;
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px;
    min-height: 32px;
    box-sizing: border-box;
    background: none;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-muted);
    transition: border-color 150ms ease, color 150ms ease;
  }

  .tracklist-toolbar-search:focus-within {
    border-color: var(--accent-primary);
    color: var(--text-primary);
  }

  .tracklist-toolbar-search input {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font: inherit;
    font-size: 13px;
    min-width: 0;
  }

  .tracklist-toolbar-search input::placeholder {
    color: var(--text-muted);
  }

  .tracklist-search-clear {
    background: none;
    border: none;
    padding: 0;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    align-items: center;
  }

  .tracklist-search-clear:hover {
    color: var(--text-primary);
  }

  .description-toggle {
    background: none;
    border: none;
    padding: 4px 0 0 0;
    font: inherit;
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .description-toggle:hover {
    color: var(--text-secondary);
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


  .table-header {
    width: 100%;
    height: 40px;
    padding: 0 16px 0 4px;
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

  /* Match the track-row's left padding (overridden below) so the # column
     and the quality badge above sit at the same x. */
  .track-list :global(.track-row) {
    padding-left: 4px;
  }

  .track-list :global(.track-row .track-number) {
    justify-content: flex-start;
  }

  .col-select-all {
    width: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    overflow: hidden;
    opacity: 0;
    pointer-events: none;
    transition: width 180ms ease, opacity 180ms ease;
  }

  .col-select-all.active {
    width: 24px;
    opacity: 1;
    pointer-events: auto;
  }

  .col-select-all input[type="checkbox"] {
    width: 15px;
    height: 15px;
    accent-color: var(--accent-primary);
    cursor: pointer;
  }

  .col-number {
    width: 48px;
    text-align: left;
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

  .work-header {
    margin-top: 24px;
    margin-bottom: 8px;
    color: var(--text-muted);
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
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
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
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
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

  :global(.spin) {
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Cover context menu */
  .cover-context-backdrop {
    position: fixed;
    inset: 0;
    z-index: 2999;
  }

  .cover-context-menu {
    position: fixed;
    z-index: 3000;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px;
    min-width: 200px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    animation: coverMenuIn 100ms ease;
  }

  @keyframes coverMenuIn {
    from { opacity: 0; transform: scale(0.95); }
    to { opacity: 1; transform: scale(1); }
  }

  .cover-context-item {
    display: block;
    width: 100%;
    padding: 8px 12px;
    font-size: 13px;
    color: var(--text-primary);
    background: none;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    transition: background 100ms ease;
  }

  .cover-context-item:hover {
    background: var(--bg-hover);
  }

  .cover-context-item.danger {
    color: var(--color-error, #ef4444);
  }

  .cover-context-item.danger:hover {
    background: rgba(239, 68, 68, 0.1);
  }

  .cover-context-divider {
    height: 1px;
    background: var(--border-subtle);
    margin: 4px 8px;
  }

  /* ---- Album metadata sidebar (label + awards) ---- */
  .track-sidebar-layout {
    display: flex;
    flex-direction: row;
    gap: 32px;
    align-items: flex-start;
  }
  .track-sidebar-layout .track-list {
    flex: 1 1 auto;
    min-width: 0;
  }
  .album-sidebar {
    flex: 0 0 254px;
    width: 254px;
    display: flex;
    flex-direction: column;
    gap: 24px;
    padding-top: 20px;
  }
  .sidebar-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .sidebar-section-title {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-muted);
    margin: 0;
  }
  .sidebar-awards-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .sidebar-entity-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    background: transparent;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    transition: background-color 150ms ease;
    color: var(--text-primary);
    width: 100%;
    min-width: 0;
  }
  .sidebar-entity-card:hover:not(:disabled) {
    background: var(--bg-tertiary);
  }
  .sidebar-entity-card:disabled {
    cursor: default;
  }
  .sidebar-entity-avatar {
    flex: 0 0 36px;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fff;
  }
  .sidebar-entity-avatar.label-avatar {
    background: linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%);
  }
  .sidebar-entity-avatar.award-avatar {
    background: linear-gradient(135deg, #b45309 0%, #eab308 100%);
  }
  .laurel-icon-xs {
    width: 58%;
    height: 58%;
    filter: brightness(0) invert(1);
    pointer-events: none;
  }
  .sidebar-entity-name {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    line-height: 1.3;
    min-width: 0;
    flex: 1;
  }

  /* Collapse sidebar under tracks on narrow windows */
  @media (max-width: 1100px) {
    .track-sidebar-layout {
      flex-direction: column;
      gap: 24px;
    }
    .album-sidebar {
      flex: 1 1 auto;
      width: 100%;
    }
  }
</style>
