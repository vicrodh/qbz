export type QconnectQueueItemSnapshot = {
  track_context_uuid?: string | null;
  track_id: number;
  queue_item_id: number;
};

export type QconnectQueueSnapshot = {
  version?: { major: number; minor: number };
  queue_items: QconnectQueueItemSnapshot[];
  shuffle_mode: boolean;
  shuffle_order?: number[] | null;
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

function isNonNegativeNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

function findQueueIndexByQueueItemId(
  queueItems: QconnectQueueItemSnapshot[],
  queueItemId: number | null | undefined
): number | null {
  if (!isNonNegativeNumber(queueItemId)) return null;
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

function isCloudPlaceholderCurrentQueueItem(
  queueItems: QconnectQueueItemSnapshot[],
  queueIndex: number | null
): queueIndex is number {
  if (queueIndex !== 0) return false;

  const currentItem = queueItems[queueIndex];
  if (!currentItem) return false;
  if (currentItem.queue_item_id !== currentItem.track_id) return false;

  return queueItems.slice(1).some((item) => isNonNegativeNumber(item.queue_item_id) && item.queue_item_id < currentItem.queue_item_id);
}

function resolvedQueueItemId(
  queueItems: QconnectQueueItemSnapshot[],
  queueIndex: number
): number {
  return isCloudPlaceholderCurrentQueueItem(queueItems, queueIndex)
    ? 0
    : queueItems[queueIndex].queue_item_id;
}

export function resolveQconnectQueueDisplayItems(
  queueSnapshot: QconnectQueueSnapshot | null | undefined
): QconnectQueueItemSnapshot[] {
  const queueItems = queueSnapshot?.queue_items ?? [];
  const shuffleOrder = queueSnapshot?.shuffle_order ?? null;

  if (!queueSnapshot?.shuffle_mode || !Array.isArray(shuffleOrder) || shuffleOrder.length === 0) {
    return queueItems;
  }

  const ordered = shuffleOrder
    .map((queueIndex) => (
      Number.isInteger(queueIndex) && queueIndex >= 0 ? queueItems[queueIndex] ?? null : null
    ))
    .filter((queueItem): queueItem is QconnectQueueItemSnapshot => queueItem !== null);

  return ordered.length === queueItems.length ? ordered : queueItems;
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
    const queueItemId = resolvedQueueItemId(queueItems, authoritativeTrackIndexBeforeNext);
    return {
      insertAfter: queueItemId,
      strategy: 'authoritative_track_id_before_renderer_next',
      queueIndex: authoritativeTrackIndexBeforeNext,
      nextQueueIndex,
      matchedTrackId: queueItems[authoritativeTrackIndexBeforeNext].track_id,
      matchedQueueItemId: queueItemId
    };
  }

  if (authoritativeTrackDisagreesWithRenderer && authoritativeCurrentTrackIndex !== null) {
    const queueItemId = resolvedQueueItemId(queueItems, authoritativeCurrentTrackIndex);
    return {
      insertAfter: queueItemId,
      strategy: 'authoritative_track_id_match',
      queueIndex: authoritativeCurrentTrackIndex,
      nextQueueIndex,
      matchedTrackId: queueItems[authoritativeCurrentTrackIndex].track_id,
      matchedQueueItemId: queueItemId
    };
  }

  if (currentQueueIndex !== null) {
    const queueItemId = resolvedQueueItemId(queueItems, currentQueueIndex);
    return {
      insertAfter: queueItemId,
      strategy: 'renderer_current_queue_item_id_verified',
      queueIndex: currentQueueIndex,
      nextQueueIndex,
      matchedTrackId: queueItems[currentQueueIndex].track_id,
      matchedQueueItemId: queueItemId
    };
  }

  const trackIndexBeforeNext = nextQueueIndex !== null
    ? findTrackIndexBefore(queueItems, currentTrack?.track_id, nextQueueIndex)
    : null;
  if (trackIndexBeforeNext !== null) {
    const queueItemId = resolvedQueueItemId(queueItems, trackIndexBeforeNext);
    return {
      insertAfter: queueItemId,
      strategy: 'queue_track_id_before_renderer_next',
      queueIndex: trackIndexBeforeNext,
      nextQueueIndex,
      matchedTrackId: queueItems[trackIndexBeforeNext].track_id,
      matchedQueueItemId: queueItemId
    };
  }

  const currentTrackIndex = findTrackIndex(queueItems, currentTrack?.track_id);
  if (currentTrackIndex !== null) {
    const queueItemId = resolvedQueueItemId(queueItems, currentTrackIndex);
    return {
      insertAfter: queueItemId,
      strategy: 'queue_track_id_match',
      queueIndex: currentTrackIndex,
      nextQueueIndex,
      matchedTrackId: queueItems[currentTrackIndex].track_id,
      matchedQueueItemId: queueItemId
    };
  }

  if (nextQueueIndex !== null && nextQueueIndex > 0) {
    const fallbackIndex = nextQueueIndex - 1;
    const queueItemId = resolvedQueueItemId(queueItems, fallbackIndex);
    return {
      insertAfter: queueItemId,
      strategy: 'queue_item_before_renderer_next',
      queueIndex: fallbackIndex,
      nextQueueIndex,
      matchedTrackId: queueItems[fallbackIndex].track_id,
      matchedQueueItemId: queueItemId
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
