/**
 * Per-user sidecar overrides for release type (Album/EP/Single/Live/Compilation).
 *
 * Stored in localStorage under the current user, keyed by `${source}|${source_item_id}`.
 * Never written back to Qobuz, the library DB, or a collection's persisted items —
 * overrides are a view-time annotation only so the user can correct obvious
 * mislabels ("this 2-track 'album' is really a single") without mutating any
 * upstream data.
 *
 * Shared between DiscographyBuilderView and MixtapeCollectionDetailView so an
 * override set in one place shows up in the other.
 */

import { writable, get } from 'svelte/store';
import { getUserItem, setUserItem } from '$lib/utils/userStorage';

export type ReleaseType = 'album' | 'ep' | 'single' | 'live' | 'compilation';

export const RELEASE_TYPE_CHOICES: ReleaseType[] = [
  'album',
  'ep',
  'single',
  'live',
  'compilation',
];

const STORAGE_KEY = 'qbz-discography-release-type-overrides';

export const releaseTypeOverrides = writable<Record<string, ReleaseType>>({});

let loaded = false;

export function overrideKey(
  source: string,
  sourceItemId: string,
): string {
  return `${source}|${sourceItemId}`;
}

export function loadReleaseTypeOverrides(): void {
  if (loaded) return;
  try {
    const raw = getUserItem(STORAGE_KEY);
    if (!raw) {
      loaded = true;
      return;
    }
    const parsed = JSON.parse(raw) as Record<string, ReleaseType>;
    if (parsed && typeof parsed === 'object') {
      releaseTypeOverrides.set(parsed);
    }
  } catch (err) {
    console.warn('[releaseTypeOverrides] load failed:', err);
  }
  loaded = true;
}

function persist(next: Record<string, ReleaseType>): void {
  try {
    setUserItem(STORAGE_KEY, JSON.stringify(next));
  } catch (err) {
    console.warn('[releaseTypeOverrides] persist failed:', err);
  }
}

export function setReleaseTypeOverride(
  source: string,
  sourceItemId: string,
  type: ReleaseType,
): void {
  const key = overrideKey(source, sourceItemId);
  const current = get(releaseTypeOverrides);
  const next = { ...current, [key]: type };
  releaseTypeOverrides.set(next);
  persist(next);
}

export function clearReleaseTypeOverride(
  source: string,
  sourceItemId: string,
): void {
  const key = overrideKey(source, sourceItemId);
  const current = get(releaseTypeOverrides);
  if (!(key in current)) return;
  const next = { ...current };
  delete next[key];
  releaseTypeOverrides.set(next);
  persist(next);
}

export function getReleaseTypeOverride(
  source: string,
  sourceItemId: string,
): ReleaseType | null {
  const current = get(releaseTypeOverrides);
  return current[overrideKey(source, sourceItemId)] ?? null;
}

export function hasReleaseTypeOverride(
  source: string,
  sourceItemId: string,
): boolean {
  return getReleaseTypeOverride(source, sourceItemId) !== null;
}
