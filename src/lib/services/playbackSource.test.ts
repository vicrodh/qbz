import { describe, expect, it } from 'vitest';

import {
  isPlaybackSourceLocal,
  resolvePlaybackSource
} from './playbackSource';

describe('playbackSource', () => {
  it('treats qobuz_connect_remote as remote Qobuz playback', () => {
    const source = resolvePlaybackSource({
      source: 'qobuz_connect_remote',
      is_local: false
    });

    expect(source).toBe('qobuz');
    expect(isPlaybackSourceLocal(source, false)).toBe(false);
  });

  it('keeps downloaded Qobuz tracks on the local playback path', () => {
    const source = resolvePlaybackSource({
      source: 'qobuz_download',
      is_local: false
    });

    expect(source).toBe('local');
    expect(isPlaybackSourceLocal(source, false)).toBe(true);
  });

  it('falls back to local playback when the backend marks a track as local', () => {
    const source = resolvePlaybackSource({
      source: undefined,
      is_local: true
    });

    expect(source).toBe('local');
    expect(isPlaybackSourceLocal(source, false)).toBe(true);
  });
});
