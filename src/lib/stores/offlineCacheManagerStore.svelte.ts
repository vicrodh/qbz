/**
 * Offline Cache Manager Store
 *
 * Phase 4 (P4.1): Types and `buildRollup` pure function.
 * Phase 5 (P5.1): Reactive store API (subscribe / refresh / event listeners).
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { SvelteSet } from 'svelte/reactivity';

export type OfflineStatus = 'queued' | 'downloading' | 'ready' | 'failed';

export type CachedTrackInfo = {
  trackId: number;
  title: string;
  artist: string;
  album: string | null;
  albumId: string | null;
  durationSecs: number;
  fileSizeBytes: number;
  quality: string;
  bitDepth: number | null;
  sampleRate: number | null;
  status: OfflineStatus;
  progressPercent: number;
  errorMessage: string | null;
  createdAt: string;
  lastAccessedAt: string;
};

export type AlbumGroup = {
  albumId: string | null;          // null => "Singles & loose tracks"
  title: string;
  artistLabel: string;
  year: number | null;
  coverUrl: string | null;
  cachedTracks: CachedTrackInfo[];
  totalSizeBytes: number;
  worstStatus: OfflineStatus;
  failedCount: number;
  isFullyCached: boolean | null;
  dominantQuality: string;
  mostRecentCachedAt: string;
};

export type ArtistGroup = {
  artistKey: string;
  artistName: string;
  albumGroups: AlbumGroup[];
  totalSizeBytes: number;
  totalTracks: number;
};

const STATUS_RANK: Record<OfflineStatus, number> = {
  ready: 0,
  queued: 1,
  downloading: 2,
  failed: 3,
};

function worseStatus(a: OfflineStatus, b: OfflineStatus): OfflineStatus {
  return STATUS_RANK[a] >= STATUS_RANK[b] ? a : b;
}

function dominantString(values: string[]): string {
  if (values.length === 0) return '';
  const counts = new Map<string, number>();
  for (const v of values) counts.set(v, (counts.get(v) ?? 0) + 1);
  let best = values[0];
  let bestCount = 0;
  for (const [v, c] of counts) {
    if (c > bestCount) { best = v; bestCount = c; }
  }
  // Heterogeneous if no single value > 50% of total
  return bestCount * 2 > values.length ? best : 'Mixed';
}

export function buildRollup(
  tracks: CachedTrackInfo[],
  fullyCachedFlags: Map<string, boolean>,
  singlesPseudoAlbumLabel: string,
): ArtistGroup[] {
  const byArtist = new Map<string, CachedTrackInfo[]>();
  for (const track of tracks) {
    const key = track.artist.trim().toLowerCase();
    if (!byArtist.has(key)) byArtist.set(key, []);
    byArtist.get(key)!.push(track);
  }

  const artists: ArtistGroup[] = [];
  for (const [artistKey, artistTracks] of byArtist) {
    const displayNames = artistTracks.map(track => track.artist);
    const artistName = dominantString(displayNames);
    // dominantString returns 'Mixed' for ties; for artist name fall back to first.
    const finalArtistName = artistName === 'Mixed' ? displayNames[0] : artistName;

    const byAlbum = new Map<string, CachedTrackInfo[]>();
    for (const track of artistTracks) {
      const albumKey = track.albumId ?? '__singles__';
      if (!byAlbum.has(albumKey)) byAlbum.set(albumKey, []);
      byAlbum.get(albumKey)!.push(track);
    }

    const albumGroups: AlbumGroup[] = [];
    for (const [albumKey, albumTracks] of byAlbum) {
      const albumId = albumKey === '__singles__' ? null : albumKey;
      const title = albumId
        ? (albumTracks[0].album ?? singlesPseudoAlbumLabel)
        : singlesPseudoAlbumLabel;
      const artistLabel = albumId
        ? dominantString(albumTracks.map(track => track.artist)) || finalArtistName
        : finalArtistName;
      const totalSizeBytes = albumTracks.reduce((s, track) => s + track.fileSizeBytes, 0);
      const failedCount = albumTracks.filter(track => track.status === 'failed').length;
      const worstStatus = albumTracks.reduce<OfflineStatus>(
        (acc, track) => worseStatus(acc, track.status),
        'ready',
      );
      const dominantQuality = dominantString(albumTracks.map(track => track.quality));
      const mostRecentCachedAt = albumTracks.reduce(
        (acc, track) => (track.createdAt > acc ? track.createdAt : acc),
        '',
      );
      const isFullyCached = albumId ? (fullyCachedFlags.get(albumId) ?? null) : null;

      albumGroups.push({
        albumId,
        title,
        artistLabel,
        year: null,           // Not in cache DB; UI may enrich later.
        coverUrl: null,       // Resolved by frontend from artwork helpers.
        cachedTracks: albumTracks.sort((a, b) => a.title.localeCompare(b.title)),
        totalSizeBytes,
        worstStatus,
        failedCount,
        isFullyCached,
        dominantQuality,
        mostRecentCachedAt,
      });
    }

    albumGroups.sort((a, b) => a.title.localeCompare(b.title));

    artists.push({
      artistKey,
      artistName: finalArtistName,
      albumGroups,
      totalSizeBytes: albumGroups.reduce((s, g) => s + g.totalSizeBytes, 0),
      totalTracks: artistTracks.length,
    });
  }

  artists.sort((a, b) => a.artistName.localeCompare(b.artistName));
  return artists;
}

type OfflineCacheStats = {
  totalTracks: number;
  readyTracks: number;
  downloadingTracks: number;
  failedTracks: number;
  totalSizeBytes: number;
  limitBytes: number | null;
  cachePath: string;
};

type SortKey = 'alpha' | 'recent' | 'largest' | 'smallest';

class OfflineCacheManagerStore {
  loading = $state(true);
  artists = $state<ArtistGroup[]>([]);
  selectedArtistKey = $state<string | null>(null);
  expandedAlbums = $state<SvelteSet<string>>(new SvelteSet());
  stats = $state<OfflineCacheStats | null>(null);
  sort = $state<SortKey>('alpha');
  showOnlyFailed = $state(false);

  private rawTracks: CachedTrackInfo[] = [];
  private fullyCachedFlags = new Map<string, boolean>();
  private singlesLabel = 'Singles & loose tracks';
  private unlistenStarted: UnlistenFn | null = null;
  private unlistenProgress: UnlistenFn | null = null;
  private unlistenProcessed: UnlistenFn | null = null;
  private unlistenFailed: UnlistenFn | null = null;

  setSinglesLabel(label: string) {
    this.singlesLabel = label;
  }

  async loadAll(): Promise<void> {
    this.loading = true;
    try {
      const tracks = await invoke<CachedTrackInfo[]>('v2_get_cached_tracks');
      this.rawTracks = tracks;

      const albumIds = Array.from(
        new Set(tracks.map(track => track.albumId).filter((id): id is string => !!id)),
      );
      let flags = new Map<string, boolean>();
      if (albumIds.length > 0) {
        const result = await invoke<Record<string, boolean>>(
          'v2_check_albums_fully_cached_batch',
          { albumIds },
        );
        flags = new Map(Object.entries(result));
      }
      this.fullyCachedFlags = flags;

      const stats = await invoke<OfflineCacheStats>('v2_get_offline_cache_stats');
      this.stats = stats;

      this.artists = buildRollup(tracks, this.fullyCachedFlags, this.singlesLabel);
      if (!this.selectedArtistKey && this.artists.length > 0) {
        this.selectedArtistKey = this.artists[0].artistKey;
      }
    } catch (err) {
      console.error('[offlineCacheManager] loadAll failed:', err);
      throw err;
    } finally {
      this.loading = false;
    }
  }

  async subscribeToProgress(): Promise<void> {
    this.unlistenStarted = await listen<{ trackId: number }>(
      'offline:caching_started',
      ev => this.applyProgress(ev.payload.trackId, 'downloading', 0, null),
    );
    this.unlistenProgress = await listen<{ trackId: number; progressPercent: number }>(
      'offline:caching_progress',
      ev => this.applyProgress(ev.payload.trackId, 'downloading', ev.payload.progressPercent ?? 0, null),
    );
    this.unlistenProcessed = await listen<{ trackId: number }>(
      'offline:caching_processed',
      ev => this.applyProgress(ev.payload.trackId, 'ready', 100, null),
    );
    this.unlistenFailed = await listen<{ trackId: number; error: string }>(
      'offline:caching_failed',
      ev => this.applyProgress(ev.payload.trackId, 'failed', 0, ev.payload.error ?? null),
    );
  }

  unsubscribe() {
    this.unlistenStarted?.();
    this.unlistenProgress?.();
    this.unlistenProcessed?.();
    this.unlistenFailed?.();
    this.unlistenStarted = null;
    this.unlistenProgress = null;
    this.unlistenProcessed = null;
    this.unlistenFailed = null;
  }

  private applyProgress(
    trackId: number,
    status: OfflineStatus,
    progressPercent: number,
    errorMessage: string | null,
  ) {
    const idx = this.rawTracks.findIndex(track => track.trackId === trackId);
    if (idx < 0) return;
    const updated: CachedTrackInfo = {
      ...this.rawTracks[idx],
      status,
      progressPercent,
      errorMessage,
    };
    this.rawTracks = [
      ...this.rawTracks.slice(0, idx),
      updated,
      ...this.rawTracks.slice(idx + 1),
    ];
    this.artists = buildRollup(this.rawTracks, this.fullyCachedFlags, this.singlesLabel);
  }

  async removeAlbum(albumId: string): Promise<{ removedTrackIds: number[]; freedBytes: number }> {
    const report = await invoke<{ albumId: string; removedTrackIds: number[]; freedBytes: number }>(
      'v2_remove_cached_album',
      { albumId },
    );
    this.rawTracks = this.rawTracks.filter(track => !report.removedTrackIds.includes(track.trackId));
    this.artists = buildRollup(this.rawTracks, this.fullyCachedFlags, this.singlesLabel);
    return { removedTrackIds: report.removedTrackIds, freedBytes: report.freedBytes };
  }

  async redownloadAlbum(albumId: string, failedOnly: boolean): Promise<{ queuedTrackIds: number[] }> {
    const report = await invoke<{ albumId: string; queuedTrackIds: number[] }>(
      'v2_redownload_cached_album',
      { albumId, failedOnly },
    );
    return { queuedTrackIds: report.queuedTrackIds };
  }

  async redownloadTrack(trackId: number): Promise<void> {
    await invoke('v2_redownload_cached_track', { trackId });
  }

  async removeTrack(trackId: number): Promise<void> {
    await invoke('v2_remove_cached_track', { trackId });
    this.rawTracks = this.rawTracks.filter(track => track.trackId !== trackId);
    this.artists = buildRollup(this.rawTracks, this.fullyCachedFlags, this.singlesLabel);
  }

  selectArtist(key: string) {
    this.selectedArtistKey = key;
  }

  toggleExpand(albumId: string) {
    if (this.expandedAlbums.has(albumId)) this.expandedAlbums.delete(albumId);
    else this.expandedAlbums.add(albumId);
  }

  setSort(sort: SortKey) {
    this.sort = sort;
  }

  setShowOnlyFailed(value: boolean) {
    this.showOnlyFailed = value;
  }

  selectedArtist(): ArtistGroup | null {
    if (!this.selectedArtistKey) return null;
    return this.artists.find(a => a.artistKey === this.selectedArtistKey) ?? null;
  }

  visibleAlbumGroups(): AlbumGroup[] {
    const artist = this.selectedArtist();
    if (!artist) return [];
    let groups = artist.albumGroups;
    if (this.showOnlyFailed) groups = groups.filter(g => g.failedCount > 0);
    const sorted = [...groups];
    switch (this.sort) {
      case 'recent':
        sorted.sort((a, b) => b.mostRecentCachedAt.localeCompare(a.mostRecentCachedAt));
        break;
      case 'largest':
        sorted.sort((a, b) => b.totalSizeBytes - a.totalSizeBytes);
        break;
      case 'smallest':
        sorted.sort((a, b) => a.totalSizeBytes - b.totalSizeBytes);
        break;
      case 'alpha':
      default:
        sorted.sort((a, b) => a.title.localeCompare(b.title));
    }
    return sorted;
  }
}

export const offlineCacheManagerStore = new OfflineCacheManagerStore();
