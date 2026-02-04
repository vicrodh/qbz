/**
 * In-memory session cache for API responses.
 * Data is cached for the lifetime of the app session (cleared on app close).
 * This avoids repeated API calls for the same data during a session.
 */

import type { QobuzArtist, QobuzAlbum, QobuzTrack } from '$lib/types';

// Artist cache by ID
const artistCache = new Map<number, QobuzArtist>();

// Album cache by ID
const albumCache = new Map<string, QobuzAlbum>();

// Track cache by ID
const trackCache = new Map<number, QobuzTrack>();

export function getCachedArtist(artistId: number): QobuzArtist | undefined {
  return artistCache.get(artistId);
}

export function setCachedArtist(artist: QobuzArtist): void {
  artistCache.set(artist.id, artist);
}

export function getCachedAlbum(albumId: string): QobuzAlbum | undefined {
  return albumCache.get(albumId);
}

export function setCachedAlbum(album: QobuzAlbum): void {
  albumCache.set(album.id, album);
}

export function getCachedTrack(trackId: number): QobuzTrack | undefined {
  return trackCache.get(trackId);
}

export function setCachedTrack(track: QobuzTrack): void {
  trackCache.set(track.id, track);
}

// Clear all caches (useful for logout or refresh)
export function clearSessionCache(): void {
  artistCache.clear();
  albumCache.clear();
  trackCache.clear();
}

// Get cache stats (for debugging)
export function getCacheStats(): { artists: number; albums: number; tracks: number } {
  return {
    artists: artistCache.size,
    albums: albumCache.size,
    tracks: trackCache.size
  };
}
