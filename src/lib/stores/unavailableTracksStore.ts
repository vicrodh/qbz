/**
 * Unavailable Tracks Store
 *
 * Tracks which tracks have been detected as unavailable on Qobuz.
 * Persists to localStorage so tracks remain marked after app restart.
 */

const STORAGE_KEY = 'qbz-unavailable-tracks';

// In-memory set of unavailable track IDs
let unavailableTrackIds: Set<number> = new Set();

// Subscribers for reactivity
type Subscriber = () => void;
const subscribers: Set<Subscriber> = new Set();

function notifySubscribers() {
  subscribers.forEach(fn => fn());
}

/**
 * Load unavailable tracks from localStorage
 */
export function loadUnavailableTracks(): void {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const ids = JSON.parse(stored) as number[];
      unavailableTrackIds = new Set(ids);
      console.log(`[UnavailableTracks] Loaded ${unavailableTrackIds.size} unavailable tracks from storage`);
    }
  } catch (err) {
    console.error('[UnavailableTracks] Failed to load from storage:', err);
    unavailableTrackIds = new Set();
  }
}

/**
 * Save unavailable tracks to localStorage
 */
function saveToStorage(): void {
  try {
    const ids = Array.from(unavailableTrackIds);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(ids));
  } catch (err) {
    console.error('[UnavailableTracks] Failed to save to storage:', err);
  }
}

/**
 * Mark a track as unavailable
 */
export function markTrackUnavailable(trackId: number): void {
  if (!unavailableTrackIds.has(trackId)) {
    unavailableTrackIds.add(trackId);
    saveToStorage();
    notifySubscribers();
    console.log(`[UnavailableTracks] Marked track ${trackId} as unavailable`);
  }
}

/**
 * Check if a track is known to be unavailable
 */
export function isTrackUnavailable(trackId: number): boolean {
  return unavailableTrackIds.has(trackId);
}

/**
 * Remove a track from the unavailable list (e.g., when removed from library)
 */
export function clearTrackUnavailable(trackId: number): void {
  if (unavailableTrackIds.has(trackId)) {
    unavailableTrackIds.delete(trackId);
    saveToStorage();
    notifySubscribers();
    console.log(`[UnavailableTracks] Cleared unavailable status for track ${trackId}`);
  }
}

/**
 * Get all unavailable track IDs
 */
export function getUnavailableTrackIds(): Set<number> {
  return new Set(unavailableTrackIds);
}

/**
 * Combined check: a track counts as removed-from-Qobuz when either the API
 * marks it unstreamable or playback hit a TrackUnavailable error this session
 * (cached in the store). Consumer views pass the result to TrackRow's
 * `isUnavailable` prop. Subscribe to the store separately for reactivity.
 */
export function isTrackRemovedFromQobuz(track: { id: number; streamable?: boolean }): boolean {
  if (track.streamable === false) return true;
  return isTrackUnavailable(track.id);
}

/**
 * Subscribe to changes in unavailable tracks
 */
export function subscribe(fn: Subscriber): () => void {
  subscribers.add(fn);
  return () => subscribers.delete(fn);
}

/**
 * Clear all unavailable tracks (for testing/reset)
 */
export function clearAll(): void {
  unavailableTrackIds.clear();
  saveToStorage();
  notifySubscribers();
}
