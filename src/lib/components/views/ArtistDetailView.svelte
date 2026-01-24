<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, User, ChevronDown, ChevronUp, Play, Music, Heart, Search, X, ChevronLeft, ChevronRight, Radio, MoreHorizontal } from 'lucide-svelte';
  import type { ArtistDetail, QobuzArtist } from '$lib/types';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackMenu from '../TrackMenu.svelte';
  import { consumeContextTrackFocus, setPlaybackContext, getPlaybackContext } from '$lib/stores/playbackContextStore';
  import { togglePlay } from '$lib/stores/playerStore';
  import { getQueue, syncQueueState, playQueueIndex } from '$lib/stores/queueStore';
  import { tick } from 'svelte';

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
    onAddAlbumToPlaylist?: (albumId: string) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: Track) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
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
    onAddAlbumToPlaylist,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    activeTrackId = null,
    isPlaybackActive = false
  }: Props = $props();

  let bioExpanded = $state(false);
  let imageError = $state(false);
  let topTracks = $state<Track[]>([]);
  let tracksLoading = $state(false);
  let isFavorite = $state(false);
  let isFavoriteLoading = $state(false);
  let isRadioLoading = $state(false);
  let radioLoadingMessage = $state('');
  let radioJustCreated = $state(false);
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

  // Page search state
  let searchOpen = $state(false);
  let searchQuery = $state('');
  let searchInputEl = $state<HTMLInputElement | null>(null);
  let currentSearchIndex = $state(0);

  // Album sorting state
  type AlbumSortMode = 'default' | 'newest' | 'oldest' | 'title-asc' | 'title-desc';
  let albumSortMode = $state<AlbumSortMode>('default');
  let showAlbumSortMenu = $state(false);

  // Popular tracks display state
  let visibleTracksCount = $state(5);
  let showTracksContextMenu = $state(false);

  // Computed visible tracks
  let visibleTracks = $derived(topTracks.slice(0, visibleTracksCount));
  let canLoadMoreTracks = $derived(visibleTracksCount < 50 && topTracks.length > visibleTracksCount);

  function loadMoreTracks() {
    if (visibleTracksCount === 5) {
      visibleTracksCount = 20;
    } else if (visibleTracksCount === 20) {
      visibleTracksCount = 50;
    }
  }

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
    albumSortMode = 'default'; // Reset sort when artist changes

    loadTopTracks();
    loadSimilarArtists();
    checkFavoriteStatus();
    loadArtistAlbumDownloadStatuses();
  });

  // Close sort menu when clicking outside
  $effect(() => {
    if (!showAlbumSortMenu) return;

    function handleClick() {
      showAlbumSortMenu = false;
    }

    // Delay to avoid closing immediately
    setTimeout(() => {
      document.addEventListener('click', handleClick);
    }, 0);

    return () => {
      document.removeEventListener('click', handleClick);
    };
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

  async function createArtistRadio() {
    if (isRadioLoading) return;

    isRadioLoading = true;
    radioJustCreated = false;

    try {
      // Show loading messages
      radioLoadingMessage = 'Preparing the artist radio...';
      await new Promise(resolve => setTimeout(resolve, 800));

      radioLoadingMessage = 'Fetching similar artists';
      const sessionId = await invoke<string>('create_artist_radio', {
        artistId: artist.id,
        artistName: artist.name
      });
      console.log(`[Radio] Artist radio created: ${sessionId}`);

      radioLoadingMessage = 'Radio function is still experimental...';
      await new Promise(resolve => setTimeout(resolve, 400));

      // Sync context from backend
      await getPlaybackContext();

      // Play first track from queue
      const firstTrack = await playQueueIndex(0);

      if (firstTrack && onTrackPlay) {
        console.log(`[Radio] First track:`, firstTrack);
        // Start playback using the onTrackPlay callback
        onTrackPlay({
          id: firstTrack.id,
          title: firstTrack.title,
          artist: firstTrack.artist,
          album: firstTrack.album,
          albumArt: firstTrack.artwork_url || '',
          duration: formatDuration(firstTrack.duration_secs),
          durationSeconds: firstTrack.duration_secs,
          hires: firstTrack.hires,
          bitDepth: firstTrack.bit_depth ?? undefined,
          samplingRate: firstTrack.sample_rate ?? undefined,
          albumId: firstTrack.album_id ?? undefined,
          artistId: firstTrack.artist_id ?? undefined,
        });
        console.log(`[Radio] Started playback of track ${firstTrack.id}`);

        // Mark as just created for visual feedback
        radioJustCreated = true;
        setTimeout(() => { radioJustCreated = false; }, 3000);
      } else {
        console.log(`[Radio] Cannot start playback - firstTrack: ${!!firstTrack}, onTrackPlay: ${!!onTrackPlay}`);
      }
    } catch (err) {
      console.error('Failed to create artist radio:', err);
      // TODO: Show user-facing error toast if available
    } finally {
      isRadioLoading = false;
      radioLoadingMessage = '';
    }
  }

  async function createTrackRadio(track: Track) {
    try {
      const trackName = track.title;
      const trackArtistId = track.performer?.id || artist.id;

      const sessionId = await invoke<string>('create_track_radio', {
        trackId: track.id,
        trackName,
        artistId: trackArtistId
      });
      console.log(`[Radio] Track radio created: ${sessionId}`);

      // Sync context from backend
      await getPlaybackContext();

      // Play first track from queue
      const firstTrack = await playQueueIndex(0);

      if (firstTrack && onTrackPlay) {
        console.log(`[Radio] First track:`, firstTrack);
        // Start playback using the onTrackPlay callback
        onTrackPlay({
          id: firstTrack.id,
          title: firstTrack.title,
          artist: firstTrack.artist,
          album: firstTrack.album,
          albumArt: firstTrack.artwork_url || '',
          duration: formatDuration(firstTrack.duration_secs),
          durationSeconds: firstTrack.duration_secs,
          hires: firstTrack.hires,
          bitDepth: firstTrack.bit_depth ?? undefined,
          samplingRate: firstTrack.sample_rate ?? undefined,
          albumId: firstTrack.album_id ?? undefined,
          artistId: firstTrack.artist_id ?? undefined,
        });
        console.log(`[Radio] Started playback of track ${firstTrack.id}`);
      }
    } catch (err) {
      console.error('Failed to create track radio:', err);
      // TODO: Show user-facing error toast if available
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

  function handlePausePlayback(event: MouseEvent) {
    event.stopPropagation();
    void togglePlay();
  }

  function buildTopTracksQueue(tracks: Track[]) {
    return tracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.performer?.name || artist.name,
      album: t.album?.title || '',
      duration_secs: t.duration,
      artwork_url: t.album?.image?.large || t.album?.image?.thumbnail || '',
      hires: t.hires_streamable ?? false,
      bit_depth: t.maximum_bit_depth ?? null,
      sample_rate: t.maximum_sampling_rate ?? null,
      is_local: false,
      album_id: t.album?.id || null,
      artist_id: t.performer?.id ?? null,
    }));
  }

  async function handleTrackPlay(track: Track, trackIndex?: number) {
    // Create artist top tracks context
    if (topTracks.length > 0) {
      const trackIds = topTracks.map(t => t.id);
      const index = trackIndex !== undefined ? trackIndex : trackIds.indexOf(track.id);
      
      if (index >= 0) {
        await setPlaybackContext(
          'artist_top',
          artist.id.toString(),
          artist.name,
          'qobuz',
          trackIds,
          index
        );
        console.log(`[Artist] Context created: "${artist.name}" top tracks, ${trackIds.length} tracks, starting at ${index}`);
        try {
          const queueTracks = buildTopTracksQueue(topTracks);
          await invoke('set_queue', { tracks: queueTracks, startIndex: index });
        } catch (err) {
          console.error('Failed to set queue:', err);
        }
      }
    }

    // Play track
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

    try {
      await handleTrackPlay(topTracks[0], 0);
    } catch (err) {
      console.error('Failed to set queue:', err);
    }
  }

  function handlePlayAllTracksNext() {
    if (!onTrackPlayNext) return;
    // Add all tracks to play next (in reverse order so first track plays first)
    for (let i = topTracks.length - 1; i >= 0; i--) {
      onTrackPlayNext(topTracks[i]);
    }
  }

  function handlePlayAllTracksLater() {
    if (!onTrackPlayLater) return;
    // Add all tracks to play later
    for (const track of topTracks) {
      onTrackPlayLater(track);
    }
  }

  async function handleShuffleAllTracks() {
    if (topTracks.length === 0 || !onTrackPlay) return;
    // Shuffle and play from random position
    const randomIndex = Math.floor(Math.random() * topTracks.length);
    try {
      await handleTrackPlay(topTracks[randomIndex], randomIndex);
    } catch (err) {
      console.error('Failed to shuffle tracks:', err);
    }
  }

  function handleAddAllTracksToPlaylist() {
    if (!onTrackAddToPlaylist || topTracks.length === 0) return;
    // Add first track to playlist (this opens the playlist picker)
    // The UI typically handles adding multiple tracks through a different flow
    onTrackAddToPlaylist(topTracks[0].id);
  }

  function handleImageError() {
    imageError = true;
  }

  // Get biography text (prefer content for full text, fall back to summary)
  let bioText = $derived(
    artist.biography?.content || artist.biography?.summary || null
  );

  // Truncate bio for collapsed view (reduced to ~530 chars, 2/3 of original 800)
  const BIO_TRUNCATE_LENGTH = 530;
  let truncatedBio = $derived(
    bioText && bioText.length > BIO_TRUNCATE_LENGTH
      ? bioText.slice(0, BIO_TRUNCATE_LENGTH) + '...'
      : bioText
  );
  let bioNeedsTruncation = $derived(bioText ? bioText.length > BIO_TRUNCATE_LENGTH : false);

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

  // Album sorting helper
  type AlbumItem = { id: string; title: string; year?: string; artwork: string; quality: string };
  function sortAlbums<T extends AlbumItem>(albums: T[], mode: AlbumSortMode): T[] {
    if (mode === 'default') return albums;
    return [...albums].sort((a, b) => {
      switch (mode) {
        case 'newest': {
          const yearA = a.year || '0000';
          const yearB = b.year || '0000';
          return yearB.localeCompare(yearA);
        }
        case 'oldest': {
          const yearA = a.year || '9999';
          const yearB = b.year || '9999';
          return yearA.localeCompare(yearB);
        }
        case 'title-asc':
          return a.title.localeCompare(b.title);
        case 'title-desc':
          return b.title.localeCompare(a.title);
        default:
          return 0;
      }
    });
  }

  // Search filtering and sorting
  let searchLower = $derived(searchQuery.toLowerCase().trim());
  let filteredAlbums = $derived.by(() => {
    let albums = searchLower
      ? artist.albums.filter(a => a.title.toLowerCase().includes(searchLower))
      : artist.albums;
    return sortAlbums(albums, albumSortMode);
  });
  let filteredEpsSingles = $derived.by(() => {
    let albums = searchLower
      ? artist.epsSingles.filter(a => a.title.toLowerCase().includes(searchLower))
      : artist.epsSingles;
    return sortAlbums(albums, albumSortMode);
  });
  let filteredLiveAlbums = $derived.by(() => {
    let albums = searchLower
      ? artist.liveAlbums.filter(a => a.title.toLowerCase().includes(searchLower))
      : artist.liveAlbums;
    return sortAlbums(albums, albumSortMode);
  });
  let filteredCompilations = $derived.by(() => {
    let albums = searchLower
      ? artist.compilations.filter(a => a.title.toLowerCase().includes(searchLower))
      : artist.compilations;
    return sortAlbums(albums, albumSortMode);
  });
  let filteredTributes = $derived.by(() => {
    let albums = searchLower
      ? artist.tributes.filter(a => a.title.toLowerCase().includes(searchLower))
      : artist.tributes;
    return sortAlbums(albums, albumSortMode);
  });
  let filteredOthers = $derived.by(() => {
    let albums = searchLower
      ? artist.others.filter(a => a.title.toLowerCase().includes(searchLower))
      : artist.others;
    return sortAlbums(albums, albumSortMode);
  });
  let filteredPlaylists = $derived(
    searchLower
      ? artist.playlists.filter(p => p.title.toLowerCase().includes(searchLower))
      : artist.playlists
  );
  let totalFilteredResults = $derived(
    filteredAlbums.length + filteredEpsSingles.length + filteredLiveAlbums.length +
    filteredCompilations.length + filteredTributes.length + filteredOthers.length +
    filteredPlaylists.length
  );

  // Collect all result IDs for navigation
  let allSearchResultIds = $derived.by(() => {
    if (!searchLower) return [];
    const ids: string[] = [];
    filteredAlbums.forEach(a => ids.push(`album-${a.id}`));
    filteredEpsSingles.forEach(a => ids.push(`album-${a.id}`));
    filteredLiveAlbums.forEach(a => ids.push(`album-${a.id}`));
    filteredCompilations.forEach(a => ids.push(`album-${a.id}`));
    filteredTributes.forEach(a => ids.push(`album-${a.id}`));
    filteredOthers.forEach(a => ids.push(`album-${a.id}`));
    filteredPlaylists.forEach(p => ids.push(`playlist-${p.id}`));
    return ids;
  });

  // Reset index when search changes
  $effect(() => {
    if (searchQuery) {
      currentSearchIndex = 0;
      // Navigate to first result
      if (allSearchResultIds.length > 0) {
        setTimeout(() => navigateToResult(0), 100);
      }
    }
  });

  function toggleSearch() {
    if (searchOpen) {
      searchOpen = false;
      searchQuery = '';
      currentSearchIndex = 0;
    } else {
      searchOpen = true;
      setTimeout(() => searchInputEl?.focus(), 100);
    }
  }

  function clearSearch() {
    searchQuery = '';
    currentSearchIndex = 0;
  }

  function navigateToResult(index: number) {
    if (allSearchResultIds.length === 0) return;
    const id = allSearchResultIds[index];
    if (!id) return;

    // Find the AlbumCard element by data attribute
    const element = artistDetailEl?.querySelector(`[data-search-id="${id}"]`);
    if (element) {
      element.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  }

  function nextResult() {
    if (allSearchResultIds.length === 0) return;
    currentSearchIndex = (currentSearchIndex + 1) % allSearchResultIds.length;
    navigateToResult(currentSearchIndex);
  }

  function prevResult() {
    if (allSearchResultIds.length === 0) return;
    currentSearchIndex = currentSearchIndex === 0
      ? allSearchResultIds.length - 1
      : currentSearchIndex - 1;
    navigateToResult(currentSearchIndex);
  }

  function scrollToSection(target: HTMLDivElement | null, id: string) {
    activeJumpSection = id;
    target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  async function scrollToTrack(trackId: number) {
    await tick();
    const target = artistDetailEl?.querySelector<HTMLElement>(`[data-track-id="${trackId}"]`);
    target?.scrollIntoView({ block: 'center' });
  }

  $effect(() => {
    if (!artistDetailEl || topTracks.length === 0) return;
    const targetId = consumeContextTrackFocus('artist_top', artist.id.toString());
    if (targetId !== null) {
      void scrollToTrack(targetId);
    }
  });

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
    <div class="artist-image-column">
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
    </div>

    <!-- Artist Info -->
    <div class="artist-info">
      <h1 class="artist-name">{artist.name}</h1>

      <!-- Biography -->
      {#if bioText}
        <div class="biography">
          <div class="bio-text">
            {@html bioExpanded ? bioText : truncatedBio}
          </div>
          {#if bioNeedsTruncation}
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
          <div class="similar-title">SIMILAR ARTISTS</div>
          <div class="similar-list">
            {#each similarArtists as similar, index}
              {#if index > 0}
                <span class="similar-separator">•</span>
              {/if}
              <button
                class="similar-artist"
                onclick={() => onTrackGoToArtist?.(similar.id)}
                title={similar.name}
              >
                {similar.name}
              </button>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Action Buttons -->
      <div class="artist-actions">
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
        <div class="radio-btn-wrapper">
          <button
            class="radio-btn"
            class:loading={isRadioLoading}
            class:glow={radioJustCreated}
            onclick={createArtistRadio}
            disabled={isRadioLoading}
            title="Start Artist Radio"
          >
            <Radio size={24} />
          </button>
          {#if isRadioLoading && radioLoadingMessage}
            {#key radioLoadingMessage}
              <span class="floating-message">{radioLoadingMessage}</span>
            {/key}
          {/if}
        </div>
      </div>
    </div>
  </div>

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
              {section.label}
            </button>
          {/each}
        </div>
      </div>
      <div class="page-search" class:open={searchOpen}>
        {#if searchOpen}
          <div class="search-input-container">
            <input
              type="text"
              class="search-input"
              placeholder="Search in this page..."
              bind:value={searchQuery}
              bind:this={searchInputEl}
              onkeydown={(e) => {
                if (e.key === 'Escape') toggleSearch();
                else if (e.key === 'Enter') {
                  e.preventDefault();
                  if (e.shiftKey) prevResult();
                  else nextResult();
                }
              }}
            />
            <div class="search-controls">
              {#if searchQuery}
                <span class="search-count">
                  {#if totalFilteredResults === 0}
                    0/0
                  {:else}
                    {currentSearchIndex + 1}/{totalFilteredResults}
                  {/if}
                </span>
                <button
                  class="search-nav-btn"
                  onclick={prevResult}
                  disabled={totalFilteredResults === 0}
                  title="Previous result (Shift+Enter)"
                >
                  <ChevronLeft size={16} />
                </button>
                <button
                  class="search-nav-btn"
                  onclick={nextResult}
                  disabled={totalFilteredResults === 0}
                  title="Next result (Enter)"
                >
                  <ChevronRight size={16} />
                </button>
              {/if}
              <button class="search-close-btn" onclick={toggleSearch} title="Close search">
                <X size={16} />
              </button>
            </div>
          </div>
        {:else}
          <button class="search-toggle" onclick={toggleSearch} title="Search in this page">
            <Search size={18} />
          </button>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Top Tracks Section -->
  {#if topTracks.length > 0 || tracksLoading}
    <div class="top-tracks-section section-anchor" bind:this={topTracksSection}>
      <div class="section-header">
        <div class="section-header-left">
          <h2 class="section-title">Popular Tracks</h2>
        </div>
        {#if topTracks.length > 0}
          <div class="section-header-actions">
            <button class="action-btn-circle primary" onclick={handlePlayAllTracks} title="Play All">
              <Play size={20} fill="white" color="white" />
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
                    Play Next
                  </button>
                  <button class="context-menu-item" onclick={() => { handlePlayAllTracksLater(); showTracksContextMenu = false; }}>
                    Play Later
                  </button>
                  <button class="context-menu-item" onclick={() => { handleShuffleAllTracks(); showTracksContextMenu = false; }}>
                    Shuffle
                  </button>
                  <button class="context-menu-item" onclick={() => { handleAddAllTracksToPlaylist(); showTracksContextMenu = false; }}>
                    Add to Playlist
                  </button>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>

      {#if tracksLoading}
        <div class="tracks-loading">Loading tracks...</div>
      {:else}
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
                {#if track.album?.image?.thumbnail || track.album?.image?.small}
                  <img src={track.album?.image?.thumbnail || track.album?.image?.small} alt={track.title} loading="lazy" decoding="async" />
                {:else}
                  <div class="track-artwork-placeholder">
                    <Music size={16} />
                  </div>
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
              <div class="track-duration">{formatDuration(track.duration)}</div>
              <div class="track-actions">
                <TrackMenu
                  onPlayNow={() => handleTrackPlay(track, index)}
                  onPlayNext={onTrackPlayNext ? () => onTrackPlayNext(track) : undefined}
                  onPlayLater={onTrackPlayLater ? () => onTrackPlayLater(track) : undefined}
                  onCreateRadio={() => createTrackRadio(track)}
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
        {#if canLoadMoreTracks}
          <button class="load-more-link" onclick={loadMoreTracks}>
            Load More
          </button>
        {/if}
      {/if}
    </div>
  {/if}

  <!-- Discography Section -->
  <div class="discography section-anchor" bind:this={discographySection}>
    <div class="section-header">
      <div class="section-header-left">
        <h2 class="section-title">Discography</h2>
        {#if artist.albums.length > 0}
          <span class="section-count">{artist.albums.length}</span>
        {/if}
      </div>
      <!-- Album Sort Dropdown -->
      <div class="sort-dropdown">
        <button class="sort-btn" onclick={() => (showAlbumSortMenu = !showAlbumSortMenu)}>
          <span>
            {#if albumSortMode === 'default'}Sort: Default
            {:else if albumSortMode === 'newest'}Sort: Newest
            {:else if albumSortMode === 'oldest'}Sort: Oldest
            {:else if albumSortMode === 'title-asc'}Sort: A-Z
            {:else if albumSortMode === 'title-desc'}Sort: Z-A
            {/if}
          </span>
          <ChevronDown size={14} />
        </button>
        {#if showAlbumSortMenu}
          <div class="sort-menu">
            <button
              class="sort-item"
              class:selected={albumSortMode === 'default'}
              onclick={() => { albumSortMode = 'default'; showAlbumSortMenu = false; }}
            >
              Default
            </button>
            <button
              class="sort-item"
              class:selected={albumSortMode === 'newest'}
              onclick={() => { albumSortMode = 'newest'; showAlbumSortMenu = false; }}
            >
              Newest First
            </button>
            <button
              class="sort-item"
              class:selected={albumSortMode === 'oldest'}
              onclick={() => { albumSortMode = 'oldest'; showAlbumSortMenu = false; }}
            >
              Oldest First
            </button>
            <button
              class="sort-item"
              class:selected={albumSortMode === 'title-asc'}
              onclick={() => { albumSortMode = 'title-asc'; showAlbumSortMenu = false; }}
            >
              Title (A-Z)
            </button>
            <button
              class="sort-item"
              class:selected={albumSortMode === 'title-desc'}
              onclick={() => { albumSortMode = 'title-desc'; showAlbumSortMenu = false; }}
            >
              Title (Z-A)
            </button>
          </div>
        {/if}
      </div>
    </div>

    {#if artist.albums.length === 0}
      <div class="no-albums">No albums found</div>
    {:else}
        <div class="albums-grid">
          {#each filteredAlbums as album}
            <AlbumCard
              albumId={album.id}
              artwork={album.artwork}
              title={album.title}
              artist={album.year || ''}
              quality={album.quality}
              searchId={`album-${album.id}`}
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
      <div class="section-header">
        <div class="section-header-left">
          <h2 class="section-title">EPs & Singles</h2>
          <span class="section-count">{artist.epsSingles.length}</span>
        </div>
      </div>
      <div class="albums-grid">
        {#each filteredEpsSingles as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            searchId={`album-${album.id}`}
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
    </div>
  {/if}

  {#if artist.liveAlbums.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={liveAlbumsSection}>
      <div class="section-header">
        <div class="section-header-left">
          <h2 class="section-title">Live Albums</h2>
          <span class="section-count">{artist.liveAlbums.length}</span>
        </div>
      </div>
      <div class="albums-grid">
        {#each filteredLiveAlbums as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            searchId={`album-${album.id}`}
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
    </div>
  {/if}

  {#if artist.compilations.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={compilationsSection}>
      <div class="section-header">
        <div class="section-header-left">
          <h2 class="section-title">Compilations</h2>
          <span class="section-count">{artist.compilations.length}</span>
        </div>
      </div>
      <div class="albums-grid">
        {#each filteredCompilations as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            searchId={`album-${album.id}`}
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
    </div>
  {/if}

  {#if artist.tributes.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={tributesSection}>
      <div class="section-header">
        <div class="section-header-left">
          <h2 class="section-title">Tributes & Covers</h2>
          <span class="section-count">{artist.tributes.length}</span>
        </div>
      </div>
      <div class="albums-grid">
        {#each filteredTributes as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            searchId={`album-${album.id}`}
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
    </div>
  {/if}

  {#if artist.others.length > 0}
    <div class="divider"></div>

    <div class="discography section-anchor" bind:this={othersSection}>
      <div class="section-header">
        <div class="section-header-left">
          <h2 class="section-title">Others</h2>
          <span class="section-count">{artist.others.length}</span>
        </div>
      </div>
      <div class="albums-grid">
        {#each filteredOthers as album}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            searchId={`album-${album.id}`}
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
    </div>
  {/if}

  {#if artist.playlists.length > 0}
    <div class="divider"></div>

    <div class="playlists-section section-anchor" bind:this={playlistsSection}>
      <div class="section-header">
        <div class="section-header-left">
          <h2 class="section-title">Playlists</h2>
          <span class="section-count">{artist.playlists.length}</span>
        </div>
      </div>
      <div class="playlists-grid">
        {#each filteredPlaylists as playlist}
          <button
            class="playlist-card"
            data-search-id={`playlist-${playlist.id}`}
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

  .artist-image-column {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    flex-shrink: 0;
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

  .artist-actions {
    display: flex;
    gap: 12px;
    margin-top: 20px;
  }

  .artist-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: flex-start;
    padding-top: 8px;
  }

  .artist-name {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 16px 0;
    text-align: left;
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

  .radio-btn {
    position: relative;
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
    transition: background 200ms ease, color 200ms ease;
    flex-shrink: 0;
  }

  .radio-btn.loading {
    cursor: default;
    color: var(--accent-primary);
  }

  /* Outer rotating arc */
  .radio-btn.loading::before {
    content: '';
    position: absolute;
    inset: -4px;
    border: 2px solid transparent;
    border-top-color: var(--accent-primary);
    border-right-color: var(--accent-primary);
    border-radius: 50%;
    animation: spinOuter 1.2s linear infinite;
    pointer-events: none;
  }

  /* Inner rotating arc (opposite direction) */
  .radio-btn.loading::after {
    content: '';
    position: absolute;
    inset: -8px;
    border: 2px solid transparent;
    border-bottom-color: rgba(var(--accent-primary-rgb, 139, 92, 246), 0.5);
    border-left-color: rgba(var(--accent-primary-rgb, 139, 92, 246), 0.5);
    border-radius: 50%;
    animation: spinInner 1.8s linear infinite reverse;
    pointer-events: none;
  }

  @keyframes spinOuter {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  @keyframes spinInner {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .radio-btn.glow {
    color: var(--accent-primary);
    box-shadow: 0 0 20px rgba(96, 165, 250, 0.4);
  }

  .radio-btn:hover:not(:disabled):not(.loading) {
    background: var(--bg-hover);
    color: var(--accent-primary);
  }

  .radio-btn:disabled:not(.loading) {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .radio-btn-wrapper {
    position: relative;
  }

  .floating-message {
    position: absolute;
    left: 50%;
    bottom: 100%;
    transform: translateX(-50%);
    white-space: nowrap;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border-radius: 12px;
    pointer-events: none;
    animation: floatUp 1.2s ease-out infinite;
  }

  @keyframes floatUp {
    0% {
      opacity: 0;
      transform: translateX(-50%) translateY(8px);
    }
    20% {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
    60% {
      opacity: 1;
      transform: translateX(-50%) translateY(-8px);
    }
    100% {
      opacity: 0;
      transform: translateX(-50%) translateY(-20px);
    }
  }

  .artist-stats {
    font-size: 16px;
    color: var(--text-muted);
    margin-bottom: 16px;
  }

  .biography {
    max-width: 100%;
    margin-bottom: 16px;
    font-weight: 300;
  }

  .bio-text {
    font-size: 14px;
    line-height: 1.7;
    color: var(--text-secondary);
    font-weight: 300;
  }

  .bio-text :global(p) {
    margin: 0 0 12px 0;
  }

  .bio-text :global(p:last-child) {
    margin-bottom: 0;
  }

  .bio-toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
    font-weight: 400;
    color: var(--accent-primary);
    background: none;
    border: none;
    cursor: pointer;
    padding: 6px 0;
    margin-top: 4px;
  }

  .bio-toggle:hover {
    text-decoration: underline;
  }

  .bio-source {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 8px;
    font-weight: 300;
  }

  .jump-nav {
    position: sticky;
    top: 0;
    z-index: 4;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 10px 24px;
    background: var(--bg-primary);
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    box-shadow: 0 4px 8px -4px rgba(0, 0, 0, 0.5);
    margin: 0 -16px 24px -24px;
    width: calc(100% + 40px);
  }

  .jump-nav-left {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
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

  /* Album Sort Dropdown */
  .sort-dropdown {
    position: relative;
  }

  .sort-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .sort-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .sort-menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    min-width: 140px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    padding: 4px;
    z-index: 50;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  }

  .sort-item {
    width: 100%;
    text-align: left;
    padding: 8px 12px;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    border-radius: 4px;
    font-size: 12px;
    transition: all 100ms ease;
  }

  .sort-item:hover,
  .sort-item.selected {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  /* Page Search */
  .page-search {
    display: flex;
    align-items: center;
  }

  .search-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 6px;
    cursor: pointer;
    transition: color 150ms ease;
  }

  .search-toggle:hover {
    color: var(--text-primary);
  }

  .search-input-container {
    display: flex;
    align-items: center;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 0 4px 0 12px;
    animation: slideInFromRight 200ms ease-out;
  }

  @keyframes slideInFromRight {
    from {
      opacity: 0;
      transform: translateX(20px);
    }
    to {
      opacity: 1;
      transform: translateX(0);
    }
  }

  .search-input {
    width: 180px;
    padding: 6px 0;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 13px;
    outline: none;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-controls {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: 8px;
  }

  .search-count {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    min-width: 32px;
    text-align: center;
    padding: 0 4px;
  }

  .search-nav-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 4px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .search-nav-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .search-nav-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .search-close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 4px;
    cursor: pointer;
    transition: all 150ms ease;
    margin-left: 2px;
  }

  .search-close-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .similar-artists {
    margin-top: 16px;
  }

  .similar-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
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
    align-items: center;
    gap: 4px;
  }

  .similar-separator {
    color: var(--text-muted);
    font-size: 10px;
    user-select: none;
  }

  .similar-artist {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    background: none;
    border: none;
    cursor: pointer;
    padding: 2px 4px;
    transition: color 150ms ease;
  }

  .similar-artist:hover {
    color: var(--accent-primary);
  }

  .divider {
    height: 1px;
    background: linear-gradient(90deg, transparent, var(--bg-tertiary) 20%, var(--bg-tertiary) 80%, transparent);
    margin: 32px 0;
  }

  .section-anchor {
    scroll-margin-top: 140px;
  }

  .discography {
    padding: 24px;
    background: var(--bg-secondary);
    border-radius: 16px;
    margin-bottom: 24px;
  }

  .discography:last-of-type {
    margin-bottom: 0;
  }

  .playlists-section {
    padding: 24px;
    background: var(--bg-secondary);
    border-radius: 16px;
    margin-bottom: 24px;
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

  .section-count {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 24px;
    height: 24px;
    padding: 0 8px;
    background: var(--bg-tertiary);
    border-radius: 12px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
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
    margin-top: 10px;
    margin-bottom: 32px;
  }

  .section-header-actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .action-btn-circle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border: none;
    border-radius: 50%;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease;
  }

  .action-btn-circle:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .action-btn-circle.primary {
    width: 44px;
    height: 44px;
    background: var(--accent-primary);
    color: white;
  }

  .action-btn-circle.primary:hover {
    background: var(--accent-hover);
  }

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
    background: var(--bg-elevated, var(--bg-secondary));
    border: 1px solid var(--border-subtle, var(--bg-tertiary));
    border-radius: 8px;
    padding: 4px 0;
    z-index: 100;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
  }

  .context-menu-item {
    display: block;
    width: 100%;
    padding: 10px 16px;
    background: none;
    border: none;
    text-align: left;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
    transition: background 150ms ease;
  }

  .context-menu-item:hover {
    background: var(--bg-hover);
  }

  .load-more-link {
    display: block;
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
    position: relative;
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
    animation: artist-equalize 1s ease-in-out infinite;
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

  @keyframes artist-equalize {
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
