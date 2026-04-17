/**
 * Immersive FPS Configuration
 *
 * Reads per-panel frame rate settings from userStorage.
 * Default is 15fps for all panels (low power consumption).
 */

import { getUserItem } from '$lib/utils/userStorage';

const FPS_KEY_PREFIX = 'qbz-immersive-fps-';
const DEFAULT_FPS = 15;

// Per-panel default overrides (panels not listed here use DEFAULT_FPS)
const PANEL_DEFAULTS: Partial<Record<ImmersivePanelId, number>> = {
  linebed: 60, // Linebed needs high FPS for dense terrain 3D effect
};

export type ImmersivePanelId =
  | 'ambient'
  | 'visualizer'
  | 'neon-flow'
  | 'tunnel-flow'
  | 'comet-flow'
  | 'lissajous'
  | 'oscilloscope'
  | 'energy-bands'
  | 'transient-pulse'
  | 'album-reactive'
  | 'spectral-ribbon'
  | 'linebed';

/**
 * Get the configured FPS for a panel.
 * Returns 0 for disabled, or the FPS value.
 */
export function getPanelFps(panelId: ImmersivePanelId): number {
  const defaultFps = PANEL_DEFAULTS[panelId] ?? DEFAULT_FPS;
  const stored = getUserItem(`${FPS_KEY_PREFIX}${panelId}`);
  if (stored === null) return defaultFps;
  const parsed = Number.parseInt(stored, 10);
  return Number.isNaN(parsed) ? defaultFps : parsed;
}

/**
 * Get the frame interval in ms for a panel (0 if disabled).
 */
export function getPanelFrameInterval(panelId: ImmersivePanelId): number {
  const fps = getPanelFps(panelId);
  return fps > 0 ? 1000 / fps : 0;
}
