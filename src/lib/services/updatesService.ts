import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  acknowledgeRelease,
  checkForUpdates,
  fetchReleaseForVersion,
  getCurrentVersion,
  getPreferences,
  hasWhatsNewBeenShown,
  ignoreRelease,
  initUpdatesStore,
  markWhatsNewShown,
  setCheckOnLaunch,
} from '$lib/stores/updatesStore';
import type { ReleaseInfo } from '$lib/stores/updatesStore';

const SESSION_ID_KEY = 'qbz-updates-session-id';
const SESSION_CHECK_DONE_KEY = 'qbz-updates-session-check-done';
const SESSION_UPDATE_SHOWN_KEY = 'qbz-updates-session-update-shown';
const SESSION_WHATS_NEW_SHOWN_KEY = 'qbz-updates-session-whats-new-shown';

const DEV_FAKE_ENABLED_KEY = 'qbz-updates-dev-fake-enabled';
const DEV_FAKE_VERSION_KEY = 'qbz-updates-dev-fake-version';
const DEV_FAKE_URL_KEY = 'qbz-updates-dev-fake-url';
const DEV_FAKE_BODY_KEY = 'qbz-updates-dev-fake-body';

let sessionId = '';
let launchFlowStarted = false;

function ensureSessionId(): string {
  if (sessionId) return sessionId;
  if (typeof window === 'undefined') {
    sessionId = 'server';
    return sessionId;
  }
  sessionId = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  try {
    localStorage.setItem(SESSION_ID_KEY, sessionId);
  } catch {
    // Ignore storage errors; we will still gate in memory.
  }
  return sessionId;
}

function sessionScopedKey(baseKey: string): string {
  return `${baseKey}:${ensureSessionId()}`;
}

function getSessionFlag(baseKey: string): boolean {
  if (typeof window === 'undefined') return false;
  try {
    return localStorage.getItem(sessionScopedKey(baseKey)) === '1';
  } catch {
    return false;
  }
}

function setSessionFlag(baseKey: string): void {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(sessionScopedKey(baseKey), '1');
  } catch {
    // Ignore storage errors.
  }
}

function parseVersionParts(version: string): number[] {
  return version
    .trim()
    .replace(/^v/i, '')
    .split('.')
    .map((segment) => {
      const numeric = segment.match(/\d+/)?.[0] ?? '0';
      return Number.parseInt(numeric, 10) || 0;
    });
}

function isNewerVersion(current: string, candidate: string): boolean {
  const a = parseVersionParts(current);
  const b = parseVersionParts(candidate);
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i += 1) {
    const av = a[i] ?? 0;
    const bv = b[i] ?? 0;
    if (bv > av) return true;
    if (bv < av) return false;
  }
  return false;
}

function getDevFakeRelease(currentVersion: string): ReleaseInfo | null {
  if (typeof window === 'undefined') return null;
  try {
    const enabled = localStorage.getItem(DEV_FAKE_ENABLED_KEY) === '1';
    if (!enabled) return null;

    const version = (localStorage.getItem(DEV_FAKE_VERSION_KEY) || '').trim();
    if (!version) return null;
    if (!isNewerVersion(currentVersion, version)) return null;

    const htmlUrl =
      localStorage.getItem(DEV_FAKE_URL_KEY) ||
      `https://github.com/vicrodh/qbz/releases/tag/v${version}`;
    const body = localStorage.getItem(DEV_FAKE_BODY_KEY);

    const publishedAtDate = new Date(Date.now() - 13 * 60 * 60 * 1000);
    const publishedAt = publishedAtDate.toISOString();

    return {
      version,
      tagName: version.startsWith('v') ? version : `v${version}`,
      name: `v${version}`,
      publishedAt,
      publishedAtEpoch: Math.floor(publishedAtDate.getTime() / 1000),
      body,
      htmlUrl,
      isOldEnough: true,
    };
  } catch {
    return null;
  }
}

export interface LaunchUpdateDecision {
  currentVersion: string;
  showFlatpakWelcome: boolean;
  whatsNewRelease: ReleaseInfo | null;
  updateRelease: ReleaseInfo | null;
}

/**
 * Check if running in Flatpak and if welcome modal has been shown.
 */
async function shouldShowFlatpakWelcome(): Promise<boolean> {
  try {
    const isFlatpak = await invoke<boolean>('is_running_in_flatpak');
    if (!isFlatpak) return false;
    const alreadyShown = await invoke<boolean>('has_flatpak_welcome_been_shown');
    return !alreadyShown;
  } catch {
    return false;
  }
}

/**
 * Mark Flatpak welcome modal as shown.
 */
export async function markFlatpakWelcomeShown(): Promise<void> {
  try {
    await invoke('mark_flatpak_welcome_shown');
  } catch (error) {
    console.debug('[Updates] Failed to mark flatpak welcome shown:', error);
  }
}

/**
 * Decide which modals should be shown on launch.
 *
 * Priority (shown sequentially, one after another):
 * 1) Flatpak welcome modal (only for Flatpak users, once ever)
 * 2) What's New modal (once per version)
 * 3) Update available modal (when newer version exists)
 */
export async function decideLaunchModals(): Promise<LaunchUpdateDecision> {
  if (launchFlowStarted) {
    return {
      currentVersion: getCurrentVersion(),
      showFlatpakWelcome: false,
      whatsNewRelease: null,
      updateRelease: null,
    };
  }
  launchFlowStarted = true;

  ensureSessionId();

  await initUpdatesStore();
  const prefs = getPreferences();
  const currentVersion = getCurrentVersion();

  if (getSessionFlag(SESSION_CHECK_DONE_KEY)) {
    return { currentVersion, showFlatpakWelcome: false, whatsNewRelease: null, updateRelease: null };
  }
  setSessionFlag(SESSION_CHECK_DONE_KEY);

  // Collect all modals that need to be shown
  let showFlatpakWelcome = false;
  let whatsNewRelease: ReleaseInfo | null = null;
  let updateRelease: ReleaseInfo | null = null;

  // Phase 1: Flatpak welcome (highest priority, shown first)
  showFlatpakWelcome = await shouldShowFlatpakWelcome();

  // Phase 2: What's New for current version
  if (prefs.showWhatsNewOnLaunch && currentVersion) {
    const alreadyShown = await hasWhatsNewBeenShown(currentVersion);
    if (!alreadyShown) {
      const scopedWhatsNewKey = `${SESSION_WHATS_NEW_SHOWN_KEY}:${currentVersion}`;
      if (!getSessionFlag(scopedWhatsNewKey)) {
        const release = await fetchReleaseForVersion(currentVersion);
        if (release) {
          setSessionFlag(scopedWhatsNewKey);
          await markWhatsNewShown(currentVersion);
          whatsNewRelease = release;
        }
      }
    }
  }

  // Phase 3: Update available (lowest priority, shown last)
  if (prefs.checkOnLaunch) {
    const devFake = getDevFakeRelease(currentVersion);
    if (devFake) {
      const devScopedKey = `${SESSION_UPDATE_SHOWN_KEY}:${devFake.version}`;
      if (!getSessionFlag(devScopedKey)) {
        setSessionFlag(devScopedKey);
        updateRelease = devFake;
      }
    }

    if (!updateRelease) {
      const result = await checkForUpdates('launch');
      if (result.status === 'update_available' && result.release) {
        const scopedUpdateKey = `${SESSION_UPDATE_SHOWN_KEY}:${result.release.version}`;
        if (!getSessionFlag(scopedUpdateKey)) {
          setSessionFlag(scopedUpdateKey);
          updateRelease = result.release;
        }
      }
    }
  }

  return { currentVersion, showFlatpakWelcome, whatsNewRelease, updateRelease };
}

export async function openReleasePageAndAcknowledge(release: ReleaseInfo): Promise<void> {
  try {
    await openUrl(release.htmlUrl);
  } catch (error) {
    console.debug('[Updates] Failed to open release URL:', error);
  }
  try {
    await acknowledgeRelease(release.version);
  } catch (error) {
    console.debug('[Updates] Failed to acknowledge release:', release.version, error);
  }
}

export async function ignoreReleaseVersion(version: string): Promise<void> {
  try {
    await ignoreRelease(version);
  } catch (error) {
    console.debug('[Updates] Failed to ignore release:', version, error);
  }
}

export async function disableUpdateChecks(): Promise<void> {
  try {
    await setCheckOnLaunch(false);
  } catch (error) {
    console.debug('[Updates] Failed to disable update checks:', error);
  }
}
