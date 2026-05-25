/**
 * Discovery V2 — data layer.
 *
 * Pure functions wrapping the V2 invoke surface. Each function returns the
 * minimum shape the corresponding `*Lite` card component needs to render.
 * Mapping happens here so individual sections don't carry the full Qobuz
 * payload around (most fields are unused by Discovery).
 */

import { invoke } from '@tauri-apps/api/core';
import type {
  QobuzAlbum,
  DiscoverResponse,
  DiscoverAlbum,
  DiscoverPlaylist,
} from '$lib/types';
import { getQobuzImageForSize, formatQuality } from '$lib/adapters/qobuzAdapters';

// Wire format from src-tauri/src/api/models.rs `SearchResultsPage<T>`.
interface SearchResultsPage<T> {
  items: T[];
  total: number;
  offset: number;
  limit: number;
}

/**
 * Minimum shape an AlbumCardLite needs. Intentionally narrower than
 * `AlbumCardData` in HomeView.svelte (which carries quality, samplingRate,
 * bitDepth, awards, parental_warning, etc. that V1 of Discovery doesn't
 * render).
 */
export type AlbumRibbonKind = 'albumOfTheWeek' | 'qobuzissime' | 'press';

export interface AlbumRibbon {
  kind: AlbumRibbonKind;
  label: string;
}

export interface DiscoveryAlbumCard {
  albumId: string;
  title: string;
  artist: string;
  artistId?: number;
  artwork?: string;
  quality?: string;
  /** True when the album is > 16-bit. Drives the title-row badge: HiRes
   *  shows the Qobuz brand logo bare; non-HiRes shows the format icon in
   *  a small framed square of matching dimensions. */
  isHiRes?: boolean;
  /** Exact stored bit depth, fed to QualityBadgeStatic so the hover
   *  tooltip ("Hi-Res: 24-bit / 96 kHz") matches reality instead of
   *  falling back to the parser's defaults from the quality string. */
  bitDepth?: number;
  samplingRate?: number;
  ribbon?: AlbumRibbon;
  genre?: string;
  releaseYear?: number;
  /** Full release date (e.g. "2025-11-06"). Drives the AlbumCardLite hover
   *  overlay's "MMM D, YYYY" label (#469); releaseYear stays as fallback. */
  releaseDate?: string;
}

function parseYear(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const m = value.match(/^(\d{4})/);
  return m ? parseInt(m[1], 10) : undefined;
}

/**
 * Map a Qobuz `awards` array onto a single ribbon. An album can carry
 * multiple awards simultaneously (e.g. "Album of the Week" alongside a
 * press accolade). We pick by priority rather than by array position:
 *
 *   1. Album of the Week  (id '151') — Qobuz's flagship editorial pick
 *   2. Qobuzissime        (id '88')  — secondary editorial accolade
 *   3. Press              (any other award) — third-party press awards
 *
 * The original HomeView simply took the last award in the array, which
 * meant cards in New Releases / Press Accolades sections often hid an
 * Album-of-the-Week badge under whatever press accolade Qobuz returned
 * later in the same array. This explicit priority pick fixes that.
 */
function pickAlbumRibbon(
  awards: { id?: string | number; name: string }[] | undefined
): AlbumRibbon | undefined {
  if (!awards || awards.length === 0) return undefined;
  const aotw = awards.find((a) => String(a.id ?? '') === '151');
  if (aotw) return { kind: 'albumOfTheWeek', label: aotw.name };
  const qobuzissime = awards.find((a) => String(a.id ?? '') === '88');
  if (qobuzissime) return { kind: 'qobuzissime', label: qobuzissime.name };
  const lastPress = awards[awards.length - 1];
  return { kind: 'press', label: lastPress.name };
}

function qobuzAlbumToCard(album: QobuzAlbum): DiscoveryAlbumCard {
  const hires = (album.maximum_bit_depth ?? 16) > 16;
  return {
    albumId: album.id,
    title: album.title,
    artist: album.artist.name,
    artistId: album.artist.id,
    artwork: getQobuzImageForSize(album.image, 'small'),
    quality: formatQuality(hires, album.maximum_bit_depth, album.maximum_sampling_rate),
    isHiRes: hires,
    bitDepth: album.maximum_bit_depth,
    samplingRate: album.maximum_sampling_rate,
    ribbon: pickAlbumRibbon(album.awards),
    genre: album.genre?.name,
    releaseYear: parseYear(album.release_date_original),
    releaseDate: album.release_date_original,
  };
}

function discoverAlbumToCard(album: DiscoverAlbum): DiscoveryAlbumCard {
  const hires = (album.audio_info?.maximum_bit_depth ?? 16) > 16;
  return {
    albumId: album.id,
    title: album.title,
    artist: album.artists?.[0]?.name ?? 'Unknown Artist',
    artistId: album.artists?.[0]?.id,
    artwork: album.image?.small || album.image?.large || album.image?.thumbnail,
    quality: formatQuality(
      hires,
      album.audio_info?.maximum_bit_depth,
      album.audio_info?.maximum_sampling_rate
    ),
    isHiRes: hires,
    bitDepth: album.audio_info?.maximum_bit_depth,
    samplingRate: album.audio_info?.maximum_sampling_rate,
    ribbon: pickAlbumRibbon(album.awards),
    genre: album.genre?.name,
    releaseYear: parseYear(album.dates?.original),
    releaseDate: album.dates?.original,
  };
}

/**
 * Fetch the "Release Watch" feed — releases from followed artists. The V2
 * command returns full Album objects in a single round-trip; the original
 * HomeView called `v2_get_album` per id on top of release-watch (N+1),
 * which Discovery sidesteps.
 */
export async function fetchReleaseWatch(limit = 8): Promise<DiscoveryAlbumCard[]> {
  try {
    const page = await invoke<SearchResultsPage<QobuzAlbum>>('v2_get_release_watch', {
      releaseType: 'artists',
      limit,
      offset: 0,
    });
    return page.items.map(qobuzAlbumToCard);
  } catch (err) {
    console.error('[discovery-v2] fetchReleaseWatch failed', err);
    return [];
  }
}

export interface DiscoveryPlaylistCard {
  playlistId: number;
  name: string;
  image?: string;
}

function discoverPlaylistToCard(playlist: DiscoverPlaylist): DiscoveryPlaylistCard {
  return {
    playlistId: playlist.id,
    name: playlist.name,
    image: playlist.image?.rectangle || playlist.image?.covers?.[0],
  };
}

/**
 * Editorial album sections — one round-trip returns five containers
 * (new releases, press accolades, most streamed, qobuzissimes, album of
 * the week) plus playlists. Discovery splits the result into the shape
 * each section needs.
 */
export interface DiscoverIndexSections {
  newReleases: DiscoveryAlbumCard[];
  pressAwards: DiscoveryAlbumCard[];
  mostStreamed: DiscoveryAlbumCard[];
  qobuzissimes: DiscoveryAlbumCard[];
  editorPicks: DiscoveryAlbumCard[];
  idealDiscography: DiscoveryAlbumCard[];
  playlists: DiscoveryPlaylistCard[];
}

export async function fetchDiscoverIndex(
  perSection = 8,
  genreIds: number[] = []
): Promise<DiscoverIndexSections> {
  const empty: DiscoverIndexSections = {
    newReleases: [],
    pressAwards: [],
    mostStreamed: [],
    qobuzissimes: [],
    editorPicks: [],
    idealDiscography: [],
    playlists: [],
  };
  try {
    const apiGenreIds = genreIds.length > 0 ? genreIds : null;
    const response = await invoke<DiscoverResponse>('v2_get_discover_index', {
      genreIds: apiGenreIds,
    });
    const c = response.containers;
    const takeAlbums = (items: DiscoverAlbum[] | undefined): DiscoveryAlbumCard[] =>
      (items ?? []).slice(0, perSection).map(discoverAlbumToCard);
    const takePlaylists = (items: DiscoverPlaylist[] | undefined): DiscoveryPlaylistCard[] =>
      (items ?? []).slice(0, perSection).map(discoverPlaylistToCard);

    return {
      newReleases: takeAlbums(c.new_releases?.data?.items),
      pressAwards: takeAlbums(c.press_awards?.data?.items),
      mostStreamed: takeAlbums(c.most_streamed?.data?.items),
      qobuzissimes: takeAlbums(c.qobuzissims?.data?.items),
      editorPicks: takeAlbums(c.album_of_the_week?.data?.items),
      idealDiscography: takeAlbums(c.ideal_discography?.data?.items),
      playlists: takePlaylists(c.playlists?.data?.items),
    };
  } catch (err) {
    console.error('[discovery-v2] fetchDiscoverIndex failed', err);
    return empty;
  }
}

/**
 * Personalized home sections — recently played, continue listening,
 * top artists, favorite albums. One round-trip; the V2 command returns
 * already-resolved minimal metadata shapes (`AlbumCardMeta`,
 * `TrackDisplayMeta`, `ArtistCardMeta`) so no additional invokes are
 * needed.
 */
export interface DiscoveryTrackCard {
  trackId: number;
  title: string;
  artist: string;
  album: string;
  albumId?: string;
  artistId?: number;
  artwork?: string;
  duration: string;
  durationSeconds: number;
  hires: boolean;
  bitDepth?: number;
  samplingRate?: number;
  isrc?: string;
}

export interface DiscoveryArtistTile {
  artistId: number;
  name: string;
  image?: string;
}

export interface HomeResolvedSections {
  recentlyPlayedAlbums: DiscoveryAlbumCard[];
  continueListening: DiscoveryTrackCard[];
  topArtists: DiscoveryArtistTile[];
  favoriteAlbums: DiscoveryAlbumCard[];
}

// Backend-resolved shapes (camelCase per Rust `serde(rename_all)`).
interface RecoAlbumCardMeta {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  artistId?: number;
  quality?: string;
}

interface RecoTrackDisplayMeta {
  id: number;
  title: string;
  artist: string;
  album: string;
  albumArt: string;
  albumId?: string;
  artistId?: number;
  duration: string;
  durationSeconds: number;
  hires: boolean;
  bitDepth?: number;
  samplingRate?: number;
  isrc?: string;
}

interface RecoArtistCardMeta {
  id: number;
  name: string;
  image?: string;
}

interface HomeResolvedWire {
  recentlyPlayedAlbums: RecoAlbumCardMeta[];
  continueListeningTracks: RecoTrackDisplayMeta[];
  topArtists: RecoArtistCardMeta[];
  favoriteAlbums: RecoAlbumCardMeta[];
}

// ============ For-You-exclusive sections ============

/**
 * "Rediscover your library" — albums the user favorited but hasn't played
 * recently. Backend already returns pre-formatted strings (artwork URL,
 * quality string, etc.), so the mapper is a thin pass-through that
 * derives `isHiRes` from the quality label.
 */
interface ForgottenAlbumWire {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  artistId?: number;
  genre: string;
  quality: string;
  releaseDate?: string;
}

export async function fetchRediscoverLibrary(
  limit = 12,
  recencyDays = 30
): Promise<DiscoveryAlbumCard[]> {
  try {
    const albums = await invoke<ForgottenAlbumWire[]>('v2_reco_get_forgotten_favorites', {
      limit,
      recencyDays,
    });
    return albums.map((a) => ({
      albumId: a.id,
      title: a.title,
      artist: a.artist,
      artistId: a.artistId,
      artwork: a.artwork || undefined,
      quality: a.quality || undefined,
      isHiRes: !!a.quality && a.quality !== 'CD Quality',
      genre: a.genre || undefined,
      releaseYear: parseYear(a.releaseDate),
      releaseDate: a.releaseDate,
    }));
  } catch (err) {
    console.error('[discovery-v2] fetchRediscoverLibrary failed', err);
    return [];
  }
}

/**
 * "Artists to follow" — for each of the user's top artists, fetch similar
 * artists from Qobuz, then filter out already-followed and the seeds
 * themselves. Limit to 10 deduped suggestions.
 */
interface QobuzArtistWire {
  id: number;
  name: string;
  image?: { small?: string; large?: string; thumbnail?: string };
}

interface SimilarArtistsPageWire {
  items: QobuzArtistWire[];
}

export async function fetchArtistsToFollow(
  topArtistIds: number[],
  limit = 10
): Promise<DiscoveryArtistTile[]> {
  if (topArtistIds.length === 0) return [];
  try {
    const favoriteIds = new Set(
      await invoke<number[]>('v2_get_cached_favorite_artists')
    );
    const seedIds = new Set(topArtistIds);
    const seeds = topArtistIds.slice(0, 3);

    const results = await Promise.allSettled(
      seeds.map((id) =>
        invoke<SimilarArtistsPageWire>('v2_get_similar_artists', {
          artistId: id,
          limit: 6,
          offset: 0,
        })
      )
    );

    const seen = new Set<number>();
    const out: DiscoveryArtistTile[] = [];
    for (const r of results) {
      if (r.status !== 'fulfilled') continue;
      for (const a of r.value.items) {
        if (seen.has(a.id) || favoriteIds.has(a.id) || seedIds.has(a.id)) continue;
        seen.add(a.id);
        out.push({
          artistId: a.id,
          name: a.name,
          image: a.image?.small || a.image?.large || a.image?.thumbnail,
        });
        if (out.length >= limit) return out;
      }
    }
    return out;
  } catch (err) {
    console.error('[discovery-v2] fetchArtistsToFollow failed', err);
    return [];
  }
}

/**
 * "Similar to X" — seeded by a recent album, fetch Qobuz's album
 * suggestions. Returns the seed title alongside the cards so the section
 * header can render "Similar to {seed}".
 */
interface AlbumSuggestResultWire {
  id: string;
  title: string;
  artist: { id: number; name: string };
  image: { small?: string; large?: string; thumbnail?: string };
  hires: boolean;
  maximum_bit_depth?: number;
  maximum_sampling_rate?: number;
  genre?: { name: string };
  release_date_original?: string;
}

export interface SimilarAlbumsSection {
  seedTitle: string;
  albums: DiscoveryAlbumCard[];
}

export async function fetchSimilarAlbums(
  recentAlbums: DiscoveryAlbumCard[],
  limit = 10
): Promise<SimilarAlbumsSection> {
  if (recentAlbums.length === 0) return { seedTitle: '', albums: [] };
  try {
    const seedIdx = Math.floor(Math.random() * Math.min(recentAlbums.length, 5));
    const seed = recentAlbums[seedIdx];
    const albums = await invoke<AlbumSuggestResultWire[]>('v2_get_album_suggestions', {
      albumId: seed.albumId,
      limit,
    });
    return {
      seedTitle: seed.title,
      albums: albums.map((a) => ({
        albumId: a.id,
        title: a.title,
        artist: a.artist?.name ?? 'Unknown Artist',
        artistId: a.artist?.id,
        artwork: a.image?.small || a.image?.large || a.image?.thumbnail,
        quality: formatQuality(a.hires, a.maximum_bit_depth, a.maximum_sampling_rate),
        isHiRes: a.hires,
        bitDepth: a.maximum_bit_depth,
        samplingRate: a.maximum_sampling_rate,
        genre: a.genre?.name,
        releaseYear: parseYear(a.release_date_original),
        releaseDate: a.release_date_original,
      })),
    };
  } catch (err) {
    console.error('[discovery-v2] fetchSimilarAlbums failed', err);
    return { seedTitle: '', albums: [] };
  }
}

/**
 * "Essentials in {genre}" — pull the user's top genre, then fetch the
 * `ideal-discography` featured-albums set for it.
 */
interface TopGenreWire {
  id: number;
  name: string;
}

export interface EssentialsSection {
  genreName: string;
  albums: DiscoveryAlbumCard[];
}

export async function fetchEssentialsByGenre(
  limit = 12
): Promise<EssentialsSection> {
  try {
    const genres = await invoke<TopGenreWire[]>('v2_reco_get_top_genres', { limit: 3 });
    if (genres.length === 0) return { genreName: '', albums: [] };
    const top = genres[0];
    const resp = await invoke<{ items: AlbumSuggestResultWire[]; total: number }>(
      'v2_get_featured_albums',
      {
        featuredType: 'ideal-discography',
        limit,
        offset: 0,
        genreId: top.id,
      }
    );
    return {
      genreName: top.name,
      albums: (resp.items || []).map((a) => ({
        albumId: a.id,
        title: a.title,
        artist: a.artist?.name ?? 'Unknown Artist',
        artistId: a.artist?.id,
        artwork: a.image?.small || a.image?.large || a.image?.thumbnail,
        quality: formatQuality(a.hires, a.maximum_bit_depth, a.maximum_sampling_rate),
        isHiRes: a.hires,
        bitDepth: a.maximum_bit_depth,
        samplingRate: a.maximum_sampling_rate,
        genre: a.genre?.name,
        releaseYear: parseYear(a.release_date_original),
        releaseDate: a.release_date_original,
      })),
    };
  } catch (err) {
    console.error('[discovery-v2] fetchEssentialsByGenre failed', err);
    return { genreName: '', albums: [] };
  }
}

/**
 * Artist spotlight — a featured-artist hero section. Picks a random
 * seed from the user's top-5 artists, fetches their artist page,
 * extracts a portrait + top tracks + up to 6 albums + playlists.
 */
interface PageArtistResponseWire {
  id: number;
  name: { display: string };
  images?: { portrait?: { hash: string; format: string } };
  artist_category?: string;
  top_tracks?: PageArtistTrackWire[];
  releases?: Array<{
    type: string;
    items: Array<{
      id: string;
      title: string;
      image?: { small?: string; large?: string; thumbnail?: string };
      artist?: { id?: number; name?: { display?: string } };
      genre?: { name: string };
      audio_info?: { maximum_bit_depth?: number; maximum_sampling_rate?: number };
      dates?: { original?: string };
    }>;
  }>;
  playlists?: {
    items: Array<{
      id: number;
      title?: string;
      images?: { rectangle?: string[] };
      tracks_count?: number;
    }>;
  };
}

interface PageArtistTrackWire {
  id: number;
  title: string;
  duration?: number;
  album?: { id?: string; title?: string; image?: { small?: string } };
}

export interface SpotlightTopTrack {
  trackId: number;
  title: string;
  durationSec?: number;
  albumId?: string;
  albumTitle?: string;
  artwork?: string;
}

export interface SpotlightSection {
  artistId: number;
  artistName: string;
  artistImage?: string;
  category?: string;
  topTracks: SpotlightTopTrack[];
  albums: DiscoveryAlbumCard[];
  playlists: DiscoveryPlaylistCard[];
}

export async function fetchArtistSpotlight(
  topArtists: DiscoveryArtistTile[]
): Promise<SpotlightSection | null> {
  if (topArtists.length === 0) return null;
  try {
    const idx = Math.floor(Math.random() * Math.min(topArtists.length, 5));
    const seed = topArtists[idx];
    const resp = await invoke<PageArtistResponseWire>('v2_get_artist_page', {
      artistId: seed.artistId,
    });

    let artistImage: string | undefined;
    if (resp.images?.portrait) {
      const { hash, format } = resp.images.portrait;
      artistImage = `https://static.qobuz.com/images/artists/covers/medium/${hash}.${format}`;
    }

    // Extract up to 6 albums by release type, preferring full albums.
    const seenAlbumIds = new Set<string>();
    const albums: DiscoveryAlbumCard[] = [];
    const releaseTypes = ['album', 'live', 'ep-single', 'compilation'];
    for (const releaseType of releaseTypes) {
      const group = (resp.releases || []).find((g) => g.type === releaseType);
      if (!group) continue;
      for (const rel of group.items) {
        if (seenAlbumIds.has(rel.id)) continue;
        seenAlbumIds.add(rel.id);
        const hires = (rel.audio_info?.maximum_bit_depth ?? 16) > 16;
        albums.push({
          albumId: rel.id,
          title: rel.title,
          artist: rel.artist?.name?.display ?? resp.name.display,
          artistId: rel.artist?.id ?? resp.id,
          artwork: rel.image?.small || rel.image?.large || rel.image?.thumbnail,
          quality: formatQuality(
            hires,
            rel.audio_info?.maximum_bit_depth,
            rel.audio_info?.maximum_sampling_rate
          ),
          isHiRes: hires,
          bitDepth: rel.audio_info?.maximum_bit_depth,
          samplingRate: rel.audio_info?.maximum_sampling_rate,
          genre: rel.genre?.name,
          releaseYear: parseYear(rel.dates?.original),
        });
        if (albums.length >= 6) break;
      }
      if (albums.length >= 6) break;
    }

    const playlists: DiscoveryPlaylistCard[] = (resp.playlists?.items || []).map((pl) => ({
      playlistId: pl.id,
      name: pl.title || '',
      image: pl.images?.rectangle?.[0],
    }));

    const topTracks: SpotlightTopTrack[] = (resp.top_tracks || []).slice(0, 5).map((track) => ({
      trackId: track.id,
      title: track.title,
      durationSec: track.duration,
      albumId: track.album?.id,
      albumTitle: track.album?.title,
      artwork: track.album?.image?.small,
    }));

    return {
      artistId: resp.id,
      artistName: resp.name.display,
      artistImage,
      category: resp.artist_category,
      topTracks,
      albums,
      playlists,
    };
  } catch (err) {
    console.error('[discovery-v2] fetchArtistSpotlight failed', err);
    return null;
  }
}

// ============ Existing home-resolved fetch ============

export async function fetchHomeResolved(
  perSection = 8
): Promise<HomeResolvedSections> {
  const empty: HomeResolvedSections = {
    recentlyPlayedAlbums: [],
    continueListening: [],
    topArtists: [],
    favoriteAlbums: [],
  };
  try {
    const resp = await invoke<HomeResolvedWire>('v2_reco_get_home_resolved', {
      limitRecentAlbums: perSection,
      limitContinueTracks: perSection,
      limitTopArtists: perSection,
      limitFavorites: perSection,
    });
    return {
      recentlyPlayedAlbums: resp.recentlyPlayedAlbums.slice(0, perSection).map((a) => ({
        albumId: a.id,
        title: a.title,
        artist: a.artist,
        artistId: a.artistId,
        artwork: a.artwork || undefined,
        quality: a.quality || undefined,
        // Backend home-resolved returns a pre-formatted string; "CD Quality"
        // is the only non-Hi-Res variant produced by formatQuality(), so
        // anything else implies > 16-bit. Keeps the i18n contract simple
        // without round-tripping the bit depth.
        isHiRes: !!a.quality && a.quality !== 'CD Quality',
      })),
      continueListening: resp.continueListeningTracks.slice(0, perSection).map((track) => ({
        trackId: track.id,
        title: track.title,
        artist: track.artist,
        album: track.album,
        albumId: track.albumId,
        artistId: track.artistId,
        artwork: track.albumArt || undefined,
        duration: track.duration,
        durationSeconds: track.durationSeconds,
        hires: track.hires,
        bitDepth: track.bitDepth,
        samplingRate: track.samplingRate,
        isrc: track.isrc,
      })),
      topArtists: resp.topArtists.slice(0, perSection).map((a) => ({
        artistId: a.id,
        name: a.name,
        image: a.image || undefined,
      })),
      favoriteAlbums: resp.favoriteAlbums.slice(0, perSection).map((a) => ({
        albumId: a.id,
        title: a.title,
        artist: a.artist,
        artistId: a.artistId,
        artwork: a.artwork || undefined,
        quality: a.quality || undefined,
        isHiRes: !!a.quality && a.quality !== 'CD Quality',
      })),
    };
  } catch (err) {
    console.error('[discovery-v2] fetchHomeResolved failed', err);
    return empty;
  }
}
