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

// ============================================
// Content Sidebars (Lyrics, Queue, Network)
// Only one can be open at a time
// ============================================

export type ContentSidebarType = 'lyrics' | 'queue' | 'network' | null;

let activeContentSidebar: ContentSidebarType = null;
const contentSidebarListeners = new Set<(type: ContentSidebarType) => void>();

function notifyContentSidebarListeners(): void {
  for (const listener of contentSidebarListeners) {
    listener(activeContentSidebar);
  }
}

/**
 * Subscribe to content sidebar changes
 */
export function subscribeContentSidebar(listener: (type: ContentSidebarType) => void): () => void {
  contentSidebarListeners.add(listener);
  listener(activeContentSidebar);
  return () => contentSidebarListeners.delete(listener);
}

/**
 * Get active content sidebar
 */
export function getActiveContentSidebar(): ContentSidebarType {
  return activeContentSidebar;
}

/**
 * Open a content sidebar (closes others)
 */
export function openContentSidebar(type: ContentSidebarType): void {
  activeContentSidebar = type;
  notifyContentSidebarListeners();
}

/**
 * Close a content sidebar
 */
export function closeContentSidebar(type: ContentSidebarType): void {
  if (activeContentSidebar === type) {
    activeContentSidebar = null;
    notifyContentSidebarListeners();
  }
}

/**
 * Toggle a content sidebar
 */
export function toggleContentSidebar(type: ContentSidebarType): void {
  if (activeContentSidebar === type) {
    activeContentSidebar = null;
  } else {
    activeContentSidebar = type;
  }
  notifyContentSidebarListeners();
}
