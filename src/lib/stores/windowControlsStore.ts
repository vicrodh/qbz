/**
 * Window Controls Store
 *
 * Manages window control button customization (position, shape, colors)
 * with localStorage persistence. Follows the same observable-store pattern
 * as titleBarStore / searchBarLocationStore.
 */

const STORAGE_KEY = 'qbz-window-controls';

export type ButtonPosition = 'right' | 'left';
/**
 * Button highlight shape. `rectangular` is already full-height with sharp
 * corners (tab-like), so we only add one new variant: `full-height-rounded`
 * (tab-like with rounded corners, matching Klassy's FullHeightRoundedRectangle
 * which is the most common preset).
 */
export type ButtonShape =
  | 'rectangular'           // full-height sharp rectangle (tab)
  | 'circular'              // small circle
  | 'square'                // small rounded square
  | 'full-height-rounded';  // full-height rounded rectangle (Klassy default)
export type ButtonSize = 'small' | 'normal' | 'large';

export interface ButtonColorSet {
  bg: string;
  bgHover: string;
  bgActive: string;
  fg: string;
  fgHover: string;
  fgActive: string;
}

export interface WindowControlsConfig {
  position: ButtonPosition;
  shape: ButtonShape;
  size: ButtonSize;
  minimizeColors: ButtonColorSet;
  maximizeColors: ButtonColorSet;
  closeColors: ButtonColorSet;
  preset: string;
}

// --- Preset Definitions ---

const PRESET_DEFAULT: Omit<WindowControlsConfig, 'position' | 'shape' | 'size'> = {
  preset: 'default',
  minimizeColors: { bg: 'transparent', bgHover: 'rgba(255,255,255,0.1)', bgActive: 'rgba(255,255,255,0.06)', fg: '#888888', fgHover: '#ffffff', fgActive: '#cccccc' },
  maximizeColors: { bg: 'transparent', bgHover: 'rgba(255,255,255,0.1)', bgActive: 'rgba(255,255,255,0.06)', fg: '#888888', fgHover: '#ffffff', fgActive: '#cccccc' },
  closeColors: { bg: 'transparent', bgHover: '#e81123', bgActive: '#b20f1c', fg: '#888888', fgHover: '#ffffff', fgActive: '#ffffff' },
};

const PRESET_MACOS: Omit<WindowControlsConfig, 'position' | 'shape' | 'size'> = {
  preset: 'macos',
  closeColors: { bg: '#ff5f57', bgHover: '#ff3b30', bgActive: '#cc4940', fg: 'transparent', fgHover: '#4d0000', fgActive: '#4d0000' },
  maximizeColors: { bg: '#28c840', bgHover: '#00b336', bgActive: '#1a9e32', fg: 'transparent', fgHover: '#004d00', fgActive: '#004d00' },
  minimizeColors: { bg: '#febc2e', bgHover: '#f0a000', bgActive: '#cc8800', fg: 'transparent', fgHover: '#4d3800', fgActive: '#4d3800' },
};

const PRESET_ADWAITA: Omit<WindowControlsConfig, 'position' | 'shape' | 'size'> = {
  preset: 'adwaita',
  minimizeColors: { bg: 'rgba(255,255,255,0.08)', bgHover: 'rgba(255,255,255,0.15)', bgActive: 'rgba(255,255,255,0.05)', fg: '#888888', fgHover: '#ffffff', fgActive: '#cccccc' },
  maximizeColors: { bg: 'rgba(255,255,255,0.08)', bgHover: 'rgba(255,255,255,0.15)', bgActive: 'rgba(255,255,255,0.05)', fg: '#888888', fgHover: '#ffffff', fgActive: '#cccccc' },
  closeColors: { bg: 'rgba(255,255,255,0.08)', bgHover: '#c0392b', bgActive: '#962d22', fg: '#888888', fgHover: '#ffffff', fgActive: '#ffffff' },
};

const PRESET_MONOCHROME: Omit<WindowControlsConfig, 'position' | 'shape' | 'size'> = {
  preset: 'monochrome',
  minimizeColors: { bg: 'transparent', bgHover: 'rgba(255,255,255,0.12)', bgActive: 'rgba(255,255,255,0.06)', fg: '#888888', fgHover: '#ffffff', fgActive: '#cccccc' },
  maximizeColors: { bg: 'transparent', bgHover: 'rgba(255,255,255,0.12)', bgActive: 'rgba(255,255,255,0.06)', fg: '#888888', fgHover: '#ffffff', fgActive: '#cccccc' },
  closeColors: { bg: 'transparent', bgHover: 'rgba(255,255,255,0.12)', bgActive: 'rgba(255,255,255,0.06)', fg: '#888888', fgHover: '#ffffff', fgActive: '#cccccc' },
};

/**
 * Placeholder used when the user selects "Klassy (auto-detect)" but the
 * runtime hasn't applied real colors yet. The dynamic applier overwrites
 * these with colors read from Plasma's `kdeglobals` / `klassyrc`.
 */
const PRESET_KLASSY_FALLBACK: Omit<WindowControlsConfig, 'position' | 'shape' | 'size'> = {
  preset: 'klassy',
  minimizeColors: { bg: 'transparent', bgHover: 'rgba(255,255,255,0.10)', bgActive: 'rgba(255,255,255,0.06)', fg: '#cdd6f4', fgHover: '#ffffff', fgActive: '#cdd6f4' },
  maximizeColors: { bg: 'transparent', bgHover: 'rgba(255,255,255,0.10)', bgActive: 'rgba(255,255,255,0.06)', fg: '#cdd6f4', fgHover: '#ffffff', fgActive: '#cdd6f4' },
  closeColors: { bg: 'transparent', bgHover: '#e64553', bgActive: '#b82c38', fg: '#cdd6f4', fgHover: '#ffffff', fgActive: '#ffffff' },
};

export const PRESETS: Record<string, Omit<WindowControlsConfig, 'position' | 'shape' | 'size'>> = {
  default: PRESET_DEFAULT,
  macos: PRESET_MACOS,
  adwaita: PRESET_ADWAITA,
  monochrome: PRESET_MONOCHROME,
  klassy: PRESET_KLASSY_FALLBACK,
};

// --- State ---

let config: WindowControlsConfig = {
  position: 'right',
  shape: 'rectangular',
  size: 'normal',
  ...PRESET_DEFAULT,
};

// --- Listeners ---

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

// --- Public API ---

export function initWindowControlsStore(): void {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      const parsed = JSON.parse(saved) as Partial<WindowControlsConfig>;
      // Deep-merge color sets so old saved data missing new fields
      // (e.g. bgActive/fgActive) gets backfilled from defaults
      const defaults = config;
      config = {
        ...defaults,
        ...parsed,
        minimizeColors: { ...defaults.minimizeColors, ...parsed.minimizeColors },
        maximizeColors: { ...defaults.maximizeColors, ...parsed.maximizeColors },
        closeColors: { ...defaults.closeColors, ...parsed.closeColors },
      };
    }
  } catch {
    // localStorage not available or invalid JSON
  }
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

export function getWindowControls(): WindowControlsConfig {
  return { ...config };
}

export function setWindowControls(newConfig: Partial<WindowControlsConfig>): void {
  config = { ...config, ...newConfig };
  persist();
  notifyListeners();
}

export function getButtonPosition(): ButtonPosition {
  return config.position;
}

export function setButtonPosition(position: ButtonPosition): void {
  config = { ...config, position };
  persist();
  notifyListeners();
}

export function getButtonShape(): ButtonShape {
  return config.shape;
}

export function setButtonShape(shape: ButtonShape): void {
  config = { ...config, shape };
  persist();
  notifyListeners();
}

export function getButtonSize(): ButtonSize {
  return config.size;
}

export function setButtonSize(size: ButtonSize): void {
  config = { ...config, size };
  persist();
  notifyListeners();
}

export function getPreset(): string {
  return config.preset;
}

/**
 * Switch to 'custom' preset, keeping current colors intact.
 * This reveals the color pickers without overwriting anything.
 */
export function setPresetCustom(): void {
  config = { ...config, preset: 'custom' };
  persist();
  notifyListeners();
}

export function applyPreset(presetName: string): void {
  const preset = PRESETS[presetName];
  if (preset) {
    config = { ...config, ...preset };
    persist();
    notifyListeners();
  }
}

/**
 * Cached result of the last successful desktop theme detection. Populated
 * lazily by `detectDesktopThemeCached()` and reused by both the
 * preset picker (to decide whether to expose the option) and the title
 * bar (to pick matching glyphs).
 */
let cachedTheme: DesktopThemeInfo | null = null;
let detectInFlight: Promise<DesktopThemeInfo | null> | null = null;

export function getCachedDesktopTheme(): DesktopThemeInfo | null {
  return cachedTheme;
}

export async function detectDesktopThemeCached(force = false): Promise<DesktopThemeInfo | null> {
  if (!force && cachedTheme) return cachedTheme;
  if (detectInFlight !== null) return detectInFlight;
  detectInFlight = (async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const info = (await invoke('detect_desktop_theme')) as DesktopThemeInfo;
      cachedTheme = info;
      return info;
    } catch (e) {
      console.warn('[windowControls] desktop theme detection failed:', e);
      return null;
    } finally {
      detectInFlight = null;
    }
  })();
  return detectInFlight;
}

/**
 * Shape returned by `detect_desktop_theme` in Rust. Mirrors
 * `DesktopThemeInfo` with camelCase fields.
 */
export interface DesktopThemeInfo {
  desktop: string;
  isKlassy: boolean;
  titlebarActiveBg?: string;
  titlebarActiveFg?: string;
  titlebarInactiveBg?: string;
  titlebarInactiveFg?: string;
  accent?: string;
  decorationHover?: string;
  klassyButtonIconStyle?: string;
  klassyButtonShape?: string;
  klassyMatchAppColor?: boolean;
  /** Best-effort default corner radius for the detected desktop (px). */
  windowCornerRadiusPx?: number;
}

/**
 * Map a Klassy `ButtonShape` string to the equivalent QBZ `ButtonShape`.
 * Klassy exposes many variants (IntegratedRoundedRectangle, FullHeightRectangle,
 * Tab, Circle, Square, ...); QBZ only has three, so we pick the closest match.
 */
export function mapKlassyShapeToQbz(klassyShape: string | undefined): ButtonShape | null {
  if (!klassyShape) return null;
  const s = klassyShape.toLowerCase();
  if (s.includes('circle')) return 'circular';
  // Klassy naming: "FullHeightRoundedRectangle", "FullHeightRectangle", "Tab"
  if (s.startsWith('fullheight')) {
    // FullHeightRoundedRectangle → rounded; FullHeightRectangle → our 'rectangular' already is sharp full-height
    return s.includes('rounded') ? 'full-height-rounded' : 'rectangular';
  }
  if (s === 'tab') return 'full-height-rounded';
  if (s === 'square' || s === 'smallsquare') return 'square';
  // IntegratedRoundedRectangle, Rectangle, SmallRoundedRectangle, etc.
  return 'rectangular';
}

function hexToRgba(hex: string, alpha: number): string {
  const cleaned = hex.startsWith('#') ? hex.slice(1) : hex;
  if (cleaned.length !== 6) return hex;
  const r = Number.parseInt(cleaned.slice(0, 2), 16);
  const g = Number.parseInt(cleaned.slice(2, 4), 16);
  const b = Number.parseInt(cleaned.slice(4, 6), 16);
  if ([r, g, b].some((n) => Number.isNaN(n))) return hex;
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

/**
 * Apply the Klassy auto-detect preset. Reads KDE + Klassy config via the
 * Tauri command and builds a color set that matches the user's system
 * decoration accent. Keeps close red for UX convention. If detection
 * fails or returns nothing useful, falls back to PRESET_KLASSY_FALLBACK.
 *
 * Returns the detected theme info so callers can surface it in the UI.
 */
export async function applyKlassyPreset(): Promise<DesktopThemeInfo | null> {
  try {
    const info = await detectDesktopThemeCached(true);
    if (!info) {
      applyPreset('klassy');
      return null;
    }
    const accent = info.accent;
    const hover = info.decorationHover;
    const fg = info.titlebarActiveFg ?? '#cdd6f4';
    if (!accent && !hover) {
      applyPreset('klassy');
      return info;
    }
    const standardHover = hover ? hexToRgba(hover, 0.85) : 'rgba(255,255,255,0.10)';
    const standardActive = hover ? hexToRgba(hover, 0.55) : 'rgba(255,255,255,0.06)';
    const accentHover = accent ? hexToRgba(accent, 0.9) : standardHover;
    const klassyColors = {
      preset: 'klassy',
      minimizeColors: { bg: 'transparent', bgHover: standardHover, bgActive: standardActive, fg, fgHover: '#ffffff', fgActive: fg },
      maximizeColors: { bg: 'transparent', bgHover: standardHover, bgActive: standardActive, fg, fgHover: '#ffffff', fgActive: fg },
      closeColors: { bg: 'transparent', bgHover: '#e64553', bgActive: '#b82c38', fg, fgHover: '#ffffff', fgActive: '#ffffff' },
    } as Omit<WindowControlsConfig, 'position' | 'shape' | 'size'>;
    const mappedShape = mapKlassyShapeToQbz(info.klassyButtonShape);
    const next: WindowControlsConfig = {
      ...config,
      ...klassyColors,
      ...(mappedShape ? { shape: mappedShape } : {}),
    };
    void accentHover;
    config = next;
    persist();
    notifyListeners();
    return info;
  } catch (e) {
    console.warn('[windowControls] Klassy detection failed:', e);
    applyPreset('klassy');
    return null;
  }
}

export function setButtonColor(
  button: 'minimize' | 'maximize' | 'close',
  field: keyof ButtonColorSet,
  value: string
): void {
  const key = `${button}Colors` as 'minimizeColors' | 'maximizeColors' | 'closeColors';
  config = { ...config, [key]: { ...config[key], [field]: value }, preset: 'custom' };
  persist();
  notifyListeners();
}
