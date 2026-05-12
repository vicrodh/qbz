/**
 * Discovery V2 — per-tab section preferences.
 *
 * Each tab (Home / Editor's Picks / For You) carries its own ordered list
 * of section preferences. All three tabs draw from the same V2 fetches
 * (release-watch + discover-index + home-resolved), so swapping the
 * "Editor's Picks" tab just renders a different curated subset in a
 * different order — no extra round-trips.
 *
 * Defaults reflect a familiar Qobuz-style split:
 *   - Home: balanced, both editorial and personalized sections.
 *   - Editor's Picks: only editorial / curatorial sections.
 *   - For You: only personalized / play-history sections.
 *
 * Users can customize any tab independently via the gear icon in the
 * toolbar; the modal targets the currently-active tab.
 */

import { writable, get } from 'svelte/store';

export type DiscoveryTab = 'home' | 'editorPicks' | 'forYou';

export type DiscoverySectionId =
  // Universal — can appear in any tab.
  | 'newReleases'
  | 'pressAwards'
  | 'qobuzPlaylists'
  | 'recentlyPlayedAlbums'
  | 'continueListening'
  | 'idealDiscography'
  | 'mostStreamed'
  | 'releaseWatch'
  | 'editorPicks'
  | 'qobuzissimes'
  | 'topArtists'
  | 'favoriteAlbums'
  // Home + For You (the legacy DailyQ/WeeklyQ/FavQ/TopQ mixes panel).
  | 'qobuzMixes'
  // For-You-exclusive.
  | 'radioStations'
  | 'similarAlbums'
  | 'rediscoverLibrary'
  | 'essentialsByGenre'
  | 'artistsToFollow'
  | 'artistSpotlight';

export interface DiscoverySectionPref {
  id: DiscoverySectionId;
  enabled: boolean;
}

export type DiscoverySectionPrefsByTab = Record<DiscoveryTab, DiscoverySectionPref[]>;

/**
 * The set of section IDs each tab supports is defined by which IDs appear
 * in its DEFAULT_PREFS list. The customize modal only shows the prefs for
 * the active tab, so tab-exclusive sections (artistSpotlight, radioStations,
 * etc. on For You) naturally hide on the other tabs' modals.
 */
const DEFAULT_PREFS: DiscoverySectionPrefsByTab = {
  // Home: 12 universal sections + qobuzMixes. Editorial + personalized
  // intermixed. User can customize order/enabled.
  home: [
    { id: 'newReleases', enabled: true },
    { id: 'pressAwards', enabled: true },
    { id: 'qobuzPlaylists', enabled: true },
    { id: 'recentlyPlayedAlbums', enabled: true },
    { id: 'continueListening', enabled: true },
    { id: 'idealDiscography', enabled: true },
    { id: 'mostStreamed', enabled: true },
    { id: 'qobuzMixes', enabled: false },
    { id: 'releaseWatch', enabled: false },
    { id: 'editorPicks', enabled: false },
    { id: 'qobuzissimes', enabled: false },
    { id: 'topArtists', enabled: false },
    { id: 'favoriteAlbums', enabled: false },
  ],
  // Editor's Picks: editorial-only. Personalized sections aren't even
  // available — they don't belong in a curated tab.
  editorPicks: [
    { id: 'newReleases', enabled: true },
    { id: 'editorPicks', enabled: true },
    { id: 'qobuzissimes', enabled: true },
    { id: 'pressAwards', enabled: true },
    { id: 'mostStreamed', enabled: true },
    { id: 'idealDiscography', enabled: true },
    { id: 'qobuzPlaylists', enabled: true },
  ],
  // For You: personalized base + the legacy ForYouTab exclusives
  // (qobuzMixes, radioStations, similarAlbums, rediscoverLibrary,
  // essentialsByGenre, artistsToFollow, artistSpotlight). The
  // editorial sections (newReleases, etc.) are intentionally NOT in
  // this tab's available list.
  forYou: [
    { id: 'qobuzMixes', enabled: true },
    { id: 'releaseWatch', enabled: true },
    { id: 'radioStations', enabled: true },
    { id: 'continueListening', enabled: true },
    { id: 'recentlyPlayedAlbums', enabled: true },
    { id: 'topArtists', enabled: true },
    { id: 'favoriteAlbums', enabled: true },
    { id: 'similarAlbums', enabled: true },
    { id: 'rediscoverLibrary', enabled: true },
    { id: 'essentialsByGenre', enabled: true },
    { id: 'artistsToFollow', enabled: true },
    { id: 'artistSpotlight', enabled: true },
  ],
};

const STORAGE_KEY = 'qbz.discovery-v2.section-prefs';

const TABS: DiscoveryTab[] = ['home', 'editorPicks', 'forYou'];

function loadPersisted(): DiscoverySectionPrefsByTab {
  if (typeof localStorage === 'undefined') return DEFAULT_PREFS;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_PREFS;
    const parsed = JSON.parse(raw);
    return migrate(parsed);
  } catch {
    return DEFAULT_PREFS;
  }
}

/**
 * Reconcile a persisted blob against the canonical structure. Handles
 * three shapes:
 *   - Legacy flat array (V1 of sectionPrefs, only had Home prefs) →
 *     becomes `home`, other tabs get defaults.
 *   - New per-tab object with partial data → fills missing tabs with
 *     defaults, fills missing sections inside each tab with defaults.
 *   - Anything else → defaults.
 */
function migrate(persisted: unknown): DiscoverySectionPrefsByTab {
  if (Array.isArray(persisted)) {
    return {
      home: reconcileList(persisted as DiscoverySectionPref[], DEFAULT_PREFS.home),
      editorPicks: DEFAULT_PREFS.editorPicks,
      forYou: DEFAULT_PREFS.forYou,
    };
  }
  if (persisted && typeof persisted === 'object') {
    const blob = persisted as Partial<DiscoverySectionPrefsByTab>;
    return {
      home: reconcileList(blob.home, DEFAULT_PREFS.home),
      editorPicks: reconcileList(blob.editorPicks, DEFAULT_PREFS.editorPicks),
      forYou: reconcileList(blob.forYou, DEFAULT_PREFS.forYou),
    };
  }
  return DEFAULT_PREFS;
}

function reconcileList(
  persisted: DiscoverySectionPref[] | undefined,
  fallback: DiscoverySectionPref[]
): DiscoverySectionPref[] {
  if (!Array.isArray(persisted)) return fallback;
  const validIds = new Set(fallback.map((p) => p.id));
  const seen = new Set<DiscoverySectionId>();
  const kept: DiscoverySectionPref[] = [];
  for (const item of persisted) {
    if (!item || typeof item.id !== 'string') continue;
    if (!validIds.has(item.id as DiscoverySectionId)) continue;
    if (seen.has(item.id as DiscoverySectionId)) continue;
    seen.add(item.id as DiscoverySectionId);
    kept.push({ id: item.id as DiscoverySectionId, enabled: !!item.enabled });
  }
  for (const def of fallback) {
    if (!seen.has(def.id)) kept.push(def);
  }
  return kept;
}

function persist(prefs: DiscoverySectionPrefsByTab) {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    /* quota / storage disabled — ignore */
  }
}

export const sectionPrefs = writable<DiscoverySectionPrefsByTab>(loadPersisted());

sectionPrefs.subscribe((value) => persist(value));

export function getTabPrefs(tab: DiscoveryTab): DiscoverySectionPref[] {
  return get(sectionPrefs)[tab];
}

export function toggleSection(tab: DiscoveryTab, id: DiscoverySectionId) {
  sectionPrefs.update((prefs) => ({
    ...prefs,
    [tab]: prefs[tab].map((p) => (p.id === id ? { ...p, enabled: !p.enabled } : p)),
  }));
}

export function moveSection(tab: DiscoveryTab, id: DiscoverySectionId, direction: -1 | 1) {
  sectionPrefs.update((prefs) => {
    const list = prefs[tab];
    const idx = list.findIndex((p) => p.id === id);
    if (idx < 0) return prefs;
    const target = idx + direction;
    if (target < 0 || target >= list.length) return prefs;
    const next = list.slice();
    [next[idx], next[target]] = [next[target], next[idx]];
    return { ...prefs, [tab]: next };
  });
}

export function resetToDefaults(tab: DiscoveryTab) {
  sectionPrefs.update((prefs) => ({ ...prefs, [tab]: DEFAULT_PREFS[tab] }));
}

export function isEnabled(tab: DiscoveryTab, id: DiscoverySectionId): boolean {
  return get(sectionPrefs)[tab].find((p) => p.id === id)?.enabled ?? false;
}

export function enabledCount(tab: DiscoveryTab): number {
  return get(sectionPrefs)[tab].filter((p) => p.enabled).length;
}

void TABS; // canonical tab list, retained for future iteration helpers
