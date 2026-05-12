/**
 * Chunked album store for the Local Library Albums grid.
 *
 * Phase A of the Plex-style pagination port: instead of loading all
 * ~1200 albums into memory at once and letting Svelte's reactivity
 * track each one, we keep a sparse `Map<chunkIdx, LocalAlbum[]>` and
 * fetch chunks on demand as the recycling-pool virtualizer reports
 * which indices it's about to bind.
 *
 * The store deliberately uses a plain class instead of a Svelte 5
 * `$state` rune for the chunk Map. Mutating a $state-backed Map only
 * triggers reactivity on Map identity changes, so deeply nested writes
 * (`chunks.set(0, [...])`) wouldn't propagate. We sidestep that by
 * pairing the plain Map with a single `version` $state — every chunk
 * write bumps the version, and consumers (the grid pool) bind to that
 * version via the pool's `dataVersion` prop. When the version changes,
 * the pool's template re-evaluates `getAlbum` for every slot, which
 * reads from the now-updated Map.
 *
 * Invalidation: changing search/sort/source filters bumps a separate
 * `paramsVersion` that clears all chunks and triggers a fresh fetch of
 * chunk 0 + total. Chunk-in-flight dedup is via a Set<chunkIdx> guard.
 */

import { invoke } from '@tauri-apps/api/core';

/** Shape-compatible with LocalLibraryView's local `LocalAlbum` interface
 *  so the snippet inside the grid pool can pass an item to legacy
 *  helpers (toggleAlbumSelect, getAlbumQualityBadge, etc.) without a
 *  cast. Rust's None serializes to JSON null at runtime; TypeScript
 *  marks the fields optional so `?? fallback` and truthy checks work
 *  for both null and undefined. */
export interface LocalAlbumLite {
  id: string;
  title: string;
  artist: string;
  all_artists?: string;
  year?: number;
  catalog_number?: string;
  artwork_path?: string;
  track_count: number;
  total_duration_secs: number;
  format: string;
  bit_depth?: number;
  sample_rate: number;
  directory_path: string;
  source_folders?: string;
  source: string;
}

export interface AlbumsPageResult {
  albums: LocalAlbumLite[];
  total: number;
}

export interface LocalAlbumsQueryParams {
  search: string;
  sortBy: 'artist' | 'title' | 'year';
  sortDir: 'asc' | 'desc';
  excludeNetworkFolders: boolean;
  /** Include Plex cache albums in the union. The backend ATTACHes
   *  `plex_cache.db` and UNIONs the aggregations so sort/filter/pagination
   *  apply to both sources as a single result set. */
  includePlex: boolean;
}

export const CHUNK_SIZE = 100;

export class LocalAlbumsChunkedStore {
  /** Bumped on every chunk write — consumers bind to it to know when
   *  to re-read `getAlbum`. Wrapped in `$state` so Svelte tracks it. */
  version = $state(0);

  /** Reset whenever `total` is updated (initial fetch or filter
   *  change). Consumers use it to refresh the scrollbar pre-allocation
   *  and to know when zero-result is final vs still-loading. */
  total = $state(0);

  /** True while the very first page (chunk 0) is in flight — used by
   *  callers to show a top-level loading state vs a "no results" empty
   *  state. Per-chunk in-flight is separate. */
  isInitialLoading = $state(false);

  /** Last completed params snapshot. Compared on `setParams` to skip
   *  no-op reconfigurations. */
  private currentParams: LocalAlbumsQueryParams | null = null;

  private chunks: Map<number, LocalAlbumLite[]> = new Map();
  private inFlight: Set<number> = new Set();
  /** Generation counter — bumped on every params change. In-flight
   *  responses are dropped if their captured generation no longer
   *  matches `latestGeneration` (the user scrolled to a chunk and
   *  then switched filters before the chunk arrived). */
  private latestGeneration = 0;

  /** Returns the album at the given flat index, or `null` if the chunk
   *  containing that index isn't loaded yet. The caller (recycling grid
   *  pool) shows a placeholder for nulls and fires `requestIndex` so
   *  the chunk loads. */
  getAlbum(globalIdx: number): LocalAlbumLite | null {
    if (globalIdx < 0 || globalIdx >= this.total) return null;
    const chunkIdx = Math.floor(globalIdx / CHUNK_SIZE);
    const chunk = this.chunks.get(chunkIdx);
    if (!chunk) return null;
    return chunk[globalIdx % CHUNK_SIZE] ?? null;
  }

  /** Trigger a chunk fetch for the chunk containing `globalIdx`, if
   *  not already loaded or in flight. Safe to call on every visible
   *  slot every scroll frame — dedup is internal. */
  requestIndex(globalIdx: number): void {
    if (globalIdx < 0 || globalIdx >= this.total) return;
    const chunkIdx = Math.floor(globalIdx / CHUNK_SIZE);
    if (this.chunks.has(chunkIdx) || this.inFlight.has(chunkIdx)) return;
    void this.loadChunk(chunkIdx);
  }

  /** Reset chunks + total and kick off chunk 0 with the new params. */
  async setParams(params: LocalAlbumsQueryParams): Promise<void> {
    if (this.currentParams && this.paramsEqual(this.currentParams, params)) return;
    this.currentParams = { ...params };
    this.latestGeneration++;
    this.chunks.clear();
    this.inFlight.clear();
    this.total = 0;
    this.isInitialLoading = true;
    this.version++;
    await this.loadChunk(0, true);
  }

  /** Drop everything and force a fresh re-fetch of chunk 0. Used when
   *  external state changes invalidate the cached data (e.g. a new
   *  album is added, a folder is rescanned). */
  invalidate(): void {
    if (!this.currentParams) return;
    this.latestGeneration++;
    this.chunks.clear();
    this.inFlight.clear();
    this.total = 0;
    this.version++;
    void this.loadChunk(0, true);
  }

  private async loadChunk(chunkIdx: number, isInitial = false): Promise<void> {
    if (!this.currentParams) return;
    if (this.inFlight.has(chunkIdx)) return;
    this.inFlight.add(chunkIdx);
    const generation = this.latestGeneration;
    const params = this.currentParams;

    try {
      const result = await invoke<AlbumsPageResult>('v2_library_get_albums_page', {
        offset: chunkIdx * CHUNK_SIZE,
        limit: CHUNK_SIZE,
        search: params.search.trim() || null,
        sortBy: params.sortBy,
        sortDir: params.sortDir,
        excludeNetworkFolders: params.excludeNetworkFolders,
        includePlex: params.includePlex,
      });
      // Discard if filter changed mid-flight.
      if (generation !== this.latestGeneration) return;
      this.chunks.set(chunkIdx, result.albums);
      this.total = result.total;
      this.version++;
    } catch (err) {
      console.error(
        '[localAlbumsChunkedStore] loadChunk failed',
        chunkIdx,
        err,
      );
    } finally {
      this.inFlight.delete(chunkIdx);
      if (isInitial) this.isInitialLoading = false;
    }
  }

  private paramsEqual(a: LocalAlbumsQueryParams, b: LocalAlbumsQueryParams): boolean {
    return (
      a.search === b.search &&
      a.sortBy === b.sortBy &&
      a.sortDir === b.sortDir &&
      a.excludeNetworkFolders === b.excludeNetworkFolders &&
      a.includePlex === b.includePlex
    );
  }
}
