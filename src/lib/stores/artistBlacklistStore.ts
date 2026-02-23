/**
 * Artist Blacklist Store
 *
 * Manages the local artist blacklist for filtering unwanted artists
 * from search results, radio, and suggestions.
 */

import { invoke } from '@tauri-apps/api/core';

// ============ Types ============

export interface BlacklistedArtist {
  artist_id: number;
  artist_name: string;
  added_at: number;
  notes: string | null;
}

export interface BlacklistSettings {
  enabled: boolean;
}

// ============ State ============

let blacklist: BlacklistedArtist[] = [];
let enabled: boolean = true;
// In-memory Set for O(1) lookups
let blacklistIds: Set<number> = new Set();

const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

function updateIdSet(): void {
  blacklistIds = new Set(blacklist.map(a => a.artist_id));
}

// ============ Public API ============

/**
 * Load blacklist from backend
 */
export async function loadBlacklist(): Promise<BlacklistedArtist[]> {
  const [list, settings] = await Promise.all([
    invoke<BlacklistedArtist[]>('v2_get_artist_blacklist'),
    invoke<BlacklistSettings>('v2_get_blacklist_settings')
  ]);
  blacklist = list;
  enabled = settings.enabled;
  updateIdSet();
  notifyListeners();
  return list;
}

/**
 * Add an artist to the blacklist
 */
export async function addToBlacklist(
  artistId: number,
  artistName: string,
  notes?: string
): Promise<void> {
  await invoke('v2_add_to_artist_blacklist', {
    artistId,
    artistName,
    notes: notes ?? null
  });
  // Reload to get the full entry with timestamp
  await loadBlacklist();
}

/**
 * Remove an artist from the blacklist
 */
export async function removeFromBlacklist(artistId: number): Promise<void> {
  await invoke('v2_remove_from_artist_blacklist', { artistId });
  // Update local state
  blacklist = blacklist.filter(a => a.artist_id !== artistId);
  updateIdSet();
  notifyListeners();
}

/**
 * Check if an artist is blacklisted (O(1) local check)
 */
export function isBlacklisted(artistId: number): boolean {
  if (!enabled) return false;
  return blacklistIds.has(artistId);
}

/**
 * Set the enabled state
 */
export async function setEnabled(value: boolean): Promise<void> {
  await invoke('v2_set_blacklist_enabled', { enabled: value });
  enabled = value;
  notifyListeners();
}

/**
 * Check if blacklist is enabled
 */
export function isEnabled(): boolean {
  return enabled;
}

/**
 * Get cached blacklist (no backend call)
 */
export function getCachedBlacklist(): BlacklistedArtist[] {
  return blacklist;
}

/**
 * Get blacklist count
 */
export function getCount(): number {
  return blacklist.length;
}

/**
 * Clear all blacklisted artists
 */
export async function clearBlacklist(): Promise<void> {
  await invoke('v2_clear_artist_blacklist');
  blacklist = [];
  updateIdSet();
  notifyListeners();
}

/**
 * Subscribe to blacklist changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Initialize blacklist store (call on app startup)
 */
export async function initBlacklistStore(): Promise<void> {
  await loadBlacklist();
}
