/**
 * Platform detection via @tauri-apps/plugin-os.
 *
 * `platform()` is synchronous (compile-time constant baked into the binary).
 * Falls back to 'linux' in non-Tauri environments (vitest, SSR, svelte-check).
 *
 * NOTE: The `macos` CSS class on `<html>` is set separately by an inline
 * script in app.html for FOUC prevention — that is intentionally kept.
 */

import { platform as tauriPlatform } from '@tauri-apps/plugin-os';

type Platform = 'macos' | 'linux' | 'windows';

let detectedPlatform: Platform;
try {
  detectedPlatform = tauriPlatform() as Platform;
} catch {
  // Fallback for non-Tauri environments (vitest, SSR, svelte-check)
  detectedPlatform = 'linux';
}

export const platform: Platform = detectedPlatform;
