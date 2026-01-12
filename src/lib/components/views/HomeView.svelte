<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Music, User, Loader2 } from 'lucide-svelte';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import HomeSettingsModal from '../HomeSettingsModal.svelte';
  import { formatDuration, formatQuality, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import {
    subscribe as subscribeHomeSettings,
    getSettings,
    getGreetingText,
    type HomeSettings,
    type HomeSectionId
  } from '$lib/stores/homeSettingsStore';
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
    quality?: string;
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
    onArtistClick?: (artistId: number) => void;
    onTrackPlay?: (track: DisplayTrack) => void;
  }

  let { userName = 'User', onAlbumClick, onArtistClick, onTrackPlay }: Props = $props();

  // Home settings state
  let homeSettings = $state<HomeSettings>(getSettings());
  let isSettingsModalOpen = $state(false);

  // Computed greeting
  const greetingText = $derived(getGreetingText(userName));


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
    recentAlbums: 12,
    continueTracks: 8,
    topArtists: 10,
    favoriteAlbums: 12,
    favoriteTracks: 10,
    featuredAlbums: 12
  };

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

  async function fetchFeaturedAlbums(featuredType: string, limit: number): Promise<AlbumCardData[]> {
    try {
      const response = await invoke<FeaturedAlbumsResponse>('get_featured_albums', {
        featuredType,
        limit
      });
      return response.items.map(toAlbumCard);
    } catch (err) {
      console.error(`Failed to fetch ${featuredType}:`, err);
      return [];
    }
  }

  function toAlbumCard(album: QobuzAlbum): AlbumCardData {
    return {
      id: album.id,
      artwork: getQobuzImage(album.image),
      title: album.title,
      artist: album.artist?.name || 'Unknown Artist',
      quality: formatQuality(album.hires_streamable, album.maximum_bit_depth, album.maximum_sampling_rate)
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

  function buildTopArtistSeedsFromTracks(tracks: DisplayTrack[]): TopArtistSeed[] {
    const counts = new Map<number, number>();
    for (const track of tracks) {
      if (!track.artistId) continue;
      counts.set(track.artistId, (counts.get(track.artistId) ?? 0) + 1);
    }

    return Array.from(counts.entries())
      .map(([artistId, playCount]) => ({ artistId, playCount }))
      .sort((a, b) => b.playCount - a.playCount)
      .slice(0, LIMITS.topArtists);
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
      limitRecentAlbums: LIMITS.recentAlbums,
      limitContinueTracks: LIMITS.continueTracks,
      limitTopArtists: LIMITS.topArtists,
      limitFavorites: Math.max(LIMITS.favoriteAlbums, LIMITS.favoriteTracks)
    });

    // Start Qobuz API calls in parallel (don't await)
    if (isSectionVisible('newReleases')) {
      fetchFeaturedAlbums('new-releases', LIMITS.featuredAlbums).then(albums => {
        newReleases = albums;
        loadingNewReleases = false;
        markSectionFinished();
      });
    } else {
      loadingNewReleases = false;
    }

    if (isSectionVisible('pressAwards')) {
      fetchFeaturedAlbums('press-awards', LIMITS.featuredAlbums).then(albums => {
        pressAwards = albums;
        loadingPressAwards = false;
        markSectionFinished();
      });
    } else {
      loadingPressAwards = false;
    }

    if (isSectionVisible('mostStreamed')) {
      fetchFeaturedAlbums('most-streamed', LIMITS.featuredAlbums).then(albums => {
        mostStreamed = albums;
        loadingMostStreamed = false;
        markSectionFinished();
      });
    } else {
      loadingMostStreamed = false;
    }

    if (isSectionVisible('qobuzissimes')) {
      fetchFeaturedAlbums('qobuzissimes', LIMITS.featuredAlbums).then(albums => {
        qobuzissimes = albums;
        loadingQobuzissimes = false;
        markSectionFinished();
      });
    } else {
      loadingQobuzissimes = false;
    }

    if (isSectionVisible('editorPicks')) {
      fetchFeaturedAlbums('editor-picks', LIMITS.featuredAlbums).then(albums => {
        editorPicks = albums;
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
        fetchAlbums(recentAlbumIds.slice(0, LIMITS.recentAlbums)).then(albums => {
          recentAlbums = albums;
          loadingRecentAlbums = false;
          markSectionFinished();
        });
      } else {
        loadingRecentAlbums = false;
      }

      // Top Artists
      if (isSectionVisible('topArtists')) {
        fetchArtists(seeds.topArtistIds.slice(0, LIMITS.topArtists)).then(artists => {
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
        fetchTracks(seeds.favoriteTrackIds.slice(0, LIMITS.favoriteTracks)).then(async favoriteTrackDetails => {
          const favoriteAlbumIds = normalizeAlbumIds([
            ...seeds.favoriteAlbumIds,
            ...favoriteTrackDetails.map(track => track.albumId)
          ]);
          const albums = await fetchAlbums(favoriteAlbumIds.slice(0, LIMITS.favoriteAlbums));
          favoriteAlbums = albums;
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
    <div class="loading-overlay" class:fade-out={sectionsFinished >= totalVisibleSections && totalVisibleSections > 0}>
      <div class="loading-content">
        <div class="loading-icon">
          <Loader2 size={36} class="spinner" />
        </div>
        <h2>Loading your home</h2>
        <p>Building recommendations from your listening history.</p>
      </div>
    </div>
  {/if}

  <!-- Header with greeting and settings -->
  <div class="home-header">
    {#if homeSettings.greeting.enabled}
      <h2 class="greeting">{greetingText}</h2>
    {:else}
      <div></div>
    {/if}
    <button class="settings-btn" onclick={() => isSettingsModalOpen = true} title="Customize Home">
      <img src="/home-gear.svg" alt="Settings" class="settings-icon" />
    </button>
  </div>

  {#if isInitializing && !isOverlayVisible}
    <div class="home-state">
      <div class="state-icon loading">
        <Loader2 size={36} class="spinner" />
      </div>
      <h1>Loading your home</h1>
      <p>Building recommendations from your listening history.</p>
    </div>
  {:else if error}
    <div class="home-state">
      <div class="state-icon">
        <Music size={36} />
      </div>
      <h1>Could not load Home</h1>
      <p>{error}</p>
    </div>
  {:else if hasContent}
    <!-- Render sections in user-defined order -->
    {#each visibleSections as sectionId (sectionId)}
      {#if sectionId === 'newReleases' && newReleases.length > 0}
        <HorizontalScrollRow title="New Releases">
          {#snippet children()}
            {#each newReleases as album}
              <AlbumCard
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                quality={album.quality}
                onclick={() => onAlbumClick?.(album.id)}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}

      {#if sectionId === 'pressAwards' && pressAwards.length > 0}
        <HorizontalScrollRow title="Press Awards">
          {#snippet children()}
            {#each pressAwards as album}
              <AlbumCard
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                quality={album.quality}
                onclick={() => onAlbumClick?.(album.id)}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}

      {#if sectionId === 'mostStreamed' && mostStreamed.length > 0}
        <HorizontalScrollRow title="Popular Albums">
          {#snippet children()}
            {#each mostStreamed as album}
              <AlbumCard
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                quality={album.quality}
                onclick={() => onAlbumClick?.(album.id)}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}

      {#if sectionId === 'qobuzissimes' && qobuzissimes.length > 0}
        <HorizontalScrollRow title="Qobuzissimes">
          {#snippet children()}
            {#each qobuzissimes as album}
              <AlbumCard
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                quality={album.quality}
                onclick={() => onAlbumClick?.(album.id)}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}

      {#if sectionId === 'editorPicks' && editorPicks.length > 0}
        <HorizontalScrollRow title="Editor's Picks">
          {#snippet children()}
            {#each editorPicks as album}
              <AlbumCard
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                quality={album.quality}
                onclick={() => onAlbumClick?.(album.id)}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}

      {#if sectionId === 'recentAlbums' && recentAlbums.length > 0}
        <HorizontalScrollRow title="Recently Played">
          {#snippet children()}
            {#each recentAlbums as album}
              <AlbumCard
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                quality={album.quality}
                onclick={() => onAlbumClick?.(album.id)}
              />
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}

      {#if sectionId === 'continueTracks' && continueTracks.length > 0}
        <div class="section">
          <div class="section-header">
            <h2>Continue Listening</h2>
          </div>
          <div class="track-list">
            {#each continueTracks as track, index}
              <TrackRow
                trackId={track.id}
                number={index + 1}
                title={track.title}
                artist={track.artist}
                duration={track.duration}
                quality={getTrackQuality(track)}
                hideDownload={true}
                compact={true}
                onPlay={() => onTrackPlay?.(track)}
                menuActions={{
                  onPlayNow: () => onTrackPlay?.(track)
                }}
              />
            {/each}
          </div>
        </div>
      {/if}

      {#if sectionId === 'topArtists' && topArtists.length > 0}
        <HorizontalScrollRow title="Your Top Artists">
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
                  <div class="artist-meta">{artist.playCount} plays</div>
                {/if}
              </button>
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      {/if}

      {#if sectionId === 'favoriteAlbums' && favoriteAlbums.length > 0}
        <HorizontalScrollRow title="More From Favorites">
          {#snippet children()}
            {#each favoriteAlbums as album}
              <AlbumCard
                artwork={album.artwork}
                title={album.title}
                artist={album.artist}
                quality={album.quality}
                onclick={() => onAlbumClick?.(album.id)}
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
      <h1>Start Listening</h1>
      <p>Play music or add favorites to activate recommendations.</p>
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
    left: var(--sidebar-width, 280px);
    right: 0;
    bottom: calc(var(--player-bar-height, 104px));
    z-index: 10;
    background: var(--bg-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: opacity 300ms ease-out;
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
    transition: transform 150ms ease, border-color 150ms ease, background-color 150ms ease;
  }

  .artist-card:hover {
    transform: translateY(-2px);
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
