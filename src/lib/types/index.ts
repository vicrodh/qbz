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
  track_number?: number;
  album?: {
    id?: string;
    title: string;
    image?: QobuzImage;
    label?: { id: number; name: string };
    genre?: { name: string };
  };
  performer?: { id?: number; name: string };
  hires_streamable?: boolean;
  /** Whether the track is streamable (false = removed/unavailable on Qobuz) */
  streamable?: boolean;
  maximum_bit_depth?: number;
  maximum_sampling_rate?: number;
  isrc?: string;
  performers?: string;
  composer?: { id?: number; name: string };
  copyright?: string;
}

// Parsed performer from performers string
export interface Performer {
  name: string;
  roles: string[];
}

// Track info response with parsed performers
export interface TrackInfo {
  track: QobuzTrack;
  performers: Performer[];
}

// Album credits - consolidated view from album header
export interface AlbumCredits {
  album: AlbumInfo;
  tracks: TrackCredits[];
}

export interface AlbumInfo {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  artist_id?: number;
  year: string;
  release_date?: string;
  label: string;
  label_id?: number;
  genre: string;
  quality: string;
  track_count: number;
  duration: string;
  bit_depth?: number;
  sampling_rate?: number;
  description?: string;
}

export interface TrackCredits {
  id: number;
  number: number;
  title: string;
  artist: string;
  duration: string;
  duration_seconds: number;
  performers: Performer[];
  copyright?: string;
  album_id?: string;
  artist_id?: number;
}

export interface QobuzAlbum {
  id: string;
  title: string;
  description?: string;
  artist: { id?: number; name: string };
  image: QobuzImage;
  release_date_original?: string;
  hires_streamable?: boolean;
  tracks_count?: number;
  duration?: number;
  label?: { id?: number; name: string };
  genre?: { name: string };
  maximum_bit_depth?: number;
  maximum_sampling_rate?: number;
  tracks?: { items: QobuzTrack[] };
  upc?: string;
  goodies?: QobuzGoody[];
}

export interface QobuzGoody {
  id: number;
  name: string;
  url: string;
  original_url: string;
  file_format_id?: number;
  description?: string;
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

export interface PlaylistDuplicateResult {
  total_tracks: number;
  duplicate_count: number;
  duplicate_track_ids: Set<number>;
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
  /** Whether the track is streamable (false = unavailable on Qobuz) */
  streamable?: boolean;
}

export interface AlbumDetail {
  id: string;
  artwork: string;
  title: string;
  artist: string;
  artistId?: number;
  year: string;
  releaseDate?: string; // Full date in YYYY-MM-DD format
  label: string;
  labelId?: number;
  genre: string;
  quality: string;
  trackCount: number;
  duration: string;
  tracks: Track[];
  upc?: string; // Universal Product Code for album.link sharing
  goodies?: QobuzGoody[];
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
    releaseDate?: string;
    quality: string;
    genre: string;
  }[];
  epsSingles: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    releaseDate?: string;
    quality: string;
    genre: string;
  }[];
  liveAlbums: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    releaseDate?: string;
    quality: string;
    genre: string;
  }[];
  compilations: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    releaseDate?: string;
    quality: string;
    genre: string;
  }[];
  tributes: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    releaseDate?: string;
    quality: string;
    genre: string;
  }[];
  others: {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    releaseDate?: string;
    quality: string;
    genre: string;
  }[];
  playlists: {
    id: number;
    title: string;
    artwork?: string;
    trackCount?: number;
    owner?: string;
  }[];
  labels: {
    id: number;
    name: string;
  }[];
  totalAlbums: number;
  albumsFetched: number;
  /** Per-category has_more flags from /artist/page release groups */
  releaseHasMore?: Record<string, boolean>;
}

export interface LabelDetail {
  id: number;
  name: string;
  description?: string;
  image?: QobuzImage;
  albums: QobuzAlbum[];
  totalAlbums: number;
  albumsFetched: number;
}

/** Response from /label/page */
export interface LabelPageData {
  id: number;
  name: string;
  description?: string;
  image?: string | Record<string, string>;
  releases?: LabelReleaseContainer[];
  playlists?: { has_more?: boolean; items?: Record<string, unknown>[] };
  top_tracks?: Record<string, unknown>[];
  top_artists?: { has_more?: boolean; items?: Record<string, unknown>[] };
}

export interface LabelReleaseContainer {
  id?: string;
  data?: { has_more?: boolean; items?: QobuzAlbum[] };
}

/** Response from /label/explore */
export interface LabelExploreResponse {
  has_more?: boolean;
  items?: LabelExploreItem[];
}

export interface LabelExploreItem {
  id: number;
  name: string;
  image?: string | Record<string, string>;
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
  /** Whether the track is streamable (false = unavailable on Qobuz) */
  streamable?: boolean;
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
  source?: string;
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

// ============ Musician Types ============

/**
 * Musician confidence level for MusicBrainz ↔ Qobuz matching
 * Determines what UI is shown when a musician is clicked:
 * - confirmed (3): Navigate to Qobuz Artist Page
 * - contextual (2): Navigate to Musician Page
 * - weak (1): Show Informational Modal only
 * - none (0): Show Informational Modal only
 */
export type MusicianConfidence = 'confirmed' | 'contextual' | 'weak' | 'none';

/**
 * Resolved musician with confidence assessment
 */
export interface ResolvedMusician {
  name: string;
  role: string;
  mbid?: string;
  qobuz_artist_id?: number;
  confidence: MusicianConfidence;
  bands: string[];
  appears_on_count: number;
}

/**
 * Album appearance for a musician
 */
export interface AlbumAppearance {
  album_id: string;
  album_title: string;
  album_artwork: string;
  artist_name: string;
  year?: string;
  role_on_album: string;
}

/**
 * Musician appearances response
 */
export interface MusicianAppearances {
  albums: AlbumAppearance[];
  total: number;
}

// ============ Preferences Types ============

export interface FavoritesPreferences {
  custom_icon_path: string | null;
  custom_icon_preset: string | null;
  icon_background: string | null;
  tab_order: string[];
}

// ============ Discover API Types ============

export interface DiscoverResponse {
  containers: DiscoverContainers;
}

export interface DiscoverContainers {
  playlists?: DiscoverContainer<DiscoverPlaylist>;
  ideal_discography?: DiscoverContainer<DiscoverAlbum>;
  playlists_tags?: DiscoverContainer<PlaylistTag>;
  new_releases?: DiscoverContainer<DiscoverAlbum>;
  qobuzissims?: DiscoverContainer<DiscoverAlbum>;
  most_streamed?: DiscoverContainer<DiscoverAlbum>;
  press_awards?: DiscoverContainer<DiscoverAlbum>;
  album_of_the_week?: DiscoverContainer<DiscoverAlbum>;
}

export interface DiscoverContainer<T> {
  id: string;
  data: DiscoverData<T>;
}

export interface DiscoverData<T> {
  has_more: boolean;
  items: T[];
}

export interface DiscoverPlaylist {
  id: number;
  name: string;
  owner: { id: number; name: string };
  image: DiscoverPlaylistImage;
  description?: string;
  duration: number;
  tracks_count: number;
  genres?: { id: number; name: string; path: number[] }[];
  tags?: PlaylistTag[];
}

export interface DiscoverPlaylistImage {
  rectangle?: string;
  covers?: string[];
}

export interface PlaylistTag {
  id: number;
  slug: string;
  name: string;
}

// Response from discover/playlists endpoint
// Note: This endpoint returns items directly at root level (not wrapped in "playlists")
export interface DiscoverPlaylistsResponse {
  has_more: boolean;
  items: DiscoverPlaylist[];
}

export interface DiscoverAlbum {
  id: string;
  title: string;
  version?: string;
  track_count?: number;
  duration?: number;
  parental_warning?: boolean;
  image: DiscoverAlbumImage;
  artists: DiscoverArtist[];
  label?: { id: number; name: string };
  genre?: { name: string };
  dates?: DiscoverAlbumDates;
  audio_info?: DiscoverAudioInfo;
}

export interface DiscoverAlbumImage {
  small?: string;
  thumbnail?: string;
  large?: string;
}

export interface DiscoverArtist {
  id: number;
  name: string;
  roles?: string[];
}

export interface DiscoverAlbumDates {
  download?: string;
  original?: string;
  stream?: string;
}

export interface DiscoverAudioInfo {
  maximum_sampling_rate?: number;
  maximum_bit_depth?: number;
  maximum_channel_count?: number;
}

// ============ Artist Page Types (/artist/page) ============

export interface PageArtistResponse {
  id: number;
  name: { display: string };
  artist_category?: string;
  biography?: PageArtistBiography;
  images?: { portrait?: { hash: string; format: string } };
  similar_artists?: { has_more: boolean; items: PageArtistSimilarItem[] };
  top_tracks?: PageArtistTrack[];
  last_release?: unknown;
  releases?: PageArtistReleaseGroup[];
  tracks_appears_on?: PageArtistTrack[];
  playlists?: { has_more: boolean; items: PageArtistPlaylist[] };
}

export interface PageArtistBiography {
  content?: string;
  source?: unknown;
  language?: string;
}

export interface PageArtistSimilarItem {
  id: number;
  name: { display: string };
  images?: { portrait?: { hash: string; format: string } };
}

export interface PageArtistReleaseGroup {
  type: string;
  has_more: boolean;
  items: PageArtistRelease[];
}

export interface PageArtistRelease {
  id: string;
  title: string;
  version?: string;
  tracks_count?: number;
  artist?: { id: number; name: { display: string } };
  artists?: { id: number; name: string; roles?: string[] }[];
  image?: { small?: string; thumbnail?: string; large?: string };
  label?: { id: number; name: string };
  genre?: { id?: number; name: string };
  release_type?: string;
  release_tags?: string[];
  duration?: number;
  dates?: DiscoverAlbumDates;
  parental_warning?: boolean;
  audio_info?: DiscoverAudioInfo;
  rights?: PageArtistRights;
  awards?: { id: number; name: string; awarded_at?: string }[];
}

export interface PageArtistRights {
  streamable?: boolean;
  hires_streamable?: boolean;
  hires_purchasable?: boolean;
  purchasable?: boolean;
  downloadable?: boolean;
  previewable?: boolean;
  sampleable?: boolean;
}

export interface PageArtistTrack {
  id: number;
  title: string;
  version?: string;
  duration?: number;
  isrc?: string;
  parental_warning?: boolean;
  artist?: { id: number; name: { display: string } };
  composer?: unknown;
  audio_info?: DiscoverAudioInfo;
  rights?: PageArtistRights;
  physical_support?: { media_number?: number; track_number?: number };
  album?: {
    id: string;
    title: string;
    version?: string;
    image?: { small?: string; thumbnail?: string; large?: string };
    label?: { id: number; name: string };
    genre?: { id?: number; name: string };
  };
}

export interface PageArtistPlaylist {
  id: number;
  title?: string;
  description?: string;
  owner?: { id: number; name?: string };
  tracks_count?: number;
  duration?: number;
  images?: { rectangle?: string[] };
}

export interface ReleasesGridResponse {
  has_more: boolean;
  items: PageArtistRelease[];
}
