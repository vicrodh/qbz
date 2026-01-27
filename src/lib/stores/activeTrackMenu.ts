import { openMenu as openGlobalMenu, closeMenu as closeGlobalMenu, subscribe as subscribeGlobal, getActiveMenuId } from './floatingMenuStore';

type Listener = (menuId: number | null) => void;

let activeMenuId: number | null = null;
const listeners = new Set<Listener>();
let nextMenuId = 1;

// Generate global menu ID for track menus
function getGlobalMenuId(localId: number): string {
  return `track-menu-${localId}`;
}

export function allocateTrackMenuId(): number {
  // Monotonic id to ensure only one TrackMenu instance matches the active id at a time.
  return nextMenuId++;
}

export function getActiveTrackMenuId(): number | null {
  return activeMenuId;
}

export function setActiveTrackMenuId(menuId: number | null): void {
  if (activeMenuId === menuId) return;

  // Close previous in global store
  if (activeMenuId !== null) {
    closeGlobalMenu(getGlobalMenuId(activeMenuId));
  }

  activeMenuId = menuId;

  // Open new in global store
  if (menuId !== null) {
    openGlobalMenu(getGlobalMenuId(menuId));
  }

  for (const listener of listeners) listener(activeMenuId);
}

export function subscribeActiveTrackMenuId(listener: Listener): () => void {
  listeners.add(listener);
  listener(activeMenuId);
  return () => {
    listeners.delete(listener);
  };
}

// Subscribe to global store to close track menus when other menus open
subscribeGlobal(() => {
  const globalActive = getActiveMenuId();
  // If global active menu is not a track menu, close any open track menu
  if (globalActive !== null && !globalActive.startsWith('track-menu-')) {
    if (activeMenuId !== null) {
      activeMenuId = null;
      for (const listener of listeners) listener(null);
    }
  }
});
