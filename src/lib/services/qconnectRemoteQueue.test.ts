import { describe, expect, it } from 'vitest';

import {
  resolveQconnectPlayNextInsertAfter,
  type QconnectQueueSnapshot,
  type QconnectRendererSnapshot
} from './qconnectRemoteQueue';

function buildQueueSnapshot(trackIds: number[], queueItemIds: number[]): QconnectQueueSnapshot {
  return {
    version: { major: 1, minor: 0 },
    queue_items: trackIds.map((trackId, index) => ({
      track_id: trackId,
      queue_item_id: queueItemIds[index],
      track_context_uuid: `ctx-${index + 1}`
    })),
    shuffle_mode: false,
    autoplay_mode: false,
    autoplay_items: []
  };
}

describe('resolveQconnectPlayNextInsertAfter', () => {
  it('prefers a renderer queue_item_id that exists in the queue snapshot', () => {
    const queueSnapshot = buildQueueSnapshot([101, 102, 103], [9001, 9002, 9003]);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: { track_id: 102, queue_item_id: 9002 },
      next_track: { track_id: 103, queue_item_id: 9003 }
    };

    expect(resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot)).toEqual({
      insertAfter: 9002,
      strategy: 'renderer_current_queue_item_id_verified',
      queueIndex: 1,
      nextQueueIndex: 2,
      matchedTrackId: 102,
      matchedQueueItemId: 9002
    });
  });

  it('accepts queue_item_id zero as a valid remote anchor', () => {
    const queueSnapshot = buildQueueSnapshot([126886862, 25584418, 25120807], [0, 1, 2]);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: { track_id: 126886862, queue_item_id: 0 },
      next_track: { track_id: 25584418, queue_item_id: 1 }
    };

    expect(resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot)).toEqual({
      insertAfter: 0,
      strategy: 'renderer_current_queue_item_id_verified',
      queueIndex: 0,
      nextQueueIndex: 1,
      matchedTrackId: 126886862,
      matchedQueueItemId: 0
    });
  });

  it('normalizes the placeholder first queue item id to zero for play next', () => {
    const queueSnapshot = buildQueueSnapshot([126886853, 123452387, 126886854], [126886853, 10, 1]);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: { track_id: 126886853, queue_item_id: 126886853 },
      next_track: { track_id: 126886854, queue_item_id: 1 }
    };

    expect(resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot)).toEqual({
      insertAfter: 0,
      strategy: 'renderer_current_queue_item_id_verified',
      queueIndex: 0,
      nextQueueIndex: 2,
      matchedTrackId: 126886853,
      matchedQueueItemId: 0
    });
  });

  it('prefers the authoritative current track when renderer current is stale but still present in queue', () => {
    const queueSnapshot = buildQueueSnapshot([123452387, 123452388, 123452389], [123452387, 1, 2]);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: { track_id: 123452387, queue_item_id: 123452387 },
      next_track: { track_id: 123452388, queue_item_id: 1 }
    };

    expect(
      resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot, {
        authoritativeCurrentTrackId: 123452388
      })
    ).toEqual({
      insertAfter: 1,
      strategy: 'authoritative_track_id_match',
      queueIndex: 1,
      nextQueueIndex: 1,
      matchedTrackId: 123452388,
      matchedQueueItemId: 1
    });
  });

  it('falls back to queue track matching when renderer queue_item_id is stale', () => {
    const queueSnapshot = buildQueueSnapshot([201, 202, 203], [7001, 7002, 7003]);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: { track_id: 202, queue_item_id: 999999 },
      next_track: { track_id: 203, queue_item_id: 7003 }
    };

    expect(resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot)).toEqual({
      insertAfter: 7002,
      strategy: 'queue_track_id_before_renderer_next',
      queueIndex: 1,
      nextQueueIndex: 2,
      matchedTrackId: 202,
      matchedQueueItemId: 7002
    });
  });

  it('handles duplicated track_ids by picking the match immediately before renderer next', () => {
    const queueSnapshot = buildQueueSnapshot([301, 302, 301, 303], [8101, 8102, 8103, 8104]);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: { track_id: 301, queue_item_id: 999999 },
      next_track: { track_id: 303, queue_item_id: 8104 }
    };

    expect(resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot)).toEqual({
      insertAfter: 8103,
      strategy: 'queue_track_id_before_renderer_next',
      queueIndex: 2,
      nextQueueIndex: 3,
      matchedTrackId: 301,
      matchedQueueItemId: 8103
    });
  });

  it('uses the queue item before renderer next when current track cannot be matched safely', () => {
    const queueSnapshot = buildQueueSnapshot([401, 402, 403], [9101, 9102, 9103]);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: { track_id: 999, queue_item_id: 8888 },
      next_track: { track_id: 403, queue_item_id: 9103 }
    };

    expect(resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot)).toEqual({
      insertAfter: 9102,
      strategy: 'queue_item_before_renderer_next',
      queueIndex: 1,
      nextQueueIndex: 2,
      matchedTrackId: 402,
      matchedQueueItemId: 9102
    });
  });

  it('returns no anchor when the queue snapshot is empty', () => {
    const queueSnapshot = buildQueueSnapshot([], []);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: { track_id: 501, queue_item_id: 9501 }
    };

    expect(resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot)).toEqual({
      insertAfter: null,
      strategy: 'no_queue_items',
      queueIndex: null,
      nextQueueIndex: null,
      matchedTrackId: null,
      matchedQueueItemId: null
    });
  });
});
