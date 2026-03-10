<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Music, User, Loader2, ArrowRight, Heart, Play, Share2, UserPlus } from 'lucide-svelte';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { t } from '$lib/i18n';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import { formatDuration, formatQuality, getQobuzImage, getQobuzImageForSize } from '$lib/adapters/qobuzAdapters';
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

  interface SpotlightData {
    artistId: number;
    artistName: string;
    artistImage?: string;
    topTracks: PageArtistTrack[];
    category?: string;
  }

  // For You-specific state
  let failedArtistImages = $state<Set<number>>(new Set());
  let radioLoading = $state<string | null>(null); // album ID currently creating radio
  let radioCardColors = $state<Record<string, string>>({}); // album ID -> dominant color

  // Phase 2: Artists to Follow
  let suggestedArtists = $state<SuggestedArtist[]>([]);
  let loadingSuggestedArtists = $state(false);
  let forYouLoaded = $state(false);

  // Phase 2: Spotlight
  let spotlightData = $state<SpotlightData | null>(null);
  let loadingSpotlight = $state(false);
  let spotlightRadioLoading = $state(false);

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

  // Load Phase 2+3 sections when component mounts (once)
  onMount(() => {
    if (!forYouLoaded) {
      forYouLoaded = true;
      loadArtistsToFollow();
      loadSpotlight();
      loadSimilarAlbums();
      loadForgottenFavorites();
      loadEssentials();
    }
  });

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

      spotlightData = {
        artistId: response.id,
        artistName: response.name.display,
        artistImage,
        topTracks: (response.top_tracks || []).slice(0, 5),
        category: response.artist_category
      };
    } catch (err) {
      console.error('Failed to load spotlight:', err);
    } finally {
      loadingSpotlight = false;
    }
  }

  async function handleSpotlightRadio() {
    if (!spotlightData || spotlightRadioLoading) return;
    spotlightRadioLoading = true;
    try {
      await invoke('v2_create_qobuz_artist_radio', {
        artistId: spotlightData.artistId,
        artistName: spotlightData.artistName
      });
    } catch (err) {
      console.error('Failed to create spotlight radio:', err);
    } finally {
      spotlightRadioLoading = false;
    }
  }

  function getSpotlightTrackQuality(track: PageArtistTrack): string {
    const bitDepth = track.audio_info?.maximum_bit_depth;
    const sampleRate = track.audio_info?.maximum_sampling_rate;
    return formatQuality(
      (bitDepth ?? 16) > 16,
      bitDepth,
      sampleRate
    );
  }

  async function handleRadioPlay(albumId: string, albumTitle: string) {
    if (radioLoading) return;
    radioLoading = albumId;
    try {
      await invoke('v2_create_album_radio', { albumId, albumName: albumTitle });
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
        // Sample several pixels and average
        let rSum = 0, gSum = 0, bSum = 0, count = 0;
        for (let i = 0; i < data.length; i += 4) {
          rSum += data[i];
          gSum += data[i + 1];
          bSum += data[i + 2];
          count++;
        }
        const r = Math.round(rSum / count);
        const g = Math.round(gSum / count);
        const b = Math.round(bSum / count);
        // Slightly darken for better contrast with white text
        const dr = Math.round(r * 0.7);
        const dg = Math.round(g * 0.7);
        const db = Math.round(b * 0.7);
        radioCardColors = { ...radioCardColors, [albumId]: `rgb(${dr}, ${dg}, ${db})` };
      } catch {
        // Canvas tainted - ignore
      }
    };
    img.src = artworkUrl;
  }

  function handleArtistImageError(artistId: number) {
    failedArtistImages = new Set([...failedArtistImages, artistId]);
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
        'continue_listening',
        'Continue Listening',
        'qobuz',
        trackIds,
        trackIndex
      );

      try {
        const queueTracks = buildContinueQueueTracks(continueTracks);
        await invoke('v2_set_queue', { tracks: queueTracks, startIndex: trackIndex });
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
  <h2 class="section-title">{$t('home.yourMixes')}</h2>
  <div class="mix-cards-row">
    <button class="mix-card" onclick={() => onNavigateDailyQ?.()}>
      <div class="mix-card-artwork mix-gradient-daily">
        <span class="mix-card-badge">qobuz</span>
        <span class="mix-card-name">DailyQ</span>
      </div>
      <p class="mix-card-desc">{$t('yourMixes.cardDesc')}</p>
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
            <span class="radio-card-label">{$t('home.radioLabel')}</span>
            <div class="radio-card-hover-overlay" class:visible={isThisLoading}>
              {#if isThisLoading}
                <div class="radio-play-spinner">
                  <svg viewBox="0 0 50 50" class="radio-spinner-svg">
                    <circle cx="25" cy="25" r="20" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
                  </svg>
                </div>
              {:else}
                <button class="radio-overlay-play-btn" type="button">
                  <Play size={18} fill="white" color="white" />
                </button>
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
              <Loader2 size={14} class="spinner" />
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
          <button class="spotlight-action-btn spotlight-play" onclick={() => handleSpotlightRadio()} disabled={spotlightRadioLoading}>
            {#if spotlightRadioLoading}
              <Loader2 size={18} class="spinner" />
            {:else}
              <Play size={18} />
            {/if}
          </button>
          <button class="spotlight-action-btn" onclick={() => onArtistClick?.(spotlightData!.artistId)}>
            <User size={18} />
          </button>
        </div>
      </div>
    </div>

    <!-- Spotlight Top Tracks -->
    {#if spotlightData.topTracks.length > 0}
      <div class="spotlight-tracks">
        <h4 class="spotlight-tracks-title">{$t('home.topTracks')}</h4>
        <div class="track-list compact">
          {#each spotlightData.topTracks as track, index (track.id)}
            {@const isThisActiveTrack = activeTrackId === track.id}
            <TrackRow
              trackId={track.id}
              number={index + 1}
              title={track.title}
              artist={track.artist?.name?.display || spotlightData.artistName}
              album={track.album?.title}
              duration={track.duration ? formatDuration(track.duration) : ''}
              quality={getSpotlightTrackQuality(track)}
              isPlaying={isPlaybackActive && isThisActiveTrack}
              isActiveTrack={isThisActiveTrack}
              compact={true}
              onPlay={onTrackPlay ? () => onTrackPlay({
                id: track.id,
                title: track.title,
                artist: track.artist?.name?.display || spotlightData!.artistName,
                album: track.album?.title,
                albumArt: track.album?.image?.small,
                albumId: track.album?.id,
                artistId: track.artist?.id || spotlightData!.artistId,
                duration: track.duration ? formatDuration(track.duration) : '',
                durationSeconds: track.duration ?? 0
              }) : undefined}
              menuActions={{
                onGoToAlbum: track.album?.id && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.album!.id) : undefined,
                onGoToArtist: onArtistClick ? () => onArtistClick(track.artist?.id || spotlightData!.artistId) : undefined,
                onShowInfo: onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined
              }}
            />
          {/each}
        </div>
      </div>
    {/if}

    <!-- Spotlight Radio Card -->
    <div class="spotlight-extras">
      <button class="spotlight-radio-card" onclick={() => handleSpotlightRadio()}>
        <div class="spotlight-radio-visual">
          {#if spotlightData.artistImage}
            <img
              use:cachedSrc={spotlightData.artistImage}
              alt={spotlightData.artistName}
              class="spotlight-radio-img"
              loading="lazy"
              decoding="async"
            />
          {/if}
          <img
            src="/image_radio_shadows.png"
            alt=""
            class="radio-card-shadow"
          />
          <span class="spotlight-radio-name">{spotlightData.artistName}</span>
          <span class="radio-card-label">{$t('home.radioLabel')}</span>
        </div>
        <div class="spotlight-radio-title">{spotlightData.artistName}</div>
        <div class="spotlight-radio-subtitle">{$t('home.andMore')}</div>
      </button>
    </div>
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
    color: rgba(255, 255, 255, 0.85);
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
    will-change: transform;
  }

  .mix-gradient-daily::before {
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 255, 230, 0.45) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(80, 30, 0, 0.35) 58%, transparent 61%),
      radial-gradient(ellipse at 30% 20%, rgba(255, 255, 255, 0.25) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 60%, rgba(255, 200, 50, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 80%, rgba(255, 140, 0, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #e8a020 0%, #d4781a 30%, #c45e18 60%, #a04010 100%);
    animation: silk-daily 30s ease-in-out infinite alternate;
  }

  .mix-gradient-weekly::before {
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 220, 255, 0.5) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(30, 0, 50, 0.4) 58%, transparent 61%),
      radial-gradient(ellipse at 40% 20%, rgba(255, 200, 255, 0.35) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 50%, rgba(200, 150, 255, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 70%, rgba(130, 80, 200, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #b060d0 0%, #8040b0 30%, #6030a0 60%, #402080 100%);
    animation: silk-weekly 34s ease-in-out infinite alternate;
  }

  @keyframes silk-daily {
    0%   { transform: translate(5%, 3%) rotate(0deg) scale(1); }
    25%  { transform: translate(-8%, 6%) rotate(6deg) scale(1.03); }
    50%  { transform: translate(3%, -5%) rotate(-4deg) scale(0.98); }
    75%  { transform: translate(-4%, 8%) rotate(8deg) scale(1.02); }
    100% { transform: translate(6%, -3%) rotate(-2deg) scale(1); }
  }

  @keyframes silk-weekly {
    0%   { transform: translate(-3%, 6%) rotate(2deg) scale(1.01); }
    20%  { transform: translate(7%, -4%) rotate(-5deg) scale(0.98); }
    45%  { transform: translate(-6%, -2%) rotate(7deg) scale(1.03); }
    70%  { transform: translate(4%, 7%) rotate(-3deg) scale(1); }
    100% { transform: translate(-5%, 3%) rotate(4deg) scale(0.99); }
  }

  .mix-gradient-favq::before {
    background:
      linear-gradient(125deg, transparent 20%, rgba(255, 200, 200, 0.45) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(80, 0, 0, 0.35) 58%, transparent 61%),
      radial-gradient(ellipse at 30% 20%, rgba(255, 180, 180, 0.25) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 60%, rgba(255, 50, 50, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 80%, rgba(200, 0, 0, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #e82020 0%, #c41818 30%, #a01010 60%, #800808 100%);
    animation: silk-favq 28s ease-in-out infinite alternate;
  }

  .mix-gradient-topq::before {
    background:
      linear-gradient(125deg, transparent 20%, rgba(200, 220, 255, 0.45) 23%, transparent 26%),
      linear-gradient(125deg, transparent 55%, rgba(0, 0, 80, 0.35) 58%, transparent 61%),
      radial-gradient(ellipse at 30% 20%, rgba(180, 200, 255, 0.25) 0%, transparent 50%),
      radial-gradient(ellipse at 70% 60%, rgba(50, 100, 255, 0.4) 0%, transparent 50%),
      radial-gradient(ellipse at 20% 80%, rgba(0, 50, 200, 0.5) 0%, transparent 60%),
      linear-gradient(135deg, #2060e8 0%, #1848c4 30%, #1030a0 60%, #081880 100%);
    animation: silk-topq 32s ease-in-out infinite alternate;
  }

  @keyframes silk-favq {
    0%   { transform: translate(5%, 3%) rotate(0deg) scale(1); }
    25%  { transform: translate(-8%, 6%) rotate(6deg) scale(1.03); }
    50%  { transform: translate(3%, -5%) rotate(-4deg) scale(0.98); }
    75%  { transform: translate(-4%, 8%) rotate(8deg) scale(1.02); }
    100% { transform: translate(6%, -3%) rotate(-2deg) scale(1); }
  }

  @keyframes silk-topq {
    0%   { transform: translate(-3%, 6%) rotate(2deg) scale(1.01); }
    20%  { transform: translate(7%, -4%) rotate(-5deg) scale(0.98); }
    45%  { transform: translate(-6%, -2%) rotate(7deg) scale(1.03); }
    70%  { transform: translate(4%, 7%) rotate(-3deg) scale(1); }
    100% { transform: translate(-5%, 3%) rotate(4deg) scale(0.99); }
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
    flex-shrink: 0;
    width: 140px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    color: inherit;
  }

  .artist-image-wrapper {
    position: relative;
    width: 120px;
    height: 120px;
    border-radius: 50%;
    overflow: hidden;
    background: var(--bg-secondary);
  }

  .artist-image-placeholder {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .artist-image {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artist-name {
    font-size: 13px;
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
    width: 140px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
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
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 140px;
  }

  .follow-artist-label {
    font-size: 11px;
    color: var(--text-muted);
  }

  .follow-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 14px;
    border: 1px solid var(--border-primary);
    border-radius: 20px;
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
    gap: 10px;
    margin-top: 4px;
  }

  .spotlight-action-btn {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    border: 1px solid var(--border-primary);
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background-color 150ms ease;
  }

  .spotlight-action-btn:hover {
    background: var(--bg-tertiary);
  }

  .spotlight-action-btn.spotlight-play {
    background: var(--text-primary);
    color: var(--bg-primary);
    border-color: var(--text-primary);
  }

  .spotlight-action-btn.spotlight-play:hover {
    opacity: 0.9;
  }

  .spotlight-tracks {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .spotlight-tracks-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-secondary);
    margin: 0;
  }

  .spotlight-extras {
    display: flex;
    gap: 16px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .spotlight-extras::-webkit-scrollbar {
    display: none;
  }

  .spotlight-radio-card {
    flex-shrink: 0;
    width: 180px;
    cursor: pointer;
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    color: inherit;
  }

  .spotlight-radio-visual {
    position: relative;
    width: 180px;
    height: 180px;
    border-radius: 8px;
    overflow: hidden;
    margin-bottom: 8px;
  }

  .spotlight-radio-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .spotlight-radio-name {
    position: absolute;
    top: 12px;
    left: 12px;
    font-size: 14px;
    font-weight: 600;
    color: #fff;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.6);
    pointer-events: none;
  }

  .spotlight-radio-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spotlight-radio-subtitle {
    font-size: 12px;
    color: var(--text-muted);
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
