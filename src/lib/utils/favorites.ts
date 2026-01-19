import type { FavoritesTab } from '$lib/stores/navigationStore';

const FALLBACK_ORDER: FavoritesTab[] = ['tracks', 'albums', 'artists', 'playlists'];

export function normalizeFavoritesTabOrder(order?: string[] | null): FavoritesTab[] {
  const result: FavoritesTab[] = [];
  const seen = new Set<FavoritesTab>();

  if (Array.isArray(order)) {
    for (const entry of order) {
      if (FALLBACK_ORDER.includes(entry as FavoritesTab)) {
        const tab = entry as FavoritesTab;
        if (!seen.has(tab)) {
          seen.add(tab);
          result.push(tab);
        }
      }
    }
  }

  for (const tab of FALLBACK_ORDER) {
    if (!seen.has(tab)) {
      result.push(tab);
    }
  }

  return result;
}

export function getDefaultFavoritesTab(order?: string[] | null): FavoritesTab {
  return normalizeFavoritesTabOrder(order)[0] ?? 'tracks';
}
