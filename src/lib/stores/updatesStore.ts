import { invoke } from '@tauri-apps/api/core';

export interface UpdatePreferences {
  checkOnLaunch: boolean;
  showWhatsNewOnLaunch: boolean;
}

export interface ReleaseInfo {
  version: string;
  tagName: string;
  name: string;
  publishedAt: string;
  publishedAtEpoch: number;
  body: string | null;
  htmlUrl: string;
  isOldEnough: boolean;
}

export type UpdateCheckStatus = 'no_updates' | 'update_available';

export interface UpdateCheckResult {
  status: UpdateCheckStatus;
  currentVersion: string;
  release: ReleaseInfo | null;
}

let preferences: UpdatePreferences = {
  checkOnLaunch: true,
  showWhatsNewOnLaunch: true,
};

let currentVersion = '';
let initialized = false;

const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) {
    listener();
  }
}

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

export function getPreferences(): UpdatePreferences {
  return preferences;
}

export function getCurrentVersion(): string {
  return currentVersion;
}

export async function initUpdatesStore(): Promise<void> {
  if (initialized) return;
  initialized = true;

  // get_current_version never needs a session (compile-time value).
  // get_update_preferences needs the SQLite store which is only
  // available after login.  Fetch them independently so a preference
  // failure doesn't lose the version.
  try {
    currentVersion = await invoke<string>('get_current_version');
  } catch (error) {
    console.error('[Updates] Failed to get current version:', error);
  }

  try {
    preferences = await invoke<UpdatePreferences>('get_update_preferences');
  } catch {
    // Expected before login — store not initialised yet.
  }

  notify();
}

export async function refreshUpdatePreferences(): Promise<void> {
  try {
    preferences = await invoke<UpdatePreferences>('get_update_preferences');
    notify();
  } catch (error) {
    console.debug('[Updates] Failed to refresh update preferences:', error);
  }
}

export async function setCheckOnLaunch(enabled: boolean): Promise<void> {
  try {
    await invoke('set_update_check_on_launch', { enabled });
    preferences.checkOnLaunch = enabled;
    notify();
  } catch (error) {
    console.error('[Updates] Failed to set checkOnLaunch:', error);
    throw error;
  }
}

export async function setShowWhatsNewOnLaunch(enabled: boolean): Promise<void> {
  try {
    await invoke('set_show_whats_new_on_launch', { enabled });
    preferences.showWhatsNewOnLaunch = enabled;
    notify();
  } catch (error) {
    console.error('[Updates] Failed to set showWhatsNewOnLaunch:', error);
    throw error;
  }
}

export async function checkForUpdates(mode: 'launch' | 'manual'): Promise<UpdateCheckResult> {
  const result = await invoke<UpdateCheckResult>('check_for_updates', { mode });
  return {
    ...result,
    release: result.release ?? null,
  };
}

export async function fetchReleaseForVersion(version: string): Promise<ReleaseInfo | null> {
  try {
    const release = await invoke<ReleaseInfo | null>('fetch_release_for_version', { version });
    return release ?? null;
  } catch (error) {
    console.debug('[Updates] Failed to fetch release for version:', version, error);
    return null;
  }
}

export async function acknowledgeRelease(version: string): Promise<void> {
  await invoke('acknowledge_release', { version });
}

export async function ignoreRelease(version: string): Promise<void> {
  await invoke('ignore_release', { version });
}

export async function hasWhatsNewBeenShown(version: string): Promise<boolean> {
  try {
    return await invoke<boolean>('has_whats_new_been_shown', { version });
  } catch (error) {
    console.debug('[Updates] Failed to check whats_new_shown:', version, error);
    return false;
  }
}

export async function markWhatsNewShown(version: string): Promise<void> {
  try {
    await invoke('mark_whats_new_shown', { version });
  } catch (error) {
    console.debug('[Updates] Failed to mark whats_new_shown:', version, error);
  }
}

