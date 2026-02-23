/**
 * Auto-Theme Store
 *
 * Manages dynamic theme generation from system wallpaper or custom images.
 * Injects CSS custom properties at runtime without modifying app.css.
 */

import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

// ── Types ───────────────────────────────────────────────────────────────────

export interface PaletteColor {
  r: number;
  g: number;
  b: number;
}

export interface ThemePalette {
  bg_primary: PaletteColor;
  bg_secondary: PaletteColor;
  bg_tertiary: PaletteColor;
  bg_hover: PaletteColor;
  accent: PaletteColor;
  is_dark: boolean;
  all_colors: PaletteColor[];
}

export interface GeneratedTheme {
  variables: Record<string, string>;
  is_dark: boolean;
  source: string;
}

export type AutoThemeSource = 'wallpaper' | 'image';

interface AutoThemePrefs {
  enabled: boolean;
  source: AutoThemeSource;
  customImagePath?: string;
}

interface AutoThemeState {
  active: boolean;
  generating: boolean;
  error: string | null;
  theme: GeneratedTheme | null;
  palette: ThemePalette | null;
  detectedDE: string | null;
}

// ── Constants ───────────────────────────────────────────────────────────────

const STORAGE_KEY_THEME = 'qbz-theme';
const STORAGE_KEY_VARS = 'qbz-auto-theme-vars';
const STORAGE_KEY_PREFS = 'qbz-auto-theme';
const AUTO_THEME_VALUE = 'auto';

// ── Store ───────────────────────────────────────────────────────────────────

const autoThemeStore = writable<AutoThemeState>({
  active: false,
  generating: false,
  error: null,
  theme: null,
  palette: null,
  detectedDE: null,
});

// ── CSS Injection ───────────────────────────────────────────────────────────

/** Inject all CSS variables from a generated theme onto the document root. */
function injectCssVariables(variables: Record<string, string>): void {
  const root = document.documentElement;
  for (const [name, value] of Object.entries(variables)) {
    root.style.setProperty(name, value);
  }
}

/** Remove all auto-theme CSS variables from the document root. */
function removeCssVariables(variables: Record<string, string>): void {
  const root = document.documentElement;
  for (const name of Object.keys(variables)) {
    root.style.removeProperty(name);
  }
}

// ── Persistence ─────────────────────────────────────────────────────────────

function saveAutoThemeVars(variables: Record<string, string>): void {
  try {
    localStorage.setItem(STORAGE_KEY_VARS, JSON.stringify(variables));
  } catch {
    // localStorage might be full, ignore
  }
}

function loadAutoThemeVars(): Record<string, string> | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY_VARS);
    if (raw) return JSON.parse(raw);
  } catch {
    // corrupted data, ignore
  }
  return null;
}

function savePrefs(prefs: AutoThemePrefs): void {
  try {
    localStorage.setItem(STORAGE_KEY_PREFS, JSON.stringify(prefs));
  } catch {
    // ignore
  }
}

function loadPrefs(): AutoThemePrefs | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY_PREFS);
    if (raw) return JSON.parse(raw);
  } catch {
    // ignore
  }
  return null;
}

function clearAutoThemeStorage(): void {
  localStorage.removeItem(STORAGE_KEY_VARS);
  localStorage.removeItem(STORAGE_KEY_PREFS);
  // Restore theme key to empty (default dark) if it was 'auto'
  const current = localStorage.getItem(STORAGE_KEY_THEME);
  if (current === AUTO_THEME_VALUE) {
    localStorage.removeItem(STORAGE_KEY_THEME);
  }
}

// ── Public API ──────────────────────────────────────────────────────────────

/**
 * Enable auto-theme from the given source.
 * Generates the theme, injects CSS variables, and persists the state.
 */
export async function enableAutoTheme(
  source: AutoThemeSource,
  imagePath?: string
): Promise<void> {
  autoThemeStore.update(s => ({ ...s, generating: true, error: null }));

  try {
    // Detect DE for display
    let detectedDE: string | null = null;
    try {
      const de = await invoke<string | { Unknown: string }>('v2_detect_desktop_environment');
      detectedDE = typeof de === 'string' ? de : de.Unknown ?? 'Unknown';
    } catch {
      // non-critical
    }

    let theme: GeneratedTheme;
    let palette: ThemePalette | null = null;

    if (source === 'wallpaper') {
      theme = await invoke<GeneratedTheme>('v2_generate_theme_from_wallpaper');
    } else {
      if (!imagePath) {
        throw new Error('Image path required for custom image source');
      }
      theme = await invoke<GeneratedTheme>('v2_generate_theme_from_image', { imagePath });
    }

    // Extract palette for preview
    const palettePath = source === 'wallpaper'
      ? await invoke<string>('v2_get_system_wallpaper').catch(() => null)
      : imagePath;

    if (palettePath) {
      try {
        palette = await invoke<ThemePalette>('v2_extract_palette', { imagePath: palettePath });
      } catch {
        // non-critical, preview won't show
      }
    }

    // Apply: set data-theme to 'auto' (no static CSS matches)
    document.documentElement.setAttribute('data-theme', AUTO_THEME_VALUE);
    injectCssVariables(theme.variables);

    // Persist
    localStorage.setItem(STORAGE_KEY_THEME, AUTO_THEME_VALUE);
    saveAutoThemeVars(theme.variables);
    savePrefs({ enabled: true, source, customImagePath: imagePath });

    autoThemeStore.set({
      active: true,
      generating: false,
      error: null,
      theme,
      palette,
      detectedDE,
    });
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    autoThemeStore.update(s => ({
      ...s,
      generating: false,
      error: message,
    }));
    throw err;
  }
}

/**
 * Disable auto-theme and clean up CSS variables.
 * Does NOT apply a static theme — the caller (SettingsView) handles that.
 */
export function disableAutoTheme(): void {
  const state = get(autoThemeStore);
  if (state.theme) {
    removeCssVariables(state.theme.variables);
  } else {
    // Fallback: remove vars from localStorage cache
    const cached = loadAutoThemeVars();
    if (cached) removeCssVariables(cached);
  }

  document.documentElement.removeAttribute('data-theme');
  clearAutoThemeStorage();

  autoThemeStore.set({
    active: false,
    generating: false,
    error: null,
    theme: null,
    palette: null,
    detectedDE: null,
  });
}

/** Check if auto-theme is currently active. */
export function isAutoThemeActive(): boolean {
  return get(autoThemeStore).active;
}

/** Get current auto-theme preferences from localStorage (for restoring UI state). */
export function getAutoThemePrefs(): AutoThemePrefs | null {
  return loadPrefs();
}

/** Subscribe to auto-theme state changes. */
export function subscribeAutoTheme(
  listener: (state: AutoThemeState) => void
): () => void {
  return autoThemeStore.subscribe(listener);
}

/** Export the store itself for use with $ syntax in Svelte components. */
export { autoThemeStore };

// ── Bootstrap ───────────────────────────────────────────────────────────────

/**
 * Restore auto-theme CSS variables from localStorage for instant display.
 * Call this early in bootstrap (before backend is ready) to prevent FOUC.
 */
export function restoreAutoThemeVars(): void {
  const savedTheme = localStorage.getItem(STORAGE_KEY_THEME);
  if (savedTheme !== AUTO_THEME_VALUE) return;

  const vars = loadAutoThemeVars();
  if (!vars) return;

  document.documentElement.setAttribute('data-theme', AUTO_THEME_VALUE);
  injectCssVariables(vars);

  // Mark store as active (will be re-synced when Settings loads)
  autoThemeStore.set({
    active: true,
    generating: false,
    error: null,
    theme: { variables: vars, is_dark: true, source: 'cached' },
    palette: null,
    detectedDE: null,
  });
}
