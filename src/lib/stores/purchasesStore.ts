import { getUserItem, setUserItem } from '$lib/utils/userStorage';

// Show/hide purchases sidebar entry
const KEY_SHOW = 'qbz-show-purchases';
let showPurchases = getUserItem(KEY_SHOW) === 'true';

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
