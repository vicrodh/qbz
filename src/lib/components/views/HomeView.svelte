<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Music, User, Loader2 } from 'lucide-svelte';
  import {
    getHomeCache,
    setHomeCache,
    clearHomeCache,
    isHomeCacheValid,
    updateHomeCacheScrollTop
  } from '$lib/stores/homeDataCache';
  import { t } from '$lib/i18n';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import QobuzPlaylistCard from '../QobuzPlaylistCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import HomeSettingsModal from '../HomeSettingsModal.svelte';
  import GenreFilterButton from '../GenreFilterButton.svelte';
  import PlaylistTagFilter from '../PlaylistTagFilter.svelte';
  import { formatDuration, formatQuality, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import { isBlacklisted as isArtistBlacklisted } from '$lib/stores/artistBlacklistStore';
  import {
    subscribe as subscribeHomeSettings,
    getSettings,
    getGreetingInfo,
    type HomeSettings,
    type HomeSectionId
  } from '$lib/stores/homeSettingsStore';
  import {
    getSelectedGenreId,
    getSelectedGenreIds,
    getFilterGenreNames,
    hasActiveFilter as hasGenreFilter
  } from '$lib/stores/genreFilterStore';
  import { setPlaybackContext } from '$lib/stores/playbackContextStore';
  import {
    getCachedArtist,
    setCachedArtist,
    getCachedAlbum,
    setCachedAlbum,
    getCachedTrack,
    setCachedTrack
  } from '$lib/stores/sessionCacheStore';
  import type {
    QobuzAlbum,
    QobuzArtist,
    QobuzTrack,
    DisplayTrack,
    DiscoverResponse,
    DiscoverPlaylist,
    DiscoverPlaylistsResponse,
    DiscoverAlbum,
    PlaylistTag
  } from '$lib/types';

  interface TopArtistSeed {
    artistId: number;
    playCount: number;
  }

  interface HomeSeeds {
    recentlyPlayedAlbumIds: string[];
    continueListeningTrackIds: number[];
    topArtistIds: TopArtistSeed[];
    favoriteAlbumIds: string[];
    favoriteTrackIds: number[];
  }

  interface AlbumCardData {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    genre: string;
    quality?: string;
    releaseDate?: string;
  }

  interface ArtistCardData {
    id: number;
    name: string;
    image?: string;
    playCount?: number;
  }

  interface Props {
    userName?: string;
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
    onArtistClick?: (artistId: number) => void;
    onTrackPlay?: (track: DisplayTrack) => void;
    onTrackPlayNext?: (track: DisplayTrack) => void;
    onTrackPlayLater?: (track: DisplayTrack) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: DisplayTrack) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
    onTrackShowInfo?: (trackId: number) => void;
    onTrackDownload?: (track: DisplayTrack) => void;
    onTrackRemoveDownload?: (trackId: number) => void;
    onTrackReDownload?: (track: DisplayTrack) => void;
    checkTrackDownloaded?: (trackId: number) => boolean;
    getTrackOfflineCacheStatus?: (trackId: number) => { status: string; progress: number };
    onPlaylistClick?: (playlistId: number) => void;
    onPlaylistPlay?: (playlistId: number) => void;
    onPlaylistPlayNext?: (playlistId: number) => void;
    onPlaylistPlayLater?: (playlistId: number) => void;
    onPlaylistCopyToLibrary?: (playlistId: number) => void;
    onPlaylistShareQobuz?: (playlistId: number) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
  }

  let {
    userName = 'User',
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
    onArtistClick,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddToPlaylist,
    onAddAlbumToPlaylist,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onTrackShowInfo,
    onTrackDownload,
    onTrackRemoveDownload,
    onTrackReDownload,
    checkTrackDownloaded,
    getTrackOfflineCacheStatus,
    onPlaylistClick,
    onPlaylistPlay,
    onPlaylistPlayNext,
    onPlaylistPlayLater,
    onPlaylistCopyToLibrary,
    onPlaylistShareQobuz,
    activeTrackId = null,
    isPlaybackActive = false
  }: Props = $props();

  // Home settings state
  let homeSettings = $state<HomeSettings>(getSettings());
  let isSettingsModalOpen = $state(false);

  // Computed greeting with i18n support — never call $t() inside $derived()
  function getGreetingText(): string {
    const info = getGreetingInfo(userName);
    if (info.type === 'custom') {
      return info.text;
    }
    return $t(info.key, { values: { name: info.name } });
  }


  // Check if a section is visible
  function isSectionVisible(sectionId: HomeSectionId): boolean {
    const section = homeSettings.sections.find(s => s.id === sectionId);
    return section?.visible ?? true;
  }

  // Get ordered visible sections
  const visibleSections = $derived(
    homeSettings.sections.filter(s => s.visible).map(s => s.id)
  );

  const renderableSections = $derived(visibleSections);

  const LIMITS = {
    recentAlbums: 20,
    continueTracks: 10,
    topArtists: 8,
    favoriteAlbums: 12,
    favoriteTracks: 10,
    featuredAlbums: 12,
    qobuzPlaylists: 15,
    essentialDiscography: 15
  };

  let homeLimits = $state(getSettings().limits);

  // Loading states for progressive render (each section independent)
  let error = $state<string | null>(null);
  let loadingNewReleases = $state(true);
  let loadingPressAwards = $state(true);
  let loadingMostStreamed = $state(true);
  let loadingQobuzissimes = $state(true);
  let loadingEditorPicks = $state(true);
  let loadingRecentAlbums = $state(true);
  let loadingContinueTracks = $state(true);
  let loadingTopArtists = $state(true);
  let loadingFavoriteAlbums = $state(true);
  let loadingQobuzPlaylists = $state(true);
  let loadingEssentialDiscography = $state(true);

  // True when all sections have finished loading (for empty state detection)
  const anyLoading = $derived(
    loadingNewReleases || loadingPressAwards || loadingMostStreamed ||
    loadingQobuzissimes || loadingEditorPicks || loadingRecentAlbums ||
    loadingContinueTracks || loadingTopArtists || loadingFavoriteAlbums ||
    loadingQobuzPlaylists || loadingEssentialDiscography
  );

  // Featured albums (from Qobuz editorial)
  let newReleases = $state<AlbumCardData[]>([]);
  let pressAwards = $state<AlbumCardData[]>([]);
  let mostStreamed = $state<AlbumCardData[]>([]);
  let qobuzissimes = $state<AlbumCardData[]>([]);
  let editorPicks = $state<AlbumCardData[]>([]);

  // User-specific content
  let recentAlbums = $state<AlbumCardData[]>([]);
  let continueTracks = $state<DisplayTrack[]>([]);
  let topArtists = $state<ArtistCardData[]>([]);
  let favoriteAlbums = $state<AlbumCardData[]>([]);

  // Discover sections
  let qobuzPlaylists = $state<DiscoverPlaylist[]>([]);
  let essentialDiscography = $state<DiscoverAlbum[]>([]);
  let playlistTags = $state<PlaylistTag[]>([]);
  let selectedTagSlug = $state<string | null>(null);

  let failedArtistImages = $state<Set<number>>(new Set());

  // Download status tracking
  let albumDownloadStatuses = $state<Map<string, boolean>>(new Map());
  let downloadStatusTick = $state(0);

  async function loadAlbumDownloadStatus(albumId: string) {
    if (!checkAlbumFullyDownloaded) return false;
    try {
      const isDownloaded = await checkAlbumFullyDownloaded(albumId);
      albumDownloadStatuses.set(albumId, isDownloaded);
      downloadStatusTick++;
      return isDownloaded;
    } catch {
      return false;
    }
  }

  async function loadAllAlbumDownloadStatuses(albums: AlbumCardData[]) {
    if (!checkAlbumFullyDownloaded || albums.length === 0) return;
    
    const BATCH_SIZE = 6;
    for (let i = 0; i < albums.length; i += BATCH_SIZE) {
      const batch = albums.slice(i, i + BATCH_SIZE);
      await Promise.all(batch.map(album => loadAlbumDownloadStatus(album.id)));
    }
  }

  function isAlbumDownloaded(albumId: string): boolean {
    void downloadStateVersion;
    void downloadStatusTick;
    return albumDownloadStatuses.get(albumId) || false;
  }

  $effect(() => {
    if (downloadStateVersion !== undefined) {
      const allAlbums = [
        ...newReleases,
        ...pressAwards,
        ...mostStreamed,
        ...qobuzissimes,
        ...editorPicks,
        ...recentAlbums,
        ...favoriteAlbums
      ];
      loadAllAlbumDownloadStatuses(allAlbums);
    }
  });

  const hasContent = $derived(
    newReleases.length > 0
    || pressAwards.length > 0
    || mostStreamed.length > 0
    || qobuzissimes.length > 0
    || editorPicks.length > 0
    || recentAlbums.length > 0
    || continueTracks.length > 0
    || topArtists.length > 0
    || favoriteAlbums.length > 0
    || qobuzPlaylists.length > 0
    || essentialDiscography.length > 0
  );


  let homeViewEl: HTMLDivElement | undefined;

  onMount(() => {
    // Subscribe to home settings changes — invalidate cache on change
    const unsubscribe = subscribeHomeSettings(() => {
      homeSettings = getSettings();
      homeLimits = getSettings().limits;
      clearHomeCache();
    });

    // Try to restore from cache
    const currentGenreIds = Array.from(getSelectedGenreIds());
    if (isHomeCacheValid(currentGenreIds)) {
      const cached = getHomeCache()!;
      // Restore all data instantly
      newReleases = cached.newReleases;
      pressAwards = cached.pressAwards;
      mostStreamed = cached.mostStreamed;
      qobuzissimes = cached.qobuzissimes;
      editorPicks = cached.editorPicks;
      recentAlbums = cached.recentAlbums;
      continueTracks = cached.continueTracks;
      topArtists = cached.topArtists;
      favoriteAlbums = cached.favoriteAlbums;
      qobuzPlaylists = cached.qobuzPlaylists;
      essentialDiscography = cached.essentialDiscography;
      playlistTags = cached.playlistTags;

      // Mark all sections as loaded
      loadingNewReleases = false;
      loadingPressAwards = false;
      loadingMostStreamed = false;
      loadingQobuzissimes = false;
      loadingEditorPicks = false;
      loadingRecentAlbums = false;
      loadingContinueTracks = false;
      loadingTopArtists = false;
      loadingFavoriteAlbums = false;
      loadingQobuzPlaylists = false;
      loadingEssentialDiscography = false;

      // Restore scroll position after DOM renders
      requestAnimationFrame(() => {
        if (homeViewEl && cached.scrollTop > 0) {
          homeViewEl.scrollTop = cached.scrollTop;
        }
      });

      // Fire-and-forget: refresh download statuses in background
      const allAlbums = [
        ...cached.newReleases, ...cached.pressAwards, ...cached.mostStreamed,
        ...cached.qobuzissimes, ...cached.editorPicks,
        ...cached.recentAlbums, ...cached.favoriteAlbums
      ];
      loadAllAlbumDownloadStatuses(allAlbums);
    } else {
      loadHome();
    }

    return unsubscribe;
  });

  // Save cache when all sections finish loading successfully
  $effect(() => {
    if (!anyLoading && hasContent) {
      const genreIds = Array.from(getSelectedGenreIds());
      setHomeCache({
        newReleases, pressAwards, mostStreamed, qobuzissimes, editorPicks,
        recentAlbums, continueTracks, topArtists, favoriteAlbums,
        qobuzPlaylists, essentialDiscography, playlistTags,
        genreIds
      });
    }
  });

  // Save scroll position incrementally
  function handleHomeScroll(e: Event) {
    const target = e.target as HTMLElement;
    updateHomeCacheScrollTop(target.scrollTop);
  }

  function handleArtistImageError(artistId: number) {
    failedArtistImages = new Set([...failedArtistImages, artistId]);
  }

  function normalizeAlbumIds(ids: Array<string | undefined | null>): string[] {
    const filtered = ids.filter((id): id is string => !!id && id.trim().length > 0);
    return Array.from(new Set(filtered));
  }

  async function fetchAlbums(ids: string[]): Promise<AlbumCardData[]> {
    if (ids.length === 0) return [];
    
    const BATCH_SIZE = 6;
    const albums: AlbumCardData[] = [];
    
    // Separate cached vs uncached
    const uncachedIds: string[] = [];
    for (const id of ids) {
      const cached = getCachedAlbum(id);
      if (cached) {
        albums.push(toAlbumCard(cached));
      } else {
        uncachedIds.push(id);
      }
    }
    
    // Fetch uncached in batches
    for (let i = 0; i < uncachedIds.length; i += BATCH_SIZE) {
      const batch = uncachedIds.slice(i, i + BATCH_SIZE);
      const results = await Promise.allSettled(
        batch.map(albumId => invoke<QobuzAlbum>('get_album', { albumId }))
      );
      
      for (const result of results) {
        if (result.status === 'fulfilled') {
          setCachedAlbum(result.value);
          albums.push(toAlbumCard(result.value));
        }
      }
    }

    return albums;
  }

  async function fetchTracks(ids: number[]): Promise<DisplayTrack[]> {
    if (ids.length === 0) return [];
    
    const BATCH_SIZE = 6;
    const tracks: DisplayTrack[] = [];
    
    // Separate cached vs uncached
    const uncachedIds: number[] = [];
    for (const id of ids) {
      const cached = getCachedTrack(id);
      if (cached) {
        tracks.push(toDisplayTrack(cached));
      } else {
        uncachedIds.push(id);
      }
    }
    
    // Fetch uncached in batches
    for (let i = 0; i < uncachedIds.length; i += BATCH_SIZE) {
      const batch = uncachedIds.slice(i, i + BATCH_SIZE);
      const results = await Promise.allSettled(
        batch.map(trackId => invoke<QobuzTrack>('get_track', { trackId }))
      );
      
      for (const result of results) {
        if (result.status === 'fulfilled') {
          setCachedTrack(result.value);
          tracks.push(toDisplayTrack(result.value));
        }
      }
    }

    return tracks;
  }

  // Fetch artists with limited concurrency and session cache
  async function fetchArtists(seeds: TopArtistSeed[]): Promise<ArtistCardData[]> {
    if (seeds.length === 0) return [];
    
    const BATCH_SIZE = 6; // Fetch 6 artists at a time (min visible at HD resolution)
    const artists: ArtistCardData[] = [];
    
    // Separate cached vs uncached
    const uncachedSeeds: TopArtistSeed[] = [];
    for (const seed of seeds) {
      const cached = getCachedArtist(seed.artistId);
      if (cached) {
        artists.push(toArtistCard(cached, seed.playCount));
      } else {
        uncachedSeeds.push(seed);
      }
    }
    
    // Fetch uncached in batches (using basic endpoint - no albums, much faster)
    for (let i = 0; i < uncachedSeeds.length; i += BATCH_SIZE) {
      const batch = uncachedSeeds.slice(i, i + BATCH_SIZE);
      const results = await Promise.allSettled(
        batch.map(seed => invoke<QobuzArtist>('get_artist_basic', { artistId: seed.artistId }))
      );
      
      results.forEach((result, index) => {
        if (result.status !== 'fulfilled') return;
        const seed = batch[index];
        setCachedArtist(result.value);
        artists.push(toArtistCard(result.value, seed.playCount));
      });
    }

    return artists;
  }

  interface FeaturedAlbumsResponse {
    items: QobuzAlbum[];
    total: number;
  }

  async function fetchFeaturedAlbumsSingle(featuredType: string, limit: number, genreId?: number): Promise<AlbumCardData[]> {
    try {
      const response = await invoke<FeaturedAlbumsResponse>('get_featured_albums', {
        featuredType,
        limit,
        genreId: genreId ?? null
      });
      return response.items.map(toAlbumCard);
    } catch (err) {
      console.error(`Failed to fetch ${featuredType}:`, err);
      return [];
    }
  }

  async function fetchFeaturedAlbums(featuredType: string, limit: number, genreIds: number[]): Promise<AlbumCardData[]> {
    if (genreIds.length === 0) {
      // No filter, fetch without genre
      return fetchFeaturedAlbumsSingle(featuredType, limit);
    }
    if (genreIds.length === 1) {
      // Single genre, use API filter
      return fetchFeaturedAlbumsSingle(featuredType, limit, genreIds[0]);
    }
    // Multiple genres: fetch each and merge (dedupe by album id)
    const perGenreLimit = Math.ceil(limit / genreIds.length) + 2;
    const results = await Promise.all(
      genreIds.map(gid => fetchFeaturedAlbumsSingle(featuredType, perGenreLimit, gid))
    );
    const seen = new Set<string>();
    const merged: AlbumCardData[] = [];
    for (const albums of results) {
      for (const album of albums) {
        if (!seen.has(album.id)) {
          seen.add(album.id);
          merged.push(album);
        }
      }
    }
    return merged.slice(0, limit);
  }

  function toAlbumCard(album: QobuzAlbum): AlbumCardData {
    return {
      id: album.id,
      artwork: getQobuzImage(album.image),
      title: album.title,
      artist: album.artist?.name || 'Unknown Artist',
      genre: album.genre?.name || 'Unknown genre',
      quality: formatQuality(album.hires_streamable, album.maximum_bit_depth, album.maximum_sampling_rate),
      releaseDate: album.release_date_original
    };
  }

  function toDisplayTrack(track: QobuzTrack): DisplayTrack {
    return {
      id: track.id,
      title: track.title,
      artist: track.performer?.name || 'Unknown Artist',
      album: track.album?.title,
      albumArt: getQobuzImage(track.album?.image),
      albumId: track.album?.id,
      artistId: track.performer?.id,
      duration: formatDuration(track.duration),
      durationSeconds: track.duration,
      hires: track.hires_streamable,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate,
      isrc: track.isrc
    };
  }

  function toArtistCard(artist: QobuzArtist, playCount?: number): ArtistCardData {
    return {
      id: artist.id,
      name: artist.name,
      image: getQobuzImage(artist.image),
      playCount
    };
  }

  function getTrackQuality(track: DisplayTrack): string {
    return formatQuality(track.hires, track.bitDepth, track.samplingRate);
  }

  function buildContinueQueueTracks(tracks: DisplayTrack[]) {
    return tracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.artist || 'Unknown Artist',
      album: t.album || '',
      duration_secs: t.durationSeconds,
      artwork_url: t.albumArt || '',
      hires: t.hires ?? false,
      bit_depth: t.bitDepth ?? null,
      sample_rate: t.samplingRate ?? null,
      is_local: t.isLocal ?? false,
      album_id: t.albumId || null,
      artist_id: t.artistId ?? null,
    }));
  }

  async function handleContinueTrackPlay(track: DisplayTrack, trackIndex: number) {
    // Create continue listening context
    if (continueTracks.length > 0) {
      const trackIds = continueTracks.map(t => t.id);

      await setPlaybackContext(
        'home_list',
        'continue_listening',
        'Continue Listening',
        'qobuz',
        trackIds,
        trackIndex
      );
    }

    if (continueTracks.length > 0) {
      try {
        const queueTracks = buildContinueQueueTracks(continueTracks);
        await invoke('set_queue', { tracks: queueTracks, startIndex: trackIndex });
      } catch (err) {
        console.error('Failed to set queue:', err);
      }
    }

    // Play track
    if (onTrackPlay) {
      onTrackPlay(track);
    }
  }

  function buildTopArtistSeedsFromTracks(tracks: DisplayTrack[]): TopArtistSeed[] {
    const counts = new Map<number, number>();
    for (const track of tracks) {
      if (!track.artistId) continue;
      counts.set(track.artistId, (counts.get(track.artistId) ?? 0) + 1);
    }

    return Array.from(counts.entries())
      .map(([artistId, playCount]) => ({ artistId, playCount }))
      .sort((a, b) => b.playCount - a.playCount)
      .slice(0, homeLimits.topArtists);
  }

  function handleGenreFilterChange() {
    clearHomeCache();
    loadHome();
  }

  function filterAlbumsByGenre(albums: AlbumCardData[]): AlbumCardData[] {
    // getFilterGenreNames returns selected genres + all children of selected parent genres
    const filterGenreNames = getFilterGenreNames();
    if (filterGenreNames.length === 0) return albums;
    // Filter albums whose genre matches any of the filter genres (case-insensitive)
    return albums.filter(album =>
      filterGenreNames.some(genreName =>
        album.genre.toLowerCase().includes(genreName.toLowerCase())
      )
    );
  }

  // Handle tag selection - fetch playlists with the new tag filter
  async function handleTagChange(slug: string | null) {
    selectedTagSlug = slug;
    loadingQobuzPlaylists = true;
    
    try {
      const response = await invoke<DiscoverPlaylistsResponse>('get_discover_playlists', {
        tag: slug,
        limit: LIMITS.qobuzPlaylists,
        offset: 0
      });
      
      if (response.items) {
        qobuzPlaylists = response.items;
      }
    } catch (err) {
      console.error('Failed to fetch playlists by tag:', err);
    } finally {
      loadingQobuzPlaylists = false;
    }
  }

  async function fetchDiscoverData() {
    try {
      const response = await invoke<DiscoverResponse>('get_discover_index', { genreIds: null });

      // Extract playlists (limited) - initial load without tag filter
      if (response.containers.playlists?.data?.items) {
        qobuzPlaylists = response.containers.playlists.data.items.slice(0, LIMITS.qobuzPlaylists);
      }

      // Extract playlist tags
      if (response.containers.playlists_tags?.data?.items) {
        playlistTags = response.containers.playlists_tags.data.items;
      }

      // Extract essential discography (limited)
      if (response.containers.ideal_discography?.data?.items) {
        essentialDiscography = response.containers.ideal_discography.data.items.slice(0, LIMITS.essentialDiscography);
      }

      loadingQobuzPlaylists = false;
      loadingEssentialDiscography = false;
    } catch (err) {
      console.error('fetchDiscoverData failed:', err);
      loadingQobuzPlaylists = false;
      loadingEssentialDiscography = false;
    }
  }

  async function loadHome() {
    error = null;
    loadingNewReleases = true;
    loadingPressAwards = true;
    loadingMostStreamed = true;
    loadingQobuzissimes = true;
    loadingEditorPicks = true;
    loadingRecentAlbums = true;
    loadingContinueTracks = true;
    loadingTopArtists = true;
    loadingFavoriteAlbums = true;
    loadingQobuzPlaylists = true;
    loadingEssentialDiscography = true;

    // Start ML data loading FIRST (local SQLite) - this gets the seeds
    const mlPromise = invoke<HomeSeeds>('reco_get_home_ml', {
      limitRecentAlbums: homeLimits.recentAlbums,
      limitContinueTracks: homeLimits.continueTracks,
      limitTopArtists: homeLimits.topArtists,
      limitFavorites: Math.max(homeLimits.favoriteAlbums, homeLimits.favoriteTracks)
    });

    // Get current genre filter (array of IDs for multi-select)
    const genreIds = Array.from(getSelectedGenreIds());

    // Start Qobuz API calls in parallel (don't await)
    if (isSectionVisible('newReleases')) {
      fetchFeaturedAlbums('new-releases', homeLimits.featuredAlbums, genreIds).then(albums => {
        newReleases = albums;
        loadingNewReleases = false;
        loadAllAlbumDownloadStatuses(albums);
      }).catch(err => {
        console.error('Failed to load newReleases:', err);
        loadingNewReleases = false;
      });
    } else {
      loadingNewReleases = false;
    }

    if (isSectionVisible('pressAwards')) {
      fetchFeaturedAlbums('press-awards', homeLimits.featuredAlbums, genreIds).then(albums => {
        pressAwards = albums;
        loadingPressAwards = false;
        loadAllAlbumDownloadStatuses(albums);
      }).catch(err => {
        console.error('Failed to load pressAwards:', err);
        loadingPressAwards = false;
      });
    } else {
      loadingPressAwards = false;
    }

    if (isSectionVisible('mostStreamed')) {
      fetchFeaturedAlbums('most-streamed', homeLimits.featuredAlbums, genreIds).then(albums => {
        mostStreamed = albums;
        loadingMostStreamed = false;
        loadAllAlbumDownloadStatuses(albums);
      }).catch(err => {
        console.error('Failed to load mostStreamed:', err);
        loadingMostStreamed = false;
      });
    } else {
      loadingMostStreamed = false;
    }

    if (isSectionVisible('qobuzissimes')) {
      fetchFeaturedAlbums('qobuzissimes', homeLimits.featuredAlbums, genreIds).then(albums => {
        qobuzissimes = albums;
        loadingQobuzissimes = false;
        loadAllAlbumDownloadStatuses(albums);
      }).catch(err => {
        console.error('Failed to load qobuzissimes:', err);
        loadingQobuzissimes = false;
      });
    } else {
      loadingQobuzissimes = false;
    }

    if (isSectionVisible('editorPicks')) {
      fetchFeaturedAlbums('editor-picks', homeLimits.featuredAlbums, genreIds).then(albums => {
        editorPicks = albums;
        loadingEditorPicks = false;
        loadAllAlbumDownloadStatuses(albums);
      }).catch(err => {
        console.error('Failed to load editorPicks:', err);
        loadingEditorPicks = false;
      });
    } else {
      loadingEditorPicks = false;
    }

    // Fetch Discover data (Qobuz Playlists + Essential Discography)
    const needsDiscoverData = isSectionVisible('qobuzPlaylists') || isSectionVisible('essentialDiscography');
    if (needsDiscoverData) {
      fetchDiscoverData();
    } else {
      loadingQobuzPlaylists = false;
      loadingEssentialDiscography = false;
    }

    try {
      // Wait for ML seeds (local data)
      const seeds = await mlPromise;

      // Load ML-based sections in parallel
      // Continue Listening (tracks)
      if (isSectionVisible('continueTracks')) {
        fetchTracks(seeds.continueListeningTrackIds).then(tracks => {
          continueTracks = tracks;
          loadingContinueTracks = false;
        }).catch(err => {
          console.error('Failed to load continueTracks:', err);
          loadingContinueTracks = false;
        });
      } else {
        loadingContinueTracks = false;
      }

      // Recently Played (albums) - start immediately with seeds
      if (isSectionVisible('recentAlbums')) {
        const recentAlbumIds = normalizeAlbumIds(seeds.recentlyPlayedAlbumIds);
        // Fetch more if filtering, to have enough after filter
        const fetchLimit = hasGenreFilter() ? homeLimits.recentAlbums * 3 : homeLimits.recentAlbums;
        fetchAlbums(recentAlbumIds.slice(0, fetchLimit)).then(albums => {
          const filtered = filterAlbumsByGenre(albums).slice(0, homeLimits.recentAlbums);
          recentAlbums = filtered;
          loadingRecentAlbums = false;
          loadAllAlbumDownloadStatuses(filtered);
        }).catch(err => {
          console.error('Failed to load recentAlbums:', err);
          loadingRecentAlbums = false;
        });
      } else {
        loadingRecentAlbums = false;
      }

      // Top Artists
      if (isSectionVisible('topArtists')) {
        fetchArtists(seeds.topArtistIds.slice(0, homeLimits.topArtists)).then(artists => {
          topArtists = artists;
          loadingTopArtists = false;
        }).catch(err => {
          console.error('Failed to load topArtists:', err);
          loadingTopArtists = false;
        });
      } else {
        loadingTopArtists = false;
      }

      // Favorite Albums
      if (isSectionVisible('favoriteAlbums')) {
        // Get favorite track details to extract album IDs
        fetchTracks(seeds.favoriteTrackIds.slice(0, homeLimits.favoriteTracks)).then(async favoriteTrackDetails => {
          const favoriteAlbumIds = normalizeAlbumIds([
            ...seeds.favoriteAlbumIds,
            ...favoriteTrackDetails.map(track => track.albumId)
          ]);
          // Fetch more if filtering, to have enough after filter
          const fetchLimit = hasGenreFilter() ? homeLimits.favoriteAlbums * 3 : homeLimits.favoriteAlbums;
          const albums = await fetchAlbums(favoriteAlbumIds.slice(0, fetchLimit));
          const filtered = filterAlbumsByGenre(albums).slice(0, homeLimits.favoriteAlbums);
          favoriteAlbums = filtered;
          loadingFavoriteAlbums = false;
          loadAllAlbumDownloadStatuses(filtered);
        }).catch(err => {
          console.error('Failed to load favoriteAlbums:', err);
          loadingFavoriteAlbums = false;
        });
      } else {
        loadingFavoriteAlbums = false;
      }

    } catch (err) {
      console.error('ML seeds failed:', err);
      error = String(err);
      loadingRecentAlbums = false;
      loadingContinueTracks = false;
      loadingTopArtists = false;
      loadingFavoriteAlbums = false;
    }
  }
</script>

<div class="home-view" bind:this={homeViewEl} onscroll={handleHomeScroll}>
  <!-- Header with greeting, filter and settings -->
  <div class="home-header">
    {#if homeSettings.greeting.enabled}
      <h2 class="greeting">{getGreetingText()}</h2>
    {:else}
      <div></div>
    {/if}
    <div class="header-actions">
      <GenreFilterButton onFilterChange={handleGenreFilterChange} />
      <button class="settings-btn" onclick={() => isSettingsModalOpen = true} title={$t('home.customizeHome')}>
        <img src="/home-gear.svg" alt="Settings" class="settings-icon" />
      </button>
    </div>
  </div>

  {#if error}
    <div class="home-state">
      <div class="state-icon">
        <Music size={36} />
      </div>
      <h1>{$t('home.loadError')}</h1>
      <p>{error}</p>
    </div>
  {/if}

  <!-- Progressive sections: each appears as soon as its data arrives -->
  {#each renderableSections as sectionId (sectionId)}
    {#if sectionId === 'newReleases'}
      {#if loadingNewReleases}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
          </div>
        </div>
      {:else if newReleases.length > 0}
        <HorizontalScrollRow title={$t('home.newReleases')}>
          {#snippet children()}
            {#each newReleases as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                genre={album.genre}
                releaseDate={album.releaseDate}
                size="large"
                quality={album.quality}
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
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'pressAwards'}
      {#if loadingPressAwards}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
          </div>
        </div>
      {:else if pressAwards.length > 0}
        <HorizontalScrollRow title={$t('home.pressAwards')}>
          {#snippet children()}
            {#each pressAwards as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                genre={album.genre}
                releaseDate={album.releaseDate}
                size="large"
                quality={album.quality}
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
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'mostStreamed'}
      {#if loadingMostStreamed}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
          </div>
        </div>
      {:else if mostStreamed.length > 0}
        <HorizontalScrollRow title={$t('home.popularAlbums')}>
          {#snippet children()}
            {#each mostStreamed as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                genre={album.genre}
                releaseDate={album.releaseDate}
                size="large"
                quality={album.quality}
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
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'qobuzissimes'}
      {#if loadingQobuzissimes}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
          </div>
        </div>
      {:else if qobuzissimes.length > 0}
        <HorizontalScrollRow title={$t('home.qobuzissimes')}>
          {#snippet children()}
            {#each qobuzissimes as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                genre={album.genre}
                releaseDate={album.releaseDate}
                size="large"
                quality={album.quality}
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
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'editorPicks'}
      {#if loadingEditorPicks}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
          </div>
        </div>
      {:else if editorPicks.length > 0}
        <HorizontalScrollRow title={$t('home.editorPicks')}>
          {#snippet children()}
            {#each editorPicks as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                genre={album.genre}
                releaseDate={album.releaseDate}
                size="large"
                quality={album.quality}
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
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'qobuzPlaylists'}
      {#if loadingQobuzPlaylists}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 5 } as _}<div class="skeleton-card-wide"></div>{/each}
          </div>
        </div>
      {:else if qobuzPlaylists.length > 0}
        <HorizontalScrollRow>
          {#snippet header()}
            <h2 class="section-title">{$t('home.qobuzPlaylists')}</h2>
            {#if playlistTags.length > 0}
              <PlaylistTagFilter
                tags={playlistTags}
                selectedTag={selectedTagSlug}
                onTagChange={handleTagChange}
              />
            {/if}
          {/snippet}
          {#snippet children()}
            {#if loadingQobuzPlaylists}
              <div class="loading-playlists">
                <Loader2 size={24} class="spinner" />
              </div>
            {:else}
              {#each qobuzPlaylists as playlist (playlist.id)}
                <QobuzPlaylistCard
                  playlistId={playlist.id}
                  name={playlist.name}
                  owner={playlist.owner?.name || 'Qobuz'}
                  image={playlist.image?.rectangle || playlist.image?.covers?.[0]}
                  trackCount={playlist.tracks_count}
                  duration={playlist.duration}
                  genre={playlist.genres?.[0]?.name}
                  onclick={onPlaylistClick ? () => onPlaylistClick(playlist.id) : undefined}
                  onPlay={onPlaylistPlay ? () => onPlaylistPlay(playlist.id) : undefined}
                  onPlayNext={onPlaylistPlayNext ? () => onPlaylistPlayNext(playlist.id) : undefined}
                  onPlayLater={onPlaylistPlayLater ? () => onPlaylistPlayLater(playlist.id) : undefined}
                  onCopyToLibrary={onPlaylistCopyToLibrary ? () => onPlaylistCopyToLibrary(playlist.id) : undefined}
                  onShareQobuz={onPlaylistShareQobuz ? () => onPlaylistShareQobuz(playlist.id) : undefined}
                />
              {/each}
            {/if}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'essentialDiscography'}
      {#if loadingEssentialDiscography}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
          </div>
        </div>
      {:else if essentialDiscography.length > 0}
        <HorizontalScrollRow title={$t('home.essentialDiscography')}>
          {#snippet children()}
            {#each essentialDiscography as album (album.id)}
              <AlbumCard
                albumId={album.id}
                artwork={album.image?.large || album.image?.small || ''}
                title={album.title}
                artist={album.artists?.[0]?.name || 'Unknown Artist'}
                genre={album.genre?.name || ''}
                releaseDate={album.dates?.original}
                size="large"
                quality={formatQuality(
                  (album.audio_info?.maximum_bit_depth ?? 16) > 16,
                  album.audio_info?.maximum_bit_depth,
                  album.audio_info?.maximum_sampling_rate
                )}
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
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'recentAlbums'}
      {#if loadingRecentAlbums}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
          </div>
        </div>
      {:else if recentAlbums.length > 0}
        <HorizontalScrollRow title={$t('home.recentlyPlayed')}>
          {#snippet children()}
            {#each recentAlbums as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                genre={album.genre}
                releaseDate={album.releaseDate}
                size="large"
                quality={album.quality}
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
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'continueTracks'}
      {#if loadingContinueTracks}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-tracks">
            {#each { length: 5 } as _}<div class="skeleton-track"></div>{/each}
          </div>
        </div>
      {:else if continueTracks.length > 0}
        <div class="section">
          <div class="section-header">
            <h2>{$t('home.continueListening')}</h2>
          </div>
          <div class="track-list compact">
            {#each continueTracks as track, index (`${track.id}-${downloadStateVersion}`)}
              {@const isActiveTrack = isPlaybackActive && activeTrackId === track.id}
              {@const cacheStatus = getTrackOfflineCacheStatus?.(track.id) ?? { status: 'none', progress: 0 }}
              {@const isTrackDownloaded = cacheStatus.status === 'ready'}
              {@const trackBlacklisted = track.artistId ? isArtistBlacklisted(track.artistId) : false}
              <TrackRow
                trackId={track.id}
                number={index + 1}
                title={track.title}
                artist={track.artist}
                album={track.album}
                duration={track.duration}
                quality={getTrackQuality(track)}
                isPlaying={isActiveTrack}
                isBlacklisted={trackBlacklisted}
                compact={true}
                hideDownload={trackBlacklisted}
                hideFavorite={trackBlacklisted}
                downloadStatus={cacheStatus.status}
                downloadProgress={cacheStatus.progress}
                onArtistClick={track.artistId && onArtistClick ? () => onArtistClick(track.artistId!) : undefined}
                onAlbumClick={track.albumId && onAlbumClick ? () => onAlbumClick(track.albumId!) : undefined}
                onPlay={trackBlacklisted ? undefined : () => handleContinueTrackPlay(track, index)}
                menuActions={trackBlacklisted ? {
                  onGoToAlbum: track.albumId && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.albumId!) : undefined,
                  onGoToArtist: track.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(track.artistId!) : undefined,
                  onShowInfo: onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined
                } : {
                  onPlayNow: () => handleContinueTrackPlay(track, index),
                  onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
                  onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined,
                  onAddToPlaylist: onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined,
                  onShareQobuz: onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
                  onShareSonglink: onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined,
                  onGoToAlbum: track.albumId && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.albumId!) : undefined,
                  onGoToArtist: track.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(track.artistId!) : undefined,
                  onShowInfo: onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined,
                  onDownload: onTrackDownload ? () => onTrackDownload(track) : undefined,
                  isTrackDownloaded,
                  onReDownload: isTrackDownloaded && onTrackReDownload ? () => onTrackReDownload(track) : undefined,
                  onRemoveDownload: isTrackDownloaded && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined
                }}
              />
            {/each}
          </div>
        </div>
      {/if}
    {/if}

    {#if sectionId === 'topArtists'}
      {#if loadingTopArtists}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-artist"></div>{/each}
          </div>
        </div>
      {:else if topArtists.length > 0}
        <HorizontalScrollRow title={$t('home.yourTopArtists')}>
          {#snippet children()}
            {#each topArtists as artist}
              <button class="artist-card" onclick={() => onArtistClick?.(artist.id)}>
                <div class="artist-image-wrapper">
                  <div class="artist-image-placeholder">
                    <User size={48} />
                  </div>
                  {#if !failedArtistImages.has(artist.id) && artist.image}
                    <img
                      src={artist.image}
                      alt={artist.name}
                      class="artist-image"
                      loading="lazy"
                      decoding="async"
                      onerror={() => handleArtistImageError(artist.id)}
                    />
                  {/if}
                </div>
                <div class="artist-name">{artist.name}</div>
                {#if artist.playCount}
                  <div class="artist-meta">{$t('home.artistPlays', { values: { count: artist.playCount } })}</div>
                {/if}
              </button>
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}

    {#if sectionId === 'favoriteAlbums'}
      {#if loadingFavoriteAlbums}
        <div class="skeleton-section">
          <div class="skeleton-title"></div>
          <div class="skeleton-row">
            {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
          </div>
        </div>
      {:else if favoriteAlbums.length > 0}
        <HorizontalScrollRow title={$t('home.moreFromFavorites')}>
          {#snippet children()}
            {#each favoriteAlbums as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                genre={album.genre}
                releaseDate={album.releaseDate}
                size="large"
                quality={album.quality}
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
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}
    {/if}
  {/each}

  <!-- Empty state: only show after all loading completes with no content -->
  {#if !anyLoading && !hasContent && !error}
    <div class="home-state">
      <div class="state-icon">
        <Music size={48} />
      </div>
      <h1>{$t('home.startListening')}</h1>
      <p>{$t('home.startListeningDescription')}</p>
    </div>
  {/if}

  <!-- Settings Modal -->
  <HomeSettingsModal
    isOpen={isSettingsModalOpen}
    onClose={() => isSettingsModalOpen = false}
  />
</div>

<style>
  .home-view {
    width: 100%;
    height: 100%;
    padding: 24px;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    position: relative;
  }

  /* Add spacing between sections - using :global to affect child components */
  .home-view > :global(*:not(:first-child)) {
    margin-top: 60px !important;
  }

  /* Second child (first section after header) gets less spacing */
  .home-view > :global(*:nth-child(2)) {
    margin-top: 30px !important;
  }

  /* Custom scrollbar */
  .home-view::-webkit-scrollbar {
    width: 6px;
  }

  .home-view::-webkit-scrollbar-track {
    background: transparent;
  }

  .home-view::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .home-view::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  /* Skeleton loading placeholders */
  .skeleton-section {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .skeleton-title {
    width: 180px;
    height: 22px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    animation: skeleton-pulse 1.5s ease-in-out infinite;
  }

  .skeleton-row {
    display: flex;
    gap: 16px;
    overflow: hidden;
  }

  .skeleton-card {
    width: 180px;
    height: 240px;
    background: var(--bg-tertiary);
    border-radius: 12px;
    flex-shrink: 0;
    animation: skeleton-pulse 1.5s ease-in-out infinite;
  }

  .skeleton-card-wide {
    width: 260px;
    height: 180px;
    background: var(--bg-tertiary);
    border-radius: 12px;
    flex-shrink: 0;
    animation: skeleton-pulse 1.5s ease-in-out infinite;
  }

  .skeleton-artist {
    width: 180px;
    height: 220px;
    background: var(--bg-tertiary);
    border-radius: 12px;
    flex-shrink: 0;
    animation: skeleton-pulse 1.5s ease-in-out infinite;
  }

  .skeleton-tracks {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .skeleton-track {
    width: 100%;
    height: 40px;
    background: var(--bg-tertiary);
    border-radius: 8px;
    animation: skeleton-pulse 1.5s ease-in-out infinite;
  }

  @keyframes skeleton-pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }

  .home-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
  }

  .greeting {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .settings-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    background: transparent;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: all 150ms ease;
    opacity: 0.7;
  }

  .settings-btn:hover {
    background: var(--bg-hover);
    opacity: 1;
  }

  .settings-icon {
    width: 22px;
    height: 22px;
    filter: invert(1) opacity(0.8);
  }

  .spacer {
    width: 60px;
    flex-shrink: 0;
  }

  .section {
    margin-bottom: 32px;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .section-header h2 {
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .section-title {
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .loading-playlists {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 180px;
    min-height: 180px;
    color: var(--text-muted);
  }

  .loading-playlists :global(.spinner) {
    animation: spin 1s linear infinite;
  }

  .track-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .track-list.compact {
    gap: 4px;
  }

  .track-list.compact :global(.track-row.compact) {
    height: 40px;
    padding: 0 10px;
  }

  .artist-card {
    width: 180px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    padding: 16px 12px;
    color: var(--text-primary);
    cursor: pointer;
    transition: border-color 150ms ease, background-color 150ms ease;
  }

  .artist-card:hover {
    border-color: var(--accent-primary);
    background-color: var(--bg-hover);
  }

  .artist-image-wrapper {
    position: relative;
    width: 140px;
    height: 140px;
    border-radius: 50%;
    overflow: hidden;
  }

  .artist-image,
  .artist-image-placeholder {
    width: 140px;
    height: 140px;
    border-radius: 50%;
  }

  .artist-image {
    position: absolute;
    inset: 0;
    object-fit: cover;
    z-index: 1;
  }

  .artist-image-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
  }

  .artist-name {
    font-size: 14px;
    font-weight: 600;
    text-align: center;
  }

  .artist-meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  .home-state {
    min-height: calc(100vh - 240px);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    gap: 12px;
    color: var(--text-muted);
  }

  .home-state h1 {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .home-state p {
    font-size: 15px;
    margin: 0;
    max-width: 360px;
  }

  .state-icon {
    width: 64px;
    height: 64px;
    border-radius: 16px;
    background: var(--bg-secondary);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
