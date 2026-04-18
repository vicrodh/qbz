/**
 * Lyrics Display Store
 *
 * Per-user preferences for the LyricsSidebar display:
 * auto-follow toggle, font, font size, and dimming level.
 *
 * Persisted via userStorage (per-user localStorage). Does not affect
 * the immersive lyrics view or the standalone lyrics-only view.
 */

import { writable } from 'svelte/store';
import { getUserItem, setUserItem } from '$lib/utils/userStorage';

export type LyricsFont =
  | 'system'
  | 'line-seed-jp'
  | 'montserrat'
  | 'noto-sans'
  | 'source-sans-3';

export type LyricsFontSize = 'small' | 'medium' | 'large' | 'xl';
export type LyricsDimming = 'off' | 'soft' | 'strong';

export interface LyricsDisplayPrefs {
  autoFollow: boolean;
  font: LyricsFont;
  fontSize: LyricsFontSize;
  dimming: LyricsDimming;
}

export const STORAGE_KEY = 'qbz-lyrics-display';

export const DEFAULT_LYRICS_DISPLAY: LyricsDisplayPrefs = {
  autoFollow: true,
  font: 'system',
  fontSize: 'medium',
  dimming: 'strong'
};

const VALID_FONTS: LyricsFont[] = [
  'system',
  'line-seed-jp',
  'montserrat',
  'noto-sans',
  'source-sans-3'
];
const VALID_SIZES: LyricsFontSize[] = ['small', 'medium', 'large', 'xl'];
const VALID_DIMMINGS: LyricsDimming[] = ['off', 'soft', 'strong'];

function sanitize(raw: unknown): LyricsDisplayPrefs {
  if (!raw || typeof raw !== 'object') return { ...DEFAULT_LYRICS_DISPLAY };
  const r = raw as Partial<LyricsDisplayPrefs>;
  return {
    autoFollow: typeof r.autoFollow === 'boolean' ? r.autoFollow : DEFAULT_LYRICS_DISPLAY.autoFollow,
    font: VALID_FONTS.includes(r.font as LyricsFont) ? (r.font as LyricsFont) : DEFAULT_LYRICS_DISPLAY.font,
    fontSize: VALID_SIZES.includes(r.fontSize as LyricsFontSize) ? (r.fontSize as LyricsFontSize) : DEFAULT_LYRICS_DISPLAY.fontSize,
    dimming: VALID_DIMMINGS.includes(r.dimming as LyricsDimming) ? (r.dimming as LyricsDimming) : DEFAULT_LYRICS_DISPLAY.dimming
  };
}

function loadInitial(): LyricsDisplayPrefs {
  try {
    const saved = getUserItem(STORAGE_KEY);
    if (!saved) return { ...DEFAULT_LYRICS_DISPLAY };
    return sanitize(JSON.parse(saved));
  } catch {
    return { ...DEFAULT_LYRICS_DISPLAY };
  }
}

export const lyricsDisplayStore = writable<LyricsDisplayPrefs>(loadInitial());

function persist(prefs: LyricsDisplayPrefs): void {
  setUserItem(STORAGE_KEY, JSON.stringify(prefs));
}

export function setLyricsAutoFollow(autoFollow: boolean): void {
  lyricsDisplayStore.update((p) => {
    const next = { ...p, autoFollow };
    persist(next);
    return next;
  });
}

export function setLyricsFont(font: LyricsFont): void {
  lyricsDisplayStore.update((p) => {
    const next = { ...p, font };
    persist(next);
    return next;
  });
}

export function setLyricsFontSize(fontSize: LyricsFontSize): void {
  lyricsDisplayStore.update((p) => {
    const next = { ...p, fontSize };
    persist(next);
    return next;
  });
}

export function setLyricsDimming(dimming: LyricsDimming): void {
  lyricsDisplayStore.update((p) => {
    const next = { ...p, dimming };
    persist(next);
    return next;
  });
}

export function resetLyricsDisplay(): void {
  const next = { ...DEFAULT_LYRICS_DISPLAY };
  persist(next);
  lyricsDisplayStore.set(next);
}

/**
 * Re-read preferences from user-scoped storage.
 *
 * The store is created at module-load time (before login), so the initial
 * read happens against the unscoped fallback key. After login, call this to
 * pick up the logged-in user's saved preferences. Matches the pattern used
 * by playerStore.resyncPersistedVolume.
 */
export function reloadLyricsDisplay(): void {
  lyricsDisplayStore.set(loadInitial());
}
