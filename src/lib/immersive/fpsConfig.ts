/**
 * Immersive FPS Configuration
 *
 * Reads per-panel frame rate settings from userStorage.
 * Default is 30fps for all panels — perceptually smooth for the kind
 * of slow-evolving audio-reactive visuals the immersive panels render,
 * without burning CPU/GPU on machines that didn't ask for 60. Users
 * who tuned specific panels higher keep their stored value — only
 * panels with no stored preference pick up the new default.
 */

import { getUserItem } from '$lib/utils/userStorage';

const FPS_KEY_PREFIX = 'qbz-immersive-fps-';
const DEFAULT_FPS = 30;

// Per-panel default overrides (panels not listed here use DEFAULT_FPS).
// linebed used to run at 60 — 30 looks fine for the terrain effect and
// aligns with the rest of the panels.
const PANEL_DEFAULTS: Partial<Record<ImmersivePanelId, number>> = {};

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
