import { writable, derived, get } from 'svelte/store';
import { downloadTrack, markTrackDownloaded } from '$lib/services/purchases';

export type TrackDownloadStatus = 'downloading' | 'complete' | 'failed';

interface AlbumDownloadState {
  trackStatuses: Record<number, TrackDownloadStatus>;
  isDownloadingAll: boolean;
  allComplete: boolean;
  destination?: string;
  formatId?: number;
}

// Persistent store: albumId -> download state (survives component unmount)
export const purchaseDownloads = writable<Record<string, AlbumDownloadState>>({});

// Flat view of all track statuses across all albums (for PurchasesView)
export const allTrackStatuses = derived(purchaseDownloads, ($downloads) => {
  const flat: Record<number, TrackDownloadStatus> = {};
  for (const state of Object.values($downloads)) {
    for (const [trackId, status] of Object.entries(state.trackStatuses)) {
      flat[Number(trackId)] = status;
    }
  }
  return flat;
});

function updateAlbumState(
  albumId: string,
  updater: (state: AlbumDownloadState) => AlbumDownloadState
) {
  purchaseDownloads.update((all) => {
    const current = all[albumId] ?? {
      trackStatuses: {},
      isDownloadingAll: false,
      allComplete: false,
    };
    return { ...all, [albumId]: updater(current) };
  });
}

/** Get the format ID that was used for the most recent download of this album. */
export function getAlbumDownloadFormatId(albumId: string): number | undefined {
  return get(purchaseDownloads)[albumId]?.formatId;
}

/** Clear in-memory download state for an album (e.g. after adding to library). */
export function clearAlbumDownloadState(albumId: string) {
  purchaseDownloads.update((all) => {
    const { [albumId]: _, ...rest } = all;
    return rest;
  });
}

export function startAlbumDownload(
  albumId: string,
  trackIds: number[],
  formatId: number,
  destination: string,
  qualityDir: string = ''
) {
  updateAlbumState(albumId, () => ({
    trackStatuses: {},
    isDownloadingAll: true,
    allComplete: false,
    destination,
    formatId,
  }));
  executeAlbumDownload(albumId, trackIds, formatId, destination, qualityDir);
}

async function executeAlbumDownload(
  albumId: string,
  trackIds: number[],
  formatId: number,
  destination: string,
  qualityDir: string
) {
  for (const trackId of trackIds) {
    updateAlbumState(albumId, (state) => ({
      ...state,
      trackStatuses: { ...state.trackStatuses, [trackId]: 'downloading' },
    }));

    try {
      const filePath = await downloadTrack(trackId, formatId, destination, qualityDir);
      updateAlbumState(albumId, (state) => ({
        ...state,
        trackStatuses: { ...state.trackStatuses, [trackId]: 'complete' },
      }));
      await markTrackDownloaded(trackId, albumId, filePath, formatId).catch(() => {});
    } catch {
      updateAlbumState(albumId, (state) => ({
        ...state,
        trackStatuses: { ...state.trackStatuses, [trackId]: 'failed' },
      }));
    }
  }

  // Finalize
  const currentState = get(purchaseDownloads)[albumId];
  if (currentState) {
    const allDone = trackIds.every(
      (id) => currentState.trackStatuses[id] === 'complete'
    );
    updateAlbumState(albumId, (state) => ({
      ...state,
      isDownloadingAll: false,
      allComplete: allDone,
    }));
  }
}

export function startTrackDownload(
  albumId: string,
  trackId: number,
  formatId: number,
  destination: string,
  qualityDir: string = ''
) {
  updateAlbumState(albumId, (state) => ({
    ...state,
    trackStatuses: { ...state.trackStatuses, [trackId]: 'downloading' },
    formatId,
  }));
  executeSingleTrackDownload(albumId, trackId, formatId, destination, qualityDir);
}

async function executeSingleTrackDownload(
  albumId: string,
  trackId: number,
  formatId: number,
  destination: string,
  qualityDir: string
) {
  try {
    const filePath = await downloadTrack(trackId, formatId, destination, qualityDir);
    updateAlbumState(albumId, (state) => ({
      ...state,
      trackStatuses: { ...state.trackStatuses, [trackId]: 'complete' },
    }));
    await markTrackDownloaded(trackId, albumId, filePath, formatId).catch(() => {});
  } catch {
    updateAlbumState(albumId, (state) => ({
      ...state,
      trackStatuses: { ...state.trackStatuses, [trackId]: 'failed' },
    }));
  }
}
