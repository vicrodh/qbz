<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Music, User } from 'lucide-svelte';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import { formatDuration, formatQuality, getQobuzImage } from '$lib/adapters/qobuzAdapters';
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
    onAlbumClick?: (albumId: string) => void;
    onArtistClick?: (artistId: number) => void;
    onTrackPlay?: (track: DisplayTrack) => void;
  }

  let { onAlbumClick, onArtistClick, onTrackPlay }: Props = $props();

  const LIMITS = {
    recentAlbums: 12,
    continueTracks: 8,
    topArtists: 10,
    favoriteAlbums: 12,
    favoriteTracks: 10
  };

  let isLoading = $state(true);
  let error = $state<string | null>(null);

  let recentAlbums = $state<AlbumCardData[]>([]);
  let continueTracks = $state<DisplayTrack[]>([]);
  let topArtists = $state<ArtistCardData[]>([]);
  let favoriteAlbums = $state<AlbumCardData[]>([]);

  let failedArtistImages = $state<Set<number>>(new Set());

  const hasContent = $derived(
    recentAlbums.length > 0
    || continueTracks.length > 0
    || topArtists.length > 0
    || favoriteAlbums.length > 0
  );

  onMount(() => {
    loadHome();
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
    isLoading = true;
    error = null;

    try {
      const seeds = await invoke<HomeSeeds>('reco_get_home', {
        limitRecentAlbums: LIMITS.recentAlbums,
        limitContinueTracks: LIMITS.continueTracks,
        limitTopArtists: LIMITS.topArtists,
        limitFavorites: Math.max(LIMITS.favoriteAlbums, LIMITS.favoriteTracks)
      });

      const fetchedContinueTracks = await fetchTracks(seeds.continueListeningTrackIds);

      let recentAlbumIds = normalizeAlbumIds(seeds.recentlyPlayedAlbumIds);
      if (recentAlbumIds.length === 0) {
        recentAlbumIds = normalizeAlbumIds(fetchedContinueTracks.map(track => track.albumId));
      }

      const favoriteTrackDetails = await fetchTracks(seeds.favoriteTrackIds.slice(0, LIMITS.favoriteTracks));
      const favoriteAlbumIds = normalizeAlbumIds([
        ...seeds.favoriteAlbumIds,
        ...favoriteTrackDetails.map(track => track.albumId)
      ]);

      let artistSeeds = seeds.topArtistIds;
      if (artistSeeds.length === 0) {
        artistSeeds = buildTopArtistSeedsFromTracks(fetchedContinueTracks);
      }

      const [recentAlbumCards, favoriteAlbumCards, artistCards] = await Promise.all([
        fetchAlbums(recentAlbumIds.slice(0, LIMITS.recentAlbums)),
        fetchAlbums(favoriteAlbumIds.slice(0, LIMITS.favoriteAlbums)),
        fetchArtists(artistSeeds.slice(0, LIMITS.topArtists))
      ]);

      recentAlbums = recentAlbumCards;
      continueTracks = fetchedContinueTracks;
      favoriteAlbums = favoriteAlbumCards;
      topArtists = artistCards;
    } catch (err) {
      console.error('Failed to load home data:', err);
      error = String(err);
    } finally {
      isLoading = false;
    }
  }
</script>

<div class="home-view">
  {#if isLoading}
    <div class="home-state">
      <div class="state-icon">
        <Music size={36} />
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
    {#if recentAlbums.length > 0}
      <HorizontalScrollRow title="Escuchado recientemente">
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

    {#if continueTracks.length > 0}
      <div class="section">
        <div class="section-header">
          <h2>Continuar escuchando</h2>
        </div>
        <div class="track-list">
          {#each continueTracks as track, index}
            <TrackRow
              number={index + 1}
              title={track.title}
              artist={track.artist}
              duration={track.duration}
              quality={getTrackQuality(track)}
              hideDownload={true}
              onPlay={() => onTrackPlay?.(track)}
            />
          {/each}
        </div>
      </div>
    {/if}

    {#if topArtists.length > 0}
      <HorizontalScrollRow title="Tus artistas mas escuchados">
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
                <div class="artist-meta">{artist.playCount} reproducciones</div>
              {/if}
            </button>
          {/each}
          <div class="spacer"></div>
        {/snippet}
      </HorizontalScrollRow>
    {/if}

    {#if favoriteAlbums.length > 0}
      <HorizontalScrollRow title="Mas de tus favoritos">
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
  {:else}
    <div class="home-state">
      <div class="state-icon">
        <Music size={48} />
      </div>
      <h1>Empieza a escuchar</h1>
      <p>Reproduce musica o anade favoritos para activar recomendaciones.</p>
    </div>
  {/if}
</div>

<style>
  .home-view {
    width: 100%;
    padding-bottom: 24px;
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
</style>
