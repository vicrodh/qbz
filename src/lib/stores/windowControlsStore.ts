/**
 * Window Controls Store
 *
 * Manages window control button customization (position, shape, colors)
 * with localStorage persistence. Follows the same observable-store pattern
 * as titleBarStore / searchBarLocationStore.
 */

const STORAGE_KEY = 'qbz-window-controls';

export type ButtonPosition = 'right' | 'left';
export type ButtonShape = 'rectangular' | 'circular' | 'square';
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

export const PRESETS: Record<string, Omit<WindowControlsConfig, 'position' | 'shape' | 'size'>> = {
  default: PRESET_DEFAULT,
  macos: PRESET_MACOS,
  adwaita: PRESET_ADWAITA,
  monochrome: PRESET_MONOCHROME,
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
