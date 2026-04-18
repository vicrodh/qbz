<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Music, User, LoaderCircle, ArrowRight, Heart, Play, Share2, UserPlus } from 'lucide-svelte';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { t } from '$lib/i18n';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import { getQobuzImageForSize } from '$lib/adapters/qobuzAdapters';
  import { replacePlaybackQueue } from '$lib/services/queuePlaybackService';
  import { playTrack } from '$lib/services/playbackService';
  import { playQueueIndex } from '$lib/stores/queueStore';
  import { resolveArtistImage } from '$lib/stores/customArtistImageStore';
  import { isBlacklisted as isArtistBlacklisted } from '$lib/stores/artistBlacklistStore';
  import { toggleArtistFavorite } from '$lib/stores/artistFavoritesStore';
  import { setPlaybackContext } from '$lib/stores/playbackContextStore';
  import type { OfflineCacheStatus } from '$lib/stores/offlineCacheState';
  import type { DisplayTrack, QobuzArtist, PageArtistResponse, PageArtistTrack } from '$lib/types';

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

  interface Props {
    // Shared data from HomeView (already loaded)
    recentAlbums: AlbumCardData[];
    continueTracks: DisplayTrack[];
    topArtists: ArtistCardData[];
    favoriteAlbums: AlbumCardData[];
    loadingRecentAlbums: boolean;
    loadingContinueTracks: boolean;
    loadingTopArtists: boolean;
    loadingFavoriteAlbums: boolean;
    // Album callbacks
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    checkAlbumFullyDownloaded?: (albumId: string) => Promise<boolean>;
    downloadStateVersion?: number;
    isAlbumDownloaded: (albumId: string) => boolean;
    loadAlbumDownloadStatus: (albumId: string) => void;
    // Artist callbacks
    onArtistClick?: (artistId: number) => void;
    // Track callbacks
    onTrackPlay?: (track: DisplayTrack) => void;
    onTrackPlayNext?: (track: DisplayTrack) => void;
    onTrackPlayLater?: (track: DisplayTrack) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
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
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
    // Navigation
    onNavigateDailyQ?: () => void;
    onNavigateWeeklyQ?: () => void;
    onNavigateFavQ?: () => void;
    onNavigateTopQ?: () => void;
    onPlaylistClick?: (playlistId: number) => void;
  }

  let {
    recentAlbums,
    continueTracks,
    topArtists,
    favoriteAlbums,
    loadingRecentAlbums,
    loadingContinueTracks,
    loadingTopArtists,
    loadingFavoriteAlbums,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    onAddAlbumToPlaylist,
    checkAlbumFullyDownloaded,
    downloadStateVersion,
    isAlbumDownloaded,
    loadAlbumDownloadStatus,
    onArtistClick,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddToPlaylist,
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
    onNavigateDailyQ,
    onNavigateWeeklyQ,
    onNavigateFavQ,
    onNavigateTopQ,
    onPlaylistClick,
  }: Props = $props();

  interface SimilarArtistsPage {
    items: QobuzArtist[];
    total: number;
    offset: number;
    limit: number;
  }

  interface SuggestedArtist {
    id: number;
    name: string;
    image?: string;
    isFavoriting: boolean;
  }

  interface SpotlightAlbum {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    artistId?: number;
    genre: string;
    quality?: string;
    releaseDate?: string;
  }

  interface SpotlightPlaylist {
    id: number;
    title: string;
    image?: string;
    tracksCount?: number;
  }

  interface SpotlightData {
    artistId: number;
    artistName: string;
    artistImage?: string;
    topTracks: PageArtistTrack[];
    category?: string;
    albums: SpotlightAlbum[];
    playlists: SpotlightPlaylist[];
  }

  // For You-specific state
  let failedArtistImages = $state<Set<number>>(new Set());
  let radioLoading = $state<string | null>(null); // album ID currently creating radio
  let radioCardColors = $state<Record<string, string>>({}); // album ID -> dominant color
  let radioCardTextColors = $state<Record<string, string>>({}); // album ID -> lightest color for RADIO text

  // Phase 2: Artists to Follow
  let suggestedArtists = $state<SuggestedArtist[]>([]);
  let loadingSuggestedArtists = $state(false);

  // Per-section load guards so a section that depends on a prop (e.g.
  // topArtists) does not permanently skip loading when the prop is empty
  // at mount and arrives later.
  let artistsToFollowLoaded = false;
  let spotlightLoaded = false;
  let similarAlbumsLoaded = false;
  let forgottenLoaded = false;
  let essentialsLoaded = false;
  let releaseWatchLoaded = false;

  // Phase 2: Spotlight
  let spotlightData = $state<SpotlightData | null>(null);
  let loadingSpotlight = $state(false);
  let spotlightRadioLoading = $state(false);
  let spotlightTopTracksLoading = $state(false);
  let spotlightRadioColor = $state<string>('');
  let spotlightRadioTextColor = $state<string>('');

  // Phase 3: Similar to [Album]
  let similarAlbums = $state<AlbumCardData[]>([]);
  let similarSeedTitle = $state('');
  let loadingSimilarAlbums = $state(false);

  // Phase 3: Rediscover your Library
  let forgottenAlbums = $state<AlbumCardData[]>([]);
  let loadingForgottenAlbums = $state(false);

  // Phase 3: Essentials [Genre]
  let essentialsGenreName = $state('');
  let essentialsAlbums = $state<AlbumCardData[]>([]);
  let loadingEssentials = $state(false);

  // Release Watch — the Qobuz mobile "Radar de Novedades" feed: new releases
  // from artists, labels, and awards the user follows.
  let releaseWatchAlbums = $state<AlbumCardData[]>([]);
  let loadingReleaseWatch = $state(false);

  // Radio Stations: mix of 3 recent + 3 favorites + 3 top-artist albums, no dupes
  let radioAlbums = $state<AlbumCardData[]>([]);
  let radioBuilt = false;

  function buildRadioStations() {
    if (radioBuilt) return;
    if (recentAlbums.length === 0 && favoriteAlbums.length === 0) return;
    radioBuilt = true;

    const seen = new Set<string>();
    const result: AlbumCardData[] = [];

    function addShuffled(source: AlbumCardData[], count: number) {
      const candidates = source.filter(a => !seen.has(a.id));
      const shuffled = [...candidates];
      for (let i = shuffled.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
      }
      for (const album of shuffled.slice(0, count)) {
        seen.add(album.id);
        result.push(album);
      }
    }

    addShuffled(recentAlbums, 3);
    addShuffled(favoriteAlbums, 3);

    const topArtistIds = new Set(topArtists.map(a => a.id));
    const topArtistAlbums = [...recentAlbums, ...favoriteAlbums].filter(
      a => a.artistId && topArtistIds.has(a.artistId) && !seen.has(a.id)
    );
    addShuffled(topArtistAlbums, 3);

    if (result.length < 9) {
      const remaining = [...recentAlbums, ...favoriteAlbums].filter(a => !seen.has(a.id));
      addShuffled(remaining, 9 - result.length);
    }

    radioAlbums = result;
  }

  // Build radio stations once data is available, extract colors
  $effect(() => {
    if (!radioBuilt && (recentAlbums.length > 0 || favoriteAlbums.length > 0)) {
      buildRadioStations();
    }
    for (const album of radioAlbums) {
      if (!radioCardColors[album.id] && album.artwork) {
        extractRadioCardColor(album.id, album.artwork);
      }
    }
  });

  // Sections that do not depend on props fire once at mount.
  onMount(() => {
    if (!forgottenLoaded) { forgottenLoaded = true; loadForgottenFavorites(); }
    if (!essentialsLoaded) { essentialsLoaded = true; loadEssentials(); }
    if (!releaseWatchLoaded) { releaseWatchLoaded = true; loadReleaseWatch(); }
  });

  // Sections that consume HomeView-provided props fire as soon as the
  // relevant prop becomes non-empty. Without this, switching to For You
  // before Home finishes loading leaves these rails permanently blank.
  $effect(() => {
    if (!artistsToFollowLoaded && topArtists.length > 0) {
      artistsToFollowLoaded = true;
      loadArtistsToFollow();
    }
  });
  $effect(() => {
    if (!spotlightLoaded && topArtists.length > 0) {
      spotlightLoaded = true;
      loadSpotlight();
    }
  });
  $effect(() => {
    if (!similarAlbumsLoaded && recentAlbums.length > 0) {
      similarAlbumsLoaded = true;
      loadSimilarAlbums();
    }
  });

  async function loadReleaseWatch() {
    loadingReleaseWatch = true;
    try {
      const result = await invoke<{ items: AlbumSuggestResult[]; total: number }>(
        'v2_get_release_watch',
        { limit: 20, offset: 0 }
      );
      releaseWatchAlbums = (result.items || []).map(album => ({
        id: album.id,
        artwork: album.image?.large || album.image?.small || '',
        title: album.title,
        artist: album.artist?.name || '',
        artistId: album.artist?.id,
        genre: album.genre?.name || '',
        quality: formatQuality(
          album.hires,
          album.maximum_bit_depth,
          album.maximum_sampling_rate
        ),
        releaseDate: album.release_date_original
      }));
    } catch (err) {
      console.error('Failed to load release watch:', err);
    } finally {
      loadingReleaseWatch = false;
    }
  }

  async function loadArtistsToFollow() {
    if (topArtists.length === 0) return;
    loadingSuggestedArtists = true;

    try {
      // Get favorite artist IDs to filter out already-followed
      const favoriteIds = new Set(
        await invoke<number[]>('v2_get_cached_favorite_artists')
      );

      // Get similar artists for top 3 artists
      const seedArtists = topArtists.slice(0, 3);
      const allSimilar: QobuzArtist[] = [];

      const results = await Promise.allSettled(
        seedArtists.map(seed =>
          invoke<SimilarArtistsPage>('v2_get_similar_artists', {
            artistId: seed.id,
            limit: 6,
            offset: 0
          })
        )
      );

      for (const result of results) {
        if (result.status === 'fulfilled') {
          allSimilar.push(...result.value.items);
        }
      }

      // Deduplicate, filter out favorites and seed artists
      const seedIds = new Set(topArtists.map(a => a.id));
      const seen = new Set<number>();
      const filtered: SuggestedArtist[] = [];

      for (const artist of allSimilar) {
        if (seen.has(artist.id) || favoriteIds.has(artist.id) || seedIds.has(artist.id)) continue;
        seen.add(artist.id);
        filtered.push({
          id: artist.id,
          name: artist.name,
          image: resolveArtistImage(artist.name, getQobuzImageForSize(artist.image, 'small')),
          isFavoriting: false
        });
        if (filtered.length >= 10) break;
      }

      suggestedArtists = filtered;
    } catch (err) {
      console.error('Failed to load suggested artists:', err);
    } finally {
      loadingSuggestedArtists = false;
    }
  }

  interface AlbumSuggestResult {
    id: string;
    title: string;
    artist: { id: number; name: string };
    image: { small: string; large: string; thumbnail: string };
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    genre?: { name: string };
    release_date_original?: string;
  }

  async function loadSimilarAlbums() {
    if (recentAlbums.length === 0) return;
    loadingSimilarAlbums = true;

    try {
      // Pick a random recent album as seed
      const seedIdx = Math.floor(Math.random() * Math.min(recentAlbums.length, 5));
      const seed = recentAlbums[seedIdx];
      similarSeedTitle = seed.title;

      const albums = await invoke<AlbumSuggestResult[]>('v2_get_album_suggestions', {
        albumId: seed.id,
        limit: 10
      });

      similarAlbums = albums.map(album => ({
        id: album.id,
        artwork: album.image?.large || album.image?.small || '',
        title: album.title,
        artist: album.artist?.name || '',
        artistId: album.artist?.id,
        genre: album.genre?.name || '',
        quality: formatQuality(
          album.hires,
          album.maximum_bit_depth,
          album.maximum_sampling_rate
        ),
        releaseDate: album.release_date_original
      }));
    } catch (err) {
      console.error('Failed to load similar albums:', err);
    } finally {
      loadingSimilarAlbums = false;
    }
  }

  interface ForgottenAlbum {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    artistId?: number;
    genre: string;
    quality: string;
    releaseDate?: string;
  }

  async function loadForgottenFavorites() {
    loadingForgottenAlbums = true;

    try {
      const albums = await invoke<ForgottenAlbum[]>('v2_reco_get_forgotten_favorites', {
        limit: 12,
        recencyDays: 30
      });

      forgottenAlbums = albums.map(album => ({
        id: album.id,
        artwork: album.artwork,
        title: album.title,
        artist: album.artist,
        artistId: album.artistId,
        genre: album.genre,
        quality: album.quality,
        releaseDate: album.releaseDate
      }));
    } catch (err) {
      console.error('Failed to load forgotten favorites:', err);
    } finally {
      loadingForgottenAlbums = false;
    }
  }

  interface TopGenre {
    id: number;
    name: string;
  }

  async function loadEssentials() {
    loadingEssentials = true;

    try {
      // Get user's top genres
      const genres = await invoke<TopGenre[]>('v2_reco_get_top_genres', { limit: 3 });
      if (genres.length === 0) return;

      // Use the top genre for essentials
      const topGenre = genres[0];
      essentialsGenreName = topGenre.name;

      // Fetch essential/ideal discography albums for that genre
      const result = await invoke<{ items: AlbumSuggestResult[]; total: number }>('v2_get_featured_albums', {
        featuredType: 'ideal-discography',
        limit: 12,
        offset: 0,
        genreId: topGenre.id
      });

      essentialsAlbums = (result.items || []).map(album => ({
        id: album.id,
        artwork: album.image?.large || album.image?.small || '',
        title: album.title,
        artist: album.artist?.name || '',
        artistId: album.artist?.id,
        genre: album.genre?.name || '',
        quality: formatQuality(
          album.hires,
          album.maximum_bit_depth,
          album.maximum_sampling_rate
        ),
        releaseDate: album.release_date_original
      }));
    } catch (err) {
      console.error('Failed to load essentials:', err);
    } finally {
      loadingEssentials = false;
    }
  }

  async function handleFollowArtist(artistId: number) {
    const idx = suggestedArtists.findIndex(a => a.id === artistId);
    if (idx === -1) return;
    suggestedArtists[idx].isFavoriting = true;

    try {
      await toggleArtistFavorite(artistId);
      // Remove from suggestions after favoriting
      suggestedArtists = suggestedArtists.filter(a => a.id !== artistId);
    } catch (err) {
      console.error('Failed to follow artist:', err);
      suggestedArtists[idx].isFavoriting = false;
    }
  }

  async function loadSpotlight() {
    if (topArtists.length === 0) return;
    loadingSpotlight = true;

    try {
      // Pick a random artist from top artists
      const randomIdx = Math.floor(Math.random() * Math.min(topArtists.length, 5));
      const seed = topArtists[randomIdx];

      const response = await invoke<PageArtistResponse>('v2_get_artist_page', {
        artistId: seed.id
      });

      let artistImage: string | undefined;
      if (response.images?.portrait) {
        const { hash, format } = response.images.portrait;
        artistImage = `https://static.qobuz.com/images/artists/covers/medium/${hash}.${format}`;
      }
      artistImage = resolveArtistImage(response.name.display, artistImage || '');

      // Extract up to 6 albums: prefer "album" type, then fill with "live", "ep-single"
      const allAlbums: SpotlightAlbum[] = [];
      const seenAlbumIds = new Set<string>();
      const releaseTypes = ['album', 'live', 'ep-single', 'compilation'];
      for (const releaseType of releaseTypes) {
        const group = (response.releases || []).find(rg => rg.type === releaseType);
        if (group) {
          for (const rel of group.items) {
            if (seenAlbumIds.has(rel.id)) continue;
            seenAlbumIds.add(rel.id);
            allAlbums.push({
              id: rel.id,
              artwork: rel.image?.large || rel.image?.small || '',
              title: rel.title,
              artist: rel.artist?.name?.display || response.name.display,
              artistId: rel.artist?.id || response.id,
              genre: rel.genre?.name || '',
              quality: formatQuality(
                (rel.audio_info?.maximum_bit_depth ?? 16) > 16,
                rel.audio_info?.maximum_bit_depth,
                rel.audio_info?.maximum_sampling_rate
              ),
              releaseDate: rel.dates?.original
            });
            if (allAlbums.length >= 6) break;
          }
        }
        if (allAlbums.length >= 6) break;
      }

      // Extract playlists
      const playlists: SpotlightPlaylist[] = (response.playlists?.items || []).map(pl => ({
        id: pl.id,
        title: pl.title || '',
        image: pl.images?.rectangle?.[0],
        tracksCount: pl.tracks_count
      }));

      spotlightData = {
        artistId: response.id,
        artistName: response.name.display,
        artistImage,
        topTracks: response.top_tracks || [],
        category: response.artist_category,
        albums: allAlbums,
        playlists
      };

      // Extract color for spotlight radio card from artist image
      if (artistImage) {
        extractSpotlightRadioColor(artistImage);
      }
    } catch (err) {
      console.error('Failed to load spotlight:', err);
    } finally {
      loadingSpotlight = false;
    }
  }

  function extractSpotlightRadioColor(artworkUrl: string) {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      try {
        const canvas = document.createElement('canvas');
        canvas.width = 8;
        canvas.height = 8;
        const ctx = canvas.getContext('2d');
        if (!ctx) return;
        ctx.drawImage(img, 0, 0, 8, 8);
        const data = ctx.getImageData(0, 0, 8, 8).data;
        let rSum = 0, gSum = 0, bSum = 0, count = 0;
        let lightR = 0, lightG = 0, lightB = 0, maxBrightness = 0;
        for (let i = 0; i < data.length; i += 4) {
          const pr = data[i], pg = data[i + 1], pb = data[i + 2];
          rSum += pr; gSum += pg; bSum += pb; count++;
          const brightness = pr * 0.299 + pg * 0.587 + pb * 0.114;
          if (brightness > maxBrightness) {
            maxBrightness = brightness;
            lightR = pr; lightG = pg; lightB = pb;
          }
        }
        const dr = Math.round((rSum / count) * 0.7);
        const dg = Math.round((gSum / count) * 0.7);
        const db = Math.round((bSum / count) * 0.7);
        spotlightRadioColor = `rgb(${dr}, ${dg}, ${db})`;
        if (maxBrightness < 60) {
          spotlightRadioTextColor = 'rgb(255, 255, 255)';
        } else {
          const boost = Math.min(1.3, 255 / Math.max(lightR, lightG, lightB, 1));
          spotlightRadioTextColor = `rgb(${Math.min(255, Math.round(lightR * boost))}, ${Math.min(255, Math.round(lightG * boost))}, ${Math.min(255, Math.round(lightB * boost))})`;
        }
      } catch {
        // Canvas tainted
      }
    };
    img.src = artworkUrl;
  }

  async function handleSpotlightRadio() {
    if (!spotlightData || spotlightRadioLoading) return;
    spotlightRadioLoading = true;
    try {
      await invoke('v2_create_qobuz_artist_radio', {
        artistId: spotlightData.artistId,
        artistName: spotlightData.artistName
      });
      await startRadioPlayback();
    } catch (err) {
      console.error('Failed to create spotlight radio:', err);
    } finally {
      spotlightRadioLoading = false;
    }
  }

  async function handleSpotlightTopTracks() {
    if (!spotlightData || spotlightTopTracksLoading || spotlightData.topTracks.length === 0) return;
    spotlightTopTracksLoading = true;
    try {
      const queueTracks = spotlightData.topTracks.map(track => ({
        id: track.id,
        title: track.title,
        artist: track.artist?.name?.display || spotlightData!.artistName,
        album: track.album?.title || '',
        duration_secs: track.duration ?? 0,
        artwork_url: track.album?.image?.small || '',
        hires: (track.audio_info?.maximum_bit_depth ?? 16) > 16,
        bit_depth: track.audio_info?.maximum_bit_depth ?? null,
        sample_rate: track.audio_info?.maximum_sampling_rate ?? null,
        is_local: false,
        album_id: track.album?.id || null,
        artist_id: track.artist?.id || spotlightData!.artistId,
      }));
      await replacePlaybackQueue(queueTracks, 0, {
        debugLabel: 'for-you:spotlight-top-tracks'
      });
      await startRadioPlayback();
    } catch (err) {
      console.error('Failed to play spotlight top tracks:', err);
    } finally {
      spotlightTopTracksLoading = false;
    }
  }

  async function startRadioPlayback() {
    const firstTrack = await playQueueIndex(0);
    if (firstTrack) {
      const quality = firstTrack.bit_depth && firstTrack.sample_rate
        ? `${firstTrack.bit_depth}bit/${firstTrack.sample_rate}kHz`
        : firstTrack.hires ? 'Hi-Res' : '-';
      await playTrack({
        id: firstTrack.id,
        title: firstTrack.title,
        artist: firstTrack.artist,
        album: firstTrack.album,
        artwork: firstTrack.artwork_url || '',
        duration: firstTrack.duration_secs,
        quality,
        bitDepth: firstTrack.bit_depth ?? undefined,
        samplingRate: firstTrack.sample_rate ?? undefined,
        albumId: firstTrack.album_id ?? undefined,
        artistId: firstTrack.artist_id ?? undefined,
      });
    }
  }

  async function handleRadioPlay(albumId: string, albumTitle: string) {
    if (radioLoading) return;
    radioLoading = albumId;
    try {
      await invoke('v2_create_album_radio', { albumId, albumName: albumTitle });
      await startRadioPlayback();
    } catch (err) {
      console.error('Failed to create radio:', err);
    } finally {
      radioLoading = null;
    }
  }

  function extractRadioCardColor(albumId: string, artworkUrl: string) {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      try {
        const canvas = document.createElement('canvas');
        canvas.width = 8;
        canvas.height = 8;
        const ctx = canvas.getContext('2d');
        if (!ctx) return;
        ctx.drawImage(img, 0, 0, 8, 8);
        const data = ctx.getImageData(0, 0, 8, 8).data;
        // Average all pixels for background color
        let rSum = 0, gSum = 0, bSum = 0, count = 0;
        // Track lightest pixel for RADIO text color
        let lightR = 0, lightG = 0, lightB = 0, maxBrightness = 0;
        for (let i = 0; i < data.length; i += 4) {
          const pr = data[i], pg = data[i + 1], pb = data[i + 2];
          rSum += pr;
          gSum += pg;
          bSum += pb;
          count++;
          // Perceived brightness (luminance-weighted)
          const brightness = pr * 0.299 + pg * 0.587 + pb * 0.114;
          if (brightness > maxBrightness) {
            maxBrightness = brightness;
            lightR = pr;
            lightG = pg;
            lightB = pb;
          }
        }
        const r = Math.round(rSum / count);
        const g = Math.round(gSum / count);
        const b = Math.round(bSum / count);
        // Darken background for contrast
        const dr = Math.round(r * 0.7);
        const dg = Math.round(g * 0.7);
        const db = Math.round(b * 0.7);
        radioCardColors = { ...radioCardColors, [albumId]: `rgb(${dr}, ${dg}, ${db})` };
        // For RADIO text: use lightest tone, fallback to bright white for very dark covers
        if (maxBrightness < 60) {
          radioCardTextColors = { ...radioCardTextColors, [albumId]: 'rgb(255, 255, 255)' };
        } else {
          // Boost the lightest color slightly for better visibility
          const boost = Math.min(1.3, 255 / Math.max(lightR, lightG, lightB, 1));
          const tr = Math.min(255, Math.round(lightR * boost));
          const tg = Math.min(255, Math.round(lightG * boost));
          const tb = Math.min(255, Math.round(lightB * boost));
          radioCardTextColors = { ...radioCardTextColors, [albumId]: `rgb(${tr}, ${tg}, ${tb})` };
        }
      } catch {
        // Canvas tainted - ignore
      }
    };
    img.src = artworkUrl;
  }

  function handleArtistImageError(artistId: number) {
    failedArtistImages = new Set([...failedArtistImages, artistId]);
  }

  function formatQuality(hires?: boolean, maximum_bit_depth?: number, maximum_sampling_rate?: number): string {
    if (!hires) return $t('quality.cdQuality');
    const depth = maximum_bit_depth ?? 16;
    const rate = maximum_sampling_rate ?? 44.1;
    return `${depth}/${rate}kHz`;
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
    if (continueTracks.length > 0) {
      const trackIds = continueTracks.map(trk => trk.id);
      await setPlaybackContext(
        'home_list',
        'forYou:continue_listening',
        'Continue Listening',
        'qobuz',
        trackIds,
        trackIndex
      );

      try {
        const queueTracks = buildContinueQueueTracks(continueTracks);
        const localTrackIds = continueTracks
          .filter((trk) => trk.isLocal)
          .map((trk) => trk.id);
        await replacePlaybackQueue(queueTracks, trackIndex, {
          localTrackIds,
          debugLabel: 'for-you:continue-listening'
        });
      } catch (err) {
        console.error('Failed to set queue:', err);
      }
    }

    if (onTrackPlay) {
      onTrackPlay(track);
    }
  }

  const hasAnyContent = $derived(
    recentAlbums.length > 0 ||
    continueTracks.length > 0 ||
    topArtists.length > 0 ||
    favoriteAlbums.length > 0 ||
    similarAlbums.length > 0 ||
    forgottenAlbums.length > 0 ||
    essentialsAlbums.length > 0
  );

  const anyLoading = $derived(
    loadingRecentAlbums || loadingContinueTracks || loadingTopArtists || loadingFavoriteAlbums
  );
</script>

<!-- Your Mixes -->
<div class="your-mixes-section">
  <h2 class="section-title">{$t('home.qobuzMixes')}</h2>
  <div class="mix-cards-row">
    <button class="mix-card" onclick={() => onNavigateDailyQ?.()}>
      <div class="mix-card-artwork mix-gradient-daily">
        <span class="mix-card-badge">qobuz</span>
        <span class="mix-card-name">DailyQ</span>
      </div>
      <p class="mix-card-desc">{$t('qobuzMixes.cardDesc')}</p>
    </button>
    <button class="mix-card" onclick={() => onNavigateWeeklyQ?.()}>
      <div class="mix-card-artwork mix-gradient-weekly">
        <span class="mix-card-badge">qobuz</span>
        <span class="mix-card-name">WeeklyQ</span>
      </div>
      <p class="mix-card-desc">{@html $t('weeklyMixes.cardDesc')}</p>
    </button>
    <button class="mix-card" onclick={() => onNavigateFavQ?.()}>
      <div class="mix-card-artwork mix-gradient-favq">
        <span class="mix-card-badge">qbz</span>
        <span class="mix-card-name">FavQ</span>
      </div>
      <p class="mix-card-desc">{$t('favMixes.cardDesc')}</p>
    </button>
    <button class="mix-card" onclick={() => onNavigateTopQ?.()}>
      <div class="mix-card-artwork mix-gradient-topq">
        <span class="mix-card-badge">qbz</span>
        <span class="mix-card-name">TopQ</span>
      </div>
      <p class="mix-card-desc">{@html $t('topMixes.cardDesc')}</p>
    </button>
  </div>
</div>

<!-- Release Watch — mobile "Radar de Novedades": new releases from
     followed artists/labels/awards. Placed right after Your Mixes so
     the personal-feed sections sit together at the top of For You. -->
{#if loadingReleaseWatch}
  <div class="skeleton-section">
    <div class="skeleton-title"></div>
    <div class="skeleton-row">
      {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
    </div>
  </div>
{:else if releaseWatchAlbums.length > 0}
  <HorizontalScrollRow>
    {#snippet header()}
      <div class="section-header-col">
        <h2 class="section-title">{$t('home.releaseWatch')}</h2>
        <p class="section-subtitle">{$t('discover.releaseWatch.subtitle')}</p>
      </div>
    {/snippet}
    {#snippet children()}
      {#each releaseWatchAlbums as album}
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

<!-- Radio Stations -->
{#if loadingRecentAlbums || loadingFavoriteAlbums}
  <div class="skeleton-section">
    <div class="skeleton-title"></div>
    <div class="skeleton-row">
      {#each { length: 4 } as _}<div class="skeleton-card"></div>{/each}
    </div>
  </div>
{:else if radioAlbums.length > 0}
  <HorizontalScrollRow>
    {#snippet header()}
      <div class="section-header-col">
        <h2 class="section-title">{$t('home.radioStations')}</h2>
        <p class="section-subtitle">{$t('home.radioStationsDesc')}</p>
      </div>
    {/snippet}
    {#snippet children()}
      {#each radioAlbums as album (album.id)}
        {@const isThisLoading = radioLoading === album.id}
        <button
          class="radio-card"
          class:loading={isThisLoading}
          onclick={() => handleRadioPlay(album.id, album.title)}
          disabled={radioLoading !== null}
        >
          <div
            class="radio-card-visual"
            style:background-color={radioCardColors[album.id] || 'var(--bg-tertiary)'}
          >
            <img
              use:cachedSrc={album.artwork}
              alt={album.title}
              class="radio-card-art"
              loading="lazy"
              decoding="async"
            />
            <img
              src="/image_radio_shadows.png"
              alt=""
              class="radio-card-shadow"
            />
            <span
              class="radio-card-label"
              style:color={radioCardTextColors[album.id] || 'rgba(255, 255, 255, 0.85)'}
            >{$t('home.radioLabel')}</span>
            <div class="radio-card-hover-overlay" class:visible={isThisLoading}>
              {#if isThisLoading}
                <div class="radio-play-spinner">
                  <svg viewBox="0 0 50 50" class="radio-spinner-svg">
                    <circle cx="25" cy="25" r="20" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
                  </svg>
                </div>
              {:else}
                <div class="radio-overlay-play-btn" role="button" tabindex="0">
                  <Play size={18} fill="white" color="white" />
                </div>
              {/if}
            </div>
          </div>
          <div class="radio-card-meta-title" title={album.title}>{album.title}</div>
          <div class="radio-card-artist">{album.artist}</div>
        </button>
      {/each}
      <div class="spacer"></div>
    {/snippet}
  </HorizontalScrollRow>
{/if}

<!-- Continue Listening -->
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
        {@const isThisActiveTrack = activeTrackId === track.id}
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
          isPlaying={isPlaybackActive && isThisActiveTrack}
          isActiveTrack={isThisActiveTrack}
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

<!-- Recently Played -->
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

<!-- Your Top Artists -->
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
                use:cachedSrc={artist.image}
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

<!-- Favorite Albums -->
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

<!-- Similar to [Album] -->
{#if loadingSimilarAlbums}
  <div class="skeleton-section">
    <div class="skeleton-title"></div>
    <div class="skeleton-row">
      {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
    </div>
  </div>
{:else if similarAlbums.length > 0}
  <HorizontalScrollRow title={$t('home.similarTo') + ' ' + similarSeedTitle}>
    {#snippet children()}
      {#each similarAlbums as album}
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

<!-- Rediscover your Library -->
{#if loadingForgottenAlbums}
  <div class="skeleton-section">
    <div class="skeleton-title"></div>
    <div class="skeleton-row">
      {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
    </div>
  </div>
{:else if forgottenAlbums.length > 0}
  <HorizontalScrollRow>
    {#snippet header()}
      <div class="section-header-col">
        <h2 class="section-title">{$t('home.rediscoverLibrary')}</h2>
        <p class="section-subtitle">{$t('home.rediscoverLibraryDesc')}</p>
      </div>
    {/snippet}
    {#snippet children()}
      {#each forgottenAlbums as album}
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

<!-- Essentials [Genre] -->
{#if loadingEssentials}
  <div class="skeleton-section">
    <div class="skeleton-title"></div>
    <div class="skeleton-row">
      {#each { length: 6 } as _}<div class="skeleton-card"></div>{/each}
    </div>
  </div>
{:else if essentialsAlbums.length > 0}
  <HorizontalScrollRow title={$t('home.essentials', { values: { genre: essentialsGenreName } })}>
    {#snippet children()}
      {#each essentialsAlbums as album}
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

<!-- Artists to Follow -->
{#if loadingSuggestedArtists}
  <div class="skeleton-section">
    <div class="skeleton-title"></div>
    <div class="skeleton-row">
      {#each { length: 5 } as _}<div class="skeleton-artist"></div>{/each}
    </div>
  </div>
{:else if suggestedArtists.length > 0}
  <HorizontalScrollRow>
    {#snippet header()}
      <div class="section-header-col">
        <h2 class="section-title">{$t('home.artistsToFollow')}</h2>
        <p class="section-subtitle">{$t('home.artistsToFollowDesc')}</p>
      </div>
    {/snippet}
    {#snippet children()}
      {#each suggestedArtists as artist (artist.id)}
        <div class="follow-artist-card">
          <button class="follow-artist-image-btn" onclick={() => onArtistClick?.(artist.id)}>
            <div class="artist-image-wrapper">
              <div class="artist-image-placeholder">
                <User size={48} />
              </div>
              {#if !failedArtistImages.has(artist.id) && artist.image}
                <img
                  use:cachedSrc={artist.image}
                  alt={artist.name}
                  class="artist-image"
                  loading="lazy"
                  decoding="async"
                  onerror={() => handleArtistImageError(artist.id)}
                />
              {/if}
            </div>
          </button>
          <button class="follow-artist-name-btn" onclick={() => onArtistClick?.(artist.id)}>
            <span class="follow-artist-name">{artist.name}</span>
            <span class="follow-artist-label">{$t('home.spotlightArtist')}</span>
          </button>
          <button
            class="follow-btn"
            class:loading={artist.isFavoriting}
            onclick={() => handleFollowArtist(artist.id)}
            disabled={artist.isFavoriting}
          >
            {#if artist.isFavoriting}
              <LoaderCircle size={14} class="spinner" />
            {:else}
              <UserPlus size={14} />
            {/if}
            <span>{$t('home.followArtist')}</span>
          </button>
        </div>
      {/each}
      <div class="spacer"></div>
    {/snippet}
  </HorizontalScrollRow>
{/if}

<!-- Spotlight -->
{#if loadingSpotlight}
  <div class="skeleton-section">
    <div class="skeleton-title"></div>
    <div class="skeleton-spotlight">
      <div class="skeleton-spotlight-hero"></div>
      <div class="skeleton-tracks">
        {#each { length: 3 } as _}<div class="skeleton-track"></div>{/each}
      </div>
    </div>
  </div>
{:else if spotlightData}
  <div class="section spotlight-section">
    <div class="section-header">
      <h2 class="section-title">{$t('home.spotlight')}</h2>
      <p class="section-subtitle">{$t('home.spotlightDesc')}</p>
    </div>

    <!-- Spotlight Hero -->
    <div class="spotlight-hero">
      <button class="spotlight-hero-image-btn" onclick={() => onArtistClick?.(spotlightData!.artistId)}>
        <div class="spotlight-image">
          {#if spotlightData.artistImage}
            <img
              use:cachedSrc={spotlightData.artistImage}
              alt={spotlightData.artistName}
              class="spotlight-img"
              loading="lazy"
              decoding="async"
            />
          {:else}
            <div class="spotlight-placeholder">
              <User size={64} />
            </div>
          {/if}
        </div>
      </button>
      <div class="spotlight-info">
        {#if spotlightData.category}
          <span class="spotlight-category">{$t('home.spotlightArtist')}</span>
        {/if}
        <h3 class="spotlight-name">{spotlightData.artistName}</h3>
        <div class="spotlight-actions">
          <button class="action-btn-circle primary" onclick={() => handleSpotlightTopTracks()} disabled={spotlightTopTracksLoading}>
            {#if spotlightTopTracksLoading}
              <LoaderCircle size={20} class="spinner" />
            {:else}
              <Play size={20} fill="currentColor" color="currentColor" />
            {/if}
          </button>
          <button class="action-btn-circle" onclick={() => onArtistClick?.(spotlightData!.artistId)}>
            <User size={18} />
          </button>
        </div>
      </div>
    </div>

    <!-- Spotlight Content Row: Top Tracks card, Radio card, Playlists, Albums -->
    <HorizontalScrollRow>
      {#snippet header()}<span></span>{/snippet}
      {#snippet children()}
        <!-- TOP TRACKS card -->
        {#if spotlightData!.topTracks.length > 0}
          {@const isTopTracksLoading = spotlightTopTracksLoading}
          <button
            class="radio-card"
            class:loading={isTopTracksLoading}
            onclick={() => handleSpotlightTopTracks()}
            disabled={spotlightTopTracksLoading}
          >
            <div class="radio-card-visual spotlight-top-tracks-visual">
              {#if spotlightData!.artistImage}
                <img
                  use:cachedSrc={spotlightData!.artistImage}
                  alt={spotlightData!.artistName}
                  class="spotlight-top-tracks-art"
                  loading="lazy"
                  decoding="async"
                />
              {/if}
              <img
                src="/image_radio_shadows.png"
                alt=""
                class="radio-card-shadow"
              />
              <span class="spotlight-top-tracks-label">{$t('home.topTracksLabel')}</span>
              <div class="radio-card-hover-overlay" class:visible={isTopTracksLoading}>
                {#if isTopTracksLoading}
                  <div class="radio-play-spinner">
                    <svg viewBox="0 0 50 50" class="radio-spinner-svg">
                      <circle cx="25" cy="25" r="20" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
                    </svg>
                  </div>
                {:else}
                  <div class="radio-overlay-play-btn" role="button" tabindex="0">
                    <Play size={18} fill="white" color="white" />
                  </div>
                {/if}
              </div>
            </div>
            <div class="radio-card-meta-title">{$t('home.topTracks')}</div>
            <div class="radio-card-artist">{$t('home.topTracksBy', { values: { artist: spotlightData!.artistName } })}</div>
          </button>
        {/if}

        <!-- RADIO card (artist radio, homologated style) -->
        <button
          class="radio-card"
          class:loading={spotlightRadioLoading}
          onclick={() => handleSpotlightRadio()}
          disabled={spotlightRadioLoading}
        >
          <div
            class="radio-card-visual"
            style:background-color={spotlightRadioColor || 'var(--bg-tertiary)'}
          >
            {#if spotlightData!.artistImage}
              <img
                use:cachedSrc={spotlightData!.artistImage}
                alt={spotlightData!.artistName}
                class="radio-card-art"
                loading="lazy"
                decoding="async"
              />
            {/if}
            <img
              src="/image_radio_shadows.png"
              alt=""
              class="radio-card-shadow"
            />
            <span
              class="radio-card-label"
              style:color={spotlightRadioTextColor || 'rgba(255, 255, 255, 0.85)'}
            >{$t('home.radioLabel')}</span>
            <div class="radio-card-hover-overlay" class:visible={spotlightRadioLoading}>
              {#if spotlightRadioLoading}
                <div class="radio-play-spinner">
                  <svg viewBox="0 0 50 50" class="radio-spinner-svg">
                    <circle cx="25" cy="25" r="20" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
                  </svg>
                </div>
              {:else}
                <div class="radio-overlay-play-btn" role="button" tabindex="0">
                  <Play size={18} fill="white" color="white" />
                </div>
              {/if}
            </div>
          </div>
          <div class="radio-card-meta-title">{spotlightData!.artistName}</div>
          <div class="radio-card-artist">{$t('home.qobuzRadioStation')}</div>
        </button>

        <!-- Playlist cards (if artist has Qobuz playlists) -->
        {#each spotlightData!.playlists as pl (pl.id)}
          <button
            class="radio-card"
            onclick={() => onPlaylistClick?.(pl.id)}
          >
            <div class="radio-card-visual spotlight-playlist-visual">
              {#if pl.image}
                <img
                  use:cachedSrc={pl.image}
                  alt={pl.title}
                  class="spotlight-playlist-img"
                  loading="lazy"
                  decoding="async"
                />
              {/if}
              <div class="radio-card-hover-overlay">
                <div class="radio-overlay-play-btn">
                  <Play size={18} fill="white" color="white" />
                </div>
              </div>
            </div>
            <div class="radio-card-meta-title" title={pl.title}>{pl.title}</div>
            <div class="radio-card-artist spotlight-playlist-label">{$t('home.playlistLabel')}</div>
          </button>
        {/each}

        <!-- Artist albums -->
        {#each spotlightData!.albums as album (album.id)}
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
  </div>
{/if}

<!-- Empty state -->
{#if !anyLoading && !hasAnyContent}
  <div class="home-state">
    <div class="state-icon">
      <Music size={48} />
    </div>
    <h1>{$t('home.startListening')}</h1>
    <p>{$t('home.startListeningDescription')}</p>
  </div>
{/if}

<style>
  /* ---- Section layout ---- */
  .section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .section-header {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .section-header h2 {
    font-size: 20px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
  }

  .section-subtitle {
    font-size: 13px;
    color: var(--text-muted);
    margin: 0;
  }

  .section-title {
    font-size: 20px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
  }

  .section-header-col {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* ---- Radio Stations ---- */

  .radio-card {
    flex-shrink: 0;
    width: 210px;
    cursor: pointer;
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    color: inherit;
  }

  .radio-card:disabled {
    cursor: wait;
  }

  .radio-card-visual {
    position: relative;
    width: 210px;
    height: 210px;
    border-radius: 8px;
    overflow: hidden;
    margin-bottom: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background-color 400ms ease;
  }

  .radio-card-visual::after {
    content: '';
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    height: 50%;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.65), transparent);
    pointer-events: none;
    z-index: 2;
    border-radius: 0 0 8px 8px;
  }

  .radio-card-art {
    position: relative;
    z-index: 1;
    width: 130px;
    height: 130px;
    object-fit: cover;
    border-radius: 4px;
  }

  .radio-card-shadow {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    pointer-events: none;
    mix-blend-mode: multiply;
    opacity: 0.7;
    z-index: 2;
  }

  .radio-card-label {
    position: absolute;
    bottom: 10px;
    left: 0;
    right: 0;
    text-align: center;
    font-size: 20px;
    font-weight: 300;
    letter-spacing: 0.35em;
    padding-left: 0.35em;
    /* color set inline from extracted lightest album tone */
    text-shadow: 0 2px 8px rgba(0, 0, 0, 0.6);
    pointer-events: none;
    z-index: 3;
  }

  .radio-card-hover-overlay {
    position: absolute;
    inset: 0;
    z-index: 4;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 150ms ease;
    background: rgba(10, 10, 10, 0.75);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    border-radius: inherit;
    pointer-events: none;
  }

  .radio-card:hover .radio-card-hover-overlay,
  .radio-card-hover-overlay.visible {
    opacity: 1;
    pointer-events: auto;
  }

  .radio-overlay-play-btn {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.85), 0 0 1px rgba(0, 0, 0, 0.3);
    transition: transform 150ms ease, background-color 150ms ease, box-shadow 150ms ease;
  }

  .radio-overlay-play-btn:hover {
    background-color: rgba(0, 0, 0, 0.3);
    box-shadow: inset 0 0 0 1px var(--accent-primary), 0 0 4px rgba(0, 0, 0, 0.5);
  }

  .radio-play-spinner {
    width: 38px;
    height: 38px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: rgba(255, 255, 255, 0.9);
  }

  .radio-spinner-svg {
    width: 34px;
    height: 34px;
    animation: radio-spin 1.2s linear infinite;
  }

  .radio-spinner-svg circle {
    stroke-dasharray: 90, 150;
    stroke-dashoffset: 0;
    animation: radio-dash 1.2s ease-in-out infinite;
  }

  @keyframes radio-spin {
    to { transform: rotate(360deg); }
  }

  @keyframes radio-dash {
    0% { stroke-dasharray: 1, 200; stroke-dashoffset: 0; }
    50% { stroke-dasharray: 90, 200; stroke-dashoffset: -35; }
    100% { stroke-dasharray: 90, 200; stroke-dashoffset: -125; }
  }

  .radio-card-meta-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .radio-card-artist {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ---- Your Mixes (duplicated from HomeView for component isolation) ---- */
  .your-mixes-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .mix-cards-row {
    display: flex;
    gap: 22px;
  }

  .mix-card {
    flex-shrink: 0;
    width: 210px;
    cursor: pointer;
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    color: inherit;
  }

  .mix-card-artwork {
    width: 210px;
    height: 210px;
    border-radius: 8px;
    overflow: hidden;
    margin-bottom: 8px;
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 14px;
    box-sizing: border-box;
  }

  .mix-gradient-daily::before,
  .mix-gradient-weekly::before,
  .mix-gradient-favq::before,
  .mix-gradient-topq::before {
    content: '';
    position: absolute;
    inset: -40%;
  }

  .mix-gradient-daily::before {
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 255, 230, 0.45) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(80, 30, 0, 0.35) 58%, transparent 61%),
      radial-gradient(ellipse at 30% 20%, rgba(255, 255, 255, 0.25) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 60%, rgba(255, 200, 50, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 80%, rgba(255, 140, 0, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #e8a020 0%, #d4781a 30%, #c45e18 60%, #a04010 100%);
  }

  .mix-gradient-weekly::before {
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 220, 255, 0.5) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(30, 0, 50, 0.4) 58%, transparent 61%),
      radial-gradient(ellipse at 40% 20%, rgba(255, 200, 255, 0.35) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 50%, rgba(200, 150, 255, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 70%, rgba(130, 80, 200, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #b060d0 0%, #8040b0 30%, #6030a0 60%, #402080 100%);
  }

  .mix-gradient-favq::before {
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 200, 200, 0.45) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(80, 0, 0, 0.35) 58%, transparent 61%),
      radial-gradient(ellipse at 30% 20%, rgba(255, 180, 180, 0.25) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 60%, rgba(255, 50, 50, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 80%, rgba(200, 0, 0, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #e82020 0%, #c41818 30%, #a01010 60%, #800808 100%);
  }

  .mix-gradient-topq::before {
    background:
      linear-gradient(125deg, transparent 20%, rgba(200, 220, 255, 0.45) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(0, 0, 80, 0.35) 58%, transparent 61%),
      radial-gradient(ellipse at 30% 20%, rgba(180, 200, 255, 0.25) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 60%, rgba(50, 100, 255, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 80%, rgba(0, 50, 200, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #2060e8 0%, #1848c4 30%, #1030a0 60%, #081880 100%);
  }

  .mix-card-badge {
    position: relative;
    z-index: 1;
    font-size: 11px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.7);
    letter-spacing: 0.02em;
    margin-bottom: 6px;
  }

  .mix-card-name {
    position: relative;
    z-index: 1;
    font-size: 22px;
    font-weight: 700;
    color: #fff;
    line-height: 1.1;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.2);
  }

  .mix-card-desc {
    font-size: 12px;
    font-weight: 400;
    color: var(--text-secondary);
    line-height: 1.4;
    margin: 0;
    min-height: calc(3 * 1.4 * 12px);
  }

  .mix-card-desc :global(strong) {
    font-weight: 600;
    color: var(--text-primary);
  }

  /* ---- Artist cards ---- */
  .artist-card {
    width: 210px;
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

  .artist-image-placeholder {
    width: 140px;
    height: 140px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
  }

  .artist-image {
    position: absolute;
    inset: 0;
    width: 140px;
    height: 140px;
    border-radius: 50%;
    object-fit: cover;
    z-index: 1;
    transition: opacity 0.15s ease-in;
  }

  .artist-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .artist-meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  /* ---- Track list ---- */
  .track-list {
    display: flex;
    flex-direction: column;
  }

  .track-list.compact {
    gap: 0;
  }

  /* ---- Skeleton loading ---- */
  .skeleton-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .skeleton-title {
    width: 200px;
    height: 24px;
    border-radius: 4px;
    background: var(--bg-tertiary);
  }

  .skeleton-row {
    display: flex;
    gap: 16px;
  }

  .skeleton-card {
    width: 210px;
    height: 280px;
    border-radius: 8px;
    background: var(--bg-tertiary);
    flex-shrink: 0;
  }

  .skeleton-artist {
    width: 120px;
    height: 160px;
    border-radius: 8px;
    background: var(--bg-tertiary);
    flex-shrink: 0;
  }

  .skeleton-tracks {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .skeleton-track {
    height: 48px;
    border-radius: 4px;
    background: var(--bg-tertiary);
  }

  /* ---- Empty state ---- */
  .home-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 60px 24px;
    color: var(--text-muted);
    gap: 12px;
  }

  .home-state .state-icon {
    opacity: 0.5;
    margin-bottom: 8px;
  }

  .home-state h1 {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .home-state p {
    font-size: 14px;
    margin: 0;
    max-width: 320px;
  }

  .spacer {
    width: 16px;
    flex-shrink: 0;
  }

  :global(.spinner) {
    animation: spin 1s linear infinite;
    color: var(--text-primary);
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  /* ---- Artists to Follow ---- */
  .follow-artist-card {
    flex-shrink: 0;
    width: 210px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    padding: 16px 12px;
    cursor: pointer;
    transition: border-color 150ms ease, background-color 150ms ease;
  }

  .follow-artist-card:hover {
    border-color: var(--accent-primary);
    background-color: var(--bg-hover);
  }

  .follow-artist-image-btn {
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }

  .follow-artist-name-btn {
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    color: inherit;
  }

  .follow-artist-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .follow-artist-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .follow-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease, border-color 150ms ease;
    white-space: nowrap;
  }

  .follow-btn:hover {
    background: var(--bg-tertiary);
    border-color: var(--text-muted);
  }

  .follow-btn.loading {
    opacity: 0.6;
    cursor: wait;
  }

  /* ---- Spotlight ---- */
  .spotlight-section {
    gap: 16px;
  }

  .spotlight-hero {
    display: flex;
    align-items: center;
    gap: 20px;
  }

  .spotlight-hero-image-btn {
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    flex-shrink: 0;
  }

  .spotlight-image {
    width: 120px;
    height: 120px;
    border-radius: 50%;
    overflow: hidden;
    background: var(--bg-secondary);
  }

  .spotlight-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .spotlight-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .spotlight-info {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .spotlight-category {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-primary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .spotlight-name {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
    line-height: 1.2;
  }

  .spotlight-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 4px;
  }

  /* Spotlight uses global .action-btn-circle and .action-btn-circle.primary */

  /* ---- Spotlight: Top Tracks card ---- */
  .spotlight-top-tracks-visual {
    background: #f5f5f5;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .spotlight-top-tracks-art {
    position: relative;
    z-index: 1;
    width: 130px;
    height: 130px;
    object-fit: cover;
    border-radius: 4px;
  }

  .spotlight-top-tracks-label {
    position: absolute;
    bottom: 10px;
    left: 0;
    right: 0;
    font-size: 20px;
    font-weight: 700;
    letter-spacing: 0.12em;
    padding-left: 0.12em;
    color: #111;
    pointer-events: none;
    z-index: 3;
    text-align: center;
    line-height: 1.2;
  }

  /* ---- Spotlight: Playlist card ---- */
  .spotlight-playlist-visual {
    background: var(--bg-tertiary);
  }

  .spotlight-playlist-img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    position: absolute;
    inset: 0;
  }

  .spotlight-playlist-label {
    color: var(--accent-primary);
    font-weight: 600;
    text-transform: uppercase;
    font-size: 11px;
    letter-spacing: 0.05em;
  }

  /* ---- Skeleton additions ---- */
  .skeleton-spotlight {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .skeleton-spotlight-hero {
    width: 100%;
    height: 120px;
    border-radius: 8px;
    background: var(--bg-tertiary);
  }
</style>
