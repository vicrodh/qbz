/**
 * Playback Context Store
 *
 * Manages the semantic origin of playback (album, playlist, artist top, etc.)
 * Determines what "next" means and displays the stack icon.
 */

import { invoke } from '@tauri-apps/api/core';

// ============ Types ============

export type ContextType = 'album' | 'playlist' | 'artist_top' | 'label_top' | 'home_list' | 'favorites' | 'local_library' | 'radio' | 'search';
export type ContentSource = 'qobuz' | 'local' | 'plex';

export interface PlaybackContext {
  type: ContextType;
  id: string;
  label: string;
  source: ContentSource;
  track_ids: number[];
  current_position: number;
}

// ============ State ============

let currentContext: PlaybackContext | null = null;
let pendingFocus: { contextType: ContextType; contextId: string; trackId: number } | null = null;
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

// ============ Public API ============

/**
 * Set a playback context (e.g., when playing from album/playlist)
 */
export async function setPlaybackContext(
  contextType: ContextType,
  id: string,
  label: string,
  source: ContentSource,
  trackIds: number[],
  startPosition: number
): Promise<void> {
  console.log('[PlaybackContext] Setting context:', { contextType, id, label, trackIds: trackIds.length, startPosition });
  
  await invoke('v2_set_playback_context', {
    contextType,
    id,
    label,
    source,
    trackIds,
    startPosition
  });

  // Update local state
  currentContext = {
    type: contextType,
    id,
    label,
    source,
    track_ids: trackIds,
    current_position: startPosition
  };

  console.log('[PlaybackContext] Context set, notifying listeners. Current context:', currentContext);
  notifyListeners();
}

/**
 * Clear the playback context (e.g., when playing single track)
 */
export async function clearPlaybackContext(): Promise<void> {
  console.log('[PlaybackContext] Clearing context');
  await invoke('v2_clear_playback_context');
  currentContext = null;
  console.log('[PlaybackContext] Context cleared, notifying listeners');
  notifyListeners();
}

/**
 * Get the current playback context
 */
export async function getPlaybackContext(): Promise<PlaybackContext | null> {
  const context = await invoke<PlaybackContext | null>('v2_get_playback_context');
  currentContext = context;
  notifyListeners();
  return context;
}

/**
 * Check if a context is active
 */
export async function hasPlaybackContext(): Promise<boolean> {
  return await invoke<boolean>('v2_has_playback_context');
}

/**
 * Get the current context (cached, no backend call)
 */
export function getCurrentContext(): PlaybackContext | null {
  return currentContext;
}

export function requestContextTrackFocus(
  contextType: ContextType,
  contextId: string,
  trackId: number
): void {
  pendingFocus = { contextType, contextId, trackId };
}

export function consumeContextTrackFocus(
  contextType: ContextType,
  contextId: string
): number | null {
  if (!pendingFocus) return null;
  if (pendingFocus.contextType !== contextType || pendingFocus.contextId !== contextId) {
    return null;
  }
  const { trackId } = pendingFocus;
  pendingFocus = null;
  return trackId;
}

/**
 * Get display info for the current context
 */
export function getContextDisplayInfo(): string | null {
  if (!currentContext) return null;

  const typeStr: Record<ContextType, string> = {
    album: 'Album',
    playlist: 'Playlist',
    artist_top: 'Artist Top Songs',
    label_top: 'Label Top Songs',
    home_list: 'Home List',
    favorites: 'Favorites',
    local_library: 'Local Library',
    radio: 'Radio',
    search: 'Search Results'
  };

  const displayType = typeStr[currentContext.type];

  return `${displayType} · ${currentContext.label}`;
}

/**
 * Subscribe to context changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Initialize context store (call on app startup)
 */
export async function initPlaybackContextStore(): Promise<void> {
  // Sync initial state from backend
  await getPlaybackContext();
}

// Export a reactive getter for Svelte components
export function usePlaybackContext() {
  let context = currentContext;

  const unsubscribe = subscribe(() => {
    context = currentContext;
  });

  return {
    get context() {
      return context;
    },
    unsubscribe
  };
}
