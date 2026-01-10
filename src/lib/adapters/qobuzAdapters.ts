/**
 * Qobuz Adapters
 *
 * Centralizes Qobuz API -> UI model conversion functions.
 * Eliminates duplicate formatting logic across the codebase.
 */

import type {
  QobuzAlbum,
  QobuzArtist,
  AlbumDetail,
  ArtistDetail
} from '$lib/types';

// ============ Formatting Utilities ============

/**
 * Format duration in seconds to "M:SS" format
 */
export function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, '0')}`;
}

/**
 * Alias for formatDuration (they were identical)
 */
export const formatDurationMinutes = formatDuration;

/**
 * Extract best available image from Qobuz image object
 */
export function getQobuzImage(image?: { large?: string; thumbnail?: string; small?: string }): string {
  return image?.large || image?.thumbnail || image?.small || '';
}

/**
 * Format quality string from bit depth and sampling rate
 */
export function formatQuality(
  hires: boolean | undefined,
  bitDepth: number | undefined,
  samplingRate: number | undefined
): string {
  if (hires && bitDepth && samplingRate) {
    return `${bitDepth}bit/${samplingRate}kHz`;
  }
  return 'CD Quality';
}

/**
 * Format album quality string (different format with dash)
 */
export function formatAlbumQuality(
  hires: boolean | undefined,
  bitDepth: number | undefined,
  samplingRate: number | undefined
): string {
  if (hires && bitDepth && samplingRate) {
    return `${bitDepth}-Bit / ${samplingRate} kHz`;
  }
  return 'CD Quality';
}

// ============ Model Converters ============

/**
 * Convert Qobuz API album response to UI AlbumDetail model
 */
export function convertQobuzAlbum(album: QobuzAlbum): AlbumDetail {
  const artwork = getQobuzImage(album.image);
  const quality = formatAlbumQuality(
    album.hires_streamable,
    album.maximum_bit_depth,
    album.maximum_sampling_rate
  );

  return {
    id: album.id,
    artwork,
    title: album.title,
    artist: album.artist?.name || 'Unknown Artist',
    artistId: album.artist?.id,
    year: album.release_date_original?.split('-')[0] || '',
    label: album.label?.name || '',
    genre: album.genre?.name || '',
    quality,
    trackCount: album.tracks_count || album.tracks?.items?.length || 0,
    duration: formatDuration(album.duration || 0),
    tracks: album.tracks?.items?.map((track, index) => ({
      id: track.id,
      number: index + 1,
      title: track.title,
      artist: track.performer?.name,
      duration: formatDuration(track.duration),
      durationSeconds: track.duration,
      quality: track.hires_streamable ? 'Hi-Res' : 'CD',
      hires: track.hires_streamable,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate,
      albumId: album.id,
      artistId: track.performer?.id ?? album.artist?.id,
      isrc: track.isrc
    })) || []
  };
}

/**
 * Convert Qobuz API artist response to UI ArtistDetail model
 */
export function convertQobuzArtist(artist: QobuzArtist): ArtistDetail {
  const image = getQobuzImage(artist.image);

  return {
    id: artist.id,
    name: artist.name,
    image,
    albumsCount: artist.albums_count,
    biography: artist.biography,
    albums: artist.albums?.items?.map(album => {
      const artwork = getQobuzImage(album.image);
      const quality = formatQuality(
        album.hires_streamable,
        album.maximum_bit_depth,
        album.maximum_sampling_rate
      );
      return {
        id: album.id,
        title: album.title,
        artwork,
        year: album.release_date_original?.split('-')[0],
        quality
      };
    }) || [],
    totalAlbums: artist.albums?.total || artist.albums_count || 0
  };
}
