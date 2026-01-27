/**
 * Global Floating Menu Store
 *
 * Ensures only one floating menu can be open at a time across the entire app.
 * All floating menus (context menus, dropdowns, popovers) should use this store.
 */

type Subscriber = () => void;

let activeMenuId: string | null = null;
const subscribers = new Set<Subscriber>();

function notify() {
  subscribers.forEach(fn => fn());
}

/**
 * Subscribe to store changes
 */
export function subscribe(fn: Subscriber): () => void {
  subscribers.add(fn);
  return () => subscribers.delete(fn);
}

/**
 * Get the currently active menu ID
 */
export function getActiveMenuId(): string | null {
  return activeMenuId;
}

/**
 * Check if a specific menu is currently active
 */
export function isMenuActive(menuId: string): boolean {
  return activeMenuId === menuId;
}

/**
 * Open a menu (closes any other open menu first)
 */
export function openMenu(menuId: string): void {
  if (activeMenuId !== menuId) {
    activeMenuId = menuId;
    notify();
  }
}

/**
 * Close a specific menu (only if it's the active one)
 */
export function closeMenu(menuId: string): void {
  if (activeMenuId === menuId) {
    activeMenuId = null;
    notify();
  }
}

/**
 * Close any open menu
 */
export function closeAll(): void {
  if (activeMenuId !== null) {
    activeMenuId = null;
    notify();
  }
}

/**
 * Standard inactivity timeout for floating menus (in milliseconds)
 */
export const MENU_INACTIVITY_TIMEOUT = 2000;
