/**
 * Library Performance Store
 *
 * Feature flags and settings for library performance optimizations.
 * Controls virtualization, thumbnails, and other performance features.
 */

export interface LibraryPerformanceSettings {
  /**
   * Enable virtualized rendering for album/artist/track lists.
   * When enabled, only visible items are rendered in the DOM.
   */
  virtualizationEnabled: boolean;

  /**
   * Enable thumbnail generation and usage for album artwork.
   * When enabled, uses smaller thumbnails in list/grid views.
   */
  thumbnailsEnabled: boolean;

  /**
   * Use optimized network folder filtering (SQL-based instead of N+1).
   */
  optimizedNetworkFilter: boolean;

  /**
   * Debounce delay for album search (ms).
   * Higher values reduce CPU usage during typing.
   */
  albumSearchDebounceMs: number;

  /**
   * Album count threshold for auto-enabling performance mode.
   * When library exceeds this, performance optimizations activate automatically.
   */
  performanceModeThreshold: number;

  /**
   * Force performance mode regardless of library size.
   * Useful for testing or user preference.
   */
  forcePerformanceMode: boolean;

  /**
   * Show album artwork next to each track in Local Library track /
   * folder list views. OFF by default because large libraries
   * (10k+ tracks) pay a per-row image-decode cost and the user
   * shouldn't opt into that without knowing. See issue #412.
   */
  showTrackArtwork: boolean;
}

const STORAGE_KEY = 'qbz-library-performance';

const DEFAULT_SETTINGS: LibraryPerformanceSettings = {
  virtualizationEnabled: true,
  thumbnailsEnabled: false, // Will enable when thumbnail system is ready
  optimizedNetworkFilter: true,
  albumSearchDebounceMs: 150,
  performanceModeThreshold: 500,
  forcePerformanceMode: false,
  showTrackArtwork: false,
};

// State
let settings: LibraryPerformanceSettings = loadSettings();
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

/**
 * Load settings from localStorage
 */
function loadSettings(): LibraryPerformanceSettings {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<LibraryPerformanceSettings>;
      return { ...DEFAULT_SETTINGS, ...parsed };
    }
  } catch (err) {
    console.error('Failed to load library performance settings:', err);
  }
  return { ...DEFAULT_SETTINGS };
}

/**
 * Save settings to localStorage
 */
function saveSettings(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch (err) {
    console.error('Failed to save library performance settings:', err);
  }
}

// ============ Public API ============

/**
 * Subscribe to settings changes
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Get current settings
 */
export function getSettings(): LibraryPerformanceSettings {
  return settings;
}

/**
 * Check if performance mode should be active based on album count
 */
export function shouldUsePerformanceMode(albumCount: number): boolean {
  return settings.forcePerformanceMode || albumCount >= settings.performanceModeThreshold;
}

/**
 * Check if virtualization is enabled
 */
export function isVirtualizationEnabled(): boolean {
  return settings.virtualizationEnabled;
}

/**
 * Check if thumbnails are enabled
 */
export function isThumbnailsEnabled(): boolean {
  return settings.thumbnailsEnabled;
}

/**
 * Check if album artwork is shown alongside each track row in the
 * Local Library list views (issue #412). Off by default.
 */
export function isTrackArtworkEnabled(): boolean {
  return settings.showTrackArtwork;
}

/**
 * Check if optimized network filter is enabled
 */
export function isOptimizedNetworkFilterEnabled(): boolean {
  return settings.optimizedNetworkFilter;
}

/**
 * Get album search debounce delay
 */
export function getAlbumSearchDebounceMs(): number {
  return settings.albumSearchDebounceMs;
}

/**
 * Update a setting
 */
export function updateSetting<K extends keyof LibraryPerformanceSettings>(
  key: K,
  value: LibraryPerformanceSettings[K]
): void {
  settings[key] = value;
  saveSettings();
  notifyListeners();
}

/**
 * Update multiple settings at once
 */
export function updateSettings(updates: Partial<LibraryPerformanceSettings>): void {
  settings = { ...settings, ...updates };
  saveSettings();
  notifyListeners();
}

/**
 * Reset to defaults
 */
export function resetToDefaults(): void {
  settings = { ...DEFAULT_SETTINGS };
  saveSettings();
  notifyListeners();
}

/**
 * Get performance mode threshold
 */
export function getPerformanceModeThreshold(): number {
  return settings.performanceModeThreshold;
}
