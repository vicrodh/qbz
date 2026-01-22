type Listener = (menuId: number | null) => void;

let activeMenuId: number | null = null;
const listeners = new Set<Listener>();

export function getActiveTrackMenuId(): number | null {
  return activeMenuId;
}

export function setActiveTrackMenuId(menuId: number | null): void {
  if (activeMenuId === menuId) return;
  activeMenuId = menuId;
  for (const listener of listeners) listener(activeMenuId);
}

export function subscribeActiveTrackMenuId(listener: Listener): () => void {
  listeners.add(listener);
  listener(activeMenuId);
  return () => {
    listeners.delete(listener);
  };
}

