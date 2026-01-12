/**
 * Navigation State Store
 *
 * Manages view navigation and history.
 * Note: Selected album/artist data objects are kept in +page.svelte since they're
 * fetched data, but selectedPlaylistId is managed here as it's just an ID.
 */

export type ViewType = 'home' | 'search' | 'library' | 'settings' | 'album' | 'artist' | 'playlist' | 'playlist-manager' | 'favorites';

// Navigation state
let activeView: ViewType = 'home';
let viewHistory: ViewType[] = ['home'];
let forwardHistory: ViewType[] = [];

// Selected playlist ID (album/artist are full data objects in +page.svelte)
let selectedPlaylistId: number | null = null;

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
  canGoBack: boolean;
  canGoForward: boolean;
}

export function getNavigationState(): NavigationState {
  return {
    activeView,
    viewHistory: [...viewHistory],
    forwardHistory: [...forwardHistory],
    selectedPlaylistId,
    canGoBack: viewHistory.length > 1,
    canGoForward: forwardHistory.length > 0
  };
}
