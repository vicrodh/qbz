/**
 * Navigation State Store
 *
 * Manages view navigation and history with per-item granularity.
 * History entries carry an optional itemId so back/forward restores
 * the exact page (e.g., a specific album, not just "album view").
 */

export type ViewType =
  | 'home'
  | 'search'
  | 'library'
  | 'library-album'
  | 'settings'
  | 'album'
  | 'artist'
  | 'musician'
  | 'label'
  | 'label-releases'
  | 'award'
  | 'award-albums'
  | 'playlist'
  | 'playlist-manager'
  | 'blacklist-manager'
  | 'favorites-tracks'
  | 'favorites-albums'
  | 'favorites-artists'
  | 'favorites-labels'
  | 'favorites-playlists'
  | 'discover-new-releases'
  | 'discover-ideal-discography'
  | 'discover-top-albums'
  | 'discover-qobuzissimes'
  | 'discover-albums-of-the-week'
  | 'discover-press-accolades'
  | 'discover-playlists'
  | 'discover-release-watch'
  | 'purchases'
  | 'purchase-album'
  | 'dailyq'
  | 'weeklyq'
  | 'favq'
  | 'topq'
  | 'artists-by-location';
export type FavoritesTab = 'tracks' | 'albums' | 'artists' | 'labels' | 'playlists';

// History entry: view + optional item identifier for granular back/forward
export interface HistoryEntry {
  view: ViewType;
  itemId?: string | number;
}

// Navigation state
let activeView: ViewType = 'home';
let activeItemId: string | number | undefined = undefined;
let viewHistory: HistoryEntry[] = [{ view: 'home' }];
let forwardHistory: HistoryEntry[] = [];

// Track whether the current navigation is a back/forward action
let isBackForwardNavigation = false;

// Scroll position memory — keyed by "viewType" or "viewType:itemId"
const SCROLL_TTL_MS = 60 * 60 * 1000; // 1 hour
interface ScrollEntry { scrollTop: number; savedAt: number; }
const scrollPositions = new Map<string, ScrollEntry>();

function scrollKey(view: ViewType, itemId?: string | number): string {
  return itemId != null ? `${view}:${itemId}` : view;
}

// Selected playlist ID (album/artist are full data objects in +page.svelte)
let selectedPlaylistId: number | null = null;

// Selected local album ID (for library-album view)
let selectedLocalAlbumId: string | null = null;

// Last visited Favorites tab (used as default when navigating to Favorites)
let lastFavoritesTab: FavoritesTab = 'tracks';

const favoritesViewMap: Record<FavoritesTab, ViewType> = {
  tracks: 'favorites-tracks',
  albums: 'favorites-albums',
  artists: 'favorites-artists',
  labels: 'favorites-labels',
  playlists: 'favorites-playlists'
};

export function isFavoritesView(view: ViewType): boolean {
  return view.startsWith('favorites-');
}

export function favoritesViewForTab(tab: FavoritesTab): ViewType {
  return favoritesViewMap[tab];
}

export function getFavoritesTabFromView(view: ViewType): FavoritesTab | null {
  switch (view) {
    case 'favorites-tracks':
      return 'tracks';
    case 'favorites-albums':
      return 'albums';
    case 'favorites-artists':
      return 'artists';
    case 'favorites-labels':
      return 'labels';
    case 'favorites-playlists':
      return 'playlists';
    default:
      return null;
  }
}

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Subscribe to navigation state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener(); // Immediately notify with current state
  return () => listeners.delete(listener);
}

// ============ Navigation Actions ============

function entriesEqual(a: HistoryEntry, b: HistoryEntry): boolean {
  return a.view === b.view && a.itemId === b.itemId;
}

/**
 * Navigate to a view, optionally with an item identifier for granular history.
 */
export function navigateTo(view: ViewType, itemId?: string | number): void {
  const newEntry: HistoryEntry = { view, itemId };
  const currentEntry = viewHistory[viewHistory.length - 1];

  if (!entriesEqual(newEntry, currentEntry)) {
    viewHistory = [...viewHistory, newEntry];
    forwardHistory = [];
    activeView = view;
    activeItemId = itemId;

    const tab = getFavoritesTabFromView(view);
    if (tab) {
      lastFavoritesTab = tab;
    }

    isBackForwardNavigation = false;
    notifyListeners();
  }
}

/**
 * Go back in history
 * @returns true if navigation happened
 */
export function goBack(): boolean {
  if (viewHistory.length > 1) {
    const lastEntry = viewHistory[viewHistory.length - 1];
    viewHistory = viewHistory.slice(0, -1);
    forwardHistory = [...forwardHistory, lastEntry];
    const currentEntry = viewHistory[viewHistory.length - 1];
    activeView = currentEntry.view;
    activeItemId = currentEntry.itemId;
    const tab = getFavoritesTabFromView(activeView);
    if (tab) {
      lastFavoritesTab = tab;
    }
    isBackForwardNavigation = true;
    notifyListeners();
    return true;
  }
  return false;
}

/**
 * Go forward in history
 * @returns true if navigation happened
 */
export function goForward(): boolean {
  if (forwardHistory.length > 0) {
    const nextEntry = forwardHistory[forwardHistory.length - 1];
    forwardHistory = forwardHistory.slice(0, -1);
    viewHistory = [...viewHistory, nextEntry];
    activeView = nextEntry.view;
    activeItemId = nextEntry.itemId;
    const tab = getFavoritesTabFromView(activeView);
    if (tab) {
      lastFavoritesTab = tab;
    }
    isBackForwardNavigation = true;
    notifyListeners();
    return true;
  }
  return false;
}

/**
 * Check if we can go back
 */
export function canGoBack(): boolean {
  return viewHistory.length > 1;
}

/**
 * Check if we can go forward
 */
export function canGoForward(): boolean {
  return forwardHistory.length > 0;
}

/**
 * Check if current navigation was triggered by back/forward.
 * Consumers should call this right after receiving a navigation notification
 * and reset it by calling clearBackForwardFlag().
 */
export function isBackForward(): boolean {
  return isBackForwardNavigation;
}

/**
 * Get the active item ID (set during back/forward navigation).
 */
export function getActiveItemId(): string | number | undefined {
  return activeItemId;
}

// ============ Playlist Selection ============

/**
 * Navigate to playlist detail view
 */
export function selectPlaylist(playlistId: number): void {
  const previousId = selectedPlaylistId;
  selectedPlaylistId = playlistId;

  // If already on playlist view, still notify so the component reloads with new ID
  if (activeView === 'playlist' && previousId !== playlistId) {
    notifyListeners();
  } else {
    navigateTo('playlist', playlistId);
  }
}

/**
 * Get selected playlist ID
 */
export function getSelectedPlaylistId(): number | null {
  return selectedPlaylistId;
}

// ============ Local Album Selection ============

/**
 * Navigate to local library album detail view
 */
export function selectLocalAlbum(albumId: string): void {
  const previousId = selectedLocalAlbumId;
  selectedLocalAlbumId = albumId;

  // If already on library-album view, still notify so the component reloads with new ID
  if (activeView === 'library-album' && previousId !== albumId) {
    notifyListeners();
  } else {
    navigateTo('library-album', albumId);
  }
}

/**
 * Clear selected local album (called when navigating back to library)
 */
export function clearLocalAlbum(): void {
  selectedLocalAlbumId = null;
}

/**
 * Get selected local album ID
 */
export function getSelectedLocalAlbumId(): string | null {
  return selectedLocalAlbumId;
}

// ============ Favorites Navigation ============
export function navigateToFavorites(tab?: FavoritesTab): void {
  const targetTab = tab ?? lastFavoritesTab;
  navigateTo(favoritesViewForTab(targetTab));
}

// ============ Session Restore ============

/**
 * Restore a view as initial state (Home always in history root).
 * Used during session restore to set the view without triggering data fetches.
 */
export function restoreView(view: ViewType): void {
  activeView = view;
  activeItemId = undefined;
  viewHistory = [{ view: 'home' }];
  if (view !== 'home') {
    viewHistory.push({ view });
  }
  forwardHistory = [];
  notifyListeners();
}

/**
 * Set playlist ID without triggering navigation (for session restore).
 */
export function setRestoredPlaylistId(playlistId: number): void {
  selectedPlaylistId = playlistId;
}

/**
 * Set local album ID without triggering navigation (for session restore).
 */
export function setRestoredLocalAlbumId(albumId: string): void {
  selectedLocalAlbumId = albumId;
}

// ============ Scroll Position ============

/**
 * Save scroll position for a view (call before navigating away).
 * Pass itemId for item-specific views (album, artist, playlist) so
 * different items don't share the same saved position.
 */
export function saveScrollPosition(view: ViewType, scrollTop: number, itemId?: string | number): void {
  scrollPositions.set(scrollKey(view, itemId), { scrollTop, savedAt: Date.now() });
}

/**
 * Get saved scroll position for a view (0 if none saved or expired).
 * Pass the same itemId used in saveScrollPosition.
 */
export function getSavedScrollPosition(view: ViewType, itemId?: string | number): number {
  const entry = scrollPositions.get(scrollKey(view, itemId));
  if (!entry || Date.now() - entry.savedAt > SCROLL_TTL_MS) return 0;
  return entry.scrollTop;
}

// ============ Getters ============

export function getActiveView(): ViewType {
  return activeView;
}

// ============ State Getter ============

export interface NavigationState {
  activeView: ViewType;
  activeItemId?: string | number;
  viewHistory: HistoryEntry[];
  forwardHistory: HistoryEntry[];
  selectedPlaylistId: number | null;
  selectedLocalAlbumId: string | null;
  canGoBack: boolean;
  canGoForward: boolean;
  isBackForward: boolean;
}

export function getNavigationState(): NavigationState {
  return {
    activeView,
    activeItemId,
    viewHistory: [...viewHistory],
    forwardHistory: [...forwardHistory],
    selectedPlaylistId,
    selectedLocalAlbumId,
    canGoBack: viewHistory.length > 1,
    canGoForward: forwardHistory.length > 0,
    isBackForward: isBackForwardNavigation
  };
}
