import { getUserItem, setUserItem } from '$lib/utils/userStorage';

// Show/hide purchases sidebar entry
const KEY_SHOW = 'qbz-show-purchases';
let showPurchases = getUserItem(KEY_SHOW) === 'true';

/**
 * Re-read all values from localStorage after setStorageUserId() has been called.
 * At module load time the userId is null, so getUserItem reads the wrong (unscoped) key.
 * This must be called after login to pick up the correct user-scoped values.
 */
export function rehydratePurchasesStore(): void {
  showPurchases = getUserItem(KEY_SHOW) === 'true';
  hideUnavailable = getUserItem(KEY_HIDE_UNAVAILABLE) === 'true';
  hideDownloaded = getUserItem(KEY_HIDE_DOWNLOADED) === 'true';
}

export function getShowPurchases(): boolean {
  return showPurchases;
}

export function setShowPurchases(v: boolean): void {
  showPurchases = v;
  setUserItem(KEY_SHOW, String(v));
}

// Filter: hide unavailable
const KEY_HIDE_UNAVAILABLE = 'qbz-purchases-hide-unavailable';
let hideUnavailable = getUserItem(KEY_HIDE_UNAVAILABLE) === 'true';

export function getHideUnavailable(): boolean {
  return hideUnavailable;
}

export function setHideUnavailable(v: boolean): void {
  hideUnavailable = v;
  setUserItem(KEY_HIDE_UNAVAILABLE, String(v));
}

// Filter: hide downloaded
const KEY_HIDE_DOWNLOADED = 'qbz-purchases-hide-downloaded';
let hideDownloaded = getUserItem(KEY_HIDE_DOWNLOADED) === 'true';

export function getHideDownloaded(): boolean {
  return hideDownloaded;
}

export function setHideDownloaded(v: boolean): void {
  hideDownloaded = v;
  setUserItem(KEY_HIDE_DOWNLOADED, String(v));
}
