/**
 * Immersive FPS Configuration
 *
 * Reads per-panel frame rate settings from userStorage.
 * Default is 60fps for all panels.
 */

import { getUserItem } from '$lib/utils/userStorage';

const FPS_KEY_PREFIX = 'qbz-immersive-fps-';
const DEFAULT_FPS = 60;

export type ImmersivePanelId =
  | 'ambient'
  | 'visualizer'
  | 'lissajous'
  | 'oscilloscope'
  | 'energy-bands'
  | 'transient-pulse'
  | 'album-reactive'
  | 'spectral-ribbon';

/**
 * Get the configured FPS for a panel.
 * Returns 0 for disabled, or the FPS value.
 */
export function getPanelFps(panelId: ImmersivePanelId): number {
  const stored = getUserItem(`${FPS_KEY_PREFIX}${panelId}`);
  if (stored === null) return DEFAULT_FPS;
  const parsed = parseInt(stored, 10);
  return isNaN(parsed) ? DEFAULT_FPS : parsed;
}

/**
 * Get the frame interval in ms for a panel (0 if disabled).
 */
export function getPanelFrameInterval(panelId: ImmersivePanelId): number {
  const fps = getPanelFps(panelId);
  return fps > 0 ? 1000 / fps : 0;
}
