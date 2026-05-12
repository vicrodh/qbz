/**
 * Frontend cache of the runtime graphics state.
 *
 * Reading the actual `v2_get_graphics_startup_status` invoke is async — too
 * slow for hot paths like the `cachedSrc` action that runs on every artwork
 * `<img>` mount. This module loads the state once at app boot and exposes
 * a sync getter that callers can read at any time.
 *
 * Conservative default: assume HW accel is ON. The hot path inside
 * `cachedSrc` reads this getter to decide whether to apply the WebKitGTK
 * 2.50+ texture-eviction workaround (`will-change: transform; transform:
 * translateZ(0)`), which helps under HW accel but hurts under software
 * compositing — see ADR-004.
 */

import { invoke } from '@tauri-apps/api/core';

interface GraphicsStartupStatus {
  using_fallback: boolean;
  is_wayland: boolean;
  has_nvidia: boolean;
  has_amd: boolean;
  has_intel: boolean;
  is_vm: boolean;
  hardware_accel_enabled: boolean;
  force_x11_active: boolean;
}

let cached: GraphicsStartupStatus | null = null;
let bootstrapped = false;

/**
 * Fetch graphics state from the backend once. Safe to call multiple times —
 * subsequent calls are no-ops. Call from app boot (before any cachedSrc
 * mount); if you forget, callers fall back to "HW accel ON" defaults.
 */
export async function bootstrapGraphicsState(): Promise<void> {
  if (bootstrapped) return;
  bootstrapped = true;
  try {
    cached = await invoke<GraphicsStartupStatus>('v2_get_graphics_startup_status');
  } catch (err) {
    console.warn('[graphicsState] bootstrap failed, using HW-accel-ON defaults', err);
    cached = null;
  }
}

/**
 * True when the WebKit instance is running with GPU compositing enabled
 * (the qbz `hardware_acceleration` setting is ON and no NVIDIA-Wayland
 * mitigation forced it off). Falls back to `true` if we haven't
 * bootstrapped yet — the worst case is a one-frame mis-apply of the
 * texture-eviction workaround that the next paint corrects.
 */
export function isHardwareAccelEnabled(): boolean {
  if (!cached) return true;
  return cached.hardware_accel_enabled;
}

export function getGraphicsStatus(): GraphicsStartupStatus | null {
  return cached;
}
