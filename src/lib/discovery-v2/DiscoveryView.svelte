<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import type { OfflineCacheStatus } from '$lib/stores/offlineCacheState';
  import type { DisplayTrack } from '$lib/types';
  import {
    fetchReleaseWatch,
    fetchDiscoverIndex,
    fetchHomeResolved,
    fetchRediscoverLibrary,
    fetchArtistsToFollow,
    fetchSimilarAlbums,
    fetchEssentialsByGenre,
    fetchArtistSpotlight,
    type DiscoveryAlbumCard,
    type DiscoveryTrackCard,
    type DiscoveryArtistTile,
    type DiscoveryPlaylistCard,
    type SimilarAlbumsSection,
    type EssentialsSection,
    type SpotlightSection,
  } from './data';
  import DiscoverySection from './DiscoverySection.svelte';
  import DiscoveryGridSection from './DiscoveryGridSection.svelte';
  import AlbumCardLite from './AlbumCardLite.svelte';
  import AlbumRowLite from './AlbumRowLite.svelte';
  import TrackRowLite from './TrackRowLite.svelte';
  import ArtistTileLite from './ArtistTileLite.svelte';
  import PlaylistCardLite from './PlaylistCardLite.svelte';
  import QobuzMixesRow from './QobuzMixesRow.svelte';
  import SpotlightLite from './SpotlightLite.svelte';
  import RadioCardLite from './RadioCardLite.svelte';
  import GenreFilterButton from '$lib/components/GenreFilterButton.svelte';
  import { getSelectedGenreIds } from '$lib/stores/genreFilterStore';
  import { sectionPrefs } from './sectionPrefs';
  import DiscoverySettingsModal from './DiscoverySettingsModal.svelte';
  import { Settings } from 'lucide-svelte';
  import {
    isAlbumFavorite,
    toggleAlbumFavorite,
    subscribe as subscribeAlbumFavs,
  } from '$lib/stores/albumFavoritesStore';
  import { toggleArtistFavorite } from '$lib/stores/artistFavoritesStore';
  import { invoke } from '@tauri-apps/api/core';
  import { replacePlaybackQueue } from '$lib/services/queuePlaybackService';
  import { playQueueIndex } from '$lib/stores/queueStore';
  import type { BackendQueueTrack } from '$lib/stores/queueStore';

  /**
   * Discovery V2 — clean-room rebuild of the home view.
   *
   * Spec: qbz-nix-docs/specs/2026-05-11-discovery-v2-clean-room.md
   *
   * Constraints:
   *  - Cero efectos. No transition/animation/will-change/backdrop-filter.
   *  - Same Props as the original HomeView so the parent mount swaps cleanly.
   *  - Sections render inline grids (no horizontal scroll containers).
   *  - V1 ships an empty/placeholder shell. Sections are filled incrementally
   *    in subsequent commits; each addition is measured.
   */

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
    onNavigateReleaseWatch?: () => void;
    onNavigateQobuzPlaylists?: () => void;
    onNavigateDailyQ?: () => void;
    onNavigateWeeklyQ?: () => void;
    onNavigateFavQ?: () => void;
    onNavigateTopQ?: () => void;
    homeTab?: 'home' | 'editorPicks' | 'forYou';
    onTabChange?: (tab: 'home' | 'editorPicks' | 'forYou') => void;
  }

  // Props accepted for drop-in compatibility with HomeView. V1 of Discovery V2
  // consumes a small subset; the rest are kept in the Props interface so
  // +page.svelte's mount site doesn't need changes when sections start
  // consuming them.
  let {
    homeTab = 'home',
    onTabChange,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onAddAlbumToPlaylist,
    onArtistClick,
    onTrackPlay,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onPlaylistClick,
    onPlaylistPlay,
    onPlaylistPlayNext,
    onPlaylistPlayLater,
    onPlaylistCopyToLibrary,
    onPlaylistShareQobuz,
    onNavigateNewReleases,
    onNavigateReleaseWatch,
    onNavigateTopAlbums,
    onNavigatePressAccolades,
    onNavigateQobuzissimes,
    onNavigateAlbumsOfTheWeek,
    onNavigateQobuzPlaylists,
    onNavigateIdealDiscography,
    onNavigateDailyQ,
    onNavigateWeeklyQ,
    onNavigateFavQ,
    onNavigateTopQ,
    activeTrackId,
    isPlaybackActive,
  }: Props = $props();

  type Tab = 'home' | 'editorPicks' | 'forYou';

  const tabs: { id: Tab; labelKey: string }[] = [
    { id: 'home', labelKey: 'home.title' },
    { id: 'editorPicks', labelKey: 'home.tabEditorPicks' },
    { id: 'forYou', labelKey: 'home.tabForYou' },
  ];

  function selectTab(id: Tab) {
    if (id === homeTab) return;
    onTabChange?.(id);
  }

  // Section state — minimal. No skeletons, no animated loaders. The grid
  // is empty until data arrives, then cards appear. Each section pulls
  // from the smallest V2 invoke that returns the shape it needs:
  //   - releaseWatch: `v2_get_release_watch` (personalized, followed artists)
  //   - newReleases / pressAwards / mostStreamed / qobuzissimes /
  //     editorPicks: a single `v2_get_discover_index` call returns all
  //     five editorial containers in one round-trip.
  let releaseWatch = $state<DiscoveryAlbumCard[]>([]);
  let newReleases = $state<DiscoveryAlbumCard[]>([]);
  let pressAwards = $state<DiscoveryAlbumCard[]>([]);
  let mostStreamed = $state<DiscoveryAlbumCard[]>([]);
  let qobuzissimes = $state<DiscoveryAlbumCard[]>([]);
  let editorPicks = $state<DiscoveryAlbumCard[]>([]);
  let idealDiscography = $state<DiscoveryAlbumCard[]>([]);
  let qobuzPlaylists = $state<DiscoveryPlaylistCard[]>([]);
  let recentlyPlayedAlbums = $state<DiscoveryAlbumCard[]>([]);
  let continueListening = $state<DiscoveryTrackCard[]>([]);
  let topArtists = $state<DiscoveryArtistTile[]>([]);
  let favoriteAlbums = $state<DiscoveryAlbumCard[]>([]);
  // For-You-exclusive section data
  let rediscoverLibrary = $state<DiscoveryAlbumCard[]>([]);
  let artistsToFollow = $state<DiscoveryArtistTile[]>([]);
  let similarAlbums = $state<SimilarAlbumsSection>({ seedTitle: '', albums: [] });
  let essentials = $state<EssentialsSection>({ genreName: '', albums: [] });
  let spotlight = $state<SpotlightSection | null>(null);

  /** Cleared once the three main fetches (release-watch, discover-index,
   *  home-resolved) have resolved. While true, the view renders a static
   *  skeleton so the first paint reads as "loading" rather than as the
   *  old "under construction" placeholder. Cero efectos — plain grey
   *  blocks shaped like the real sections. */
  let initialLoadComplete = $state(false);

  async function loadAll() {
    // Three parallel fetches:
    //  - release-watch (followed-artists radar, not genre-filtered)
    //  - discover-index (5 editorial album sections + playlists; respects genre)
    //  - home-resolved (4 personalized sections from local reco DB)
    // Each is independent so they race without blocking.
    const genreIds = Array.from(getSelectedGenreIds('home'));
    const [watch, index, resolved] = await Promise.all([
      fetchReleaseWatch(18),
      fetchDiscoverIndex(18, genreIds),
      fetchHomeResolved(18),
    ]);
    releaseWatch = watch;
    newReleases = index.newReleases;
    pressAwards = index.pressAwards;
    mostStreamed = index.mostStreamed;
    qobuzissimes = index.qobuzissimes;
    editorPicks = index.editorPicks;
    idealDiscography = index.idealDiscography;
    qobuzPlaylists = index.playlists;
    recentlyPlayedAlbums = resolved.recentlyPlayedAlbums;
    continueListening = resolved.continueListening;
    topArtists = resolved.topArtists;
    favoriteAlbums = resolved.favoriteAlbums;
    // Three main fetches done — drop the skeleton even if for-you-exclusive
    // sections (loaded below) are still pending; their absence reads as
    // empty/skipped rather than as a "still loading" state.
    initialLoadComplete = true;

    // For-You-exclusive sections depend on the home-resolved seeds
    // (top artists for spotlight/follow, recent albums for similar).
    // Fired after the main fetches so the seeds are populated.
    const topArtistIds = resolved.topArtists.map((a) => a.artistId);
    void Promise.all([
      fetchRediscoverLibrary(12),
      fetchArtistsToFollow(topArtistIds, 10),
      fetchSimilarAlbums(resolved.recentlyPlayedAlbums, 10),
      fetchEssentialsByGenre(12),
      fetchArtistSpotlight(resolved.topArtists),
    ]).then(([rediscover, follow, similar, ess, spot]) => {
      rediscoverLibrary = rediscover;
      artistsToFollow = follow;
      similarAlbums = similar;
      essentials = ess;
      spotlight = spot;
    });
  }

  onMount(() => {
    void loadAll();
  });

  function handleGenreFilterChange() {
    void loadAll();
  }

  // Discovery settings modal (toggle/reorder sections).
  let settingsOpen = $state(false);

  // Album favorites — subscribe so card `isFavorite` reads stay reactive.
  // The store's `isAlbumFavorite(id)` is a sync getter; we bump a $state
  // counter on each subscribe notification so {@const reads in the
  // album-card snippet re-evaluate.
  let favoritesVersion = $state(0);
  onMount(() => {
    const unsub = subscribeAlbumFavs(() => {
      favoritesVersion++;
    });
    return unsub;
  });
  function isFav(albumId: string): boolean {
    void favoritesVersion;
    return isAlbumFavorite(albumId);
  }

  /**
   * Spotlight handlers, ported faithfully from the legacy ForYouTab
   * (see src/lib/components/views/ForYouTab.svelte handleSpotlight*).
   *
   * handleSpotlightTopTracks: builds a queue from the spotlight artist's
   *   top tracks and plays from index 0. We don't have full audio_info
   *   in the trimmed SpotlightTopTrack shape, so we pass conservative
   *   defaults (hires=false, 16/44100); the player resolves the real
   *   format from the track id at stream-resolution time.
   *
   * handleSpotlightRadio: calls v2_create_qobuz_artist_radio to seed a
   *   radio queue, then kicks off playback from index 0. Mirrors legacy
   *   exactly.
   */
  let spotlightTopTracksLoading = $state(false);
  let spotlightRadioLoading = $state(false);

  async function handleSpotlightTopTracks() {
    if (!spotlight || spotlightTopTracksLoading || spotlight.topTracks.length === 0) return;
    spotlightTopTracksLoading = true;
    try {
      const artistName = spotlight.artistName;
      const artistId = spotlight.artistId;
      const queueTracks: BackendQueueTrack[] = spotlight.topTracks.map((track) => ({
        id: track.trackId,
        title: track.title,
        version: null,
        artist: artistName,
        album: track.albumTitle ?? '',
        duration_secs: track.durationSec ?? 0,
        artwork_url: track.artwork ?? null,
        hires: false,
        bit_depth: null,
        sample_rate: null,
        is_local: false,
        album_id: track.albumId ?? null,
        artist_id: artistId,
      }));
      await replacePlaybackQueue(queueTracks, 0, { debugLabel: 'discovery-v2:spotlight-top-tracks' });
      await playQueueIndex(0);
    } catch (err) {
      console.error('[discovery-v2] spotlight top tracks failed', err);
    } finally {
      spotlightTopTracksLoading = false;
    }
  }

  async function handleSpotlightRadio() {
    if (!spotlight || spotlightRadioLoading) return;
    spotlightRadioLoading = true;
    try {
      await invoke('v2_create_qobuz_artist_radio', {
        artistId: spotlight.artistId,
        artistName: spotlight.artistName,
      });
      await playQueueIndex(0);
    } catch (err) {
      console.error('[discovery-v2] spotlight radio failed', err);
    } finally {
      spotlightRadioLoading = false;
    }
  }

  // Per-artist follow state for the Artists-to-Follow tile button. Once
  // an artist is favorited we strip them from the suggestion list, just
  // like the legacy ForYouTab.handleFollowArtist did.
  let followingArtistIds = $state<Set<number>>(new Set());
  async function handleFollowArtist(artistId: number) {
    if (followingArtistIds.has(artistId)) return;
    followingArtistIds.add(artistId);
    followingArtistIds = new Set(followingArtistIds);
    try {
      await toggleArtistFavorite(artistId);
      artistsToFollow = artistsToFollow.filter((a) => a.artistId !== artistId);
    } catch (err) {
      console.error('[discovery-v2] follow artist failed', err);
    } finally {
      followingArtistIds.delete(artistId);
      followingArtistIds = new Set(followingArtistIds);
    }
  }

  // `activeTrackId` + `isPlaybackActive` drive the playing indicator on
  // TrackCardLite within Continue Listening (the only section where the
  // card carries a trackId-level identity). Album/playlist cards stay
  // un-highlighted until we surface a `currentAlbumId` from the parent
  // playback context.
</script>

<div class="discovery">
  <div class="toolbar" data-tauri-drag-region>
    <div class="tabs" data-tauri-drag-region="false">
      {#each tabs as tab (tab.id)}
        <button
          class="tab"
          class:active={homeTab === tab.id}
          type="button"
          onclick={() => selectTab(tab.id)}
        >
          {$t(tab.labelKey)}
        </button>
      {/each}
    </div>
    <div class="genre-slot" data-tauri-drag-region="false">
      <GenreFilterButton
        onFilterChange={handleGenreFilterChange}
        context="home"
        variant="default"
        align="right"
      />
      <button
        type="button"
        class="settings-btn"
        aria-label={$t('discovery.customize')}
        title={$t('discovery.customize')}
        onclick={() => (settingsOpen = true)}
      >
        <Settings size={18} />
      </button>
    </div>
  </div>

  <DiscoverySettingsModal isOpen={settingsOpen} onClose={() => (settingsOpen = false)} tab={homeTab} />

  {#snippet albumCard(album: DiscoveryAlbumCard)}
    <AlbumCardLite
      albumId={album.albumId}
      title={album.title}
      artist={album.artist}
      artwork={album.artwork}
      quality={album.quality}
      isHiRes={album.isHiRes}
      bitDepth={album.bitDepth}
      samplingRate={album.samplingRate}
      ribbon={album.ribbon}
      genre={album.genre}
      releaseYear={album.releaseYear}
      isFavorite={isFav(album.albumId)}
      onClick={() => onAlbumClick?.(album.albumId)}
      onPlay={() => onAlbumPlay?.(album.albumId)}
      onFavorite={() => { void toggleAlbumFavorite(album.albumId); }}
      onArtistClick={album.artistId !== undefined
        ? () => onArtistClick?.(album.artistId!)
        : undefined}
      onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext?.(album.albumId) : undefined}
      onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater?.(album.albumId) : undefined}
      onAddToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist?.(album.albumId) : undefined}
      onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz?.(album.albumId) : undefined}
      onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink?.(album.albumId) : undefined}
      onDownload={onAlbumDownload ? () => onAlbumDownload?.(album.albumId) : undefined}
    />
  {/snippet}

  {#snippet albumRow(album: DiscoveryAlbumCard, index: number)}
    <AlbumRowLite
      albumId={album.albumId}
      title={album.title}
      artist={album.artist}
      artwork={album.artwork}
      rank={index + 1}
      onClick={() => onAlbumClick?.(album.albumId)}
      onPlay={() => onAlbumPlay?.(album.albumId)}
      onArtistClick={album.artistId !== undefined
        ? () => onArtistClick?.(album.artistId!)
        : undefined}
      onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext?.(album.albumId) : undefined}
      onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater?.(album.albumId) : undefined}
      onAddToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist?.(album.albumId) : undefined}
      onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz?.(album.albumId) : undefined}
      onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink?.(album.albumId) : undefined}
      onDownload={onAlbumDownload ? () => onAlbumDownload?.(album.albumId) : undefined}
    />
  {/snippet}

  {#snippet trackRow(track: DiscoveryTrackCard)}
    <TrackRowLite
      trackId={track.trackId}
      title={track.title}
      artist={track.artist}
      artwork={track.artwork}
      isPlaying={track.trackId === activeTrackId && isPlaybackActive === true}
      onClick={() => onTrackPlay?.({
        id: track.trackId,
        title: track.title,
        artist: track.artist,
        album: track.album,
        albumId: track.albumId,
        artistId: track.artistId,
        albumArt: track.artwork,
      } as DisplayTrack)}
      onArtistClick={track.artistId !== undefined
        ? () => onTrackGoToArtist?.(track.artistId!)
        : undefined}
    />
  {/snippet}

  {#snippet playlistCard(playlist: DiscoveryPlaylistCard)}
    <PlaylistCardLite
      playlistId={playlist.playlistId}
      name={playlist.name}
      image={playlist.image}
      onClick={() => onPlaylistClick?.(playlist.playlistId)}
      onPlay={() => onPlaylistPlay?.(playlist.playlistId)}
      onPlayNext={onPlaylistPlayNext ? () => onPlaylistPlayNext?.(playlist.playlistId) : undefined}
      onPlayLater={onPlaylistPlayLater ? () => onPlaylistPlayLater?.(playlist.playlistId) : undefined}
      onCopyToLibrary={onPlaylistCopyToLibrary ? () => onPlaylistCopyToLibrary?.(playlist.playlistId) : undefined}
      onShareQobuz={onPlaylistShareQobuz ? () => onPlaylistShareQobuz?.(playlist.playlistId) : undefined}
    />
  {/snippet}

  {#snippet radioCard(album: DiscoveryAlbumCard)}
    <RadioCardLite
      seedTitle={album.title}
      seedSubtitle={album.artist}
      artwork={album.artwork}
      onClick={() => onAlbumClick?.(album.albumId)}
      onPlay={() => onAlbumPlay?.(album.albumId)}
    />
  {/snippet}

  {#snippet artistTile(artist: DiscoveryArtistTile)}
    <ArtistTileLite
      artistId={artist.artistId}
      name={artist.name}
      image={artist.image}
      onClick={() => onArtistClick?.(artist.artistId)}
    />
  {/snippet}

  {#snippet artistTileFollowable(artist: DiscoveryArtistTile)}
    <ArtistTileLite
      artistId={artist.artistId}
      name={artist.name}
      image={artist.image}
      onClick={() => onArtistClick?.(artist.artistId)}
      onFollow={() => { void handleFollowArtist(artist.artistId); }}
      isFollowing={followingArtistIds.has(artist.artistId)}
    />
  {/snippet}

  <div class="scroll-area">
    {#if !initialLoadComplete}
      <!-- Skeleton — three placeholder sections (title bar + a row of card
           silhouettes). Cero efectos: no shimmer, no animation. The visible
           shape immediately signals "loading" so users don't see the empty
           state momentarily and read it as the final view. -->
      {#each [0, 1, 2] as i (i)}
        <section class="skel-section">
          <div class="skel-title"></div>
          <div class="skel-row">
            {#each [0, 1, 2, 3, 4] as j (j)}
              <div class="skel-card"></div>
            {/each}
          </div>
        </section>
      {/each}
    {:else}
    {#each $sectionPrefs[homeTab] as pref (pref.id)}
      {#if pref.enabled}
        {#if pref.id === 'newReleases' && newReleases.length > 0}
          <DiscoverySection
            title={$t('home.newReleases')}
            items={newReleases}
            renderItem={albumCard}
            onSeeAll={onNavigateNewReleases}
          />
        {:else if pref.id === 'pressAwards' && pressAwards.length > 0}
          <DiscoverySection
            title={$t('home.pressAwards')}
            items={pressAwards}
            renderItem={albumCard}
            onSeeAll={onNavigatePressAccolades}
          />
        {:else if pref.id === 'qobuzPlaylists' && qobuzPlaylists.length > 0}
          <DiscoverySection
            title={$t('home.qobuzPlaylists')}
            items={qobuzPlaylists}
            renderItem={playlistCard}
            onSeeAll={onNavigateQobuzPlaylists}
          />
        {:else if pref.id === 'recentlyPlayedAlbums' && recentlyPlayedAlbums.length > 0}
          <DiscoverySection
            title={$t('home.recentlyPlayed')}
            items={recentlyPlayedAlbums}
            renderItem={albumCard}
          />
        {:else if pref.id === 'continueListening' && continueListening.length > 0}
          <DiscoveryGridSection
            title={$t('home.continueListening')}
            items={continueListening}
            renderItem={trackRow}
          />
        {:else if pref.id === 'idealDiscography' && idealDiscography.length > 0}
          <DiscoverySection
            title={$t('discover.idealDiscography')}
            items={idealDiscography}
            renderItem={albumCard}
            onSeeAll={onNavigateIdealDiscography}
          />
        {:else if pref.id === 'mostStreamed' && mostStreamed.length > 0}
          <DiscoveryGridSection
            title={$t('home.mostStreamed')}
            items={mostStreamed.slice(0, 15)}
            renderItem={albumRow}
            onSeeAll={onNavigateTopAlbums}
          />
        {:else if pref.id === 'releaseWatch' && releaseWatch.length > 0}
          <DiscoverySection
            title={$t('home.releaseWatch')}
            items={releaseWatch}
            renderItem={albumCard}
            onSeeAll={onNavigateReleaseWatch}
          />
        {:else if pref.id === 'editorPicks' && editorPicks.length > 0}
          <DiscoverySection
            title={$t('home.editorPicks')}
            items={editorPicks}
            renderItem={albumCard}
            onSeeAll={onNavigateAlbumsOfTheWeek}
          />
        {:else if pref.id === 'qobuzissimes' && qobuzissimes.length > 0}
          <DiscoverySection
            title={$t('home.qobuzissimes')}
            items={qobuzissimes}
            renderItem={albumCard}
            onSeeAll={onNavigateQobuzissimes}
          />
        {:else if pref.id === 'topArtists' && topArtists.length > 0}
          <DiscoverySection
            title={$t('home.yourTopArtists')}
            items={topArtists}
            renderItem={artistTile}
            cardWidth={170}
          />
        {:else if pref.id === 'favoriteAlbums' && favoriteAlbums.length > 0}
          <DiscoverySection
            title={$t('home.favoriteAlbums')}
            items={favoriteAlbums}
            renderItem={albumCard}
          />
        {:else if pref.id === 'qobuzMixes'}
          <section class="mixes-section">
            <header class="head">
              <h2 class="title">{$t('home.qobuzMixes')}</h2>
            </header>
            <QobuzMixesRow
              onDailyQ={onNavigateDailyQ}
              onWeeklyQ={onNavigateWeeklyQ}
              onFavQ={onNavigateFavQ}
              onTopQ={onNavigateTopQ}
            />
          </section>
        {:else if pref.id === 'radioStations' && (recentlyPlayedAlbums.length > 0 || favoriteAlbums.length > 0)}
          <DiscoverySection
            title={$t('home.radioStations')}
            items={[...recentlyPlayedAlbums, ...favoriteAlbums]
              .filter((a, idx, arr) => arr.findIndex((b) => b.albumId === a.albumId) === idx)
              .slice(0, 12)}
            renderItem={radioCard}
          />
        {:else if pref.id === 'rediscoverLibrary' && rediscoverLibrary.length > 0}
          <DiscoverySection
            title={$t('discovery.rediscoverLibrary')}
            items={rediscoverLibrary}
            renderItem={albumCard}
          />
        {:else if pref.id === 'artistsToFollow' && artistsToFollow.length > 0}
          <DiscoverySection
            title={$t('discovery.artistsToFollow')}
            items={artistsToFollow}
            renderItem={artistTileFollowable}
            cardWidth={170}
          />
        {:else if pref.id === 'similarAlbums' && similarAlbums.albums.length > 0}
          <DiscoverySection
            title={$t('discovery.similarTo', { values: { seed: similarAlbums.seedTitle } })}
            items={similarAlbums.albums}
            renderItem={albumCard}
          />
        {:else if pref.id === 'essentialsByGenre' && essentials.albums.length > 0}
          <DiscoverySection
            title={$t('discovery.essentialsIn', { values: { genre: essentials.genreName } })}
            items={essentials.albums}
            renderItem={albumCard}
          />
        {:else if pref.id === 'artistSpotlight' && spotlight}
          <SpotlightLite
            {spotlight}
            onArtistClick={onArtistClick}
            onAlbumClick={onAlbumClick}
            onPlaylistClick={onPlaylistClick}
            onPlayTopTracks={() => { void handleSpotlightTopTracks(); }}
            onStartRadio={() => { void handleSpotlightRadio(); }}
            onAlbumPlay={onAlbumPlay}
            onAlbumPlayNext={onAlbumPlayNext}
            onAlbumPlayLater={onAlbumPlayLater}
            onAlbumAddToPlaylist={onAddAlbumToPlaylist}
            onAlbumShareQobuz={onAlbumShareQobuz}
            onAlbumShareSonglink={onAlbumShareSonglink}
            onAlbumDownload={onAlbumDownload}
          />
        {/if}
      {/if}
    {/each}

      {#if releaseWatch.length === 0 && newReleases.length === 0 && recentlyPlayedAlbums.length === 0 && topArtists.length === 0 && favoriteAlbums.length === 0}
        <p class="placeholder">{$t('empty.noContent')}</p>
      {/if}
    {/if}
  </div>
</div>

<style>
  /* Discovery V2 — zero effects.
     Toolbar is a fixed-height static row (NOT position:sticky). The scroll
     happens on .scroll-area below. Single scroll container, no stacking
     context entanglement, no transition on layout properties. */
  .discovery {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .toolbar {
    flex: 0 0 56px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 32px;
    gap: 16px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    padding: 3px;
  }

  .tab {
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 13px;
    font-weight: 500;
    padding: 6px 14px;
    border-radius: 4px;
    cursor: pointer;
    font-family: inherit;
  }

  .tab.active {
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .genre-slot {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .settings-btn {
    width: 32px;
    height: 32px;
    border-radius: 4px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .settings-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .scroll-area {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 24px 32px 100px;
  }

  .placeholder {
    font-size: 14px;
    color: var(--text-muted);
    margin: 0;
  }

  /* Skeleton sections — static grey blocks mirroring the section layout
     while the first fetch is in flight. Cero efectos: no shimmer keyframe,
     no transition. The shape alone communicates "loading content here". */
  .skel-section {
    margin-bottom: 48px;
  }

  .skel-title {
    width: 180px;
    height: 18px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    margin-bottom: 16px;
  }

  .skel-row {
    display: flex;
    gap: 32px;
    overflow: hidden;
  }

  .skel-card {
    flex: 0 0 220px;
    width: 220px;
    height: 220px;
    border-radius: 8px;
    background: var(--bg-tertiary);
  }

  /* Qobuz Mixes section — doesn't use DiscoverySection since its content
     isn't paginated. Match outer spacing + header so it reads as part of
     the same scroll. Artist Spotlight has its own wrapper inside
     SpotlightLite so it doesn't need styles here. */
  .mixes-section {
    margin-bottom: 48px;
  }

  .mixes-section .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  .mixes-section .title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }
</style>
