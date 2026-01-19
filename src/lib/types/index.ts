/**
 * Shared Type Definitions
 *
 * Central location for types used across the application.
 */

// ============ Qobuz API Types ============
// Raw response types from Qobuz API (via backend)

export interface QobuzImage {
  small?: string;
  thumbnail?: string;
  large?: string;
}

export interface QobuzTrack {
  id: number;
  title: string;
  duration: number;
  album?: {
    id?: string;
    title: string;
    image?: QobuzImage;
  };
  performer?: { id?: number; name: string };
  hires_streamable?: boolean;
  maximum_bit_depth?: number;
  maximum_sampling_rate?: number;
  isrc?: string;
}

export interface QobuzAlbum {
  id: string;
  title: string;
  artist: { id?: number; name: string };
  image: QobuzImage;
  release_date_original?: string;
  hires_streamable?: boolean;
  tracks_count?: number;
  duration?: number;
  label?: { name: string };
  genre?: { name: string };
  maximum_bit_depth?: number;
  maximum_sampling_rate?: number;
  tracks?: { items: QobuzTrack[] };
}

export interface QobuzPlaylist {
  id: number;
  name: string;
  description?: string;
  owner?: { id?: number; name: string };
  images?: string[];
  tracks_count?: number;
  duration?: number;
}

export interface QobuzArtist {
  id: number;
  name: string;
  image?: QobuzImage;
  albums_count?: number;
  biography?: {
    summary?: string;
    content?: string;
    source?: string;
  };
  albums?: {
    items: QobuzAlbum[];
    total: number;
    offset: number;
    limit: number;
  };
  tracks_appears_on?: {
    items: QobuzTrack[];
    total: number;
    offset: number;
    limit: number;
  };
  playlists?: QobuzPlaylist[];
}

// ============ UI Display Types ============
// Converted/formatted types for UI components

export interface Track {
  id: number;
  number: number;
  title: string;
  artist?: string;
  duration: string;
  durationSeconds: number;
  quality?: string;
  hires?: boolean;
  bitDepth?: number;
  samplingRate?: number;
  albumId?: string;
  artistId?: number;
  isrc?: string;
}

export interface AlbumDetail {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  artistId?: number;
  year: string;
  label: string;
  genre: string;
  quality: string;
  trackCount: number;
  duration: string;
  tracks: Track[];
}

export interface ArtistDetail {
  id: number;
  name: string;
  image?: string;
  albumsCount?: number;
  biography?: {
    summary?: string;
    content?: string;
    source?: string;
  };
  albums: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    quality: string;
  }[];
  epsSingles: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    quality: string;
  }[];
  liveAlbums: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    quality: string;
  }[];
  compilations: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    quality: string;
  }[];
  tributes: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    quality: string;
  }[];
  others: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    quality: string;
  }[];
  playlists: {
    id: number;
    title: string;
    artwork?: string;
    trackCount?: number;
    owner?: string;
  }[];
  totalAlbums: number;
  albumsFetched: number;
}

export interface PlaylistTrack {
  id: number;
  number: number;
  title: string;
  artist?: string;
  album?: string;
  albumArt?: string;
  duration: string;
  durationSeconds: number;
  hires?: boolean;
  bitDepth?: number;
  samplingRate?: number;
  albumId?: string;
  artistId?: number;
  isrc?: string;
}

/**
 * Unified display track interface used across views
 * Compatible with PlaylistTrack, FavoritesTrack, and ArtistTrack displays
 */
export interface DisplayTrack {
  id: number;
  number?: number;
  title: string;
  artist?: string;
  album?: string;
  albumArt?: string;
  albumId?: string;
  artistId?: number;
  duration: string;
  durationSeconds: number;
  hires?: boolean;
  bitDepth?: number;
  samplingRate?: number;
  isrc?: string;
  isLocal?: boolean;
  localTrackId?: number;
  artworkPath?: string;
}

// ============ Local Library Types ============

export interface LocalLibraryTrack {
  id: number;
  file_path: string;
  title: string;
  artist: string;
  album: string;
  duration_secs: number;
  format: string;
  bit_depth?: number;
  sample_rate: number;
  artwork_path?: string;
}

// ============ External API Types ============

export interface SongLinkResponse {
  pageUrl: string;
  title?: string;
  artist?: string;
  thumbnailUrl?: string;
  platforms: Record<string, string>;
  identifier: string;
  contentType: string;
}

// ============ Preferences Types ============

export interface FavoritesPreferences {
  custom_icon_path: string | null;
  custom_icon_preset: string | null;
  icon_background: string | null;
  tab_order: string[];
}
