/**
 * Sidebar State Store
 *
 * Manages sidebar expanded/collapsed state with localStorage persistence.
 */

const STORAGE_KEY = 'qbz-sidebar-expanded';

// State
let isExpanded = true;

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
export function initSidebarStore(): void {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved !== null) {
      isExpanded = saved === 'true';
    }
  } catch (e) {
    // localStorage not available
  }
}

/**
 * Subscribe to sidebar state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener(); // Immediately notify with current state
  return () => listeners.delete(listener);
}

/**
 * Get current expanded state
 */
export function getIsExpanded(): boolean {
  return isExpanded;
}

/**
 * Set expanded state and persist to localStorage
 */
function setExpanded(value: boolean): void {
  isExpanded = value;
  try {
    localStorage.setItem(STORAGE_KEY, String(value));
  } catch (e) {
    // localStorage not available
  }
  notifyListeners();
}

/**
 * Expand the sidebar
 */
export function expandSidebar(): void {
  setExpanded(true);
}

/**
 * Collapse the sidebar
 */
export function collapseSidebar(): void {
  setExpanded(false);
}

/**
 * Toggle sidebar expanded state
 */
export function toggleSidebar(): void {
  setExpanded(!isExpanded);
}
