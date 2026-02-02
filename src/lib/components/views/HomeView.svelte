<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Music, User, Loader2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import HomeSettingsModal from '../HomeSettingsModal.svelte';
  import GenreFilterButton from '../GenreFilterButton.svelte';
  import { formatDuration, formatQuality, getQobuzImage } from '$lib/adapters/qobuzAdapters';
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
  import type { QobuzAlbum, QobuzArtist, QobuzTrack, DisplayTrack } from '$lib/types';

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
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
    sidebarExpanded?: boolean;
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
    activeTrackId = null,
    isPlaybackActive = false,
    sidebarExpanded = true
  }: Props = $props();

  // Home settings state
  let homeSettings = $state<HomeSettings>(getSettings());
  let isSettingsModalOpen = $state(false);

  // Computed greeting with i18n support
  const greetingText = $derived.by(() => {
    const info = getGreetingInfo(userName);
    if (info.type === 'custom') {
      return info.text;
    }
    return $t(info.key, { name: info.name });
  });


  // Check if a section is visible
  function isSectionVisible(sectionId: HomeSectionId): boolean {
    const section = homeSettings.sections.find(s => s.id === sectionId);
    return section?.visible ?? true;
  }

  // Get ordered visible sections
  const visibleSections = $derived(
    homeSettings.sections.filter(s => s.visible).map(s => s.id)
  );

  const LIMITS = {
    recentAlbums: 20,
    continueTracks: 10,
    topArtists: 20,
    favoriteAlbums: 12,
    favoriteTracks: 10,
    featuredAlbums: 12
  };

  let homeLimits = $state(getSettings().limits);

  // Loading states for progressive render
  let isInitializing = $state(true);
  let isOverlayVisible = $state(true); // Overlay that fades out when ALL sections ready
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

  // Track loading completion for overlay
  let totalVisibleSections = $state(0);
  let sectionsFinished = $state(0);

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
    await Promise.all(albums.map(album => loadAlbumDownloadStatus(album.id)));
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
  );

  // Mark a section as finished loading and check if we can hide overlay
  function markSectionFinished() {
    sectionsFinished++;
    checkAllSectionsReady();
  }

  // Check if all visible sections have finished loading
  function checkAllSectionsReady() {
    if (sectionsFinished >= totalVisibleSections && totalVisibleSections > 0 && isOverlayVisible) {
      // Small delay to ensure DOM has rendered, then fade out
      setTimeout(() => {
        isOverlayVisible = false;
        isInitializing = false;
      }, 150);
    }
  }

  onMount(() => {
    // Subscribe to home settings changes
    const unsubscribe = subscribeHomeSettings(() => {
      homeSettings = getSettings();
      homeLimits = getSettings().limits;
    });

    // Load home data
    loadHome();

    return unsubscribe;
  });

  function handleArtistImageError(artistId: number) {
    failedArtistImages = new Set([...failedArtistImages, artistId]);
  }

  function normalizeAlbumIds(ids: Array<string | undefined | null>): string[] {
    const filtered = ids.filter((id): id is string => !!id && id.trim().length > 0);
    return Array.from(new Set(filtered));
  }

  async function fetchAlbums(ids: string[]): Promise<AlbumCardData[]> {
    if (ids.length === 0) return [];
    const results = await Promise.allSettled(
      ids.map(albumId => invoke<QobuzAlbum>('get_album', { albumId }))
    );

    return results.flatMap(result => {
      if (result.status !== 'fulfilled') return [];
      return [toAlbumCard(result.value)];
    });
  }

  async function fetchTracks(ids: number[]): Promise<DisplayTrack[]> {
    if (ids.length === 0) return [];
    const results = await Promise.allSettled(
      ids.map(trackId => invoke<QobuzTrack>('get_track', { trackId }))
    );

    return results.flatMap(result => {
      if (result.status !== 'fulfilled') return [];
      return [toDisplayTrack(result.value)];
    });
  }

  async function fetchArtists(seeds: TopArtistSeed[]): Promise<ArtistCardData[]> {
    if (seeds.length === 0) return [];
    const results = await Promise.allSettled(
      seeds.map(seed => invoke<QobuzArtist>('get_artist', { artistId: seed.artistId }))
    );

    const artists: ArtistCardData[] = [];
    results.forEach((result, index) => {
      if (result.status !== 'fulfilled') return;
      const seed = seeds[index];
      artists.push(toArtistCard(result.value, seed.playCount));
    });

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
      console.log(`[Home] Context created: Continue Listening, ${trackIds.length} tracks, starting at ${trackIndex}`);
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
    // Reload home page with new genre filter
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

  async function loadHome() {
    isInitializing = true;
    isOverlayVisible = true;
    sectionsFinished = 0;
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

    // Count total visible sections to know when we're done
    totalVisibleSections = 0;
    if (isSectionVisible('newReleases')) totalVisibleSections++;
    if (isSectionVisible('pressAwards')) totalVisibleSections++;
    if (isSectionVisible('mostStreamed')) totalVisibleSections++;
    if (isSectionVisible('qobuzissimes')) totalVisibleSections++;
    if (isSectionVisible('editorPicks')) totalVisibleSections++;
    if (isSectionVisible('recentAlbums')) totalVisibleSections++;
    if (isSectionVisible('continueTracks')) totalVisibleSections++;
    if (isSectionVisible('topArtists')) totalVisibleSections++;
    if (isSectionVisible('favoriteAlbums')) totalVisibleSections++;

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
      fetchFeaturedAlbums('new-releases', homeLimits.featuredAlbums, genreIds).then(async albums => {
        newReleases = albums;
        await loadAllAlbumDownloadStatuses(albums);
        loadingNewReleases = false;
        markSectionFinished();
      });
    } else {
      loadingNewReleases = false;
    }

    if (isSectionVisible('pressAwards')) {
      fetchFeaturedAlbums('press-awards', homeLimits.featuredAlbums, genreIds).then(async albums => {
        pressAwards = albums;
        await loadAllAlbumDownloadStatuses(albums);
        loadingPressAwards = false;
        markSectionFinished();
      });
    } else {
      loadingPressAwards = false;
    }

    if (isSectionVisible('mostStreamed')) {
      fetchFeaturedAlbums('most-streamed', homeLimits.featuredAlbums, genreIds).then(async albums => {
        mostStreamed = albums;
        await loadAllAlbumDownloadStatuses(albums);
        loadingMostStreamed = false;
        markSectionFinished();
      });
    } else {
      loadingMostStreamed = false;
    }

    if (isSectionVisible('qobuzissimes')) {
      fetchFeaturedAlbums('qobuzissimes', homeLimits.featuredAlbums, genreIds).then(async albums => {
        qobuzissimes = albums;
        await loadAllAlbumDownloadStatuses(albums);
        loadingQobuzissimes = false;
        markSectionFinished();
      });
    } else {
      loadingQobuzissimes = false;
    }

    if (isSectionVisible('editorPicks')) {
      fetchFeaturedAlbums('editor-picks', homeLimits.featuredAlbums, genreIds).then(async albums => {
        editorPicks = albums;
        await loadAllAlbumDownloadStatuses(albums);
        loadingEditorPicks = false;
        markSectionFinished();
      });
    } else {
      loadingEditorPicks = false;
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
          markSectionFinished();
        });
      } else {
        loadingContinueTracks = false;
      }

      // Recently Played (albums) - start immediately with seeds
      if (isSectionVisible('recentAlbums')) {
        const recentAlbumIds = normalizeAlbumIds(seeds.recentlyPlayedAlbumIds);
        // Fetch more if filtering, to have enough after filter
        const fetchLimit = hasGenreFilter() ? homeLimits.recentAlbums * 3 : homeLimits.recentAlbums;
        fetchAlbums(recentAlbumIds.slice(0, fetchLimit)).then(async albums => {
          const filtered = filterAlbumsByGenre(albums).slice(0, homeLimits.recentAlbums);
          recentAlbums = filtered;
          await loadAllAlbumDownloadStatuses(filtered);
          loadingRecentAlbums = false;
          markSectionFinished();
        });
      } else {
        loadingRecentAlbums = false;
      }

      // Top Artists
      if (isSectionVisible('topArtists')) {
        fetchArtists(seeds.topArtistIds.slice(0, homeLimits.topArtists)).then(artists => {
          topArtists = artists;
          loadingTopArtists = false;
          markSectionFinished();
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
          await loadAllAlbumDownloadStatuses(filtered);
          loadingFavoriteAlbums = false;
          markSectionFinished();
        });
      } else {
        loadingFavoriteAlbums = false;
      }

    } catch (err) {
      console.error('Failed to load home data:', err);
      error = String(err);
      isInitializing = false;
      isOverlayVisible = false;
      loadingRecentAlbums = false;
      loadingContinueTracks = false;
      loadingTopArtists = false;
      loadingFavoriteAlbums = false;
    }
  }
</script>

<div class="home-view">
  <!-- Loading Overlay - fades out when ALL sections are ready -->
  {#if isOverlayVisible}
    <div class="loading-overlay" class:fade-out={sectionsFinished >= totalVisibleSections && totalVisibleSections > 0} style="left: {sidebarExpanded ? '280px' : '64px'}">
      <div class="loading-content">
        <div class="loading-icon">
          <Loader2 size={36} class="spinner" />
        </div>
        <h2>{$t('home.loading')}</h2>
        <p>{$t('home.loadingDescription')}</p>
      </div>
    </div>
  {/if}

  <!-- Header with greeting, filter and settings -->
  <div class="home-header">
    {#if homeSettings.greeting.enabled}
      <h2 class="greeting">{greetingText}</h2>
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

  {#if isInitializing && !isOverlayVisible}
    <div class="home-state">
      <div class="state-icon loading">
        <Loader2 size={36} class="spinner" />
      </div>
      <h1>{$t('home.loading')}</h1>
      <p>{$t('home.loadingDescription')}</p>
    </div>
  {:else if error}
    <div class="home-state">
      <div class="state-icon">
        <Music size={36} />
      </div>
      <h1>{$t('home.loadError')}</h1>
      <p>{error}</p>
    </div>
  {:else if hasContent}
    <!-- Render sections in user-defined order -->
    {#each visibleSections as sectionId (sectionId)}
      {#if sectionId === 'newReleases' && newReleases.length > 0}
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

      {#if sectionId === 'pressAwards' && pressAwards.length > 0}
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

      {#if sectionId === 'mostStreamed' && mostStreamed.length > 0}
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

      {#if sectionId === 'qobuzissimes' && qobuzissimes.length > 0}
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

      {#if sectionId === 'editorPicks' && editorPicks.length > 0}
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

      {#if sectionId === 'recentAlbums' && recentAlbums.length > 0}
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

      {#if sectionId === 'continueTracks' && continueTracks.length > 0}
        <div class="section">
          <div class="section-header">
            <h2>{$t('home.continueListening')}</h2>
          </div>
          <div class="track-list compact">
            {#each continueTracks as track, index (`${track.id}-${downloadStateVersion}`)}
              {@const isActiveTrack = isPlaybackActive && activeTrackId === track.id}
              {@const cacheStatus = getTrackOfflineCacheStatus?.(track.id) ?? { status: 'none', progress: 0 }}
              {@const isTrackDownloaded = cacheStatus.status === 'ready'}
              <TrackRow
                trackId={track.id}
                number={index + 1}
                title={track.title}
                artist={track.artist}
                album={track.album}
                duration={track.duration}
                quality={getTrackQuality(track)}
                isPlaying={isActiveTrack}
                compact={true}
                downloadStatus={cacheStatus.status}
                downloadProgress={cacheStatus.progress}
                onArtistClick={track.artistId && onArtistClick ? () => onArtistClick(track.artistId!) : undefined}
                onAlbumClick={track.albumId && onAlbumClick ? () => onAlbumClick(track.albumId!) : undefined}
                onPlay={() => handleContinueTrackPlay(track, index)}
                menuActions={{
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

      {#if sectionId === 'topArtists' && topArtists.length > 0}
        <HorizontalScrollRow title={$t('home.yourTopArtists')}>
          {#snippet children()}
            {#each topArtists as artist}
              <button class="artist-card" onclick={() => onArtistClick?.(artist.id)}>
                {#if failedArtistImages.has(artist.id) || !artist.image}
                  <div class="artist-image-placeholder">
                    <User size={32} />
                  </div>
                {:else}
                  <img
                    src={artist.image}
                    alt={artist.name}
                    class="artist-image"
                    onerror={() => handleArtistImageError(artist.id)}
                  />
                {/if}
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

      {#if sectionId === 'favoriteAlbums' && favoriteAlbums.length > 0}
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
    {/each}
  {:else}
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

  /* Loading Overlay */
  .loading-overlay {
    position: fixed;
    top: 0;
    /* left is set dynamically via inline style based on sidebar state */
    right: 0;
    bottom: calc(var(--player-bar-height, 104px));
    z-index: 10;
    background: var(--bg-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: opacity 300ms ease-out, left 200ms ease;
  }

  .loading-overlay.fade-out {
    opacity: 0;
    pointer-events: none;
  }

  .loading-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    text-align: center;
  }

  .loading-icon {
    width: 64px;
    height: 64px;
    border-radius: 16px;
    background: linear-gradient(135deg, var(--accent-primary) 0%, var(--accent-secondary, #6366f1) 100%);
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
  }

  .loading-content h2 {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .loading-content p {
    font-size: 15px;
    color: var(--text-muted);
    margin: 0;
    max-width: 360px;
  }

  .loading-icon :global(.spinner) {
    animation: spin 1s linear infinite;
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
    width: 160px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
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

  .artist-image,
  .artist-image-placeholder {
    width: 96px;
    height: 96px;
    border-radius: 50%;
  }

  .artist-image {
    object-fit: cover;
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

  .state-icon.loading {
    background: linear-gradient(135deg, var(--accent-primary) 0%, var(--accent-secondary, #6366f1) 100%);
    color: white;
  }

  .state-icon :global(.spinner) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
