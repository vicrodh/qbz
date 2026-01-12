/**
 * Qobuz Adapters
 *
 * Centralizes Qobuz API -> UI model conversion functions.
 * Eliminates duplicate formatting logic across the codebase.
 */

import type {
  QobuzAlbum,
  QobuzArtist,
  QobuzPlaylist,
  QobuzTrack,
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

type ArtistAlbumSummary = ArtistDetail['albums'][number];
type ArtistPlaylistSummary = ArtistDetail['playlists'][number];

function isEpOrSingle(album: QobuzAlbum): boolean {
  const trackCount = album.tracks_count ?? 0;
  const duration = album.duration ?? 0;
  const title = album.title?.toLowerCase() ?? '';

  if (trackCount > 0 && trackCount <= 8) return true;
  if (duration > 0 && duration <= 1800) return true;
  if (/\b(ep|single)\b/.test(title)) return true;

  return false;
}

function toArtistAlbumSummary(album: QobuzAlbum): ArtistAlbumSummary {
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
}

function buildCompilationAlbums(tracks: QobuzTrack[] | undefined): ArtistAlbumSummary[] {
  if (!tracks || tracks.length === 0) return [];

  const seen = new Set<string>();
  const compilations: ArtistAlbumSummary[] = [];

  for (const track of tracks) {
    const album = track.album;
    if (!album?.id || seen.has(album.id)) continue;

    seen.add(album.id);
    compilations.push({
      id: album.id,
      title: album.title,
      artwork: getQobuzImage(album.image),
      year: undefined,
      quality: formatQuality(
        track.hires_streamable,
        track.maximum_bit_depth,
        track.maximum_sampling_rate
      )
    });
  }

  return compilations;
}

function toArtistPlaylists(playlists: QobuzPlaylist[] | undefined): ArtistPlaylistSummary[] {
  if (!playlists) return [];

  return playlists.map(playlist => ({
    id: playlist.id,
    title: playlist.name,
    artwork: playlist.images?.[0],
    trackCount: playlist.tracks_count,
    owner: playlist.owner?.name
  }));
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
  const albumItems = artist.albums?.items || [];
  const albumsFetched = (artist.albums?.offset || 0) + albumItems.length;

  const albums: ArtistAlbumSummary[] = [];
  const epsSingles: ArtistAlbumSummary[] = [];

  for (const album of albumItems) {
    const summary = toArtistAlbumSummary(album);
    if (isEpOrSingle(album)) {
      epsSingles.push(summary);
    } else {
      albums.push(summary);
    }
  }

  return {
    id: artist.id,
    name: artist.name,
    image,
    albumsCount: artist.albums_count,
    biography: artist.biography,
    albums,
    epsSingles,
    compilations: buildCompilationAlbums(artist.tracks_appears_on?.items),
    playlists: toArtistPlaylists(artist.playlists),
    totalAlbums: artist.albums?.total || artist.albums_count || 0,
    albumsFetched
  };
}

export function appendArtistAlbums(
  artist: ArtistDetail,
  newAlbums: QobuzAlbum[],
  totalAlbums?: number,
  albumsFetched?: number
): ArtistDetail {
  if (newAlbums.length === 0) return artist;

  const existingAlbumIds = new Set(artist.albums.map(album => album.id));
  const existingEpIds = new Set(artist.epsSingles.map(album => album.id));

  const albums = [...artist.albums];
  const epsSingles = [...artist.epsSingles];

  for (const album of newAlbums) {
    const summary = toArtistAlbumSummary(album);
    if (isEpOrSingle(album)) {
      if (!existingEpIds.has(summary.id)) {
        epsSingles.push(summary);
        existingEpIds.add(summary.id);
      }
    } else if (!existingAlbumIds.has(summary.id)) {
      albums.push(summary);
      existingAlbumIds.add(summary.id);
    }
  }

  return {
    ...artist,
    albums,
    epsSingles,
    totalAlbums: totalAlbums ?? artist.totalAlbums,
    albumsFetched: albumsFetched ?? artist.albumsFetched + newAlbums.length
  };
}
