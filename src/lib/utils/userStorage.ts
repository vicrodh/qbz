/**
 * Per-user localStorage scoping
 *
 * Prefixes localStorage keys with the active Qobuz user ID so that
 * different accounts get independent UI preferences.
 *
 * Keys follow the pattern: `qbz.{userId}.{originalKey}`
 * When no user is active, falls back to the original key (pre-migration).
 */

let currentUserId: number | null = null;

const MIGRATION_MARKER_PREFIX = 'qbz.__migrated_to_user.';

/**
 * Set the active user ID for localStorage scoping.
 * Called after login, before any store reads.
 */
export function setStorageUserId(userId: number | null): void {
  currentUserId = userId;
}

/**
 * Get the current storage user ID.
 */
export function getStorageUserId(): number | null {
  return currentUserId;
}

/**
 * Build a user-scoped key from an original key.
 * `qbz-theme` → `qbz.12345678.qbz-theme`
 */
function userKey(key: string): string {
  if (currentUserId === null) {
    return key;
  }
  return `qbz.${currentUserId}.${key}`;
}

/**
 * Read a per-user localStorage value.
 */
export function getUserItem(key: string): string | null {
  return localStorage.getItem(userKey(key));
}

/**
 * Write a per-user localStorage value.
 */
export function setUserItem(key: string, value: string): void {
  localStorage.setItem(userKey(key), value);
}

/**
 * Remove a per-user localStorage value.
 */
export function removeUserItem(key: string): void {
  localStorage.removeItem(userKey(key));
}

/**
 * One-time migration of old global `qbz-*` keys to user-scoped keys.
 *
 * Copies (not moves) existing values so that a rollback to an older
 * version still works. The marker prevents re-running.
 */
export function migrateLocalStorage(userId: number): void {
  const marker = `${MIGRATION_MARKER_PREFIX}${userId}`;
  if (localStorage.getItem(marker)) {
    return;
  }

  let migrated = 0;
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (!key || !key.startsWith('qbz-')) continue;
    // Skip keys that are already user-scoped
    if (key.startsWith('qbz.')) continue;

    const value = localStorage.getItem(key);
    if (value !== null) {
      const scoped = `qbz.${userId}.${key}`;
      // Only copy if the user-scoped key doesn't already exist
      if (localStorage.getItem(scoped) === null) {
        localStorage.setItem(scoped, value);
        migrated++;
      }
    }
  }

  localStorage.setItem(marker, String(Date.now()));

  if (migrated > 0) {
    console.log(`[userStorage] Migrated ${migrated} localStorage keys for user ${userId}`);
  }
}
