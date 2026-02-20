import { writable, derived, get } from 'svelte/store';
import { downloadTrack, markTrackDownloaded } from '$lib/services/purchases';

export type TrackDownloadStatus = 'downloading' | 'complete' | 'failed';

interface AlbumDownloadState {
  trackStatuses: Record<number, TrackDownloadStatus>;
  isDownloadingAll: boolean;
  allComplete: boolean;
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

export function startAlbumDownload(
  albumId: string,
  trackIds: number[],
  formatId: number,
  destination: string
) {
  updateAlbumState(albumId, () => ({
    trackStatuses: {},
    isDownloadingAll: true,
    allComplete: false,
  }));
  executeAlbumDownload(albumId, trackIds, formatId, destination);
}

async function executeAlbumDownload(
  albumId: string,
  trackIds: number[],
  formatId: number,
  destination: string
) {
  for (const trackId of trackIds) {
    updateAlbumState(albumId, (state) => ({
      ...state,
      trackStatuses: { ...state.trackStatuses, [trackId]: 'downloading' },
    }));

    try {
      const filePath = await downloadTrack(trackId, formatId, destination);
      updateAlbumState(albumId, (state) => ({
        ...state,
        trackStatuses: { ...state.trackStatuses, [trackId]: 'complete' },
      }));
      await markTrackDownloaded(trackId, albumId, filePath).catch(() => {});
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
  destination: string
) {
  updateAlbumState(albumId, (state) => ({
    ...state,
    trackStatuses: { ...state.trackStatuses, [trackId]: 'downloading' },
  }));
  executeSingleTrackDownload(albumId, trackId, formatId, destination);
}

async function executeSingleTrackDownload(
  albumId: string,
  trackId: number,
  formatId: number,
  destination: string
) {
  try {
    const filePath = await downloadTrack(trackId, formatId, destination);
    updateAlbumState(albumId, (state) => ({
      ...state,
      trackStatuses: { ...state.trackStatuses, [trackId]: 'complete' },
    }));
    await markTrackDownloaded(trackId, albumId, filePath).catch(() => {});
  } catch {
    updateAlbumState(albumId, (state) => ({
      ...state,
      trackStatuses: { ...state.trackStatuses, [trackId]: 'failed' },
    }));
  }
}
