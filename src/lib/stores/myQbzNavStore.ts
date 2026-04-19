import { writable } from 'svelte/store';
import { getUserItem, setUserItem, removeUserItem } from '$lib/utils/userStorage';

const DEFAULT_LABEL = 'My QBZ';
const DEFAULT_ICON = '/my-qbz.svg'; // served from /static
const LABEL_KEY = 'qbz-my-qbz-label';
const ICON_KEY = 'qbz-my-qbz-icon-path';
const EXPANDED_KEY = 'qbz-my-qbz-expanded';

export interface MyQbzNavState {
  label: string;
  /** Either an absolute filesystem path (user-chosen) or '/my-qbz.svg' for default */
  iconPath: string;
  /** Whether the collapsible section is expanded in the sidebar */
  expanded: boolean;
}

function loadInitial(): MyQbzNavState {
  const savedLabel = getUserItem(LABEL_KEY);
  const savedIcon = getUserItem(ICON_KEY);
  const savedExpanded = getUserItem(EXPANDED_KEY);
  return {
    label: savedLabel || DEFAULT_LABEL,
    iconPath: savedIcon || DEFAULT_ICON,
    expanded: savedExpanded === null ? true : savedExpanded === 'true',
  };
}

export const myQbzNavStore = writable<MyQbzNavState>(loadInitial());

export function setMyQbzLabel(label: string): void {
  const trimmed = label.trim();
  const next = trimmed === '' ? DEFAULT_LABEL : trimmed;
  setUserItem(LABEL_KEY, next);
  myQbzNavStore.update((state) => ({ ...state, label: next }));
}

export function setMyQbzIconPath(iconPath: string | null): void {
  if (iconPath === null || iconPath.trim() === '') {
    removeUserItem(ICON_KEY);
    myQbzNavStore.update((state) => ({ ...state, iconPath: DEFAULT_ICON }));
    return;
  }
  setUserItem(ICON_KEY, iconPath);
  myQbzNavStore.update((state) => ({ ...state, iconPath }));
}

export function setMyQbzExpanded(expanded: boolean): void {
  setUserItem(EXPANDED_KEY, String(expanded));
  myQbzNavStore.update((state) => ({ ...state, expanded }));
}

export function reloadMyQbzNav(): void {
  myQbzNavStore.set(loadInitial());
}

export { DEFAULT_LABEL, DEFAULT_ICON };
