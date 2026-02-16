/**
 * Recommendation Event Service
 *
 * Logs lightweight usage events for home recommendations.
 */

import { invoke } from '@tauri-apps/api/core';

export type RecoEventType = 'play' | 'favorite' | 'playlist_add';
export type RecoItemType = 'track' | 'album' | 'artist';

export interface RecoEventInput {
  eventType: RecoEventType;
  itemType: RecoItemType;
  trackId?: number;
  albumId?: string;
  artistId?: number;
  playlistId?: number;
}

export interface TopArtistSeed {
  artistId: number;
  playCount: number;
}

export interface HomeSeeds {
  recentlyPlayedAlbumIds: string[];
  continueListeningTrackIds: number[];
  topArtistIds: TopArtistSeed[];
  favoriteAlbumIds: string[];
  favoriteTrackIds: number[];
}

export async function logRecoEvent(event: RecoEventInput): Promise<void> {
  try {
    await invoke('v2_reco_log_event', { event });
  } catch (err) {
    console.debug('Reco event log failed:', err);
  }
}

export async function logPlaylistAdd(trackIds: number[], playlistId: number): Promise<void> {
  if (trackIds.length === 0) return;
  const tasks = trackIds.map(trackId =>
    logRecoEvent({
      eventType: 'playlist_add',
      itemType: 'track',
      trackId,
      playlistId
    })
  );
  await Promise.allSettled(tasks);
}

export async function getHomeSeeds(limits?: {
  recentAlbums?: number;
  continueTracks?: number;
  topArtists?: number;
  favorites?: number;
}): Promise<HomeSeeds> {
  return invoke<HomeSeeds>('reco_get_home', {
    limitRecentAlbums: limits?.recentAlbums,
    limitContinueTracks: limits?.continueTracks,
    limitTopArtists: limits?.topArtists,
    limitFavorites: limits?.favorites
  });
}

export async function trainScores(options?: {
  lookbackDays?: number;
  halfLifeDays?: number;
  maxEvents?: number;
  maxPerType?: number;
}): Promise<void> {
  await invoke('v2_reco_train_scores', {
    lookbackDays: options?.lookbackDays,
    halfLifeDays: options?.halfLifeDays,
    maxEvents: options?.maxEvents,
    maxPerType: options?.maxPerType
  });
}

export async function getHomeSeedsML(limits?: {
  recentAlbums?: number;
  continueTracks?: number;
  topArtists?: number;
  favorites?: number;
}): Promise<HomeSeeds> {
  return invoke<HomeSeeds>('reco_get_home_ml', {
    limitRecentAlbums: limits?.recentAlbums,
    limitContinueTracks: limits?.continueTracks,
    limitTopArtists: limits?.topArtists,
    limitFavorites: limits?.favorites
  });
}
