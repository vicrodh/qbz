/**
 * Immersive Rendering Module - Public API
 *
 * This is the ONLY file that should be imported from outside the module.
 *
 * Usage:
 *   import { isAvailable, init, destroy } from '$lib/immersive';
 *
 * IMPORTANT: No file outside this module should:
 *   - Import WebGL APIs directly
 *   - Import shader files
 *   - Assume canvas/GPU availability
 */

// Re-export types (always safe to import)
export type {
  ImmersiveState,
  ImmersiveBackend,
  UnavailableReason,
  ImmersiveMetrics,
  ImmersiveConfig,
} from './types';

export { DEFAULT_IMMERSIVE_CONFIG } from './types';

// Re-export config utilities
export {
  BUILD_IMMERSIVE_ENABLED,
  isWebGL2Available,
  getWebGL2Info,
  isRuntimeEnabled,
  setRuntimeEnabled,
  getConfig,
  setConfig,
  shouldImmersiveBeAvailable,
} from './config';

// Re-export renderer API
export {
  // Stores
  immersiveState,
  isImmersiveActive,
  immersiveBackend,
  immersiveMetrics,
  // Functions
  isAvailable,
  isEnabled,
  setEnabled,
  init,
  destroy,
  getState,
  // Internal (for canvas component)
  updateMetrics,
  updateTextureCount,
  handleContextLost,
  handleContextRestored,
} from './ImmersiveRenderer';
