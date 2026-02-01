/**
 * Shader Loader
 *
 * Imports shader sources as raw strings using Vite's ?raw feature.
 * This allows shaders to be:
 * - Type-checked at import time
 * - Tree-shaken if unused
 * - Bundled inline (no runtime fetch)
 */

// Vertex shaders
import fullscreenVert from './fullscreen.vert?raw';

// Fragment shaders
import staticFrag from './static.frag?raw';
import ambientFrag from './ambient.frag?raw';

/**
 * Available shader programs.
 * Each program consists of a vertex and fragment shader.
 */
export const SHADERS = {
  /**
   * Static shader: displays texture without effects.
   * Used as Phase 1 baseline.
   */
  static: {
    vertex: fullscreenVert,
    fragment: staticFrag,
  },

  /**
   * Ambient shader: adds motion effects.
   * UV drift, zoom oscillation, color breathing.
   * Requires u_time and u_intensity uniforms.
   */
  ambient: {
    vertex: fullscreenVert,
    fragment: ambientFrag,
  },
} as const;

export type ShaderName = keyof typeof SHADERS;
