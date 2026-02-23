/**
 * Auto-Theme Store
 *
 * Manages dynamic theme generation from system accent colors, wallpaper, or custom images.
 * Injects CSS custom properties at runtime without modifying app.css.
 *
 * Source cascade for 'system' mode:
 *   1. Try DE accent color → build theme from accent
 *   2. If accent unavailable → try wallpaper image → extract palette
 *   3. If both fail → throw error (caller handles fallback to static theme)
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

/**
 * - 'system': accent color first, wallpaper fallback (default)
 * - 'wallpaper': explicitly use wallpaper image
 * - 'image': user-selected custom image
 */
export type AutoThemeSource = 'system' | 'wallpaper' | 'image';

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
  const current = localStorage.getItem(STORAGE_KEY_THEME);
  if (current === AUTO_THEME_VALUE) {
    localStorage.removeItem(STORAGE_KEY_THEME);
  }
}

// ── Internal: Theme generation strategies ───────────────────────────────────

/** Try to generate theme from DE accent color. Returns null if unavailable. */
async function tryAccentColorTheme(): Promise<GeneratedTheme | null> {
  try {
    const accentColor = await invoke<PaletteColor>('v2_get_system_accent_color');
    // Build a theme using the accent color with a neutral dark base
    // We generate from a synthetic image isn't ideal, so we build the theme
    // by passing the accent to the wallpaper generator with a hint
    // For now, use the accent as override on a wallpaper-based theme
    // or generate a minimal theme from accent alone.
    //
    // Strategy: try wallpaper first (for backgrounds), but override accent with system accent
    let baseTheme: GeneratedTheme | null = null;
    try {
      baseTheme = await invoke<GeneratedTheme>('v2_generate_theme_from_wallpaper');
    } catch {
      // No wallpaper available, build from accent alone
    }

    if (baseTheme) {
      // Override accent variables with system accent color
      const accentHex = `#${accentColor.r.toString(16).padStart(2, '0')}${accentColor.g.toString(16).padStart(2, '0')}${accentColor.b.toString(16).padStart(2, '0')}`;
      baseTheme.variables['--accent-primary'] = accentHex;
      // Compute hover/active shifts (simple lightness approximation)
      baseTheme.variables['--accent-hover'] = shiftHexLightness(accentHex, 0.10);
      baseTheme.variables['--accent-active'] = shiftHexLightness(accentHex, -0.10);
      // Button text: white if accent is dark
      const accentLum = (0.2126 * srgbLinear(accentColor.r) + 0.7152 * srgbLinear(accentColor.g) + 0.0722 * srgbLinear(accentColor.b));
      baseTheme.variables['--btn-primary-text'] = accentLum < 0.5 ? '#ffffff' : '#000000';
      baseTheme.source = 'system-accent+wallpaper';
      return baseTheme;
    }

    // No wallpaper: build minimal theme from accent color only
    // Use a neutral dark base
    const isDark = true; // default to dark
    const vars: Record<string, string> = {};
    const accentHex = `#${accentColor.r.toString(16).padStart(2, '0')}${accentColor.g.toString(16).padStart(2, '0')}${accentColor.b.toString(16).padStart(2, '0')}`;

    // Dark base backgrounds
    vars['--bg-primary'] = '#0f0f0f';
    vars['--bg-secondary'] = '#1a1a1a';
    vars['--bg-tertiary'] = '#2a2a2a';
    vars['--bg-hover'] = '#1f1f1f';

    // Text
    vars['--text-primary'] = '#ffffff';
    vars['--text-secondary'] = '#cccccc';
    vars['--text-muted'] = '#888888';
    vars['--text-disabled'] = '#555555';

    // Accent from system
    vars['--accent-primary'] = accentHex;
    vars['--accent-hover'] = shiftHexLightness(accentHex, 0.10);
    vars['--accent-active'] = shiftHexLightness(accentHex, -0.10);
    const accentLum = (0.2126 * srgbLinear(accentColor.r) + 0.7152 * srgbLinear(accentColor.g) + 0.0722 * srgbLinear(accentColor.b));
    vars['--btn-primary-text'] = accentLum < 0.5 ? '#ffffff' : '#000000';

    // Status
    vars['--danger'] = '#ef4444';
    vars['--danger-bg'] = 'rgba(239, 68, 68, 0.1)';
    vars['--danger-border'] = 'rgba(239, 68, 68, 0.3)';
    vars['--danger-hover'] = 'rgba(239, 68, 68, 0.2)';
    vars['--warning'] = '#fbbf24';
    vars['--warning-bg'] = 'rgba(251, 191, 36, 0.1)';
    vars['--warning-border'] = 'rgba(251, 191, 36, 0.3)';
    vars['--warning-hover'] = 'rgba(251, 191, 36, 0.2)';

    // Borders
    vars['--border-subtle'] = '#2a2a2a';
    vars['--border-strong'] = '#3a3a3a';

    // Alpha tokens (white-based for dark)
    const alphaLevels = [0.04, 0.05, 0.06, 0.08, 0.10, 0.15, 0.18, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.60, 0.70, 0.80, 0.85, 0.90, 0.95];
    const alphaNames = ['4', '5', '6', '8', '10', '15', '18', '20', '25', '30', '35', '40', '45', '50', '60', '70', '80', '85', '90', '95'];
    for (let i = 0; i < alphaLevels.length; i++) {
      vars[`--alpha-${alphaNames[i]}`] = `rgba(255, 255, 255, ${alphaLevels[i]})`;
    }

    return { variables: vars, is_dark: isDark, source: 'system-accent' };
  } catch {
    return null;
  }
}

/** Try to generate theme from system wallpaper. Throws if wallpaper unavailable. */
async function generateFromWallpaper(): Promise<GeneratedTheme> {
  return invoke<GeneratedTheme>('v2_generate_theme_from_wallpaper');
}

/** Generate theme from a specific image path. */
async function generateFromImage(imagePath: string): Promise<GeneratedTheme> {
  return invoke<GeneratedTheme>('v2_generate_theme_from_image', { imagePath });
}

// ── Color math helpers (frontend-side, minimal) ─────────────────────────────

function srgbLinear(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

function shiftHexLightness(hex: string, amount: number): string {
  const r = parseInt(hex.slice(1, 3), 16) / 255;
  const g = parseInt(hex.slice(3, 5), 16) / 255;
  const b = parseInt(hex.slice(5, 7), 16) / 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  let h = 0;
  let s = 0;
  const l = (max + min) / 2;

  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
    else if (max === g) h = ((b - r) / d + 2) / 6;
    else h = ((r - g) / d + 4) / 6;
  }

  const newL = Math.max(0, Math.min(1, l + amount));

  function hue2rgb(p: number, q: number, ht: number): number {
    if (ht < 0) ht += 1;
    if (ht > 1) ht -= 1;
    if (ht < 1 / 6) return p + (q - p) * 6 * ht;
    if (ht < 1 / 2) return q;
    if (ht < 2 / 3) return p + (q - p) * (2 / 3 - ht) * 6;
    return p;
  }

  let nr: number, ng: number, nb: number;
  if (s === 0) {
    nr = ng = nb = newL;
  } else {
    const q = newL < 0.5 ? newL * (1 + s) : newL + s - newL * s;
    const p = 2 * newL - q;
    nr = hue2rgb(p, q, h + 1 / 3);
    ng = hue2rgb(p, q, h);
    nb = hue2rgb(p, q, h - 1 / 3);
  }

  const toHex = (v: number) => Math.round(v * 255).toString(16).padStart(2, '0');
  return `#${toHex(nr)}${toHex(ng)}${toHex(nb)}`;
}

// ── Public API ──────────────────────────────────────────────────────────────

/**
 * Enable auto-theme from the given source.
 *
 * - 'system': try accent color first, fall back to wallpaper, throw if both fail
 * - 'wallpaper': generate directly from system wallpaper
 * - 'image': generate from user-selected image (imagePath required)
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

    if (source === 'system') {
      // Cascade: accent color → wallpaper → error
      const accentTheme = await tryAccentColorTheme();
      if (accentTheme) {
        theme = accentTheme;
      } else {
        // Fallback to wallpaper
        try {
          theme = await generateFromWallpaper();
        } catch (wallpaperErr) {
          throw new Error(
            `Could not infer system colors or wallpaper. ${wallpaperErr instanceof Error ? wallpaperErr.message : String(wallpaperErr)}`
          );
        }
      }
    } else if (source === 'wallpaper') {
      theme = await generateFromWallpaper();
    } else {
      if (!imagePath) {
        throw new Error('Image path required for custom image source');
      }
      theme = await generateFromImage(imagePath);
    }

    // Extract palette for preview
    let palettePath: string | null = null;
    if (source === 'image') {
      palettePath = imagePath ?? null;
    } else {
      try {
        palettePath = await invoke<string>('v2_get_system_wallpaper');
      } catch {
        // no wallpaper, skip palette preview
      }
    }

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
