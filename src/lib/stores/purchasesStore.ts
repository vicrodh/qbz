import { getUserItem, setUserItem } from '$lib/utils/userStorage';

const KEY = 'qbz-show-purchases';

let showPurchases = getUserItem(KEY) === 'true';

export function getShowPurchases(): boolean {
  return showPurchases;
}

export function setShowPurchases(v: boolean): void {
  showPurchases = v;
  setUserItem(KEY, String(v));
}
