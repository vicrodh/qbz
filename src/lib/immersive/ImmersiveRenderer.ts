/**
 * Immersive Renderer - Lifecycle Manager
 *
 * This module owns the immersive rendering lifecycle:
 * - Capability detection
 * - Initialization / destruction
 * - State management
 * - Debug instrumentation
 *
 * UI components should use this as the single entry point.
 */

import { writable, derived, get } from 'svelte/store';
import {
  BUILD_IMMERSIVE_ENABLED,
  shouldImmersiveBeAvailable,
  getWebGL2Info,
  getConfig,
  setConfig,
  setRuntimeEnabled as setConfigRuntimeEnabled,
} from './config';
import type {
  ImmersiveState,
  ImmersiveBackend,
  UnavailableReason,
  ImmersiveMetrics,
  BackgroundMode,
} from './types';
import { showToast } from '$lib/stores/toastStore';

// ============================================================================
// State Store
// ============================================================================

const initialState: ImmersiveState = {
  active: false,
  backend: 'disabled',
  unavailableReason: BUILD_IMMERSIVE_ENABLED ? undefined : 'build-disabled',
};

/** Internal writable store */
const stateStore = writable<ImmersiveState>(initialState);

/** Public read-only store for immersive state */
export const immersiveState = { subscribe: stateStore.subscribe };

/** Derived store: is immersive currently active? */
export const isImmersiveActive = derived(stateStore, ($state) => $state.active);

/** Derived store: current backend */
export const immersiveBackend = derived(stateStore, ($state) => $state.backend);

/** Derived store: debug metrics (null if not active) */
export const immersiveMetrics = derived(stateStore, ($state) => $state.metrics ?? null);

// ============================================================================
// Metrics Tracking
// ============================================================================

let frameCount = 0;
let lastFpsUpdate = 0;
let currentFps = 0;
let lastFrameTime = 0;

// Performance watchdog: auto-degrade background mode on sustained low FPS
const PERF_FPS_THRESHOLD = 15;      // Below this = performance issue (real failures drop to single digits)
const PERF_WARMUP_MS = 3000;        // Ignore first 3s (texture loading, init)
const PERF_SUSTAIN_SECONDS = 5;     // Must be sustained for 5s before acting
let perfWatchdogStart = 0;           // When immersive was initialized
let lowFpsConsecutive = 0;           // Consecutive 1-second ticks below threshold
let perfDegraded = false;            // Already degraded this session (don't loop)

/**
 * Update metrics from render loop.
 * Called by ImmersiveAmbientCanvas on each frame.
 */
export function updateMetrics(frameTimeMs: number): void {
  frameCount++;
  lastFrameTime = frameTimeMs;

  const now = performance.now();
  if (now - lastFpsUpdate >= 1000) {
    currentFps = frameCount;
    frameCount = 0;
    lastFpsUpdate = now;

    // Update store with new metrics
    stateStore.update((state) => {
      if (!state.active) return state;
      return {
        ...state,
        metrics: {
          ...state.metrics!,
          fps: currentFps,
          frameTimeMs: lastFrameTime,
        },
      };
    });

    // Performance watchdog (only after warmup, only once per session)
    if (!perfDegraded && perfWatchdogStart > 0 && (now - perfWatchdogStart) > PERF_WARMUP_MS) {
      if (currentFps > 0 && currentFps < PERF_FPS_THRESHOLD) {
        lowFpsConsecutive++;
        if (lowFpsConsecutive >= PERF_SUSTAIN_SECONDS) {
          degradeBackgroundMode();
        }
      } else {
        // Reset counter if FPS recovers
        lowFpsConsecutive = 0;
      }
    }
  }
}

/**
 * Auto-degrade background mode due to detected performance issues.
 * Full → Lite → Off. Persists the change and notifies the user.
 */
function degradeBackgroundMode(): void {
  const config = getConfig();
  const currentMode: BackgroundMode = config.backgroundMode ?? 'full';

  let newMode: BackgroundMode;
  if (currentMode === 'full') {
    newMode = 'lite';
  } else if (currentMode === 'lite') {
    newMode = 'off';
  } else {
    // Already off, nothing to degrade to
    perfDegraded = true;
    return;
  }

  console.warn(
    `[Immersive] Performance watchdog: ${currentFps} FPS for ${PERF_SUSTAIN_SECONDS}s, ` +
    `degrading background ${currentMode} → ${newMode}`
  );

  // Persist the change
  setConfig({ backgroundMode: newMode });
  perfDegraded = true;
  lowFpsConsecutive = 0;

  // Notify user
  const modeLabel = newMode === 'lite' ? 'Lite' : 'Off';
  showToast(
    `Low FPS detected — background switched to ${modeLabel} for better performance`,
    'info',
    5000
  );

  // Emit event so ImmersiveBackground can react without full remount
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('immersive:background-degraded', {
      detail: { from: currentMode, to: newMode }
    }));
  }
}

/**
 * Set GPU info in metrics (called once during init).
 */
function setGpuInfo(renderer: string): void {
  stateStore.update((state) => ({
    ...state,
    metrics: {
      fps: 0,
      frameTimeMs: 0,
      gpuRenderer: renderer,
      textureCount: 0,
      gpuMemoryBytes: 0,
    },
  }));
}

/**
 * Update texture count in metrics.
 */
export function updateTextureCount(count: number, memoryBytes: number): void {
  stateStore.update((state) => {
    if (!state.metrics) return state;
    return {
      ...state,
      metrics: {
        ...state.metrics,
        textureCount: count,
        gpuMemoryBytes: memoryBytes,
      },
    };
  });
}

// ============================================================================
// Public API
// ============================================================================

/**
 * Check if immersive rendering is available.
 * This checks build flags, runtime preferences, and WebGL2 capability.
 */
export function isAvailable(): boolean {
  return shouldImmersiveBeAvailable().available;
}

/**
 * Check if immersive is currently enabled (user preference).
 */
export function isEnabled(): boolean {
  const { available, reason } = shouldImmersiveBeAvailable();
  if (!available && reason === 'runtime-disabled') {
    return false;
  }
  return available;
}

/**
 * Enable or disable immersive rendering (user preference).
 */
export function setEnabled(enabled: boolean): void {
  setConfigRuntimeEnabled(enabled);

  if (enabled) {
    // Check if we can actually enable
    const { available, reason } = shouldImmersiveBeAvailable();
    if (!available) {
      stateStore.set({
        active: false,
        backend: 'disabled',
        unavailableReason: reason,
      });
    }
  } else {
    stateStore.set({
      active: false,
      backend: 'disabled',
      unavailableReason: 'runtime-disabled',
    });
  }
}

/**
 * Initialize immersive rendering.
 * Call this when the immersive view is mounted.
 *
 * Returns true if initialization succeeded.
 */
export async function init(): Promise<boolean> {
  const { available, reason } = shouldImmersiveBeAvailable();

  if (!available) {
    console.log(`[Immersive] Not available: ${reason}`);
    stateStore.set({
      active: false,
      backend: 'disabled',
      unavailableReason: reason,
    });
    return false;
  }

  // Get GPU info for debugging
  const gpuInfo = getWebGL2Info();
  const renderer = gpuInfo?.renderer ?? 'Unknown GPU';
  console.log(`[Immersive] Initializing with WebGL2 - ${renderer}`);

  // Reset performance watchdog
  perfWatchdogStart = performance.now();
  lowFpsConsecutive = 0;
  perfDegraded = false;

  // Set initial state
  stateStore.set({
    active: true,
    backend: 'webgl2',
    metrics: {
      fps: 0,
      frameTimeMs: 0,
      gpuRenderer: renderer,
      textureCount: 0,
      gpuMemoryBytes: 0,
    },
  });

  return true;
}

/**
 * Destroy immersive rendering.
 * Call this when the immersive view is unmounted.
 */
export function destroy(): void {
  console.log('[Immersive] Destroyed');

  // Reset metrics and watchdog
  frameCount = 0;
  lastFpsUpdate = 0;
  currentFps = 0;
  perfWatchdogStart = 0;
  lowFpsConsecutive = 0;

  stateStore.set({
    active: false,
    backend: 'disabled',
  });
}

/**
 * Handle WebGL context loss.
 * Call this from the canvas component when context is lost.
 */
export function handleContextLost(): void {
  console.warn('[Immersive] WebGL context lost');
  stateStore.update((state) => ({
    ...state,
    active: false,
    backend: 'fallback',
    unavailableReason: 'context-lost',
  }));
}

/**
 * Handle WebGL context restored.
 * Call this from the canvas component when context is restored.
 */
export function handleContextRestored(): void {
  console.log('[Immersive] WebGL context restored');
  stateStore.update((state) => ({
    ...state,
    active: true,
    backend: 'webgl2',
    unavailableReason: undefined,
  }));
}

/**
 * Get current state snapshot (for debugging).
 */
export function getState(): ImmersiveState {
  return get(stateStore);
}

/**
 * Get current configuration.
 */
export { getConfig } from './config';
