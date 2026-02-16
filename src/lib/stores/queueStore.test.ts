/**
 * Queue Store Unit Tests
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import {
  subscribe,
  getQueue,
  getQueueTotalTracks,
  getIsShuffle,
  getRepeatMode,
  getQueueState,
  isLocalTrack,
  setLocalTrackIds,
  clearLocalTrackIds,
  reset,
  syncQueueState,
  toggleShuffle,
  toggleRepeat,
  type BackendQueueTrack
} from './queueStore';

const mockedInvoke = vi.mocked(invoke);

describe('queueStore', () => {
  beforeEach(() => {
    // Reset store state between tests
    reset();
    vi.clearAllMocks();
  });

  describe('initial state', () => {
    it('should have empty queue', () => {
      expect(getQueue()).toEqual([]);
    });

    it('should have zero total tracks', () => {
      expect(getQueueTotalTracks()).toBe(0);
    });

    it('should have shuffle off', () => {
      expect(getIsShuffle()).toBe(false);
    });

    it('should have repeat mode off', () => {
      expect(getRepeatMode()).toBe('off');
    });
  });

  describe('subscribe', () => {
    it('should notify listener immediately on subscribe', () => {
      const listener = vi.fn();
      subscribe(listener);
      expect(listener).toHaveBeenCalledTimes(1);
    });

    it('should return unsubscribe function', () => {
      const listener = vi.fn();
      const unsubscribe = subscribe(listener);
      expect(typeof unsubscribe).toBe('function');
    });

    it('should not notify after unsubscribe', () => {
      const listener = vi.fn();
      const unsubscribe = subscribe(listener);
      listener.mockClear();
      unsubscribe();
      reset(); // This normally triggers notifications
      expect(listener).not.toHaveBeenCalled();
    });
  });

  describe('getQueueState', () => {
    it('should return complete state object', () => {
      const state = getQueueState();
      expect(state).toEqual({
        queue: [],
        queueTotalTracks: 0,
        isShuffle: false,
        repeatMode: 'off'
      });
    });

    it('should return a copy of queue array', () => {
      const state1 = getQueueState();
      const state2 = getQueueState();
      expect(state1.queue).not.toBe(state2.queue);
    });
  });

  describe('local track management', () => {
    it('should track local track IDs', () => {
      setLocalTrackIds([1, 2, 3]);
      expect(isLocalTrack(1)).toBe(true);
      expect(isLocalTrack(2)).toBe(true);
      expect(isLocalTrack(3)).toBe(true);
      expect(isLocalTrack(4)).toBe(false);
    });

    it('should clear local track IDs', () => {
      setLocalTrackIds([1, 2, 3]);
      clearLocalTrackIds();
      expect(isLocalTrack(1)).toBe(false);
    });

    it('should reset local tracks on full reset', () => {
      setLocalTrackIds([1, 2, 3]);
      reset();
      expect(isLocalTrack(1)).toBe(false);
    });
  });

  describe('syncQueueState', () => {
    it('should sync state from backend', async () => {
      mockedInvoke.mockResolvedValueOnce({
        current_track: null,
        current_index: null,
        upcoming: [
          { id: 1, title: 'Song 1', artist: 'Artist', album: 'Album', duration_secs: 180, artwork_url: 'http://art.jpg' },
          { id: 2, title: 'Song 2', artist: 'Artist', album: 'Album', duration_secs: 240, artwork_url: null }
        ],
        history: [],
        shuffle: true,
        repeat: 'All',
        total_tracks: 5
      });

      await syncQueueState();

      expect(getQueue()).toHaveLength(2);
      expect(getQueue()[0]).toEqual({
        id: '1',
        artwork: 'http://art.jpg',
        title: 'Song 1',
        artist: 'Artist',
        duration: '3:00',
        available: true
      });
      expect(getQueue()[1].duration).toBe('4:00');
      expect(getQueueTotalTracks()).toBe(5);
      expect(getIsShuffle()).toBe(true);
      expect(getRepeatMode()).toBe('all');
    });

    it('should handle empty artwork gracefully', async () => {
      mockedInvoke.mockResolvedValueOnce({
        current_track: null,
        current_index: null,
        upcoming: [
          { id: 1, title: 'Song', artist: 'Artist', album: 'Album', duration_secs: 60, artwork_url: null }
        ],
        history: [],
        shuffle: false,
        repeat: 'Off',
        total_tracks: 1
      });

      await syncQueueState();

      expect(getQueue()[0].artwork).toBe('');
    });

    it('should handle sync errors gracefully', async () => {
      mockedInvoke.mockRejectedValueOnce(new Error('Backend error'));

      // Should not throw
      await syncQueueState();

      // State remains unchanged
      expect(getQueue()).toEqual([]);
    });
  });

  describe('toggleShuffle', () => {
    it('should toggle shuffle on', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);

      const result = await toggleShuffle();

      expect(result).toEqual({ success: true, enabled: true });
      expect(getIsShuffle()).toBe(true);
      expect(mockedInvoke).toHaveBeenCalledWith('v2_toggle_shuffle');
    });

    it('should toggle shuffle off', async () => {
      // First toggle on
      mockedInvoke.mockResolvedValueOnce(undefined);
      await toggleShuffle();

      // Then toggle off
      mockedInvoke.mockResolvedValueOnce(undefined);
      const result = await toggleShuffle();

      expect(result).toEqual({ success: true, enabled: false });
      expect(getIsShuffle()).toBe(false);
    });

    it('should revert on error', async () => {
      mockedInvoke.mockRejectedValueOnce(new Error('Failed'));

      const result = await toggleShuffle();

      expect(result).toEqual({ success: false, enabled: false });
      expect(getIsShuffle()).toBe(false);
    });
  });

  describe('toggleRepeat', () => {
    it('should cycle through repeat modes', async () => {
      mockedInvoke.mockResolvedValue(undefined);

      // off -> all
      let result = await toggleRepeat();
      expect(result.mode).toBe('all');

      // all -> one
      result = await toggleRepeat();
      expect(result.mode).toBe('one');

      // one -> off
      result = await toggleRepeat();
      expect(result.mode).toBe('off');
    });

    it('should not change mode on error', async () => {
      mockedInvoke.mockRejectedValueOnce(new Error('Failed'));

      const result = await toggleRepeat();

      expect(result).toEqual({ success: false, mode: 'off' });
      expect(getRepeatMode()).toBe('off');
    });
  });

  describe('reset', () => {
    it('should reset all state', async () => {
      // Set up some state
      mockedInvoke.mockResolvedValueOnce({
        current_track: null,
        current_index: null,
        upcoming: [{ id: 1, title: 'Song', artist: 'A', album: 'B', duration_secs: 60, artwork_url: null }],
        history: [],
        shuffle: true,
        repeat: 'One',
        total_tracks: 1
      });
      await syncQueueState();
      setLocalTrackIds([1, 2, 3]);

      // Verify state was set
      expect(getQueue()).toHaveLength(1);
      expect(getIsShuffle()).toBe(true);

      // Reset
      reset();

      // Verify everything is cleared
      expect(getQueue()).toEqual([]);
      expect(getQueueTotalTracks()).toBe(0);
      expect(getIsShuffle()).toBe(false);
      expect(getRepeatMode()).toBe('off');
      expect(isLocalTrack(1)).toBe(false);
    });

    it('should notify listeners on reset', () => {
      const listener = vi.fn();
      subscribe(listener);
      listener.mockClear();

      reset();

      expect(listener).toHaveBeenCalledTimes(1);
    });
  });
});
