import { describe, expect, it } from 'vitest';

import {
  evaluateQconnectPlaybackReportSkip,
  evaluateQconnectSessionPersistence,
  isQconnectPeerRendererActive,
  isQconnectRemoteModeActive,
  resolveQconnectPlayNextAuthoritativeTrackId,
  shouldQconnectSuppressLocalPlaybackAutomation,
  type QconnectConnectionStatus
} from './qconnectRuntime';
import type { QconnectQueueSnapshot, QconnectRendererSnapshot } from './qconnectRemoteQueue';

function buildQueueSnapshot(trackIds: number[]): QconnectQueueSnapshot {
  return {
    version: { major: 2, minor: 1 },
    queue_items: trackIds.map((trackId, index) => ({
      track_id: trackId,
      queue_item_id: index + 1,
      track_context_uuid: `ctx-${index + 1}`
    })),
    shuffle_mode: false,
    autoplay_mode: false,
    autoplay_items: []
  };
}

describe('evaluateQconnectPlaybackReportSkip', () => {
  it('does not skip when the current track exists in the remote queue', () => {
    const result = evaluateQconnectPlaybackReportSkip({
      currentTrackId: 23943863,
      queueSnapshot: buildQueueSnapshot([193849747, 23943863, 218534]),
      rendererSnapshot: null,
      lastSkipSignature: ''
    });

    expect(result).toEqual({
      shouldSkip: false,
      nextSkipSignature: '',
      diagnosticPayload: null
    });
  });

  it('skips and emits a diagnostic when the local track is outside the remote queue and renderer has no current track', () => {
    const result = evaluateQconnectPlaybackReportSkip({
      currentTrackId: 46848340,
      queueSnapshot: buildQueueSnapshot([193849747, 23943863, 218534]),
      rendererSnapshot: {
        current_track: null,
        next_track: { track_id: 23943863, queue_item_id: 2 }
      },
      lastSkipSignature: ''
    });

    expect(result.shouldSkip).toBe(true);
    expect(result.nextSkipSignature).toBe('46848340:2.1');
    expect(result.diagnosticPayload).toEqual({
      reason: 'local_track_not_in_remote_queue',
      current_track_id: 46848340,
      renderer_current_track_id: null,
      remote_queue_preview: buildQueueSnapshot([193849747, 23943863, 218534]).queue_items,
      renderer_current: null,
      renderer_next: { track_id: 23943863, queue_item_id: 2 }
    });
  });

  it('suppresses duplicate diagnostics for the same skip signature', () => {
    const queueSnapshot = buildQueueSnapshot([193849747, 23943863, 218534]);
    const rendererSnapshot: QconnectRendererSnapshot = {
      current_track: null,
      next_track: { track_id: 23943863, queue_item_id: 2 }
    };

    const result = evaluateQconnectPlaybackReportSkip({
      currentTrackId: 46848340,
      queueSnapshot,
      rendererSnapshot,
      lastSkipSignature: '46848340:2.1'
    });

    expect(result).toEqual({
      shouldSkip: true,
      nextSkipSignature: '46848340:2.1',
      diagnosticPayload: null
    });
  });

  it('skips when renderer current track is stale and does not match the local track', () => {
    const result = evaluateQconnectPlaybackReportSkip({
      currentTrackId: 46848340,
      queueSnapshot: buildQueueSnapshot([193849747, 23943863, 218534]),
      rendererSnapshot: {
        current_track: { track_id: 193849747, queue_item_id: 1 },
        next_track: { track_id: 23943863, queue_item_id: 2 }
      },
      lastSkipSignature: ''
    });

    expect(result.shouldSkip).toBe(true);
    expect(result.nextSkipSignature).toBe('46848340:2.1');
    expect(result.diagnosticPayload).toEqual({
      reason: 'local_track_not_in_remote_queue',
      current_track_id: 46848340,
      renderer_current_track_id: 193849747,
      remote_queue_preview: buildQueueSnapshot([193849747, 23943863, 218534]).queue_items,
      renderer_current: { track_id: 193849747, queue_item_id: 1 },
      renderer_next: { track_id: 23943863, queue_item_id: 2 }
    });
  });

  it('does not skip when renderer current track already matches the local track', () => {
    const result = evaluateQconnectPlaybackReportSkip({
      currentTrackId: 46848340,
      queueSnapshot: buildQueueSnapshot([193849747, 23943863, 218534]),
      rendererSnapshot: {
        current_track: { track_id: 46848340, queue_item_id: 999 },
        next_track: { track_id: 23943863, queue_item_id: 2 }
      },
      lastSkipSignature: ''
    });

    expect(result).toEqual({
      shouldSkip: false,
      nextSkipSignature: '',
      diagnosticPayload: null
    });
  });
});

describe('QConnect runtime state helpers', () => {
  it('treats transport connectivity as remote mode even if the local flag is stale', () => {
    const status: QconnectConnectionStatus = {
      running: true,
      transport_connected: true,
      endpoint_url: 'ws://127.0.0.1:12345',
      last_error: null
    };

    expect(isQconnectRemoteModeActive(false, status)).toBe(true);
  });

  it('always persists the local session regardless of remote mode', () => {
    // Issue #304: local session is saved even when Qobuz Connect is
    // active so track-level restore keeps working. Priority between
    // local vs QConnect state is resolved at restore time.
    expect(evaluateQconnectSessionPersistence(false, false)).toEqual({
      shouldPersist: true,
      nextSkipLogged: false,
      shouldLogSkip: false
    });
    expect(evaluateQconnectSessionPersistence(false, true)).toEqual({
      shouldPersist: true,
      nextSkipLogged: false,
      shouldLogSkip: false
    });
    expect(evaluateQconnectSessionPersistence(true, false)).toEqual({
      shouldPersist: true,
      nextSkipLogged: false,
      shouldLogSkip: false
    });
    expect(evaluateQconnectSessionPersistence(true, true)).toEqual({
      shouldPersist: true,
      nextSkipLogged: false,
      shouldLogSkip: false
    });
  });

  it('detects when the local renderer is still active', () => {
    expect(
      isQconnectPeerRendererActive({
        session_uuid: 'session-1',
        active_renderer_id: 11,
        local_renderer_id: 11,
        renderers: []
      })
    ).toBe(false);
  });

  it('detects when another peer renderer is active', () => {
    expect(
      isQconnectPeerRendererActive({
        session_uuid: 'session-1',
        active_renderer_id: 4,
        local_renderer_id: 11,
        renderers: []
      })
    ).toBe(true);
  });

  it('suppresses local playback automation only when a peer renderer is active', () => {
    expect(
      shouldQconnectSuppressLocalPlaybackAutomation(true, {
        session_uuid: 'session-1',
        active_renderer_id: 4,
        local_renderer_id: 11,
        renderers: []
      })
    ).toBe(true);

    expect(
      shouldQconnectSuppressLocalPlaybackAutomation(true, {
        session_uuid: 'session-1',
        active_renderer_id: 11,
        local_renderer_id: 11,
        renderers: []
      })
    ).toBe(false);

    expect(shouldQconnectSuppressLocalPlaybackAutomation(false, null)).toBe(false);
  });

  it('does not trust the local current track for play next when a peer renderer is active', () => {
    expect(
      resolveQconnectPlayNextAuthoritativeTrackId({
        sessionSnapshot: {
          session_uuid: 'session-1',
          active_renderer_id: 4,
          local_renderer_id: 11,
          renderers: []
        },
        localCurrentTrackId: 48425288
      })
    ).toBeNull();
  });

  it('keeps the local current track authoritative when the local renderer is active', () => {
    expect(
      resolveQconnectPlayNextAuthoritativeTrackId({
        sessionSnapshot: {
          session_uuid: 'session-1',
          active_renderer_id: 11,
          local_renderer_id: 11,
          renderers: []
        },
        localCurrentTrackId: 48425288
      })
    ).toBe(48425288);
  });

  it('does not trust the local current track when the session snapshot is unavailable', () => {
    expect(
      resolveQconnectPlayNextAuthoritativeTrackId({
        sessionSnapshot: null,
        localCurrentTrackId: 48425288
      })
    ).toBeNull();
  });
});
