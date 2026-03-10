/**
 * Titlebar Navigation Store
 *
 * Manages whether Discover, Favorites, and Local Library navigation
 * buttons appear in the custom title bar. Opt-in, off by default.
 * Auto-reverts when user switches to system title bar.
 *
 * Follows the same observable-store pattern as titleBarStore.
 */

const STORAGE_KEY = 'qbz-titlebar-nav';

export type TitlebarNavPosition = 'auto' | 'left' | 'right';

export interface TitlebarNavConfig {
  enabled: boolean;
  position: TitlebarNavPosition;
}

// State
let config: TitlebarNavConfig = {
  enabled: false,
  position: 'auto',
};

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

function persist(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
  } catch {
    // localStorage not available
  }
}

/**
 * Initialize from localStorage
 */
export function initTitlebarNavStore(): void {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<TitlebarNavConfig>;
      config = { ...config, ...parsed };
    }
  } catch {
    // localStorage not available or invalid JSON
  }
}

/**
 * Subscribe to state changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

/**
 * Get current config
 */
export function getTitlebarNavConfig(): TitlebarNavConfig {
  return { ...config };
}

/**
 * Check if nav should be shown in titlebar (enabled AND custom titlebar active)
 */
export function isTitlebarNavEnabled(): boolean {
  return config.enabled;
}

/**
 * Get the resolved position based on window controls position.
 * 'auto' means opposite side of window controls.
 */
export function getResolvedPosition(windowControlsPosition: 'left' | 'right'): 'left' | 'right' {
  if (config.position === 'auto') {
    return windowControlsPosition === 'right' ? 'left' : 'right';
  }
  return config.position;
}

/**
 * Enable/disable nav in titlebar
 */
export function setTitlebarNavEnabled(enabled: boolean): void {
  config = { ...config, enabled };
  persist();
  notifyListeners();
}

/**
 * Set position preference
 */
export function setTitlebarNavPosition(position: TitlebarNavPosition): void {
  config = { ...config, position };
  persist();
  notifyListeners();
}

/**
 * Update full config
 */
export function setTitlebarNavConfig(newConfig: Partial<TitlebarNavConfig>): void {
  config = { ...config, ...newConfig };
  persist();
  notifyListeners();
}
