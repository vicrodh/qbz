/**
 * Player Store Unit Tests
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  subscribe,
  getCurrentTrack,
  getIsPlaying,
  getCurrentTime,
  getDuration,
  getVolume,
  getIsFavorite,
  getIsSkipping,
  getPlayerState,
  setCurrentTrack,
  setIsFavorite,
  setIsSkipping,
  setIsPlaying,
  setQueueEnded,
  togglePlay,
  seek,
  setVolume,
  stop,
  startPolling,
  stopPolling,
  isPollingActive,
  reset,
  type PlayingTrack
} from './playerStore';

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

const mockTrack: PlayingTrack = {
  id: 123,
  title: 'Test Song',
  artist: 'Test Artist',
  album: 'Test Album',
  artwork: 'http://art.jpg',
  duration: 180,
  quality: 'CD Quality',
  bitDepth: 16,
  samplingRate: 44100
};

describe('playerStore', () => {
  beforeEach(() => {
    reset();
    vi.clearAllMocks();
  });

  describe('initial state', () => {
    it('should have no current track', () => {
      expect(getCurrentTrack()).toBeNull();
    });

    it('should not be playing', () => {
      expect(getIsPlaying()).toBe(false);
    });

    it('should have zero current time', () => {
      expect(getCurrentTime()).toBe(0);
    });

    it('should have zero duration', () => {
      expect(getDuration()).toBe(0);
    });

    it('should have default volume of 75', () => {
      expect(getVolume()).toBe(75);
    });

    it('should not be favorite', () => {
      expect(getIsFavorite()).toBe(false);
    });

    it('should not be skipping', () => {
      expect(getIsSkipping()).toBe(false);
    });
  });

  describe('subscribe', () => {
    it('should notify listener immediately', () => {
      const listener = vi.fn();
      subscribe(listener);
      expect(listener).toHaveBeenCalledTimes(1);
    });

    it('should return unsubscribe function', () => {
      const listener = vi.fn();
      const unsubscribe = subscribe(listener);
      expect(typeof unsubscribe).toBe('function');
    });

    it('should stop notifying after unsubscribe', () => {
      const listener = vi.fn();
      const unsubscribe = subscribe(listener);
      listener.mockClear();
      unsubscribe();
      setIsPlaying(true);
      expect(listener).not.toHaveBeenCalled();
    });
  });

  describe('getPlayerState', () => {
    it('should return complete state', () => {
      setCurrentTrack(mockTrack);
      setIsPlaying(true);
      setIsFavorite(true);

      const state = getPlayerState();

      expect(state).toEqual({
        currentTrack: mockTrack,
        isPlaying: true,
        currentTime: 0,
        duration: 180,
        volume: 75,
        isFavorite: true,
        isSkipping: false
      });
    });
  });

  describe('setCurrentTrack', () => {
    it('should set track and update duration', () => {
      setCurrentTrack(mockTrack);

      expect(getCurrentTrack()).toEqual(mockTrack);
      expect(getDuration()).toBe(180);
      expect(getCurrentTime()).toBe(0);
    });

    it('should reset times when clearing track', () => {
      setCurrentTrack(mockTrack);
      setCurrentTrack(null);

      expect(getCurrentTrack()).toBeNull();
      expect(getDuration()).toBe(0);
      expect(getCurrentTime()).toBe(0);
    });

    it('should notify listeners', () => {
      const listener = vi.fn();
      subscribe(listener);
      listener.mockClear();

      setCurrentTrack(mockTrack);

      expect(listener).toHaveBeenCalledTimes(1);
    });
  });

  describe('setIsFavorite', () => {
    it('should update favorite status', () => {
      setIsFavorite(true);
      expect(getIsFavorite()).toBe(true);

      setIsFavorite(false);
      expect(getIsFavorite()).toBe(false);
    });

    it('should notify listeners', () => {
      const listener = vi.fn();
      subscribe(listener);
      listener.mockClear();

      setIsFavorite(true);

      expect(listener).toHaveBeenCalledTimes(1);
    });
  });

  describe('setIsSkipping', () => {
    it('should update skipping status', () => {
      setIsSkipping(true);
      expect(getIsSkipping()).toBe(true);
    });
  });

  describe('setIsPlaying', () => {
    it('should update playing status', () => {
      setIsPlaying(true);
      expect(getIsPlaying()).toBe(true);
    });
  });

  describe('togglePlay', () => {
    it('should do nothing without current track', async () => {
      await togglePlay();
      expect(mockedInvoke).not.toHaveBeenCalled();
    });

    it('should resume playback when paused', async () => {
      setCurrentTrack(mockTrack);
      mockedInvoke.mockResolvedValueOnce(undefined);

      await togglePlay();

      expect(getIsPlaying()).toBe(true);
      expect(mockedInvoke).toHaveBeenCalledWith('v2_resume_playback');
    });

    it('should pause playback when playing', async () => {
      setCurrentTrack(mockTrack);
      setIsPlaying(true);
      mockedInvoke.mockResolvedValueOnce(undefined);

      await togglePlay();

      expect(getIsPlaying()).toBe(false);
      expect(mockedInvoke).toHaveBeenCalledWith('v2_pause_playback');
    });

    it('should revert on error', async () => {
      setCurrentTrack(mockTrack);
      mockedInvoke.mockRejectedValueOnce(new Error('Failed'));

      await togglePlay();

      // Should revert to false after failure
      expect(getIsPlaying()).toBe(false);
    });
  });

  describe('v2_seek', () => {
    it('should clamp position to valid range', async () => {
      setCurrentTrack(mockTrack); // duration = 180
      mockedInvoke.mockResolvedValue(undefined);

      // Test negative
      await seek(-10);
      expect(getCurrentTime()).toBe(0);

      // Test beyond duration
      await seek(200);
      expect(getCurrentTime()).toBe(180);

      // Test valid position
      await seek(90);
      expect(getCurrentTime()).toBe(90);
    });

    it('should invoke backend with floored position', async () => {
      setCurrentTrack(mockTrack);
      mockedInvoke.mockResolvedValueOnce(undefined);

      await seek(45.7);

      expect(mockedInvoke).toHaveBeenCalledWith('v2_seek', { position: 45 });
    });
  });

  describe('setVolume', () => {
    it('should clamp volume to 0-100', async () => {
      mockedInvoke.mockResolvedValue(undefined);

      await setVolume(-10);
      expect(getVolume()).toBe(0);

      await setVolume(150);
      expect(getVolume()).toBe(100);

      await setVolume(50);
      expect(getVolume()).toBe(50);
    });

    it('should invoke backend with normalized volume', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);

      await setVolume(80);

      expect(mockedInvoke).toHaveBeenCalledWith('v2_set_volume', { volume: 0.8 });
    });
  });

  describe('stop', () => {
    it('should stop playback and clear state', async () => {
      setCurrentTrack(mockTrack);
      setIsPlaying(true);
      mockedInvoke.mockResolvedValueOnce(undefined);

      await stop();

      expect(getIsPlaying()).toBe(false);
      expect(getCurrentTrack()).toBeNull();
      expect(getCurrentTime()).toBe(0);
      expect(getDuration()).toBe(0);
    });
  });

  describe('event listening', () => {
    it('should start listening for events', async () => {
      const mockUnlisten = vi.fn();
      mockedListen.mockResolvedValueOnce(mockUnlisten);

      await startPolling();

      expect(mockedListen).toHaveBeenCalledWith('playback:state', expect.any(Function));
      expect(isPollingActive()).toBe(true);
    });

    it('should not start if already listening', async () => {
      const mockUnlisten = vi.fn();
      mockedListen.mockResolvedValue(mockUnlisten);

      await startPolling();
      await startPolling();

      expect(mockedListen).toHaveBeenCalledTimes(1);
    });

    it('should stop listening', async () => {
      const mockUnlisten = vi.fn();
      mockedListen.mockResolvedValueOnce(mockUnlisten);

      await startPolling();
      stopPolling();

      expect(mockUnlisten).toHaveBeenCalled();
      expect(isPollingActive()).toBe(false);
    });
  });

  describe('reset', () => {
    it('should reset all state', async () => {
      setCurrentTrack(mockTrack);
      setIsPlaying(true);
      setIsFavorite(true);
      setIsSkipping(true);

      const mockUnlisten = vi.fn();
      mockedListen.mockResolvedValueOnce(mockUnlisten);
      await startPolling();

      reset();

      expect(getCurrentTrack()).toBeNull();
      expect(getIsPlaying()).toBe(false);
      expect(getCurrentTime()).toBe(0);
      expect(getDuration()).toBe(0);
      expect(getIsFavorite()).toBe(false);
      expect(getIsSkipping()).toBe(false);
      expect(isPollingActive()).toBe(false);
    });
  });
});
