export type QconnectQueueItemSnapshot = {
  track_context_uuid?: string | null;
  track_id: number;
  queue_item_id: number;
};

export type QconnectQueueSnapshot = {
  version?: { major: number; minor: number };
  queue_items: QconnectQueueItemSnapshot[];
  shuffle_mode: boolean;
  autoplay_mode: boolean;
  autoplay_items: QconnectQueueItemSnapshot[];
};

export type QconnectRendererTrackSnapshot = {
  track_id?: number | null;
  queue_item_id?: number | null;
};

export type QconnectRendererSnapshot = {
  active?: boolean | null;
  playing_state?: number | null;
  current_position_ms?: number | null;
  current_track?: QconnectRendererTrackSnapshot | null;
  next_track?: QconnectRendererTrackSnapshot | null;
  volume?: number | null;
  muted?: boolean | null;
  loop_mode?: number | null;
  shuffle_mode?: boolean | null;
  updated_at_ms?: number | null;
};

export type QconnectDiagnosticsPayload = {
  ts?: number;
  channel: string;
  level?: 'info' | 'warn' | 'error';
  payload: unknown;
};

export type QconnectRendererReportDebugPayload = {
  requested_current_queue_item_id: number | null;
  requested_next_queue_item_id: number | null;
  resolved_current_queue_item_id: number | null;
  resolved_next_queue_item_id: number | null;
  sent_current_queue_item_id: number | null;
  sent_next_queue_item_id: number | null;
  current_track_id: number | null;
  playing_state: number;
  current_position: number | null;
  duration: number | null;
  queue_version: {
    major: number;
    minor: number;
  };
  resolution_strategy: string;
};

export type QconnectPlayNextAnchorResolution = {
  insertAfter: number | null;
  strategy: string;
  queueIndex: number | null;
  nextQueueIndex: number | null;
  matchedTrackId: number | null;
  matchedQueueItemId: number | null;
};

type QconnectPlayNextResolutionOptions = {
  authoritativeCurrentTrackId?: number | null;
};

function isPositiveNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0;
}

function findQueueIndexByQueueItemId(
  queueItems: QconnectQueueItemSnapshot[],
  queueItemId: number | null | undefined
): number | null {
  if (!isPositiveNumber(queueItemId)) return null;
  const index = queueItems.findIndex((item) => item.queue_item_id === queueItemId);
  return index >= 0 ? index : null;
}

function findTrackIndexBefore(
  queueItems: QconnectQueueItemSnapshot[],
  trackId: number | null | undefined,
  endExclusive: number
): number | null {
  if (!isPositiveNumber(trackId) || endExclusive <= 0) return null;
  for (let index = endExclusive - 1; index >= 0; index -= 1) {
    if (queueItems[index]?.track_id === trackId) {
      return index;
    }
  }
  return null;
}

function findTrackIndex(
  queueItems: QconnectQueueItemSnapshot[],
  trackId: number | null | undefined
): number | null {
  if (!isPositiveNumber(trackId)) return null;
  const index = queueItems.findIndex((item) => item.track_id === trackId);
  return index >= 0 ? index : null;
}

export function resolveQconnectPlayNextInsertAfter(
  queueSnapshot: QconnectQueueSnapshot | null | undefined,
  rendererSnapshot: QconnectRendererSnapshot | null | undefined,
  options: QconnectPlayNextResolutionOptions = {}
): QconnectPlayNextAnchorResolution {
  const queueItems = queueSnapshot?.queue_items ?? [];
  const currentTrack = rendererSnapshot?.current_track ?? null;
  const nextTrack = rendererSnapshot?.next_track ?? null;
  const authoritativeCurrentTrackId = options.authoritativeCurrentTrackId;

  if (queueItems.length === 0) {
    return {
      insertAfter: null,
      strategy: 'no_queue_items',
      queueIndex: null,
      nextQueueIndex: null,
      matchedTrackId: null,
      matchedQueueItemId: null
    };
  }

  const currentQueueIndex = findQueueIndexByQueueItemId(queueItems, currentTrack?.queue_item_id);
  const nextQueueIndex = findQueueIndexByQueueItemId(queueItems, nextTrack?.queue_item_id);
  const authoritativeTrackIndexBeforeNext = nextQueueIndex !== null
    ? findTrackIndexBefore(queueItems, authoritativeCurrentTrackId, nextQueueIndex)
    : null;
  const authoritativeCurrentTrackIndex = findTrackIndex(queueItems, authoritativeCurrentTrackId);
  const authoritativeTrackDisagreesWithRenderer = isPositiveNumber(authoritativeCurrentTrackId)
    && authoritativeCurrentTrackId !== currentTrack?.track_id;

  if (authoritativeTrackDisagreesWithRenderer && authoritativeTrackIndexBeforeNext !== null) {
    return {
      insertAfter: queueItems[authoritativeTrackIndexBeforeNext].queue_item_id,
      strategy: 'authoritative_track_id_before_renderer_next',
      queueIndex: authoritativeTrackIndexBeforeNext,
      nextQueueIndex,
      matchedTrackId: queueItems[authoritativeTrackIndexBeforeNext].track_id,
      matchedQueueItemId: queueItems[authoritativeTrackIndexBeforeNext].queue_item_id
    };
  }

  if (authoritativeTrackDisagreesWithRenderer && authoritativeCurrentTrackIndex !== null) {
    return {
      insertAfter: queueItems[authoritativeCurrentTrackIndex].queue_item_id,
      strategy: 'authoritative_track_id_match',
      queueIndex: authoritativeCurrentTrackIndex,
      nextQueueIndex,
      matchedTrackId: queueItems[authoritativeCurrentTrackIndex].track_id,
      matchedQueueItemId: queueItems[authoritativeCurrentTrackIndex].queue_item_id
    };
  }

  if (currentQueueIndex !== null) {
    return {
      insertAfter: queueItems[currentQueueIndex].queue_item_id,
      strategy: 'renderer_current_queue_item_id_verified',
      queueIndex: currentQueueIndex,
      nextQueueIndex,
      matchedTrackId: queueItems[currentQueueIndex].track_id,
      matchedQueueItemId: queueItems[currentQueueIndex].queue_item_id
    };
  }

  const trackIndexBeforeNext = nextQueueIndex !== null
    ? findTrackIndexBefore(queueItems, currentTrack?.track_id, nextQueueIndex)
    : null;
  if (trackIndexBeforeNext !== null) {
    return {
      insertAfter: queueItems[trackIndexBeforeNext].queue_item_id,
      strategy: 'queue_track_id_before_renderer_next',
      queueIndex: trackIndexBeforeNext,
      nextQueueIndex,
      matchedTrackId: queueItems[trackIndexBeforeNext].track_id,
      matchedQueueItemId: queueItems[trackIndexBeforeNext].queue_item_id
    };
  }

  const currentTrackIndex = findTrackIndex(queueItems, currentTrack?.track_id);
  if (currentTrackIndex !== null) {
    return {
      insertAfter: queueItems[currentTrackIndex].queue_item_id,
      strategy: 'queue_track_id_match',
      queueIndex: currentTrackIndex,
      nextQueueIndex,
      matchedTrackId: queueItems[currentTrackIndex].track_id,
      matchedQueueItemId: queueItems[currentTrackIndex].queue_item_id
    };
  }

  if (nextQueueIndex !== null && nextQueueIndex > 0) {
    const fallbackIndex = nextQueueIndex - 1;
    return {
      insertAfter: queueItems[fallbackIndex].queue_item_id,
      strategy: 'queue_item_before_renderer_next',
      queueIndex: fallbackIndex,
      nextQueueIndex,
      matchedTrackId: queueItems[fallbackIndex].track_id,
      matchedQueueItemId: queueItems[fallbackIndex].queue_item_id
    };
  }

  return {
    insertAfter: null,
    strategy: 'no_safe_anchor',
    queueIndex: null,
    nextQueueIndex,
    matchedTrackId: null,
    matchedQueueItemId: null
  };
}
