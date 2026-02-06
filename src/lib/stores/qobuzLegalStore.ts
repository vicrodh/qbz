import { browser } from '$app/environment';
import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

// Store for ToS acceptance - persisted in Rust (survives app updates)
export const qobuzTosAccepted = writable<boolean>(false);

// Flag to track if we've loaded from Rust
let initialized = false;

/**
 * Load ToS acceptance state from Rust backend.
 * Should be called early in app initialization.
 */
export async function loadTosAcceptance(): Promise<boolean> {
  if (!browser) return false;

  try {
    const accepted = await invoke<boolean>('get_qobuz_tos_accepted');
    qobuzTosAccepted.set(accepted);
    initialized = true;
    return accepted;
  } catch (err) {
    console.debug('Failed to load ToS acceptance from backend:', err);
    // Fallback to localStorage for migration from old versions
    const localValue = localStorage.getItem('qbz-qobuz-tos-accepted') === 'true';
    if (localValue) {
      // Migrate to Rust storage
      try {
        await invoke('set_qobuz_tos_accepted', { accepted: true });
        qobuzTosAccepted.set(true);
        localStorage.removeItem('qbz-qobuz-tos-accepted');
        console.log('Migrated ToS acceptance to Rust storage');
        return true;
      } catch (migrateErr) {
        console.error('Failed to migrate ToS acceptance:', migrateErr);
      }
    }
    initialized = true;
    return localValue;
  }
}

/**
 * Set ToS acceptance state in Rust backend.
 */
export async function setTosAcceptance(accepted: boolean): Promise<void> {
  if (!browser) return;

  try {
    await invoke('set_qobuz_tos_accepted', { accepted });
    qobuzTosAccepted.set(accepted);
  } catch (err) {
    console.error('Failed to save ToS acceptance:', err);
    throw err;
  }
}

/**
 * Subscribe to changes and persist to Rust.
 * Only persists if value actually changed and we're initialized.
 */
if (browser) {
  let lastValue: boolean | null = null;

  qobuzTosAccepted.subscribe((value) => {
    // Skip if not initialized or value hasn't changed
    if (!initialized || lastValue === value) {
      lastValue = value;
      return;
    }
    lastValue = value;

    // Persist to Rust (fire and forget, errors logged in setTosAcceptance)
    invoke('set_qobuz_tos_accepted', { accepted: value }).catch((err) => {
      console.debug('Failed to persist ToS acceptance:', err);
    });
  });
}
