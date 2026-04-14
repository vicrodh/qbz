/**
 * Award catalog — resolves award ids by name for the cases where
 * /album/get returns an award with only a name and no id. Sources:
 *   1. Hardcoded seed of Qobuz editorial awards with stable ids and
 *      known locale aliases (Qobuzissime / Album of the Week / Ideal
 *      Discography) — resolves instantly, survives locale changes.
 *   2. /award/explore catalog fetched lazily on first unknown name.
 *
 * Names are compared normalized: lowercased + diacritics stripped +
 * whitespace collapsed, so "Discoteca Ideal Qobuz",
 * "Qobuz Ideal Discography" and "Discothèque Idéale Qobuz" all hash
 * to the same key for their respective aliases.
 */

import { invoke } from '@tauri-apps/api/core';

interface ExploreResponse {
  has_more?: boolean;
  items?: Array<{ id?: string | number; name?: string }>;
}

const PAGE_SIZE = 100;

/** Known Qobuz editorial awards. IDs captured from the API (stable,
 *  locale-independent). Names include every locale we ship. */
const SEED_AWARDS: Array<{ id: string; aliases: string[] }> = [
  {
    id: '88',
    aliases: ['Qobuzissime'],
  },
  {
    id: '151',
    aliases: [
      'Qobuz Album of the Week',
      'Álbum de la semana Qobuz',
      'Album der Woche Qobuz',
      'Album de la semaine Qobuz',
      'Álbum da semana Qobuz',
    ],
  },
  {
    id: '70',
    aliases: [
      'Qobuz Ideal Discography',
      'Discoteca Ideal Qobuz',
      'Diskografie Qobuz',
      'Ideale Qobuz-Diskografie',
      'Discothèque idéale Qobuz',
      'Discografia ideal Qobuz',
    ],
  },
];

// normalized name → id
let byName = new Map<string, string>();
let seeded = false;
let catalogLoaded = false;
let inflight: Promise<void> | null = null;

function normalize(name: string): string {
  return name
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .trim()
    .toLowerCase()
    .replace(/\s+/g, ' ');
}

function seed() {
  if (seeded) return;
  for (const entry of SEED_AWARDS) {
    for (const alias of entry.aliases) {
      byName.set(normalize(alias), entry.id);
    }
  }
  seeded = true;
}

async function loadCatalog(): Promise<void> {
  if (catalogLoaded) return;
  if (inflight) return inflight;
  seed();

  inflight = (async () => {
    let offset = 0;
    while (true) {
      try {
        const result = await invoke<ExploreResponse>('v2_get_award_explore', {
          limit: PAGE_SIZE,
          offset,
        });
        const items = result.items ?? [];
        for (const item of items) {
          if (item?.id == null || !item?.name) continue;
          byName.set(normalize(item.name), String(item.id));
        }
        if (items.length < PAGE_SIZE) break;
        if (result.has_more === false) break;
        offset += items.length;
        // Reasonable upper bound to avoid infinite loops on a broken API.
        if (offset >= 2000) break;
      } catch (err) {
        console.error('[AwardCatalog] explore failed:', err);
        break;
      }
    }
    catalogLoaded = true;
    inflight = null;
  })();

  return inflight;
}

/**
 * Return the award id for a given name. Tries the seeded mapping
 * first (instant for editorial Qobuz awards in any locale), then
 * falls back to the /award/explore catalog (fetched once per
 * session). Resolves to null if still not found.
 */
export async function resolveAwardIdByName(name: string): Promise<string | null> {
  if (!name) return null;
  seed();
  const key = normalize(name);
  const fromSeed = byName.get(key);
  if (fromSeed) return fromSeed;
  await loadCatalog();
  return byName.get(key) ?? null;
}

/** Synchronous lookup — hits only the seed + already-loaded catalog. */
export function lookupAwardIdByName(name: string): string | null {
  if (!name) return null;
  seed();
  return byName.get(normalize(name)) ?? null;
}

export function isCatalogLoaded(): boolean {
  return catalogLoaded;
}
