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

/**
 * Check if album belongs to a different artist (should go to Tributes)
 * This catches albums that appear in an artist's discography but are actually by someone else
 */
function isDifferentArtist(album: QobuzAlbum, mainArtistId: number | undefined): boolean {
  if (!mainArtistId || !album.artist?.id) return false;
  return album.artist.id !== mainArtistId;
}

/**
 * Check if this is a tribute/cover album by OTHER artists
 * These go to the Tributes section, not Others
 */
function isTributeAlbum(album: QobuzAlbum, mainArtistId: number | undefined): boolean {
  const title = album.title?.toLowerCase() ?? '';

  // Explicit tribute/cover patterns in title
  const tributePatterns = /\b(tribute to|tribute|a]tribute|covers? of|as made famous|karaoke|in the style of|salute to|celebrating)\b/;

  // If it's by a different artist AND has tribute-like patterns
  if (isDifferentArtist(album, mainArtistId)) {
    // Any album by another artist with tribute patterns
    if (tributePatterns.test(title)) return true;

    // Albums that mention the main artist's name in title but are by someone else
    // These are likely tribute albums
    return true; // All different artist albums go to tributes
  }

  return false;
}

/**
 * Check if this is an unofficial/bootleg release (should go to Others)
 */
function isUnofficialRelease(album: QobuzAlbum): boolean {
  const title = album.title?.toLowerCase() ?? '';
  const label = album.label?.name?.toLowerCase() ?? '';
  const trackCount = album.tracks_count ?? 0;
  const duration = album.duration ?? 0;

  // FM Broadcasts, radio recordings, bootlegs (including plural forms)
  const broadcastPatterns = /\b(fm broadcasts?|radio broadcasts?|broadcasts?|bootleg|unofficial|pirate)\b/;

  // Interview albums, spoken word that isn't the artist's music
  const nonMusicPatterns = /\b(interviews?|speaks|talking|in their own words|the interviews?|memories:?\s*the)\b/;

  // Podcasts, radio shows, reports (single long track, episodic naming)
  const podcastPatterns = /\b(episode|report|podcast|show|program|special)\s*\d+|\b(episode|report)\b/i;
  const isPodcastLike = podcastPatterns.test(title) && trackCount === 1 && duration >= 1200; // 20+ min single track

  // Labels known for unofficial releases
  const bootlegLabels = /\b(leftfield media|purple pyramid|cleopatra|laser media|radio lu|broadcast archives)\b/;

  return broadcastPatterns.test(title) ||
         nonMusicPatterns.test(title) ||
         bootlegLabels.test(label) ||
         isPodcastLike;
}

/**
 * Check if this is a compilation/greatest hits (should go to Others)
 * Note: Tributes are now handled separately
 */
function isCompilationAlbum(album: QobuzAlbum): boolean {
  const title = album.title?.toLowerCase() ?? '';

  // Greatest hits, best of, anthologies
  const compilationPatterns = /\b(greatest hits|best of|anthology|the very best|essentials?|definitive collection|gold|platinum|hits collection|complete collection|hit collection|b-sides|rarities)\b/;

  // Various artists compilations
  const variousPatterns = /\b(various artists|v\.?a\.?|original soundtrack|ost)\b/;

  return compilationPatterns.test(title) || variousPatterns.test(title);
}

/**
 * Check if this is a live album (legitimate live recordings)
 */
function isLiveAlbum(album: QobuzAlbum): boolean {
  const title = album.title?.toLowerCase() ?? '';
  const originalTitle = album.title ?? '';
  const genre = album.genre?.name?.toLowerCase() ?? '';

  // Don't classify FM broadcasts as live - they go to Others
  if (/\b(fm broadcasts?|radio broadcasts?|broadcasts?)\b/i.test(title)) {
    return false;
  }

  // Genre explicitly says live
  if (genre.includes('live')) return true;

  // Common live album patterns (English)
  const livePatterns = /\blive\b|\blive at\b|\blive in\b|\bin concert\b|\bunplugged\b|\bmtv unplugged\b|\bacoustic live\b|\balive\b|\bon stage\b/;

  // Live album patterns in other languages
  // Spanish: "en vivo", "en directo" | Portuguese: "ao vivo" | Italian: "dal vivo" | French: "en direct"
  // German: "Konzert", "Live in" | Dutch: "concert" | Japanese: ライブ | Korean: 라이브, 콘서트
  const liveMultilang = /\b(en vivo|en directo|ao vivo|dal vivo|en direct|konzert|liveopname)\b|ライブ|라이브|콘서트/i;

  // Tour recordings (but not "tour edition" which is a studio album variant)
  const tourPatterns = /\b(tour|on tour)\b(?!.*\bedition\b)/;

  // Check for studio variant indicators
  const isStudioVariant = /\b(remaster|deluxe|anniversary|expanded)\b/.test(title);

  if (livePatterns.test(title)) return true;
  if (liveMultilang.test(title)) return true;
  if (tourPatterns.test(title) && !isStudioVariant) return true;

  // City/Location + Year pattern (e.g., "Seattle 1989", "Tokyo 1986", "Dallas, Texas 1989")
  // Common cities for live recordings
  const cities = /\b(seattle|tokyo|london|paris|new york|los angeles|chicago|dallas|amsterdam|berlin|sydney|melbourne|montreal|toronto|rio|sao paulo|mexico city|denver|boston|philadelphia|san francisco|cleveland|detroit|atlanta|miami|phoenix|houston|minneapolis|st\.? louis|kansas city|nashville|memphis|austin|portland|oakland|stockholm|oslo|dublin|manchester|birmingham|glasgow|brussels|madrid|barcelona|rome|milan|vienna|munich|hamburg|zurich|prague|warsaw|moscow|seoul|osaka|singapore|hong kong|taipei|manila|jakarta|mumbai|delhi|bangalore|cape town|johannesburg)\b/i;
  const yearPattern = /\b(19[6-9]\d|20[0-2]\d)\b/;

  // If title contains a city and a year (likely a live recording location/date)
  if (cities.test(originalTitle) && yearPattern.test(originalTitle) && !isStudioVariant) {
    return true;
  }

  return false;
}

/**
 * Check if this is an EP or Single
 * More conservative than before - only if explicitly marked or very short
 */
function isEpOrSingle(album: QobuzAlbum): boolean {
  const trackCount = album.tracks_count ?? 0;
  const duration = album.duration ?? 0;
  const title = album.title?.toLowerCase() ?? '';

  // Explicitly marked as EP or Single in title (highest priority)
  if (/\b(- ep|\.ep|\(ep\))\b/.test(title)) return true;
  if (/\bep\s*$/.test(title)) return true; // Ends with "EP"
  if (/\b(- single|\(single\))\b/.test(title)) return true;
  // "Single" alone in title (but not "single version" which could be on any album)
  if (/\bsingle\b/.test(title) && !/\b(version|mix|edit)\b/.test(title)) return true;

  // Very short releases: 1-4 tracks (classic 45rpm single had 2 tracks per side = 4 total)
  if (trackCount > 0 && trackCount <= 4) return true;

  // Under 15 minutes is definitely EP/Single territory
  if (duration > 0 && duration <= 900) return true;

  // 5-6 tracks AND under 20 minutes = likely EP
  if (trackCount >= 5 && trackCount <= 6 && duration > 0 && duration <= 1200) return true;

  return false;
}

/**
 * Check if this should remain in main Discography (studio albums)
 * Deluxe editions, remasters, anniversary editions of studio albums stay here
 */
function isStudioAlbum(album: QobuzAlbum): boolean {
  const title = album.title?.toLowerCase() ?? '';
  const trackCount = album.tracks_count ?? 0;
  const duration = album.duration ?? 0;

  // Single-track releases over 20 min are likely podcasts/radio shows, not studio albums
  if (trackCount === 1 && duration >= 1200) return false;

  // Deluxe, remastered, anniversary editions are studio albums
  const studioVariantPatterns = /\b(deluxe|remaster|anniversary|expanded|special edition|collector|box set)\b/;

  // If it has these patterns and reasonable length, it's a studio album
  if (studioVariantPatterns.test(title)) {
    // Box sets with many tracks are still studio albums
    if (trackCount >= 7 || duration >= 2100) return true; // 35+ minutes
  }

  // Standard album: 7+ tracks or 25+ minutes (lowered slightly to catch prog albums)
  if (trackCount >= 7) return true;
  if (duration >= 1500) return true; // 25 minutes

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
    quality,
    genre: album.genre?.name || "Unknown genre"
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
      ),
      genre: album.genre?.name || ''
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
    releaseDate: album.release_date_original,
    label: album.label?.name || '',
    labelId: album.label?.id,
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
    })) || [],
    upc: album.upc
  };
}

/**
 * Categorize an album into the appropriate section
 * Priority order matters - check most specific categories first
 */
function categorizeAlbum(album: QobuzAlbum, mainArtistId: number): 'tributes' | 'others' | 'live' | 'eps' | 'albums' {
  // 1. Albums by different artists -> Tributes (highest priority)
  // These are tribute albums, covers, or albums that somehow ended up in the discography
  if (isTributeAlbum(album, mainArtistId)) {
    return 'tributes';
  }

  // 2. Unofficial releases (broadcasts, bootlegs, podcasts) -> Others
  if (isUnofficialRelease(album)) {
    return 'others';
  }

  // 3. Compilations (greatest hits, best of) -> Others
  if (isCompilationAlbum(album)) {
    return 'others';
  }

  // 4. Live albums -> Live
  if (isLiveAlbum(album)) {
    return 'live';
  }

  // 5. Check if it's a studio album first (deluxe, remasters, long albums)
  // This prevents deluxe editions from being classified as EPs
  if (isStudioAlbum(album)) {
    return 'albums';
  }

  // 6. EPs and Singles (only if not already classified as studio album)
  if (isEpOrSingle(album)) {
    return 'eps';
  }

  // 7. Default: treat as studio album
  return 'albums';
}

/**
 * Extract unique labels from albums
 */
function extractLabelsFromAlbums(albums: QobuzAlbum[]): { id: number; name: string }[] {
  const labelMap = new Map<number, string>();

  for (const album of albums) {
    if (album.label?.id && album.label?.name) {
      // Only add if we haven't seen this label ID before
      if (!labelMap.has(album.label.id)) {
        labelMap.set(album.label.id, album.label.name);
      }
    }
  }

  // Convert to array and sort by name
  return Array.from(labelMap.entries())
    .map(([id, name]) => ({ id, name }))
    .sort((a, b) => a.name.localeCompare(b.name));
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
  const liveAlbums: ArtistAlbumSummary[] = [];
  const tributes: ArtistAlbumSummary[] = [];
  const others: ArtistAlbumSummary[] = [];

  for (const album of albumItems) {
    const summary = toArtistAlbumSummary(album);
    const category = categorizeAlbum(album, artist.id);

    switch (category) {
      case 'live':
        liveAlbums.push(summary);
        break;
      case 'eps':
        epsSingles.push(summary);
        break;
      case 'tributes':
        tributes.push(summary);
        break;
      case 'others':
        others.push(summary);
        break;
      case 'albums':
      default:
        albums.push(summary);
        break;
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
    liveAlbums,
    compilations: buildCompilationAlbums(artist.tracks_appears_on?.items),
    tributes,
    others,
    playlists: toArtistPlaylists(artist.playlists),
    labels: extractLabelsFromAlbums(albumItems),
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
  const existingLiveIds = new Set(artist.liveAlbums.map(album => album.id));
  const existingTributeIds = new Set(artist.tributes.map(album => album.id));
  const existingOtherIds = new Set(artist.others.map(album => album.id));

  const albums = [...artist.albums];
  const epsSingles = [...artist.epsSingles];
  const liveAlbums = [...artist.liveAlbums];
  const tributes = [...artist.tributes];
  const others = [...artist.others];

  for (const album of newAlbums) {
    const summary = toArtistAlbumSummary(album);
    const category = categorizeAlbum(album, artist.id);

    switch (category) {
      case 'live':
        if (!existingLiveIds.has(summary.id)) {
          liveAlbums.push(summary);
          existingLiveIds.add(summary.id);
        }
        break;
      case 'eps':
        if (!existingEpIds.has(summary.id)) {
          epsSingles.push(summary);
          existingEpIds.add(summary.id);
        }
        break;
      case 'tributes':
        if (!existingTributeIds.has(summary.id)) {
          tributes.push(summary);
          existingTributeIds.add(summary.id);
        }
        break;
      case 'others':
        if (!existingOtherIds.has(summary.id)) {
          others.push(summary);
          existingOtherIds.add(summary.id);
        }
        break;
      case 'albums':
      default:
        if (!existingAlbumIds.has(summary.id)) {
          albums.push(summary);
          existingAlbumIds.add(summary.id);
        }
        break;
    }
  }

  // Merge new labels from new albums
  const existingLabelIds = new Set(artist.labels.map(l => l.id));
  const newLabels = extractLabelsFromAlbums(newAlbums)
    .filter(l => !existingLabelIds.has(l.id));
  const labels = [...artist.labels, ...newLabels]
    .sort((a, b) => a.name.localeCompare(b.name));

  return {
    ...artist,
    albums,
    epsSingles,
    liveAlbums,
    tributes,
    others,
    labels,
    totalAlbums: totalAlbums ?? artist.totalAlbums,
    albumsFetched: albumsFetched ?? artist.albumsFetched + newAlbums.length
  };
}
