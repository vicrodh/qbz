/**
 * Navigation State Store
 *
 * Manages view navigation and history.
 * Note: Selected album/artist data objects are kept in +page.svelte since they're
 * fetched data, but selectedPlaylistId is managed here as it's just an ID.
 */

export type ViewType =
  | 'home'
  | 'search'
  | 'library'
  | 'library-album'
  | 'settings'
  | 'album'
  | 'artist'
  | 'playlist'
  | 'playlist-manager'
  | 'favorites-tracks'
  | 'favorites-albums'
  | 'favorites-artists'
  | 'favorites-playlists';
export type FavoritesTab = 'tracks' | 'albums' | 'artists' | 'playlists';

// Navigation state
let activeView: ViewType = 'home';
let viewHistory: ViewType[] = ['home'];
let forwardHistory: ViewType[] = [];

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

/**
 * Navigate to a view
 */
export function navigateTo(view: ViewType): void {
  if (view !== activeView) {
    viewHistory = [...viewHistory, view];
    forwardHistory = [];
    activeView = view;

    const tab = getFavoritesTabFromView(view);
    if (tab) {
      lastFavoritesTab = tab;
    }

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
    const tab = getFavoritesTabFromView(activeView);
    if (tab) {
      lastFavoritesTab = tab;
    }
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
    const tab = getFavoritesTabFromView(activeView);
    if (tab) {
      lastFavoritesTab = tab;
    }
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

// ============ Favorites Navigation ============
export function navigateToFavorites(tab?: FavoritesTab): void {
  const targetTab = tab ?? lastFavoritesTab;
  navigateTo(favoritesViewForTab(targetTab));
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
    canGoBack: viewHistory.length > 1,
    canGoForward: forwardHistory.length > 0
  };
}
