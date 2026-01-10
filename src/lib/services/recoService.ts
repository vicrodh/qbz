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

export async function logRecoEvent(event: RecoEventInput): Promise<void> {
  try {
    await invoke('reco_log_event', { event });
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
