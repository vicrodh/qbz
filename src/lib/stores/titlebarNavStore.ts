/**
 * Titlebar Navigation Store
 *
 * Manages which navigation buttons appear in the custom title bar.
 * Each item is individually toggleable, all off by default.
 * Auto-reverts when user switches to system title bar.
 *
 * Follows the same observable-store pattern as titleBarStore.
 */

const STORAGE_KEY = 'qbz-titlebar-nav';

export type TitlebarNavPosition = 'auto' | 'left' | 'right';

export interface TitlebarNavConfig {
  discover: boolean;
  favorites: boolean;
  library: boolean;
  myQbz: boolean;
  purchases: boolean;
  position: TitlebarNavPosition;
}

// State — defaults to all core nav items in the titlebar for NEW
// installs. Existing users are unaffected because initTitlebarNavStore
// reads qbz-titlebar-nav from localStorage and overlays persisted
// values on top of these defaults. Purchases stays off because the
// feature is opt-in (not every Qobuz account has purchase history).
let config: TitlebarNavConfig = {
  discover: true,
  favorites: true,   // shown as "Library" in the UI per i18n
  library: true,     // shown as "Local Library" in the UI
  myQbz: true,
  purchases: false,
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
      // Migrate from old single-toggle format
      if ('enabled' in parsed && typeof (parsed as Record<string, unknown>).enabled === 'boolean') {
        const wasEnabled = (parsed as Record<string, unknown>).enabled as boolean;
        config = {
          ...config,
          discover: wasEnabled,
          favorites: wasEnabled,
          library: wasEnabled,
          ...parsed,
        };
      } else {
        config = { ...config, ...parsed };
      }
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
 * Check if ANY nav item is enabled for the titlebar
 */
export function isTitlebarNavEnabled(): boolean {
  return config.discover || config.favorites || config.library || config.myQbz || config.purchases;
}

/**
 * Check individual items
 */
export function isDiscoverInTitlebar(): boolean { return config.discover; }
export function isFavoritesInTitlebar(): boolean { return config.favorites; }
export function isLibraryInTitlebar(): boolean { return config.library; }
export function isMyQbzInTitlebar(): boolean { return config.myQbz; }
export function isPurchasesInTitlebar(): boolean { return config.purchases; }

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
 * Set individual item visibility
 */
export function setDiscoverInTitlebar(value: boolean): void {
  config = { ...config, discover: value };
  persist();
  notifyListeners();
}

export function setFavoritesInTitlebar(value: boolean): void {
  config = { ...config, favorites: value };
  persist();
  notifyListeners();
}

export function setLibraryInTitlebar(value: boolean): void {
  config = { ...config, library: value };
  persist();
  notifyListeners();
}

export function setMyQbzInTitlebar(value: boolean): void {
  config = { ...config, myQbz: value };
  persist();
  notifyListeners();
}

export function setPurchasesInTitlebar(value: boolean): void {
  config = { ...config, purchases: value };
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
