import { writable, derived, get } from 'svelte/store';
import { downloadTrack, markTrackDownloaded } from '$lib/services/purchases';

export type TrackDownloadStatus = 'downloading' | 'complete' | 'failed' | 'cancelled';

interface AlbumDownloadState {
  trackStatuses: Record<number, TrackDownloadStatus>;
  isDownloadingAll: boolean;
  allComplete: boolean;
  destination?: string;
  formatId?: number;
}

// Persistent store: albumId -> download state (survives component unmount)
export const purchaseDownloads = writable<Record<string, AlbumDownloadState>>({});

// Abort flags keyed by albumId — checked between sequential track downloads
const abortFlags = new Map<string, boolean>();

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
  abortFlags.delete(albumId);
  purchaseDownloads.update((all) => {
    const { [albumId]: _, ...rest } = all;
    return rest;
  });
}

/** Cancel an in-progress album download. The current track will finish, but no more will start. */
export function cancelAlbumDownload(albumId: string) {
  abortFlags.set(albumId, true);
}

export function startAlbumDownload(
  albumId: string,
  trackIds: number[],
  formatId: number,
  destination: string,
  qualityDir: string = ''
) {
  abortFlags.delete(albumId);
  updateAlbumState(albumId, () => ({
    trackStatuses: {},
    isDownloadingAll: true,
    allComplete: false,
    destination,
    formatId,
  }));
  executeAlbumDownload(albumId, trackIds, formatId, destination, qualityDir);
}

/**
 * Extract the album-level folder from a downloaded file path.
 * Download structure: destination/artist/album/[qualityDir/]track.ext
 * We want: destination/artist/album/
 */
function albumFolderFromFilePath(filePath: string, hasQualityDir: boolean): string {
  // Remove filename
  let lastSep = filePath.lastIndexOf('/');
  if (lastSep < 0) lastSep = filePath.lastIndexOf('\\');
  let dir = lastSep >= 0 ? filePath.substring(0, lastSep) : filePath;

  // If there's a quality subdirectory, go up one more level
  if (hasQualityDir) {
    let prevSep = dir.lastIndexOf('/');
    if (prevSep < 0) prevSep = dir.lastIndexOf('\\');
    if (prevSep >= 0) {
      dir = dir.substring(0, prevSep);
    }
  }

  return dir;
}

async function executeAlbumDownload(
  albumId: string,
  trackIds: number[],
  formatId: number,
  destination: string,
  qualityDir: string
) {
  let albumFolderResolved = false;

  for (const trackId of trackIds) {
    // Check cancellation before starting each track
    if (abortFlags.get(albumId)) {
      // Mark remaining tracks as cancelled
      const currentState = get(purchaseDownloads)[albumId];
      const remaining: Record<number, TrackDownloadStatus> = {};
      for (const id of trackIds) {
        if (!currentState?.trackStatuses[id]) {
          remaining[id] = 'cancelled';
        }
      }
      updateAlbumState(albumId, (state) => ({
        ...state,
        trackStatuses: { ...state.trackStatuses, ...remaining },
        isDownloadingAll: false,
        allComplete: false,
      }));
      abortFlags.delete(albumId);
      return;
    }

    updateAlbumState(albumId, (state) => ({
      ...state,
      trackStatuses: { ...state.trackStatuses, [trackId]: 'downloading' },
    }));

    try {
      const filePath = await downloadTrack(trackId, formatId, destination, qualityDir);

      // After the first successful download, resolve the album-level folder
      // so "Add to Library" adds only this album, not the entire root.
      if (!albumFolderResolved) {
        albumFolderResolved = true;
        const albumFolder = albumFolderFromFilePath(filePath, qualityDir.length > 0);
        updateAlbumState(albumId, (state) => ({
          ...state,
          destination: albumFolder,
        }));
      }

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
  abortFlags.delete(albumId);
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
