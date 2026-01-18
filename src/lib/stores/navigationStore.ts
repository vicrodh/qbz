/**
 * Navigation State Store
 *
 * Manages view navigation and history.
 * Note: Selected album/artist data objects are kept in +page.svelte since they're
 * fetched data, but selectedPlaylistId is managed here as it's just an ID.
 */

export type ViewType = 'home' | 'search' | 'library' | 'library-album' | 'settings' | 'album' | 'artist' | 'playlist' | 'playlist-manager' | 'favorites';
export type FavoritesTab = 'tracks' | 'albums' | 'artists' | 'playlists';

// Navigation state
let activeView: ViewType = 'home';
let viewHistory: ViewType[] = ['home'];
let forwardHistory: ViewType[] = [];

// Selected playlist ID (album/artist are full data objects in +page.svelte)
let selectedPlaylistId: number | null = null;

// Selected local album ID (for library-album view)
let selectedLocalAlbumId: string | null = null;

// Selected Favorites tab (for history-aware navigation)
let selectedFavoritesTab: FavoritesTab = 'tracks';

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

/**
 * Navigate to a view
 */
export function navigateTo(view: ViewType, favoritesTab?: FavoritesTab): void {
  // For favorites view, allow sub-navigation by tracking tab changes
  if (view === 'favorites' && favoritesTab && activeView === 'favorites' && favoritesTab !== selectedFavoritesTab) {
    // Changing tabs within Favorites - push to history
    viewHistory = [...viewHistory, view];
    selectedFavoritesTab = favoritesTab;
    notifyListeners();
    return;
  }

  if (view !== activeView) {
    viewHistory = [...viewHistory, view];
    forwardHistory = [];
    activeView = view;
    
    // Update favorites tab if specified
    if (view === 'favorites' && favoritesTab) {
      selectedFavoritesTab = favoritesTab;
    }
    
    notifyListeners();
  } else if (view === 'favorites' && favoritesTab && favoritesTab !== selectedFavoritesTab) {
    // Same view but different tab - just update tab without history
    selectedFavoritesTab = favoritesTab;
    notifyListeners();
  }
}

/**
 * Go back in history
 * @returns true if navigation happened
 */
export function goBack(): boolean {
  if (viewHistory.length > 1) {
    const lastView = viewHistory[viewHistory.length - 1];
    viewHistory = viewHistory.slice(0, -1);
    forwardHistory = [...forwardHistory, lastView];
    activeView = viewHistory[viewHistory.length - 1];
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
    const nextView = forwardHistory[forwardHistory.length - 1];
    forwardHistory = forwardHistory.slice(0, -1);
    viewHistory = [...viewHistory, nextView];
    activeView = nextView;
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
    navigateTo('playlist');
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
    navigateTo('library-album');
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

// ============ Favorites Tab Selection ============

/**
 * Get selected Favorites tab
 */
export function getSelectedFavoritesTab(): FavoritesTab {
  return selectedFavoritesTab;
}

/**
 * Set selected Favorites tab (creates history entry if already on favorites view)
 */
export function setFavoritesTab(tab: FavoritesTab): void {
  if (activeView === 'favorites' && tab !== selectedFavoritesTab) {
    // Push to history to allow back navigation
    viewHistory = [...viewHistory, 'favorites'];
    selectedFavoritesTab = tab;
    notifyListeners();
  } else if (activeView !== 'favorites') {
    selectedFavoritesTab = tab;
  }
}

// ============ Getters ============

export function getActiveView(): ViewType {
  return activeView;
}

// ============ State Getter ============

export interface NavigationState {
  activeView: ViewType;
  viewHistory: ViewType[];
  forwardHistory: ViewType[];
  selectedPlaylistId: number | null;
  selectedLocalAlbumId: string | null;
  selectedFavoritesTab: FavoritesTab;
  canGoBack: boolean;
  canGoForward: boolean;
}

export function getNavigationState(): NavigationState {
  return {
    activeView,
    viewHistory: [...viewHistory],
    forwardHistory: [...forwardHistory],
    selectedPlaylistId,
    selectedLocalAlbumId,
    selectedFavoritesTab,
    canGoBack: viewHistory.length > 1,
    canGoForward: forwardHistory.length > 0
  };
}
