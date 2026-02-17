/**
 * Keybindings Store
 *
 * Centralized keybindings system with:
 * - Default shortcuts for all actions
 * - User customization support
 * - Conflict detection
 * - Persistence to localStorage
 */

import {
  eventToShortcut,
  formatShortcutDisplay,
  isInputTarget,
} from '$lib/utils/keyboard';

// Re-export utilities for convenience
export { eventToShortcut, formatShortcutDisplay, isInputTarget };

// ============================================================================
// Types
// ============================================================================

export type KeybindingCategory = 'playback' | 'navigation' | 'ui' | 'focus';

export interface KeybindingAction {
  /** Unique identifier, e.g., 'playback.toggle' */
  id: string;
  /** Label for UI display */
  label: string;
  /** Category for grouping in UI */
  category: KeybindingCategory;
  /** Default shortcut */
  defaultShortcut: string;
  /** Optional description for tooltips */
  description?: string;
  /** If true, only works in certain contexts (e.g., focus mode) */
  contextual?: boolean;
}

export interface CategoryInfo {
  id: KeybindingCategory;
  label: string;
  description: string;
}

// ============================================================================
// Constants
// ============================================================================

export const CATEGORIES: CategoryInfo[] = [
  { id: 'playback', label: 'Playback', description: 'Playback controls' },
  { id: 'navigation', label: 'Navigation', description: 'App navigation' },
  { id: 'ui', label: 'Interface', description: 'UI elements' },
  { id: 'focus', label: 'Focus Mode', description: 'Focus mode controls' },
];

export const ACTIONS: KeybindingAction[] = [
  // Playback
  {
    id: 'playback.toggle',
    label: 'Play / Pause',
    category: 'playback',
    defaultShortcut: 'Space',
    description: 'Toggle playback',
  },
  {
    id: 'playback.next',
    label: 'Next Track',
    category: 'playback',
    defaultShortcut: 'Ctrl+ArrowRight',
    description: 'Skip to next track',
  },
  {
    id: 'playback.prev',
    label: 'Previous Track',
    category: 'playback',
    defaultShortcut: 'Ctrl+ArrowLeft',
    description: 'Go to previous track',
  },

  // Navigation
  {
    id: 'nav.back',
    label: 'Go Back',
    category: 'navigation',
    defaultShortcut: 'Alt+ArrowLeft',
    description: 'Navigate to previous page',
  },
  {
    id: 'nav.forward',
    label: 'Go Forward',
    category: 'navigation',
    defaultShortcut: 'Alt+ArrowRight',
    description: 'Navigate to next page',
  },
  {
    id: 'nav.search',
    label: 'Search',
    category: 'navigation',
    defaultShortcut: 'Ctrl+f',
    description: 'Focus search field',
  },

  // UI
  {
    id: 'ui.focusMode',
    label: 'Focus Mode',
    category: 'ui',
    defaultShortcut: 'f',
    description: 'Toggle focus mode',
  },
  {
    id: 'ui.queue',
    label: 'Queue',
    category: 'ui',
    defaultShortcut: 'q',
    description: 'Toggle queue panel',
  },
  {
    id: 'ui.escape',
    label: 'Close / Dismiss',
    category: 'ui',
    defaultShortcut: 'Escape',
    description: 'Close active panel or modal',
  },
  {
    id: 'ui.showShortcuts',
    label: 'Show Shortcuts',
    category: 'ui',
    defaultShortcut: '?',
    description: 'Show keyboard shortcuts modal',
  },
  {
    id: 'ui.openLink',
    label: 'Open Qobuz Link',
    category: 'ui',
    defaultShortcut: 'Ctrl+l',
    description: 'Paste and open a Qobuz link',
  },

  // Focus Mode specific
  {
    id: 'focus.seekForward',
    label: 'Seek Forward (5s)',
    category: 'focus',
    defaultShortcut: 'ArrowRight',
    description: 'Seek forward 5 seconds',
    contextual: true,
  },
  {
    id: 'focus.seekBack',
    label: 'Seek Back (5s)',
    category: 'focus',
    defaultShortcut: 'ArrowLeft',
    description: 'Seek back 5 seconds',
    contextual: true,
  },
  {
    id: 'focus.seekForwardLong',
    label: 'Seek Forward (10s)',
    category: 'focus',
    defaultShortcut: 'Shift+ArrowRight',
    description: 'Seek forward 10 seconds',
    contextual: true,
  },
  {
    id: 'focus.seekBackLong',
    label: 'Seek Back (10s)',
    category: 'focus',
    defaultShortcut: 'Shift+ArrowLeft',
    description: 'Seek back 10 seconds',
    contextual: true,
  },
];

/** Map of default bindings for quick lookup */
export const DEFAULT_BINDINGS: Record<string, string> = Object.fromEntries(
  ACTIONS.map((a) => [a.id, a.defaultShortcut])
);

// ============================================================================
// Storage
// ============================================================================

const STORAGE_KEY = 'qbz_keybindings';
const STORAGE_VERSION = 1;

interface StoredKeybindings {
  version: number;
  bindings: Record<string, string>;
}

function loadUserBindings(): Record<string, string> {
  if (typeof window === 'undefined') return {};

  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (!stored) return {};

    const parsed = JSON.parse(stored);

    // Handle legacy format (no version)
    if (!parsed.version) {
      return validateBindings(parsed);
    }

    // Current format
    if (parsed.version === STORAGE_VERSION) {
      return validateBindings(parsed.bindings);
    }

    // Future version - try to load anyway
    return validateBindings(parsed.bindings || {});
  } catch (error) {
    console.error('[Keybindings] Failed to load:', error);
    return {};
  }
}

function saveUserBindings(bindings: Record<string, string>): void {
  if (typeof window === 'undefined') return;

  const data: StoredKeybindings = {
    version: STORAGE_VERSION,
    bindings,
  };

  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch (error) {
    console.error('[Keybindings] Failed to save:', error);
  }
}

function validateBindings(bindings: unknown): Record<string, string> {
  if (typeof bindings !== 'object' || bindings === null) {
    return {};
  }

  const validActionIds = new Set(ACTIONS.map((a) => a.id));
  const validated: Record<string, string> = {};

  for (const [key, value] of Object.entries(bindings)) {
    if (validActionIds.has(key) && typeof value === 'string') {
      validated[key] = value;
    }
  }

  return validated;
}

// ============================================================================
// State (using regular variables - no runes at module level)
// ============================================================================

/** User overrides (persisted) */
let userBindings: Record<string, string> = {};

/** Callbacks registered by components */
const actionCallbacks = new Map<string, () => void>();

/** Cached active bindings */
let cachedActiveBindings: Record<string, string> | null = null;

/** Cached reverse map */
let cachedShortcutToAction: Map<string, string> | null = null;

/** Initialize user bindings (call after module load) */
function ensureInitialized(): void {
  if (cachedActiveBindings === null) {
    userBindings = loadUserBindings();
    invalidateCache();
  }
}

/** Invalidate caches when bindings change */
function invalidateCache(): void {
  cachedActiveBindings = { ...DEFAULT_BINDINGS, ...userBindings };
  cachedShortcutToAction = new Map<string, string>();
  for (const [actionId, shortcut] of Object.entries(cachedActiveBindings)) {
    cachedShortcutToAction.set(shortcut, actionId);
  }
}

/** Get active bindings (defaults + user overrides) */
export function getActiveBindings(): Record<string, string> {
  ensureInitialized();
  return cachedActiveBindings!;
}

/** Get shortcut-to-action map */
export function getShortcutToAction(): Map<string, string> {
  ensureInitialized();
  return cachedShortcutToAction!;
}

// ============================================================================
// Manager Functions
// ============================================================================

/**
 * Registers a callback for an action
 */
export function registerAction(actionId: string, callback: () => void): void {
  actionCallbacks.set(actionId, callback);
}

/**
 * Unregisters a callback
 */
export function unregisterAction(actionId: string): void {
  actionCallbacks.delete(actionId);
}

/**
 * Unregisters all callbacks
 */
export function unregisterAll(): void {
  actionCallbacks.clear();
}

/**
 * Gets the current shortcut for an action
 */
export function getBinding(actionId: string): string {
  return getActiveBindings()[actionId] || '';
}

/**
 * Sets a new shortcut for an action
 * @returns true if set, false if there's a conflict
 */
export function setBinding(actionId: string, shortcut: string): boolean {
  // Check for conflict
  const existingAction = getShortcutToAction().get(shortcut);
  if (existingAction && existingAction !== actionId) {
    return false; // Conflict
  }

  userBindings = { ...userBindings, [actionId]: shortcut };
  invalidateCache();
  saveUserBindings(userBindings);
  return true;
}

/**
 * Resets an action to its default shortcut
 */
export function resetBinding(actionId: string): void {
  const { [actionId]: _, ...rest } = userBindings;
  userBindings = rest;
  invalidateCache();

  // If no overrides remain, clear localStorage
  if (Object.keys(userBindings).length === 0) {
    localStorage.removeItem(STORAGE_KEY);
  } else {
    saveUserBindings(userBindings);
  }
}

/**
 * Resets all bindings to defaults
 */
export function resetAllBindings(): void {
  userBindings = {};
  invalidateCache();
  if (typeof window !== 'undefined') {
    localStorage.removeItem(STORAGE_KEY);
  }
}

/**
 * Checks if a shortcut has a conflict
 * @param shortcut The shortcut to check
 * @param excludeActionId Action to exclude from check (for editing)
 */
export function hasConflict(
  shortcut: string,
  excludeActionId?: string
): boolean {
  const existingAction = getShortcutToAction().get(shortcut);
  return existingAction !== undefined && existingAction !== excludeActionId;
}

/**
 * Gets the action that conflicts with a shortcut
 */
export function getConflictingAction(
  shortcut: string,
  excludeActionId?: string
): KeybindingAction | null {
  const existingActionId = getShortcutToAction().get(shortcut);
  if (!existingActionId || existingActionId === excludeActionId) {
    return null;
  }
  return ACTIONS.find((a) => a.id === existingActionId) || null;
}

/**
 * Global keydown handler
 * @param event The keyboard event
 * @param context Current context (for contextual shortcuts)
 */
export function handleKeydown(
  event: KeyboardEvent,
  context: { focusMode?: boolean } = {}
): boolean {
  // Ignore if we're in an input
  if (isInputTarget(event)) {
    return false;
  }

  const shortcut = eventToShortcut(event);
  if (!shortcut) return false;

  const actionId = getShortcutToAction().get(shortcut);
  if (!actionId) return false;

  // Check if contextual and if context applies
  const action = ACTIONS.find((a) => a.id === actionId);
  if (action?.contextual) {
    if (action.category === 'focus' && !context.focusMode) {
      return false;
    }
  }

  // Execute callback
  const callback = actionCallbacks.get(actionId);
  if (callback) {
    event.preventDefault();
    callback();
    return true;
  }

  return false;
}

/**
 * Gets all actions grouped by category
 */
export function getActionsByCategory(): Map<KeybindingCategory, KeybindingAction[]> {
  const grouped = new Map<KeybindingCategory, KeybindingAction[]>();

  for (const category of CATEGORIES) {
    const actions = ACTIONS.filter((a) => a.category === category.id);
    if (actions.length > 0) {
      grouped.set(category.id, actions);
    }
  }

  return grouped;
}
