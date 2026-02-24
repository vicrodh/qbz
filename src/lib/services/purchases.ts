import { invoke } from '@tauri-apps/api/core';
import type {
  PurchaseIdsResponse,
  PurchaseResponse,
  PurchasedAlbum,
  PurchaseFormatOption
} from '$lib/types/purchases';

const USE_MOCK =
  import.meta.env.DEV && import.meta.env.VITE_MOCK_PURCHASES === 'true';
const MOCK_URL = 'http://localhost:8787';

export async function getPurchases(
  limit = 50,
  offset = 0
): Promise<PurchaseResponse> {
  if (USE_MOCK) {
    const res = await fetch(
      `${MOCK_URL}/purchases?limit=${limit}&offset=${offset}`
    );
    return res.json();
  }
  return invoke<PurchaseResponse>('v2_purchases_get_all', { limit, offset });
}

export async function searchPurchases(
  query: string
): Promise<PurchaseResponse> {
  if (USE_MOCK) {
    const res = await fetch(
      `${MOCK_URL}/purchases/search?q=${encodeURIComponent(query)}`
    );
    return res.json();
  }
  return invoke<PurchaseResponse>('v2_purchases_search', { query });
}

export async function getPurchasesByType(
  purchaseType: 'albums' | 'tracks'
): Promise<PurchaseResponse> {
  if (USE_MOCK) {
    const res = await fetch(`${MOCK_URL}/purchases/${purchaseType}`);
    return res.json();
  }

  return invoke<PurchaseResponse>('v2_purchases_get_by_type', { purchaseType });
}

export async function getPurchaseIds(
  limit = 500,
  offset = 0,
  purchaseType?: 'albums' | 'tracks'
): Promise<PurchaseIdsResponse> {
  if (USE_MOCK) {
    const params = new URLSearchParams({
      limit: String(limit),
      offset: String(offset)
    });
    if (purchaseType) params.set('type', purchaseType);
    const res = await fetch(`${MOCK_URL}/purchases/ids?${params.toString()}`);
    return res.json();
  }

  return invoke<PurchaseIdsResponse>('v2_purchases_get_ids', {
    limit,
    offset,
    purchaseType: purchaseType ?? null
  });
}

export async function getAlbumDetail(
  albumId: string
): Promise<PurchasedAlbum> {
  if (USE_MOCK) {
    const res = await fetch(`${MOCK_URL}/purchases/album/${albumId}`);
    return res.json();
  }
  return invoke<PurchasedAlbum>('v2_purchases_get_album', { albumId });
}

export async function getFormats(
  albumId: string
): Promise<PurchaseFormatOption[]> {
  if (USE_MOCK) {
    const res = await fetch(`${MOCK_URL}/purchases/formats/${albumId}`);
    return res.json();
  }
  return invoke<PurchaseFormatOption[]>('v2_purchases_get_formats', { albumId });
}

export async function downloadAlbum(
  albumId: string,
  formatId: number,
  destination: string,
  qualityDir: string = ''
): Promise<void> {
  if (USE_MOCK) {
    // In mock mode we just fire and forget the SSE stream
    await fetch(`${MOCK_URL}/purchases/download/album/${albumId}`, {
      method: 'POST'
    });
    return;
  }
  return invoke<void>('v2_purchases_download_album', {
    albumId,
    formatId,
    destination,
    qualityDir
  });
}

export async function downloadTrack(
  trackId: number,
  formatId: number,
  destination: string,
  qualityDir: string = ''
): Promise<string> {
  if (USE_MOCK) {
    await fetch(`${MOCK_URL}/purchases/download/track/${trackId}`, {
      method: 'POST'
    });
    return destination;
  }
  return invoke<string>('v2_purchases_download_track', {
    trackId,
    formatId,
    destination,
    qualityDir
  });
}

// ── Downloaded Purchases Registry (V2 backend, SQLite) ──

export async function getDownloadedTrackIds(): Promise<Set<number>> {
  const ids = await invoke<number[]>('v2_purchases_get_downloaded_track_ids');
  return new Set(ids);
}

export async function markTrackDownloaded(
  trackId: number,
  albumId: string | undefined,
  filePath: string,
  formatId: number = 0
): Promise<void> {
  return invoke<void>('v2_purchases_mark_downloaded', {
    trackId,
    albumId: albumId ?? null,
    filePath,
    formatId
  });
}

export async function removeDownloadedTrack(trackId: number): Promise<void> {
  return invoke<void>('v2_purchases_remove_downloaded', { trackId });
}
