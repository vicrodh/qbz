<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Music, User, Loader2, ArrowRight } from 'lucide-svelte';
  import { type OfflineCacheStatus } from '$lib/stores/offlineCacheState';
  import {
    getHomeCache,
    setHomeCache,
    clearHomeCache,
    getHomeCacheStatus,
    updateHomeCacheScrollTop,
    type HomeCacheData
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

  interface AlbumCardData {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    artistId?: number;
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

  interface HomeResolved {
    recentlyPlayedAlbums: AlbumCardData[];
    continueListeningTracks: DisplayTrack[];
    topArtists: ArtistCardData[];
    favoriteAlbums: AlbumCardData[];
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
    getTrackOfflineCacheStatus?: (trackId: number) => { status: OfflineCacheStatus; progress: number };
    onPlaylistClick?: (playlistId: number) => void;
    onPlaylistPlay?: (playlistId: number) => void;
    onPlaylistPlayNext?: (playlistId: number) => void;
    onPlaylistPlayLater?: (playlistId: number) => void;
    onPlaylistCopyToLibrary?: (playlistId: number) => void;
    onPlaylistShareQobuz?: (playlistId: number) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
    onNavigateNewReleases?: () => void;
    onNavigateIdealDiscography?: () => void;
    onNavigateTopAlbums?: () => void;
    onNavigateQobuzissimes?: () => void;
    onNavigateAlbumsOfTheWeek?: () => void;
    onNavigatePressAccolades?: () => void;
    onNavigateQobuzPlaylists?: () => void;
    onNavigateDailyQ?: () => void;
    onNavigateWeeklyQ?: () => void;
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
    isPlaybackActive = false,
    onNavigateNewReleases,
    onNavigateIdealDiscography,
    onNavigateTopAlbums,
    onNavigateQobuzissimes,
    onNavigateAlbumsOfTheWeek,
    onNavigatePressAccolades,
    onNavigateQobuzPlaylists,
    onNavigateDailyQ,
    onNavigateWeeklyQ,
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

  function restoreFromCache(cached: HomeCacheData) {
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

    requestAnimationFrame(() => {
      if (homeViewEl && cached.scrollTop > 0) {
        homeViewEl.scrollTop = cached.scrollTop;
      }
    });

    const allAlbums = [
      ...cached.newReleases, ...cached.pressAwards, ...cached.mostStreamed,
      ...cached.qobuzissimes, ...cached.editorPicks,
      ...cached.recentAlbums, ...cached.favoriteAlbums
    ];
    loadAllAlbumDownloadStatuses(allAlbums);
  }

  onMount(() => {
    // Subscribe to home settings changes — invalidate cache on change
    const unsubscribe = subscribeHomeSettings(() => {
      homeSettings = getSettings();
      homeLimits = getSettings().limits;
      clearHomeCache();
    });

    const currentGenreIds = Array.from(getSelectedGenreIds());
    const cacheStatus = getHomeCacheStatus(currentGenreIds);

    if (cacheStatus === 'fresh') {
      // Fresh: use cache as-is, no revalidation
      restoreFromCache(getHomeCache()!);
    } else if (cacheStatus === 'stale') {
      // Stale: show cache immediately, then revalidate silently in background
      restoreFromCache(getHomeCache()!);
      // Background revalidation — no skeletons, just update data if changed
      loadHome({ showSkeletons: false });
    } else {
      // Empty: full skeleton load
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
        batch.map(albumId => invoke<QobuzAlbum>('v2_get_album', { albumId }))
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
        batch.map(trackId => invoke<QobuzTrack>('v2_get_track', { trackId }))
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

  function discoverAlbumToCardData(album: DiscoverAlbum): AlbumCardData {
    return {
      id: album.id,
      artwork: album.image?.large || album.image?.small || '',
      title: album.title,
      artist: album.artists?.[0]?.name || 'Unknown Artist',
      artistId: album.artists?.[0]?.id,
      genre: album.genre?.name || '',
      quality: formatQuality(
        (album.audio_info?.maximum_bit_depth ?? 16) > 16,
        album.audio_info?.maximum_bit_depth,
        album.audio_info?.maximum_sampling_rate
      ),
      releaseDate: album.dates?.original
    };
  }

  function toAlbumCard(album: QobuzAlbum): AlbumCardData {
    return {
      id: album.id,
      artwork: getQobuzImage(album.image),
      title: album.title,
      artist: album.artist?.name || 'Unknown Artist',
      artistId: album.artist?.id,
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
    return tracks.map(track => ({
      id: track.id,
      title: track.title,
      artist: track.artist || 'Unknown Artist',
      album: track.album || '',
      duration_secs: track.durationSeconds,
      artwork_url: track.albumArt || '',
      hires: track.hires ?? false,
      bit_depth: track.bitDepth ?? null,
      sample_rate: track.samplingRate ?? null,
      is_local: track.isLocal ?? false,
      album_id: track.albumId || null,
      artist_id: track.artistId ?? null,
    }));
  }

  async function handleContinueTrackPlay(track: DisplayTrack, trackIndex: number) {
    // Create continue listening context
    if (continueTracks.length > 0) {
      const trackIds = continueTracks.map(track => track.id);

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
        await invoke('v2_set_queue', { tracks: queueTracks, startIndex: trackIndex });
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
      const response = await invoke<DiscoverPlaylistsResponse>('v2_get_discover_playlists', {
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

  async function fetchAllDiscoverData(genreIds: number[]) {
    try {
      const apiGenreIds = genreIds.length > 0 ? genreIds : null;
      const response = await invoke<DiscoverResponse>('v2_get_discover_index', { genreIds: apiGenreIds });
      const c = response.containers;

      // Extract editorial album sections
      if (isSectionVisible('newReleases') && c.new_releases?.data?.items) {
        newReleases = c.new_releases.data.items.slice(0, homeLimits.featuredAlbums).map(discoverAlbumToCardData);
        loadingNewReleases = false;
        await tick();
        loadAllAlbumDownloadStatuses(newReleases).catch(() => {});
      } else {
        loadingNewReleases = false;
      }

      if (isSectionVisible('pressAwards') && c.press_awards?.data?.items) {
        pressAwards = c.press_awards.data.items.slice(0, homeLimits.featuredAlbums).map(discoverAlbumToCardData);
        loadingPressAwards = false;
        await tick();
        loadAllAlbumDownloadStatuses(pressAwards).catch(() => {});
      } else {
        loadingPressAwards = false;
      }

      if (isSectionVisible('mostStreamed') && c.most_streamed?.data?.items) {
        mostStreamed = c.most_streamed.data.items.slice(0, homeLimits.featuredAlbums).map(discoverAlbumToCardData);
        loadingMostStreamed = false;
        await tick();
        loadAllAlbumDownloadStatuses(mostStreamed).catch(() => {});
      } else {
        loadingMostStreamed = false;
      }

      if (isSectionVisible('qobuzissimes') && c.qobuzissims?.data?.items) {
        qobuzissimes = c.qobuzissims.data.items.slice(0, homeLimits.featuredAlbums).map(discoverAlbumToCardData);
        loadingQobuzissimes = false;
        await tick();
        loadAllAlbumDownloadStatuses(qobuzissimes).catch(() => {});
      } else {
        loadingQobuzissimes = false;
      }

      if (isSectionVisible('editorPicks') && c.album_of_the_week?.data?.items) {
        editorPicks = c.album_of_the_week.data.items.slice(0, homeLimits.featuredAlbums).map(discoverAlbumToCardData);
        loadingEditorPicks = false;
        await tick();
        loadAllAlbumDownloadStatuses(editorPicks).catch(() => {});
      } else {
        loadingEditorPicks = false;
      }

      // Extract playlists (limited) - initial load without tag filter
      if (isSectionVisible('qobuzPlaylists') && c.playlists?.data?.items) {
        qobuzPlaylists = c.playlists.data.items.slice(0, LIMITS.qobuzPlaylists);
      }
      loadingQobuzPlaylists = false;

      // Fetch localized playlist tags from dedicated endpoint
      if (playlistTags.length === 0) {
        invoke<PlaylistTag[]>('v2_get_playlist_tags')
          .then(tags => { playlistTags = tags; })
          .catch(err => console.error('Failed to fetch playlist tags:', err));
      }

      // Extract essential discography (limited)
      if (isSectionVisible('essentialDiscography') && c.ideal_discography?.data?.items) {
        essentialDiscography = c.ideal_discography.data.items.slice(0, LIMITS.essentialDiscography);
      }
      loadingEssentialDiscography = false;
    } catch (err) {
      console.error('fetchAllDiscoverData failed:', err);
      loadingNewReleases = false;
      loadingPressAwards = false;
      loadingMostStreamed = false;
      loadingQobuzissimes = false;
      loadingEditorPicks = false;
      loadingQobuzPlaylists = false;
      loadingEssentialDiscography = false;
    }
  }

  async function loadHome(options?: { showSkeletons?: boolean }) {
    const showSkeletons = options?.showSkeletons ?? true;
    error = null;
    if (showSkeletons) {
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
    }

    // Get current genre filter (array of IDs for multi-select)
    const genreIds = Array.from(getSelectedGenreIds());

    // Two parallel paths:
    // 1. Single Qobuz discover API call (all editorial content)
    // 2. ML seeds from local SQLite -> user-specific sections
    const discoverPromise = fetchAllDiscoverData(genreIds);

    // Single IPC call returns fully-resolved card data (3-tier cache in Rust)
    const mlPromise = invoke<HomeResolved>('v2_reco_get_home_resolved', {
      limitRecentAlbums: homeLimits.recentAlbums,
      limitContinueTracks: homeLimits.continueTracks,
      limitTopArtists: homeLimits.topArtists,
      limitFavorites: Math.max(homeLimits.favoriteAlbums, homeLimits.favoriteTracks)
    });

    try {
      const resolved = await mlPromise;

      // Recently Played Albums
      if (isSectionVisible('recentAlbums')) {
        const filtered = filterAlbumsByGenre(resolved.recentlyPlayedAlbums).slice(0, homeLimits.recentAlbums);
        recentAlbums = filtered;
        loadingRecentAlbums = false;
        await tick();
        loadAllAlbumDownloadStatuses(filtered).catch(() => {});
      } else {
        loadingRecentAlbums = false;
      }

      // Continue Listening Tracks
      if (isSectionVisible('continueTracks')) {
        continueTracks = resolved.continueListeningTracks;
        loadingContinueTracks = false;
      } else {
        loadingContinueTracks = false;
      }

      // Top Artists
      if (isSectionVisible('topArtists')) {
        topArtists = resolved.topArtists;
        loadingTopArtists = false;
      } else {
        loadingTopArtists = false;
      }

      // Favorite Albums
      if (isSectionVisible('favoriteAlbums')) {
        const filtered = filterAlbumsByGenre(resolved.favoriteAlbums).slice(0, homeLimits.favoriteAlbums);
        favoriteAlbums = filtered;
        loadingFavoriteAlbums = false;
        await tick();
        loadAllAlbumDownloadStatuses(filtered).catch(() => {});
      } else {
        loadingFavoriteAlbums = false;
      }

    } catch (err) {
      console.error('Home resolved failed:', err);
      error = String(err);
      loadingRecentAlbums = false;
      loadingContinueTracks = false;
      loadingTopArtists = false;
      loadingFavoriteAlbums = false;
    }

    // Ensure discover promise completes (errors already handled internally)
    await discoverPromise;
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
        <img
          src="/home-gear.svg"
          alt="Settings"
          class="settings-icon"
          width="22"
          height="22"
          style="width:22px;height:22px;filter:invert(1) opacity(0.8);"
        />
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
        <HorizontalScrollRow>
          {#snippet header()}
            <div class="section-header-group">
              <h2 class="section-title">{$t('home.newReleases')}</h2>
              {#if onNavigateNewReleases}
                <button class="see-all-link" onclick={onNavigateNewReleases}>{$t('home.seeAll')}<ArrowRight size={14} /></button>
              {/if}
            </div>
          {/snippet}
          {#snippet children()}
            {#each newReleases as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                artistId={album.artistId}
                onArtistClick={onArtistClick}
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
        <HorizontalScrollRow>
          {#snippet header()}
            <div class="section-header-group">
              <h2 class="section-title">{$t('home.pressAwards')}</h2>
              {#if onNavigatePressAccolades}
                <button class="see-all-link" onclick={onNavigatePressAccolades}>{$t('home.seeAll')}<ArrowRight size={14} /></button>
              {/if}
            </div>
          {/snippet}
          {#snippet children()}
            {#each pressAwards as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                artistId={album.artistId}
                onArtistClick={onArtistClick}
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
        <HorizontalScrollRow>
          {#snippet header()}
            <div class="section-header-group">
              <h2 class="section-title">{$t('home.popularAlbums')}</h2>
              {#if onNavigateTopAlbums}
                <button class="see-all-link" onclick={onNavigateTopAlbums}>{$t('home.seeAll')}<ArrowRight size={14} /></button>
              {/if}
            </div>
          {/snippet}
          {#snippet children()}
            {#each mostStreamed as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                artistId={album.artistId}
                onArtistClick={onArtistClick}
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
        <HorizontalScrollRow>
          {#snippet header()}
            <div class="section-header-group">
              <h2 class="section-title">{$t('home.qobuzissimes')}</h2>
              {#if onNavigateQobuzissimes}
                <button class="see-all-link" onclick={onNavigateQobuzissimes}>{$t('home.seeAll')}<ArrowRight size={14} /></button>
              {/if}
            </div>
          {/snippet}
          {#snippet children()}
            {#each qobuzissimes as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                artistId={album.artistId}
                onArtistClick={onArtistClick}
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
        <HorizontalScrollRow>
          {#snippet header()}
            <div class="section-header-group">
              <h2 class="section-title">{$t('home.editorPicks')}</h2>
              {#if onNavigateAlbumsOfTheWeek}
                <button class="see-all-link" onclick={onNavigateAlbumsOfTheWeek}>{$t('home.seeAll')}<ArrowRight size={14} /></button>
              {/if}
            </div>
          {/snippet}
          {#snippet children()}
            {#each editorPicks as album}
              <AlbumCard
                albumId={album.id}
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                artistId={album.artistId}
                onArtistClick={onArtistClick}
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
            <div class="section-header-with-tags">
              <div class="section-header-group">
                <h2 class="section-title">{$t('home.qobuzPlaylists')}</h2>
                {#if onNavigateQobuzPlaylists}
                  <button class="see-all-link" onclick={onNavigateQobuzPlaylists}>{$t('home.seeAll')}<ArrowRight size={14} /></button>
                {/if}
              </div>
              {#if playlistTags.length > 0}
                <PlaylistTagFilter
                  tags={playlistTags}
                  selectedTag={selectedTagSlug}
                  onTagChange={handleTagChange}
                />
              {/if}
            </div>
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
        <HorizontalScrollRow>
          {#snippet header()}
            <div class="section-header-group">
              <h2 class="section-title">{$t('home.essentialDiscography')}</h2>
              {#if onNavigateIdealDiscography}
                <button class="see-all-link" onclick={onNavigateIdealDiscography}>{$t('home.seeAll')}<ArrowRight size={14} /></button>
              {/if}
            </div>
          {/snippet}
          {#snippet children()}
            {#each essentialDiscography as album (album.id)}
              <AlbumCard
                albumId={album.id}
                artwork={album.image?.large || album.image?.small || ''}
                title={album.title}
                artist={album.artists?.[0]?.name || 'Unknown Artist'}
                artistId={album.artists?.[0]?.id}
                onArtistClick={onArtistClick}
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
                artistId={album.artistId}
                onArtistClick={onArtistClick}
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
            {#each continueTracks as track, index (track.id)}
              {@const isActiveTrack = isPlaybackActive && activeTrackId === track.id}
              {@const cacheStatus = getTrackOfflineCacheStatus?.(track.id) ?? { status: 'none' as const, progress: 0 }}
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
                onDownload={!trackBlacklisted && onTrackDownload ? () => onTrackDownload(track) : undefined}
                onRemoveDownload={isTrackDownloaded && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
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
                artistId={album.artistId}
                onArtistClick={onArtistClick}
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

    {#if sectionId === 'yourMixes'}
      <div class="your-mixes-section">
        <h2 class="section-title">{$t('home.yourMixes')}</h2>
        <div class="mix-cards-row">
          <button class="mix-card" onclick={() => onNavigateDailyQ?.()}>
            <div class="mix-card-artwork mix-gradient-daily">
              <span class="mix-card-badge">qobuz</span>
              <span class="mix-card-name">DailyQ</span>
            </div>
            <div class="mix-card-info">
              <div class="mix-card-title">{$t('yourMixes.title')}</div>
              <div class="mix-card-owner">Qobuz</div>
            </div>
          </button>
          <button class="mix-card" onclick={() => onNavigateWeeklyQ?.()}>
            <div class="mix-card-artwork mix-gradient-weekly">
              <span class="mix-card-badge">qobuz</span>
              <span class="mix-card-name">WeeklyQ</span>
            </div>
            <div class="mix-card-info">
              <div class="mix-card-title">{$t('weeklyMixes.title')}</div>
              <div class="mix-card-owner">Qobuz</div>
            </div>
          </button>
        </div>
      </div>
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

  .section-title {
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .section-header-group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .section-header-with-tags {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .see-all-link {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: none;
    border: none;
    color: #666666;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    padding: 0;
    white-space: nowrap;
    transition: color 150ms ease;
  }

  .see-all-link:hover {
    color: var(--text-primary);
  }

  .your-mixes-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .mix-cards-row {
    display: flex;
    gap: 16px;
  }

  .mix-card {
    flex-shrink: 0;
    width: 180px;
    cursor: pointer;
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    color: inherit;
  }

  .mix-card-artwork {
    width: 180px;
    height: 180px;
    border-radius: 8px;
    overflow: hidden;
    margin-bottom: 8px;
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 12px;
    box-sizing: border-box;
  }

  .mix-gradient-daily {
    background:
      radial-gradient(ellipse at 30% 20%, rgba(255, 255, 255, 0.25) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 60%, rgba(255, 200, 50, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 80%, rgba(255, 140, 0, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #e8a020 0%, #d4781a 30%, #c45e18 60%, #a04010 100%);
  }

  .mix-gradient-weekly {
    background:
      radial-gradient(ellipse at 40% 20%, rgba(255, 200, 255, 0.35) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 50%, rgba(200, 150, 255, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 70%, rgba(130, 80, 200, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #b060d0 0%, #8040b0 30%, #6030a0 60%, #402080 100%);
  }

  .mix-card-badge {
    font-size: 11px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.7);
    letter-spacing: 0.02em;
  }

  .mix-card-name {
    font-size: 22px;
    font-weight: 700;
    color: #fff;
    line-height: 1.1;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.2);
  }

  .mix-card-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.4;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-bottom: 2px;
  }

  .mix-card-owner {
    font-size: 12px;
    font-weight: 400;
    color: var(--text-muted);
    line-height: 1.4;
  }

</style>
