<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import type { OfflineCacheStatus } from '$lib/stores/offlineCacheState';
  import type { DisplayTrack } from '$lib/types';
  import {
    fetchReleaseWatch,
    fetchDiscoverIndex,
    fetchHomeResolved,
    type DiscoveryAlbumCard,
    type DiscoveryTrackCard,
    type DiscoveryArtistTile,
    type DiscoveryPlaylistCard,
  } from './data';
  import DiscoverySection from './DiscoverySection.svelte';
  import AlbumCardLite from './AlbumCardLite.svelte';
  import TrackCardLite from './TrackCardLite.svelte';
  import ArtistTileLite from './ArtistTileLite.svelte';
  import PlaylistCardLite from './PlaylistCardLite.svelte';

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
    onArtistClick,
    onTrackPlay,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onPlaylistClick,
    onPlaylistPlay,
    onNavigateNewReleases,
    onNavigateReleaseWatch,
    onNavigateTopAlbums,
    onNavigatePressAccolades,
    onNavigateQobuzissimes,
    onNavigateAlbumsOfTheWeek,
    onNavigateQobuzPlaylists,
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
  let qobuzPlaylists = $state<DiscoveryPlaylistCard[]>([]);
  let recentlyPlayedAlbums = $state<DiscoveryAlbumCard[]>([]);
  let continueListening = $state<DiscoveryTrackCard[]>([]);
  let topArtists = $state<DiscoveryArtistTile[]>([]);
  let favoriteAlbums = $state<DiscoveryAlbumCard[]>([]);

  onMount(async () => {
    // Three parallel fetches:
    //  - release-watch (followed-artists radar)
    //  - discover-index (5 editorial album sections + playlists)
    //  - home-resolved (4 personalized sections from local reco DB)
    // Each is independent so they race without blocking.
    const [watch, index, resolved] = await Promise.all([
      fetchReleaseWatch(8),
      fetchDiscoverIndex(8),
      fetchHomeResolved(8),
    ]);
    releaseWatch = watch;
    newReleases = index.newReleases;
    pressAwards = index.pressAwards;
    mostStreamed = index.mostStreamed;
    qobuzissimes = index.qobuzissimes;
    editorPicks = index.editorPicks;
    qobuzPlaylists = index.playlists;
    recentlyPlayedAlbums = resolved.recentlyPlayedAlbums;
    continueListening = resolved.continueListening;
    topArtists = resolved.topArtists;
    favoriteAlbums = resolved.favoriteAlbums;
  });

  // `activeTrackId` and `isPlaybackActive` are destructured but not yet read
  // by V1 — the album-card level doesn't know the playing track's albumId
  // without joining against the current playback context. They'll wire up
  // once track-level sections (Continue Listening) land.
</script>

<div class="discovery">
  <div class="toolbar">
    <div class="tabs">
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
    <div class="genre-slot">
      <!-- Genre filter button slot — wire actual selector after measure. -->
    </div>
  </div>

  {#snippet albumGrid(albums: DiscoveryAlbumCard[])}
    {#each albums as album (album.albumId)}
      <AlbumCardLite
        albumId={album.albumId}
        title={album.title}
        artist={album.artist}
        artwork={album.artwork}
        onClick={() => onAlbumClick?.(album.albumId)}
        onPlay={() => onAlbumPlay?.(album.albumId)}
        onArtistClick={album.artistId !== undefined
          ? () => onArtistClick?.(album.artistId!)
          : undefined}
      />
    {/each}
  {/snippet}

  <div class="scroll-area">
    {#if recentlyPlayedAlbums.length > 0}
      <DiscoverySection title={$t('home.recentlyPlayed')}>
        {#snippet children()}{@render albumGrid(recentlyPlayedAlbums)}{/snippet}
      </DiscoverySection>
    {/if}

    {#if continueListening.length > 0}
      <DiscoverySection title={$t('home.continueListening')}>
        {#snippet children()}
          {#each continueListening as track (track.trackId)}
            <TrackCardLite
              trackId={track.trackId}
              title={track.title}
              artist={track.artist}
              artwork={track.artwork}
              onPlay={() => onTrackPlay?.({
                id: track.trackId,
                title: track.title,
                artist: track.artist,
                album: track.album,
                albumId: track.albumId,
                artistId: track.artistId,
                albumArt: track.artwork,
              } as DisplayTrack)}
              onAlbumClick={track.albumId
                ? () => onTrackGoToAlbum?.(track.albumId!)
                : undefined}
              onArtistClick={track.artistId !== undefined
                ? () => onTrackGoToArtist?.(track.artistId!)
                : undefined}
            />
          {/each}
        {/snippet}
      </DiscoverySection>
    {/if}

    {#if releaseWatch.length > 0}
      <DiscoverySection
        title={$t('home.releaseWatch')}
        onSeeAll={onNavigateReleaseWatch}
      >
        {#snippet children()}{@render albumGrid(releaseWatch)}{/snippet}
      </DiscoverySection>
    {/if}

    {#if newReleases.length > 0}
      <DiscoverySection
        title={$t('home.newReleases')}
        onSeeAll={onNavigateNewReleases}
      >
        {#snippet children()}{@render albumGrid(newReleases)}{/snippet}
      </DiscoverySection>
    {/if}

    {#if mostStreamed.length > 0}
      <DiscoverySection
        title={$t('home.mostStreamed')}
        onSeeAll={onNavigateTopAlbums}
      >
        {#snippet children()}{@render albumGrid(mostStreamed)}{/snippet}
      </DiscoverySection>
    {/if}

    {#if editorPicks.length > 0}
      <DiscoverySection
        title={$t('home.editorPicks')}
        onSeeAll={onNavigateAlbumsOfTheWeek}
      >
        {#snippet children()}{@render albumGrid(editorPicks)}{/snippet}
      </DiscoverySection>
    {/if}

    {#if qobuzissimes.length > 0}
      <DiscoverySection
        title={$t('home.qobuzissimes')}
        onSeeAll={onNavigateQobuzissimes}
      >
        {#snippet children()}{@render albumGrid(qobuzissimes)}{/snippet}
      </DiscoverySection>
    {/if}

    {#if pressAwards.length > 0}
      <DiscoverySection
        title={$t('home.pressAwards')}
        onSeeAll={onNavigatePressAccolades}
      >
        {#snippet children()}{@render albumGrid(pressAwards)}{/snippet}
      </DiscoverySection>
    {/if}

    {#if qobuzPlaylists.length > 0}
      <DiscoverySection
        title={$t('home.qobuzPlaylists')}
        onSeeAll={onNavigateQobuzPlaylists}
      >
        {#snippet children()}
          {#each qobuzPlaylists as playlist (playlist.playlistId)}
            <PlaylistCardLite
              playlistId={playlist.playlistId}
              name={playlist.name}
              image={playlist.image}
              onClick={() => onPlaylistClick?.(playlist.playlistId)}
              onPlay={() => onPlaylistPlay?.(playlist.playlistId)}
            />
          {/each}
        {/snippet}
      </DiscoverySection>
    {/if}

    {#if topArtists.length > 0}
      <DiscoverySection title={$t('home.yourTopArtists')}>
        {#snippet children()}
          {#each topArtists as artist (artist.artistId)}
            <ArtistTileLite
              artistId={artist.artistId}
              name={artist.name}
              image={artist.image}
              onClick={() => onArtistClick?.(artist.artistId)}
            />
          {/each}
        {/snippet}
      </DiscoverySection>
    {/if}

    {#if favoriteAlbums.length > 0}
      <DiscoverySection title={$t('home.favoriteAlbums')}>
        {#snippet children()}{@render albumGrid(favoriteAlbums)}{/snippet}
      </DiscoverySection>
    {/if}

    {#if releaseWatch.length === 0 && newReleases.length === 0 && recentlyPlayedAlbums.length === 0}
      <p class="placeholder">{$t('discovery.comingSoon')}</p>
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
</style>
