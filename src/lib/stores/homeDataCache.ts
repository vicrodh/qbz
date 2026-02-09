/**
 * Home Data Cache Store (SWR)
 *
 * Module-level in-memory cache for HomeView data with stale-while-revalidate.
 * - fresh (< 5 min): Use cache, no revalidation
 * - stale (5-60 min): Show cache immediately, revalidate in background
 * - empty (no data / >60 min / genre mismatch): Full skeleton load
 *
 * Persisted to sessionStorage for cross-navigation survival.
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

export type HomeCacheStatus = 'fresh' | 'stale' | 'empty';

const FRESH_TTL_MS = 5 * 60 * 1000;   // 5 minutes
const MAX_TTL_MS = 60 * 60 * 1000;     // 60 minutes
const SESSION_KEY = 'qbz-home-cache';

let cache: HomeCacheData | null = null;

// Restore from sessionStorage on module load
function restoreFromSession(): void {
  try {
    const stored = sessionStorage.getItem(SESSION_KEY);
    if (stored) {
      cache = JSON.parse(stored);
    }
  } catch {
    // sessionStorage not available or corrupted
  }
}

function persistToSession(): void {
  if (!cache) {
    try { sessionStorage.removeItem(SESSION_KEY); } catch {}
    return;
  }
  try {
    sessionStorage.setItem(SESSION_KEY, JSON.stringify(cache));
  } catch {
    // sessionStorage full or not available
  }
}

// Initialize from sessionStorage
restoreFromSession();

function genreIdsMatch(cachedIds: number[], currentIds: number[]): boolean {
  const cachedSorted = [...cachedIds].sort((a, b) => a - b);
  const currentSorted = [...currentIds].sort((a, b) => a - b);
  if (cachedSorted.length !== currentSorted.length) return false;
  for (let i = 0; i < cachedSorted.length; i++) {
    if (cachedSorted[i] !== currentSorted[i]) return false;
  }
  return true;
}

export function getHomeCacheStatus(currentGenreIds: number[]): HomeCacheStatus {
  if (!cache) return 'empty';

  // Genre filter mismatch = empty (data is for a different genre)
  if (!genreIdsMatch(cache.genreIds, currentGenreIds)) return 'empty';

  const age = Date.now() - cache.timestamp;

  // Too old = empty
  if (age > MAX_TTL_MS) return 'empty';

  // Fresh = use as-is
  if (age <= FRESH_TTL_MS) return 'fresh';

  // In between = stale (show cache, revalidate)
  return 'stale';
}

export function getHomeCache(): HomeCacheData | null {
  return cache;
}

export function setHomeCache(data: Omit<HomeCacheData, 'timestamp' | 'scrollTop'> & { genreIds: number[] }): void {
  cache = {
    ...data,
    timestamp: Date.now(),
    scrollTop: cache?.scrollTop ?? 0,
  };
  persistToSession();
}

export function clearHomeCache(): void {
  cache = null;
  persistToSession();
}

export function updateHomeCacheScrollTop(scrollTop: number): void {
  if (cache) {
    cache.scrollTop = scrollTop;
    // Don't persist on every scroll - too frequent
  }
}
