/**
 * Immersive Rendering Types
 *
 * Type definitions for the WebGL2-based immersive rendering system.
 * This module has no runtime dependencies and can be safely imported anywhere.
 */

/** Rendering backend currently in use */
export type ImmersiveBackend = 'webgl2' | 'fallback' | 'disabled';

/** Reason why immersive is unavailable */
export type UnavailableReason =
  | 'build-disabled'      // Compiled out via QBZ_IMMERSIVE=false
  | 'runtime-disabled'    // User disabled in settings
  | 'no-webgl2'           // Browser/WebView doesn't support WebGL2
  | 'context-lost'        // WebGL context was lost
  | 'init-failed';        // Initialization failed for other reason

/** Current state of the immersive renderer */
export interface ImmersiveState {
  /** Whether immersive rendering is currently active */
  active: boolean;
  /** Current backend in use */
  backend: ImmersiveBackend;
  /** If not active, why */
  unavailableReason?: UnavailableReason;
  /** Debug metrics (only populated when active) */
  metrics?: ImmersiveMetrics;
}

/** Performance metrics for debugging */
export interface ImmersiveMetrics {
  /** Frames per second */
  fps: number;
  /** Last frame time in milliseconds */
  frameTimeMs: number;
  /** GPU renderer string (e.g., "Mesa Intel UHD 620") */
  gpuRenderer: string;
  /** Number of textures currently loaded */
  textureCount: number;
  /** Estimated GPU memory usage in bytes */
  gpuMemoryBytes: number;
}

/** Configuration options for the immersive renderer */
export interface ImmersiveConfig {
  /** Target FPS for animation (lower = less power) */
  targetFps: number;
  /** Ambient motion intensity (0 = static, 1 = full motion) */
  ambientIntensity: number;
  /** Whether to pause rendering when tab is hidden */
  pauseWhenHidden: boolean;
  /** Enable debug overlay with metrics */
  debugOverlay: boolean;
}

/** Default configuration */
export const DEFAULT_IMMERSIVE_CONFIG: ImmersiveConfig = {
  targetFps: 30,
  ambientIntensity: 1.0, // Full intensity for maximum ambient motion
  pauseWhenHidden: true,
  debugOverlay: false,
};
