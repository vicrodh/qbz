/**
 * Home Data Cache Store
 *
 * Module-level in-memory cache for HomeView data.
 * Prevents re-fetching all 11 sections when navigating back to Home.
 * Same pattern as searchState.ts — plain module state, no Svelte stores.
 */

import type { DisplayTrack, DiscoverPlaylist, DiscoverAlbum, PlaylistTag } from '$lib/types';

export interface AlbumCardData {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  artistId?: number;
  genre: string;
  quality?: string;
  releaseDate?: string;
}

export interface ArtistCardData {
  id: number;
  name: string;
  image?: string;
  playCount?: number;
}

export interface HomeCacheData {
  // Featured (Qobuz editorial)
  newReleases: AlbumCardData[];
  pressAwards: AlbumCardData[];
  mostStreamed: AlbumCardData[];
  qobuzissimes: AlbumCardData[];
  editorPicks: AlbumCardData[];

  // User-specific (ML)
  recentAlbums: AlbumCardData[];
  continueTracks: DisplayTrack[];
  topArtists: ArtistCardData[];
  favoriteAlbums: AlbumCardData[];

  // Discover
  qobuzPlaylists: DiscoverPlaylist[];
  essentialDiscography: DiscoverAlbum[];
  playlistTags: PlaylistTag[];

  // Metadata
  timestamp: number;
  genreIds: number[]; // snapshot of genre filter at cache time
  scrollTop: number;
}

const CACHE_TTL_MS = 5 * 60 * 1000; // 5 minutes

let cache: HomeCacheData | null = null;

export function getHomeCache(): HomeCacheData | null {
  return cache;
}

export function setHomeCache(data: Omit<HomeCacheData, 'timestamp' | 'scrollTop'> & { genreIds: number[] }): void {
  cache = {
    ...data,
    timestamp: Date.now(),
    scrollTop: cache?.scrollTop ?? 0,
  };
}

export function clearHomeCache(): void {
  cache = null;
}

export function isHomeCacheValid(currentGenreIds: number[]): boolean {
  if (!cache) return false;

  // Check TTL
  if (Date.now() - cache.timestamp > CACHE_TTL_MS) {
    cache = null;
    return false;
  }

  // Check genre filter match
  const cachedSorted = [...cache.genreIds].sort((a, b) => a - b);
  const currentSorted = [...currentGenreIds].sort((a, b) => a - b);
  if (cachedSorted.length !== currentSorted.length) return false;
  for (let i = 0; i < cachedSorted.length; i++) {
    if (cachedSorted[i] !== currentSorted[i]) return false;
  }

  return true;
}

export function updateHomeCacheScrollTop(scrollTop: number): void {
  if (cache) {
    cache.scrollTop = scrollTop;
  }
}
