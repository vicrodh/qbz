/**
 * Queue Store Unit Tests
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
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
  startQueueEventListener,
  stopQueueEventListener,
  toggleShuffle,
  toggleRepeat,
  clearQueue,
  type BackendQueueTrack
} from './queueStore';

const mockedInvoke = vi.mocked(invoke);
const mockedListen = vi.mocked(listen);

describe('queueStore', () => {
  beforeEach(() => {
    // Reset store state between tests
    stopQueueEventListener();
    reset();
    mockedInvoke.mockReset();
    mockedListen.mockReset();
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
          { id: 1, title: 'Song 1', artist: 'Artist', album: 'Album', duration_secs: 180, artwork_url: 'https://art.jpg' },
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
        artwork: 'https://art.jpg',
        title: 'Song 1',
        artist: 'Artist',
        duration: '3:00',
        available: true,
        parental_warning: false
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
    it('should request shuffle on without mutating local state optimistically', async () => {
      mockedInvoke.mockResolvedValueOnce(undefined);

      const result = await toggleShuffle();

      expect(result).toEqual({ success: true, enabled: true });
      expect(getIsShuffle()).toBe(false);
      expect(mockedInvoke).toHaveBeenCalledWith('v2_toggle_shuffle');
      expect(mockedInvoke).toHaveBeenCalledTimes(1);
    });

    it('should request shuffle off from the current authoritative state', async () => {
      mockedInvoke.mockResolvedValueOnce({
        current_track: null,
        current_index: null,
        upcoming: [],
        history: [],
        shuffle: true,
        repeat: 'Off',
        total_tracks: 0
      });
      await syncQueueState();

      mockedInvoke.mockResolvedValueOnce(undefined);
      const result = await toggleShuffle();

      expect(result).toEqual({ success: true, enabled: false });
      expect(getIsShuffle()).toBe(true);
    });

    it('should revert on error', async () => {
      mockedInvoke.mockRejectedValueOnce(new Error('Failed'));

      const result = await toggleShuffle();

      expect(result).toEqual({ success: false, enabled: false });
      expect(getIsShuffle()).toBe(false);
    });
  });

  describe('toggleRepeat', () => {
    it('should request the next repeat mode without mutating local state optimistically', async () => {
      mockedInvoke.mockResolvedValueOnce({
        current_track: null,
        current_index: null,
        upcoming: [],
        history: [],
        shuffle: false,
        repeat: 'Off',
        total_tracks: 0
      });
      mockedInvoke.mockResolvedValueOnce(undefined);

      // off -> all
      let result = await toggleRepeat();
      expect(result.mode).toBe('all');
      expect(getRepeatMode()).toBe('off');

      // all -> one
      const eventHandlers = new Map<string, (event: { payload: unknown }) => void>();
      mockedListen.mockImplementation(async (eventName, handler) => {
        eventHandlers.set(String(eventName), handler as (event: { payload: unknown }) => void);
        return () => {
          eventHandlers.delete(String(eventName));
        };
      });
      await startQueueEventListener();
      eventHandlers.get('queue:repeat-changed')?.({ payload: 'all' });
      await Promise.resolve();

      mockedInvoke.mockResolvedValueOnce({
        current_track: null,
        current_index: null,
        upcoming: [],
        history: [],
        shuffle: false,
        repeat: 'All',
        total_tracks: 0
      });
      mockedInvoke.mockResolvedValueOnce(undefined);
      result = await toggleRepeat();
      expect(result.mode).toBe('one');
      expect(getRepeatMode()).toBe('all');

      // one -> off
      eventHandlers.get('queue:repeat-changed')?.({ payload: 'one' });
      await Promise.resolve();

      mockedInvoke.mockResolvedValueOnce({
        current_track: null,
        current_index: null,
        upcoming: [],
        history: [],
        shuffle: false,
        repeat: 'One',
        total_tracks: 0
      });
      mockedInvoke.mockResolvedValueOnce(undefined);
      result = await toggleRepeat();
      expect(result.mode).toBe('off');
      expect(getRepeatMode()).toBe('one');
    });

    it('should allow rapid consecutive repeat toggles before remote confirmation arrives', async () => {
      mockedInvoke.mockImplementation(async (command, payload) => {
        if (command === 'v2_get_queue_state') {
          return {
            current_track: null,
            current_index: null,
            upcoming: [],
            history: [],
            shuffle: false,
            repeat: 'Off',
            total_tracks: 0
          };
        }

        if (command === 'v2_set_repeat_mode') {
          return undefined;
        }

        throw new Error(`Unexpected invoke: ${String(command)} ${JSON.stringify(payload)}`);
      });

      await syncQueueState();
      mockedInvoke.mockClear();

      const first = await toggleRepeat();
      const second = await toggleRepeat();

      expect(first).toEqual({ success: true, mode: 'all' });
      expect(second).toEqual({ success: true, mode: 'one' });
      expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'v2_set_repeat_mode', { mode: 'All' });
      expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'v2_set_repeat_mode', { mode: 'One' });
      expect(mockedInvoke).toHaveBeenCalledTimes(2);
    });

    it('should not change mode on error', async () => {
      mockedInvoke.mockRejectedValue(new Error('Failed'));

      const result = await toggleRepeat();

      expect(result).toEqual({ success: false, mode: 'off' });
      expect(getRepeatMode()).toBe('off');
    });

    it('should derive the next repeat mode from authoritative backend state when local state is stale', async () => {
      mockedInvoke.mockImplementation(async (command) => {
        if (command === 'v2_get_queue_state') {
          return {
            current_track: null,
            current_index: null,
            upcoming: [],
            history: [],
            shuffle: false,
            repeat: 'One',
            total_tracks: 0
          };
        }

        if (command === 'v2_set_repeat_mode') {
          return undefined;
        }

        throw new Error(`Unexpected invoke: ${String(command)}`);
      });

      const result = await toggleRepeat();

      expect(result).toEqual({ success: true, mode: 'off' });
      expect(getRepeatMode()).toBe('one');
      expect(mockedInvoke).toHaveBeenNthCalledWith(1, 'v2_get_queue_state');
      expect(mockedInvoke).toHaveBeenNthCalledWith(2, 'v2_set_repeat_mode', { mode: 'Off' });
    });
  });

  describe('clearQueue', () => {
    it('should request clear without mutating local queue optimistically', async () => {
      mockedInvoke.mockResolvedValueOnce({
        current_track: null,
        current_index: null,
        upcoming: [
          { id: 1, title: 'Song 1', artist: 'Artist', album: 'Album', duration_secs: 180, artwork_url: null }
        ],
        history: [],
        shuffle: false,
        repeat: 'Off',
        total_tracks: 1
      });
      await syncQueueState();

      mockedInvoke.mockResolvedValueOnce(undefined);

      const success = await clearQueue();

      expect(success).toBe(true);
      expect(mockedInvoke).toHaveBeenLastCalledWith('v2_clear_queue');
      expect(getQueue().map(track => track.id)).toEqual(['1']);
      expect(getQueueTotalTracks()).toBe(1);
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

  describe('queue events', () => {
    it('should apply authoritative queue order from queue:updated', async () => {
      const eventHandlers = new Map<string, (event: { payload: unknown }) => void>();
      mockedListen.mockImplementation(async (eventName, handler) => {
        eventHandlers.set(String(eventName), handler as (event: { payload: unknown }) => void);
        return () => {
          eventHandlers.delete(String(eventName));
        };
      });

      await startQueueEventListener();

      const queueUpdated = eventHandlers.get('queue:updated');
      expect(queueUpdated).toBeDefined();

      queueUpdated?.({
        payload: {
          current_track: null,
          current_index: 0,
          upcoming: [
            {
              id: 22,
              title: 'Remote First',
              artist: 'Artist',
              album: 'Album',
              duration_secs: 180,
              artwork_url: null
            },
            {
              id: 16,
              title: 'Remote Second',
              artist: 'Artist',
              album: 'Album',
              duration_secs: 200,
              artwork_url: null
            }
          ],
          history: [],
          shuffle: true,
          repeat: 'All',
          total_tracks: 36
        }
      });

      await Promise.resolve();

      expect(getQueue().map(track => track.id)).toEqual(['22', '16']);
      expect(getQueue().map(track => track.title)).toEqual(['Remote First', 'Remote Second']);
      expect(getQueueTotalTracks()).toBe(36);
      expect(getIsShuffle()).toBe(true);
      expect(getRepeatMode()).toBe('all');
    });

    it('should ignore queue:shuffle-changed until queue:updated arrives', async () => {
      const eventHandlers = new Map<string, (event: { payload: unknown }) => void>();
      mockedListen.mockImplementation(async (eventName, handler) => {
        eventHandlers.set(String(eventName), handler as (event: { payload: unknown }) => void);
        return () => {
          eventHandlers.delete(String(eventName));
        };
      });

      await startQueueEventListener();

      const shuffleChanged = eventHandlers.get('queue:shuffle-changed');
      expect(shuffleChanged).toBeDefined();

      shuffleChanged?.({ payload: true });
      await Promise.resolve();

      expect(mockedInvoke).not.toHaveBeenCalledWith('v2_get_queue_state');
      expect(getIsShuffle()).toBe(false);
      expect(getQueue()).toEqual([]);
    });

    it('should apply repeat mode from queue:repeat-changed', async () => {
      const eventHandlers = new Map<string, (event: { payload: unknown }) => void>();
      mockedListen.mockImplementation(async (eventName, handler) => {
        eventHandlers.set(String(eventName), handler as (event: { payload: unknown }) => void);
        return () => {
          eventHandlers.delete(String(eventName));
        };
      });

      await startQueueEventListener();

      const repeatChanged = eventHandlers.get('queue:repeat-changed');
      expect(repeatChanged).toBeDefined();

      repeatChanged?.({ payload: 'one' });
      await Promise.resolve();

      expect(getRepeatMode()).toBe('one');
    });
  });
});
