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
import { getQobuzImageForSize } from '$lib/adapters/qobuzAdapters';

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
export interface DiscoveryAlbumCard {
  albumId: string;
  title: string;
  artist: string;
  artistId?: number;
  artwork?: string;
}

function qobuzAlbumToCard(album: QobuzAlbum): DiscoveryAlbumCard {
  return {
    albumId: album.id,
    title: album.title,
    artist: album.artist.name,
    artistId: album.artist.id,
    artwork: getQobuzImageForSize(album.image, 'small'),
  };
}

function discoverAlbumToCard(album: DiscoverAlbum): DiscoveryAlbumCard {
  return {
    albumId: album.id,
    title: album.title,
    artist: album.artists?.[0]?.name ?? 'Unknown Artist',
    artistId: album.artists?.[0]?.id,
    artwork: album.image?.small || album.image?.large || album.image?.thumbnail,
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
}

interface RecoTrackDisplayMeta {
  id: number;
  title: string;
  artist: string;
  album: string;
  albumArt: string;
  albumId?: string;
  artistId?: number;
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
      })),
      continueListening: resp.continueListeningTracks.slice(0, perSection).map((track) => ({
        trackId: track.id,
        title: track.title,
        artist: track.artist,
        album: track.album,
        albumId: track.albumId,
        artistId: track.artistId,
        artwork: track.albumArt || undefined,
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
      })),
    };
  } catch (err) {
    console.error('[discovery-v2] fetchHomeResolved failed', err);
    return empty;
  }
}
