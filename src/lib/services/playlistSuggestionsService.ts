/**
 * Playlist Suggestions Service (v2)
 *
 * Uses vector-based artist similarity to suggest tracks for playlists.
 * Combines MusicBrainz relationships and Qobuz similar artists.
 */

import { invoke } from '@tauri-apps/api/core';

// ============ Types ============

export interface SuggestionConfig {
  max_artists?: number;
  tracks_per_artist?: number;
  max_pool_size?: number;
  vector_max_age_days?: number;
  min_similarity?: number;
  /** Skip building vectors - only use existing cached vectors (faster but may have fewer results) */
  skip_vector_build?: boolean;
}

export interface SuggestedTrack {
  track_id: number;
  title: string;
  artist_name: string;
  /** Qobuz artist ID for navigation */
  artist_id?: number;
  artist_mbid?: string;
  album_title: string;
  album_id: string;
  /** Direct URL to album cover image */
  album_image_url?: string;
  duration: number;
  similarity_score: number;
  reason?: string;
}

export interface SuggestionResult {
  tracks: SuggestedTrack[];
  source_artists: string[];
  playlist_artists_count: number;
  similar_artists_count: number;
}

export interface VectorStoreStats {
  artist_count: number;
  vector_count: number;
  total_entries: number;
  db_size_bytes: number;
}

// ============ Local Storage for Dismissed Tracks ============

const DISMISSED_STORAGE_PREFIX = 'playlist_suggestions_dismissed_';

/**
 * Get dismissed track IDs for a playlist
 */
export function getDismissedTrackIds(playlistId: number): Set<number> {
  try {
    const stored = localStorage.getItem(`${DISMISSED_STORAGE_PREFIX}${playlistId}`);
    if (stored) {
      return new Set(JSON.parse(stored) as number[]);
    }
  } catch {
    // Ignore parse errors
  }
  return new Set();
}

/**
 * Add a track to the dismissed set for a playlist
 */
export function dismissTrack(playlistId: number, trackId: number): void {
  const dismissed = getDismissedTrackIds(playlistId);
  dismissed.add(trackId);
  localStorage.setItem(
    `${DISMISSED_STORAGE_PREFIX}${playlistId}`,
    JSON.stringify([...dismissed])
  );
}

/**
 * Clear dismissed tracks for a playlist
 */
export function clearDismissedTracks(playlistId: number): void {
  localStorage.removeItem(`${DISMISSED_STORAGE_PREFIX}${playlistId}`);
}

// ============ Main API ============

export interface PlaylistArtist {
  name: string;
  qobuzId?: number;
}

/**
 * Get suggestions for a playlist based on artist similarity
 *
 * @param artists - Unique artists from playlist tracks
 * @param excludeTrackIds - Track IDs to exclude (already in playlist)
 * @param includeReasons - Whether to include explanation strings (dev mode)
 * @param config - Optional configuration overrides
 */
export async function getPlaylistSuggestionsV2(
  artists: PlaylistArtist[],
  excludeTrackIds: number[],
  includeReasons: boolean = false,
  config?: SuggestionConfig
): Promise<SuggestionResult> {
  // Filter out empty names and deduplicate
  const uniqueArtists = artists.filter(a => a.name?.trim());
  const seen = new Set<string>();
  const dedupedArtists = uniqueArtists.filter(a => {
    if (seen.has(a.name)) return false;
    seen.add(a.name);
    return true;
  });

  if (dedupedArtists.length === 0) {
    return {
      tracks: [],
      source_artists: [],
      playlist_artists_count: 0,
      similar_artists_count: 0
    };
  }

  // Call backend - MBID resolution happens server-side with caching
  const result = await invoke<SuggestionResult>('get_playlist_suggestions_v2', {
    input: {
      artists: dedupedArtists.map(a => ({
        name: a.name,
        qobuz_id: a.qobuzId ?? null
      })),
      exclude_track_ids: excludeTrackIds,
      include_reasons: includeReasons,
      config: config ?? null
    }
  });

  return result;
}

/**
 * Get vector store statistics (for debugging)
 */
export async function getVectorStoreStats(): Promise<VectorStoreStats> {
  return invoke<VectorStoreStats>('get_vector_store_stats');
}

/**
 * Clean up expired vectors from the store
 *
 * @param maxAgeDays - Remove vectors older than this (default: 30)
 * @returns Number of vectors removed
 */
export async function cleanupVectorStore(maxAgeDays?: number): Promise<number> {
  return invoke<number>('cleanup_vector_store', { maxAgeDays: maxAgeDays ?? null });
}

// ============ Helpers ============

/**
 * Calculate adaptive artist limit based on playlist size
 * Larger playlists = more seed artists for better discovery
 */
function calculateAdaptiveLimit(trackCount: number): number {
  if (trackCount < 15) return Math.max(3, Math.min(5, trackCount));
  if (trackCount < 50) return Math.min(10, Math.ceil(trackCount * 0.3));
  if (trackCount < 100) return Math.min(15, Math.ceil(trackCount * 0.2));
  return Math.min(20, Math.ceil(trackCount * 0.15));
}

/**
 * Shuffle array using Fisher-Yates algorithm
 */
function shuffleArray<T>(array: T[]): T[] {
  const result = [...array];
  for (let i = result.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [result[i], result[j]] = [result[j], result[i]];
  }
  return result;
}

/**
 * Extract artists from playlist tracks with adaptive quantity and mixed selection
 *
 * This function balances coherence (top artists by frequency) with discovery
 * (random selection from less frequent artists).
 *
 * @param tracks - Playlist tracks
 * @param options - Configuration options
 * @param options.topRatio - Ratio of top artists vs random (default: 0.6 = 60% top, 40% random)
 * @param options.forceLimit - Override adaptive limit with fixed number
 */
export function extractAdaptiveArtists(
  tracks: Array<{ artist?: string; artistId?: number }>,
  options: { topRatio?: number; forceLimit?: number } = {}
): PlaylistArtist[] {
  const { topRatio = 0.6, forceLimit } = options;

  // Count tracks per artist
  const artistCounts = new Map<string, { count: number; qobuzId?: number }>();

  for (const track of tracks) {
    if (track.artist) {
      const existing = artistCounts.get(track.artist);
      if (existing) {
        existing.count++;
      } else {
        artistCounts.set(track.artist, { count: 1, qobuzId: track.artistId });
      }
    }
  }

  const uniqueArtistCount = artistCounts.size;
  if (uniqueArtistCount === 0) return [];

  // Calculate how many artists to select
  const limit = forceLimit ?? calculateAdaptiveLimit(tracks.length);
  const actualLimit = Math.min(limit, uniqueArtistCount);

  // If we have few artists, just return all of them shuffled
  if (uniqueArtistCount <= actualLimit) {
    const all = [...artistCounts.entries()].map(([name, data]) => ({
      name,
      qobuzId: data.qobuzId
    }));
    return shuffleArray(all);
  }

  // Sort by count descending
  const sorted = [...artistCounts.entries()]
    .sort((a, b) => b[1].count - a[1].count);

  // Split into top artists and the rest
  const topCount = Math.max(1, Math.floor(actualLimit * topRatio));
  const randomCount = actualLimit - topCount;

  // Take top artists (coherence)
  const topArtists = sorted.slice(0, topCount);

  // Randomly select from remaining artists (discovery)
  const remaining = sorted.slice(topCount);
  const randomArtists = shuffleArray(remaining).slice(0, randomCount);

  // Combine and shuffle the final selection
  const combined = [...topArtists, ...randomArtists];
  const shuffled = shuffleArray(combined);

  return shuffled.map(([name, data]) => ({
    name,
    qobuzId: data.qobuzId
  }));
}

/**
 * Extract top artists from playlist tracks, sorted by frequency (track count)
 * @deprecated Use extractAdaptiveArtists for better discovery
 *
 * @param tracks - Playlist tracks
 * @param limit - Maximum number of artists to return
 */
export function extractTopArtists(
  tracks: Array<{ artist?: string; artistId?: number }>,
  limit: number = 30
): PlaylistArtist[] {
  // Count tracks per artist
  const artistCounts = new Map<string, { count: number; qobuzId?: number }>();

  for (const track of tracks) {
    if (track.artist) {
      const existing = artistCounts.get(track.artist);
      if (existing) {
        existing.count++;
      } else {
        artistCounts.set(track.artist, { count: 1, qobuzId: track.artistId });
      }
    }
  }

  // Sort by count descending, take top N
  const sorted = [...artistCounts.entries()]
    .sort((a, b) => b[1].count - a[1].count)
    .slice(0, limit);

  return sorted.map(([name, data]) => ({
    name,
    qobuzId: data.qobuzId
  }));
}

/**
 * Extract unique artists from playlist tracks (legacy, use extractTopArtists instead)
 * @deprecated Use extractTopArtists for better performance
 */
export function extractUniqueArtists(
  tracks: Array<{ artist?: string; artistId?: number }>
): PlaylistArtist[] {
  return extractTopArtists(tracks, Infinity);
}

/**
 * Format duration in seconds to mm:ss
 */
export function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, '0')}`;
}
