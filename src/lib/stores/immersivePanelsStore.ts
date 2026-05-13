/**
 * Per-panel enable/disable state for immersive views.
 *
 * Each panel can be turned off by the user from Settings; disabled panels
 * are filtered out of the immersive selector and submenu so a CPU-bound
 * user never accidentally lands on one that pins their CPU.
 *
 * In CPU mode (HW accel detected off), five known-heavy panels default to
 * disabled: AlbumReactive, NeonFlow (Laser), TunnelFlow, CometFlow,
 * TransientPulse. These rely on technique that is simply not viable on
 * software compositing (3D perspective shaders, particle systems, dense
 * per-pixel filter passes). In GPU mode all panels default enabled.
 *
 * User overrides win. If the user explicitly re-enables a heavy panel in
 * CPU mode, we respect that choice — we just don't push them into it by
 * default.
 */

import { writable } from 'svelte/store';
import { isHardwareAccelEnabled } from '$lib/runtime/graphicsState';
import { getUserItem, setUserItem } from '$lib/utils/userStorage';

export type ImmersivePanelId =
  | 'visualizer'
  | 'oscilloscope'
  | 'spectral-ribbon'
  | 'energy-bands'
  | 'lissajous'
  | 'transient-pulse'
  | 'album-reactive'
  | 'linebed'
  | 'neon-flow'
  | 'tunnel-flow'
  | 'comet-flow';

export const ALL_IMMERSIVE_PANELS: ImmersivePanelId[] = [
  'visualizer',
  'oscilloscope',
  'spectral-ribbon',
  'energy-bands',
  'lissajous',
  'transient-pulse',
  'album-reactive',
  'linebed',
  'neon-flow',
  'tunnel-flow',
  'comet-flow',
];

// Panels known to require GPU compositing to be usable. In CPU mode they
// default to disabled so the user doesn't land on them by accident.
export const HEAVY_PANELS: ImmersivePanelId[] = [
  'album-reactive',
  'neon-flow',
  'tunnel-flow',
  'comet-flow',
  'transient-pulse',
];

const STORAGE_KEY = 'qbz-immersive-panels-enabled';

function loadFromStorage(): Partial<Record<ImmersivePanelId, boolean>> {
  const raw = getUserItem(STORAGE_KEY);
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw);
    return typeof parsed === 'object' && parsed !== null ? parsed : {};
  } catch {
    return {};
  }
}

function persistToStorage(state: Record<ImmersivePanelId, boolean>) {
  setUserItem(STORAGE_KEY, JSON.stringify(state));
}

function computeInitialState(): Record<ImmersivePanelId, boolean> {
  const stored = loadFromStorage();
  const lowProfile = !isHardwareAccelEnabled();
  const heavySet = new Set<ImmersivePanelId>(HEAVY_PANELS);

  const result = {} as Record<ImmersivePanelId, boolean>;
  for (const id of ALL_IMMERSIVE_PANELS) {
    if (id in stored) {
      result[id] = stored[id] === true;
    } else {
      result[id] = lowProfile ? !heavySet.has(id) : true;
    }
  }
  return result;
}

const initial = computeInitialState();
export const immersivePanelsStore = writable<Record<ImmersivePanelId, boolean>>(initial);

export function isPanelEnabled(id: ImmersivePanelId): boolean {
  let snapshot: Record<ImmersivePanelId, boolean> = initial;
  immersivePanelsStore.subscribe((value) => {
    snapshot = value;
  })();
  return snapshot[id] === true;
}

export function setPanelEnabled(id: ImmersivePanelId, enabled: boolean): void {
  immersivePanelsStore.update((current) => {
    const next = { ...current, [id]: enabled };
    persistToStorage(next);
    return next;
  });
}

/**
 * Re-apply the CPU-mode default-off rule for the five heavy panels.
 *
 * Why this exists: the graphics-accel detection runs through an async
 * `invoke()` that may not have resolved by the time this store's module
 * loads. The initial state computed at load time can therefore be wrong
 * — `isHardwareAccelEnabled()` returns its `true` fallback while the
 * cache is still null, so a CPU-mode user gets all panels enabled on
 * their FIRST launch (no localStorage yet to correct it).
 *
 * The `bootstrapGraphicsState()` flow calls this AFTER the cache loads.
 * It only overrides panels the user hasn't explicitly toggled — anything
 * already present in the persisted localStorage snapshot is left alone,
 * because that represents a user override that must be respected.
 */
export function applyCpuModeDefaultsIfNeeded(lowProfile: boolean): void {
  if (!lowProfile) return;
  const stored = loadFromStorage();
  immersivePanelsStore.update((current) => {
    let mutated = false;
    const next = { ...current };
    for (const id of HEAVY_PANELS) {
      // Only override if the user has never expressed a preference for
      // this panel. If `id in stored`, the user has explicitly toggled
      // it at some point — keep their choice.
      if (!(id in stored) && next[id] !== false) {
        next[id] = false;
        mutated = true;
      }
    }
    return mutated ? next : current;
  });
}
