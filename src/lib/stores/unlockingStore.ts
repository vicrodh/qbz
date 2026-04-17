/**
 * Unlocking Store
 *
 * Tracks which tracks are currently being decrypted (offline CMAF bundle
 * unlock). The backend emits `offline:unlock_start` / `offline:unlock_end`
 * around `load_cmaf_bundle` calls. TrackRow subscribes and swaps the
 * play/equalizer glyph for an animated padlock while the id is in the
 * active set.
 *
 * IDs here are "display ids" — whatever the UI keys tracks by. For Qobuz
 * flow that's the Qobuz track id; for Local Library that's the library
 * row id. The backend helper decides which one to emit based on context,
 * so the same store handles both flows transparently.
 */
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

const unlockingIds = new Set<number>();
const listeners = new Set<() => void>();
let backendUnlisteners: UnlistenFn[] = [];
let started = false;

function notify(): void {
  for (const listener of listeners) {
    try {
      listener();
    } catch {
      // never let a single subscriber break the fanout
    }
  }
}

export function isUnlocking(trackId: number | null | undefined): boolean {
  if (trackId == null) return false;
  return unlockingIds.has(trackId);
}

/**
 * Subscribe to unlocking-state changes. Svelte 5 runes-friendly: call
 * from a $derived or $effect; the callback is invoked on every add /
 * remove event so the caller can re-evaluate `isUnlocking(id)`.
 */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * Start listening for backend unlock events. Idempotent; subsequent
 * calls are no-ops. Called once during app boot.
 */
export async function startPolling(): Promise<void> {
  if (started) return;
  started = true;

  try {
    const stopStart = await listen<{ trackId: number }>(
      'offline:unlock_start',
      (event) => {
        const id = event.payload?.trackId;
        if (typeof id !== 'number') return;
        if (!unlockingIds.has(id)) {
          unlockingIds.add(id);
          notify();
        }
      }
    );
    const stopEnd = await listen<{ trackId: number; success?: boolean }>(
      'offline:unlock_end',
      (event) => {
        const id = event.payload?.trackId;
        if (typeof id !== 'number') return;
        if (unlockingIds.delete(id)) {
          notify();
        }
      }
    );
    backendUnlisteners = [stopStart, stopEnd];
  } catch (err) {
    console.error('[UnlockingStore] Failed to register listeners:', err);
    started = false;
  }
}

/**
 * Stop listening for backend events and clear state. Called on session
 * teardown so the next user doesn't see stale animations.
 */
export function stopPolling(): void {
  for (const unlisten of backendUnlisteners) {
    try {
      unlisten();
    } catch {
      // ignore; best-effort cleanup
    }
  }
  backendUnlisteners = [];
  unlockingIds.clear();
  started = false;
  notify();
}
