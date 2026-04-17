/**
 * Award catalog — resolves award ids by name for cases where
 * /album/get returns an award entry without an id. Agnostic, no
 * hardcoded name→id mappings.
 *
 * Two sources feed the catalog, in priority order:
 *   1. Harvested pairs from album responses the user naturally
 *      encounters (Home discover rails, Release Watch, AlbumView,
 *      AwardView, etc). Album responses from /discover/*, /album/get,
 *      /favorite/getNewReleases etc. embed awards with ids already —
 *      we capture them as they flow by. Persisted to sessionStorage
 *      so they survive tab navigation.
 *   2. /award/explore (paginated). Returns a curated subset, only
 *      useful as a last resort.
 *
 * Names compared normalized (NFD + diacritics stripped + lowercase +
 * whitespace collapsed) so accent/case/locale differences still match.
 */

import { invoke } from '@tauri-apps/api/core';

interface ExploreResponse {
  has_more?: boolean;
  items?: Array<{ id?: string | number; name?: string }>;
}

const PAGE_SIZE = 100;
const MAX_PAGES = 40;
// Bump when the catalog population strategy changes so stale
// sessionStorage entries don't shadow a better crawl.
const CATALOG_SCHEMA_VERSION = 2;
const STORAGE_KEY = `qbz-award-catalog-v${CATALOG_SCHEMA_VERSION}`;

// normalized name → id
let byName = new Map<string, string>();
let catalogLoaded = false;
let inflight: Promise<void> | null = null;
let dirty = false;

function normalize(name: string): string {
  return name
    .normalize('NFD')
    .replaceAll(/[\u0300-\u036f]/g, '')
    .trim()
    .toLowerCase()
    .replaceAll(/\s+/g, ' ');
}

function loadFromSession(): void {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw) as Record<string, string>;
    for (const [k, v] of Object.entries(parsed)) {
      if (k && v) byName.set(k, v);
    }
  } catch {
    // ignore
  }
}

function persist(): void {
  if (!dirty) return;
  dirty = false;
  try {
    const obj: Record<string, string> = {};
    for (const [k, v] of byName) obj[k] = v;
    sessionStorage.setItem(STORAGE_KEY, JSON.stringify(obj));
  } catch {
    // ignore (quota / disabled)
  }
}

// Load any previously harvested pairs on module init.
loadFromSession();

/** Record a single (id, name) pair. No-op on empty inputs. */
export function rememberAward(id: string | number | null | undefined, name: string | null | undefined): void {
  if (id == null || !name) return;
  const key = normalize(name);
  if (!key) return;
  const idStr = String(id);
  if (byName.get(key) === idStr) return;
  byName.set(key, idStr);
  dirty = true;
  // Coalesce writes — persist on next microtask.
  queueMicrotask(persist);
}

/** Record every award in a list. Accepts any shape with id and name. */
export function rememberAwards(
  awards: Array<{ id?: string | number | null; name?: string | null }> | null | undefined
): void {
  if (!awards) return;
  for (const a of awards) rememberAward(a?.id, a?.name);
}

/** Walk a list of album-shaped objects and harvest their awards. */
export function rememberAwardsFromAlbums(
  albums: Array<{ awards?: Array<{ id?: string | number | null; name?: string | null }> | null }> | null | undefined
): void {
  if (!albums) return;
  for (const album of albums) if (album?.awards) rememberAwards(album.awards);
}

async function loadCatalog(): Promise<void> {
  if (catalogLoaded) return;
  if (inflight !== null) return inflight;

  inflight = (async () => {
    let offset = 0;
    let pages = 0;
    let totalSeen = 0;
    const seenIds = new Set<string>();
    // Keep paging until the endpoint returns an empty items array —
    // don't early-exit on items.length < PAGE_SIZE, some Qobuz pages
    // return fewer than the limit but still have has_more=true.
    while (pages < MAX_PAGES) {
      try {
        const result = await invoke<ExploreResponse>('v2_get_award_explore', {
          limit: PAGE_SIZE,
          offset,
        });
        const items = result.items ?? [];
        console.log(
          `[AwardCatalog] /award/explore page=${pages + 1} offset=${offset} ` +
          `returned=${items.length} has_more=${result.has_more}`
        );
        if (items.length === 0) {
          console.log('[AwardCatalog] empty page — stopping pagination');
          break;
        }
        // Detect servers that ignore offset (would loop forever returning
        // the same items) by watching for repeated ids on consecutive pages.
        let newOnThisPage = 0;
        for (const item of items) {
          if (item?.id == null || !item?.name) continue;
          const idStr = String(item.id);
          if (!seenIds.has(idStr)) {
            seenIds.add(idStr);
            newOnThisPage += 1;
          }
          byName.set(normalize(item.name), idStr);
        }
        totalSeen += items.length;
        pages += 1;
        if (newOnThisPage === 0) {
          console.log('[AwardCatalog] page repeated prior ids — stopping pagination');
          break;
        }
        if (result.has_more === false) {
          console.log('[AwardCatalog] has_more=false — stopping pagination');
          break;
        }
        offset += items.length;
      } catch (err) {
        console.error('[AwardCatalog] /award/explore failed at offset', offset, err);
        break;
      }
    }
    catalogLoaded = true;
    inflight = null;
    console.log(
      `[AwardCatalog] cache ready: ${byName.size} distinct name keys, ` +
      `${seenIds.size} unique ids, ${totalSeen} items seen across ${pages} page(s)`
    );
  })();

  return inflight;
}

/**
 * Return the award id for a given name. Fetches the /award/explore
 * catalog on first call (cache-first afterwards). Resolves to null
 * if the name isn't in the catalog.
 */
export async function resolveAwardIdByName(name: string): Promise<string | null> {
  if (!name) return null;
  const key = normalize(name);

  // Cache-first: if we already resolved this name (either from a prior
  // lookup or from the catalog), return immediately.
  const cached = byName.get(key);
  if (cached) return cached;

  await loadCatalog();
  const hit = byName.get(key) ?? null;
  if (!hit) {
    console.warn(`[AwardCatalog] no match for award name "${name}" (normalized: "${key}") among ${byName.size} cached entries`);
  }
  return hit;
}

/** Synchronous lookup — hits only the already-loaded catalog. */
export function lookupAwardIdByName(name: string): string | null {
  if (!name) return null;
  return byName.get(normalize(name)) ?? null;
}

export function isCatalogLoaded(): boolean {
  return catalogLoaded;
}
