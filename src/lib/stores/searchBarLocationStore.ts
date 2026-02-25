/**
 * Search Bar Location Store
 *
 * Manages where the search bar is displayed: in the sidebar (default) or
 * in the custom title bar.  Follows the same observable-store pattern as
 * titleBarStore / sidebarStore with localStorage persistence.
 */

const STORAGE_KEY = 'qbz-search-bar-location';

export type SearchBarLocation = 'sidebar' | 'titlebar';

// State
let location: SearchBarLocation = 'sidebar';

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Initialize the store from localStorage
 */
export function initSearchBarLocation(): void {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'sidebar' || saved === 'titlebar') {
      location = saved;
    }
  } catch (e) {
    // localStorage not available
  }
}

/**
 * Subscribe to search bar location changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

/**
 * Get current search bar location preference
 */
export function getSearchBarLocation(): SearchBarLocation {
  return location;
}

/**
 * Set search bar location and persist
 */
export function setSearchBarLocation(value: SearchBarLocation): void {
  location = value;
  try {
    localStorage.setItem(STORAGE_KEY, value);
  } catch (e) {
    // localStorage not available
  }
  notifyListeners();
}

/**
 * Toggle between sidebar and titlebar
 */
export function toggleSearchBarLocation(): void {
  setSearchBarLocation(location === 'sidebar' ? 'titlebar' : 'sidebar');
}
