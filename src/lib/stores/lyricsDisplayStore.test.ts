import { describe, it, expect, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';

// Mock userStorage before importing the store under test
vi.mock('$lib/utils/userStorage', () => {
  const mem = new Map<string, string>();
  return {
    getUserItem: (k: string) => (mem.has(k) ? (mem.get(k) as string) : null),
    setUserItem: (k: string, v: string) => {
      mem.set(k, v);
    },
    removeUserItem: (k: string) => {
      mem.delete(k);
    },
    __reset: () => mem.clear()
  };
});

import * as userStorage from '$lib/utils/userStorage';
import {
  lyricsDisplayStore,
  setLyricsAutoFollow,
  setLyricsFont,
  setLyricsFontSize,
  setLyricsDimming,
  resetLyricsDisplay,
  DEFAULT_LYRICS_DISPLAY,
  STORAGE_KEY
} from './lyricsDisplayStore';

beforeEach(() => {
  (userStorage as unknown as { __reset: () => void }).__reset();
  resetLyricsDisplay();
});

describe('lyricsDisplayStore', () => {
  it('has the expected defaults', () => {
    expect(DEFAULT_LYRICS_DISPLAY).toEqual({
      autoFollow: true,
      font: 'system',
      fontSize: 'medium',
      dimming: 'strong'
    });
  });

  it('uses the hyphenated storage key', () => {
    expect(STORAGE_KEY).toBe('qbz-lyrics-display');
  });

  it('setters update the store and persist', () => {
    setLyricsAutoFollow(false);
    setLyricsFont('montserrat');
    setLyricsFontSize('large');
    setLyricsDimming('soft');

    expect(get(lyricsDisplayStore)).toEqual({
      autoFollow: false,
      font: 'montserrat',
      fontSize: 'large',
      dimming: 'soft'
    });

    const persisted = userStorage.getUserItem(STORAGE_KEY);
    expect(persisted).not.toBeNull();
    expect(JSON.parse(persisted as string)).toEqual(get(lyricsDisplayStore));
  });

  it('resetLyricsDisplay returns to defaults and persists', () => {
    setLyricsFontSize('large');
    setLyricsDimming('off');
    resetLyricsDisplay();

    expect(get(lyricsDisplayStore)).toEqual(DEFAULT_LYRICS_DISPLAY);
    expect(JSON.parse(userStorage.getUserItem(STORAGE_KEY) as string)).toEqual(
      DEFAULT_LYRICS_DISPLAY
    );
  });

  it('reset persists defaults even when called repeatedly', () => {
    setLyricsDimming('off');
    resetLyricsDisplay();
    resetLyricsDisplay();
    expect(get(lyricsDisplayStore)).toEqual(DEFAULT_LYRICS_DISPLAY);
    expect(JSON.parse(userStorage.getUserItem(STORAGE_KEY) as string)).toEqual(
      DEFAULT_LYRICS_DISPLAY
    );
  });
});
