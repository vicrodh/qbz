/**
 * Track Actions Service
 *
 * Centralizes common track actions that can be used by any component.
 * Reduces prop drilling by providing direct access to track operations.
 */

import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import { get } from 'svelte/store';
import {
  addToQueueNext,
  addToQueue,
  type BackendQueueTrack
} from '$lib/stores/queueStore';
import { getPlayerState } from '$lib/stores/playerStore';
import { addTrackToFavorites } from '$lib/services/playbackService';
import { openPlaylistModal } from '$lib/stores/uiStore';
import { showToast as storeShowToast, type ToastType } from '$lib/stores/toastStore';
import { t } from '$lib/i18n';
import type { QobuzTrack, Track, PlaylistTrack, LocalLibraryTrack, DisplayTrack } from '$lib/types';
import {
  qconnectAdmissionReasonKey,
  resolveQconnectPlayNextAuthoritativeTrackId
} from '$lib/services/qconnectRuntime';
import type {
  QconnectConnectionStatus,
  QconnectSessionSnapshot
} from '$lib/services/qconnectRuntime';
import {
  resolveQconnectPlayNextInsertAfter,
  type QconnectDiagnosticsPayload,
  type QconnectQueueSnapshot,
  type QconnectRendererSnapshot
} from '$lib/services/qconnectRemoteQueue';

// ============ Toast Integration ============

function showToast(message: string, type: ToastType): void {
  storeShowToast(message, type);
}

function translate(key: string): string {
  return get(t)(key);
}

type QconnectTrackOrigin =
  | 'qobuz_online'
  | 'qobuz_offline_cache'
  | 'local_library'
  | 'plex'
  | 'external_unknown';

type QconnectAdmissionResult = {
  accepted: boolean;
  reason: string;
  handoff_intent: 'continue_locally' | 'send_to_connect';
};

type QconnectQueueCommandType = 'queue_add_tracks' | 'queue_insert_tracks' | 'queue_load_tracks';
type QueueTrackActionOptions = {
  silent?: boolean;
};

const QCONNECT_DIAGNOSTIC_EVENT = 'qconnect:diagnostic';

function resolveQconnectTrackOrigin(queueTrack: BackendQueueTrack, isLocal: boolean): QconnectTrackOrigin {
  const source = (queueTrack.source ?? '').toLowerCase();
  if (source === 'plex') return 'plex';
  if (source === 'qobuz_download') return isLocal ? 'local_library' : 'qobuz_offline_cache';
  if (source === 'qobuz') return 'qobuz_online';
  if (source === 'local' || isLocal || queueTrack.is_local) return 'local_library';
  return 'external_unknown';
}

async function isQconnectConnected(): Promise<boolean> {
  try {
    const status = await invoke<QconnectConnectionStatus>('v2_qconnect_status');
    return Boolean(status.transport_connected);
  } catch (err) {
    console.warn('[QConnect] isQconnectConnected failed:', err);
    return false;
  }
}

async function evaluateQconnectAdmission(origin: QconnectTrackOrigin): Promise<QconnectAdmissionResult | null> {
  try {
    return await invoke<QconnectAdmissionResult>('v2_qconnect_evaluate_queue_admission', { origin });
  } catch (err) {
    console.error('[QConnect] evaluateQconnectAdmission failed:', err);
    return null;
  }
}

async function emitQconnectDiagnostic(
  channel: string,
  level: 'info' | 'warn' | 'error',
  payload: unknown
): Promise<void> {
  const eventPayload: QconnectDiagnosticsPayload = {
    ts: Date.now(),
    channel,
    level,
    payload
  };

  try {
    await emit(QCONNECT_DIAGNOSTIC_EVENT, eventPayload);
  } catch (err) {
    console.warn('[QConnect] diagnostic emit failed:', err);
  }
}

type QconnectPlayNextResolution = {
  insertAfter: number | null;
  strategy: string;
  queueSnapshot: QconnectQueueSnapshot | null;
  rendererSnapshot: QconnectRendererSnapshot | null;
  snapshotError: string | null;
};

async function resolveQconnectPlayNextInsertAfterFromSnapshots(): Promise<QconnectPlayNextResolution> {
  const [queueSnapshotResult, rendererSnapshotResult, sessionSnapshotResult] = await Promise.allSettled([
    invoke<QconnectQueueSnapshot>('v2_qconnect_queue_snapshot'),
    invoke<QconnectRendererSnapshot>('v2_qconnect_renderer_snapshot'),
    invoke<QconnectSessionSnapshot>('v2_qconnect_session_snapshot')
  ]);

  const queueSnapshot = queueSnapshotResult.status === 'fulfilled' ? queueSnapshotResult.value : null;
  const rendererSnapshot = rendererSnapshotResult.status === 'fulfilled' ? rendererSnapshotResult.value : null;
  const sessionSnapshot = sessionSnapshotResult.status === 'fulfilled' ? sessionSnapshotResult.value : null;
  const localCurrentTrackId = resolveQconnectPlayNextAuthoritativeTrackId({
    sessionSnapshot,
    localCurrentTrackId: getPlayerState().currentTrack?.id ?? null
  });
  const resolution = resolveQconnectPlayNextInsertAfter(queueSnapshot, rendererSnapshot, {
    authoritativeCurrentTrackId: localCurrentTrackId
  });

  const snapshotError = [
    queueSnapshotResult.status === 'rejected'
      ? `queue_snapshot=${String(queueSnapshotResult.reason)}`
      : null,
    rendererSnapshotResult.status === 'rejected'
      ? `renderer_snapshot=${String(rendererSnapshotResult.reason)}`
      : null,
    sessionSnapshotResult.status === 'rejected'
      ? `session_snapshot=${String(sessionSnapshotResult.reason)}`
      : null
  ].filter((value): value is string => value !== null).join('; ');

  if (resolution.insertAfter !== null) {
    return {
      insertAfter: resolution.insertAfter,
      strategy: resolution.strategy,
      queueSnapshot,
      rendererSnapshot,
      snapshotError: snapshotError || null
    };
  }

  const rendererQueueItemId = rendererSnapshot?.current_track?.queue_item_id;
  if (!queueSnapshot && typeof rendererQueueItemId === 'number') {
    return {
      insertAfter: rendererQueueItemId,
      strategy: 'renderer_current_queue_item_id_unverified_fallback',
      queueSnapshot,
      rendererSnapshot,
      snapshotError: snapshotError || null
    };
  }

  return {
    insertAfter: null,
    strategy: resolution.strategy,
    queueSnapshot,
    rendererSnapshot,
    snapshotError: snapshotError || null
  };
}

async function sendQconnectQueueCommandWithAdmission(
  commandType: QconnectQueueCommandType,
  origin: QconnectTrackOrigin,
  payload: Record<string, unknown>
): Promise<void> {
  await invoke('v2_qconnect_send_command_with_admission', {
    request: {
      command_type: commandType,
      origin,
      payload
    }
  });
}

// ============ Queue Builders ============

export function buildQueueTrackFromQobuz(track: QobuzTrack): BackendQueueTrack {
  const artwork = track.album?.image?.small || track.album?.image?.thumbnail || track.album?.image?.large || '';

  // Log if track is not streamable (for debugging unavailable tracks)
  if (track.streamable === false) {
    console.warn(`[Track] Non-streamable track: "${track.title}" by ${track.performer?.name} (ID: ${track.id})`);
  }

  return {
    id: track.id,
    title: track.title,
    artist: track.performer?.name || 'Unknown Artist',
    album: track.album?.title || '',
    duration_secs: track.duration,
    artwork_url: artwork || null,
    hires: track.hires_streamable ?? false,
    bit_depth: track.maximum_bit_depth ?? null,
    sample_rate: track.maximum_sampling_rate ?? null,
    is_local: false,
    album_id: track.album?.id || null,
    artist_id: track.performer?.id ?? null,
    streamable: track.streamable ?? true,
    source: 'qobuz',
    parental_warning: track.parental_warning ?? false
  };
}

export function buildQueueTrackFromAlbumTrack(
  track: Track,
  albumArtwork: string,
  albumArtist: string,
  albumTitle: string,
  albumId?: string,
  artistId?: number
): BackendQueueTrack {
  // Log if track is not streamable
  if (track.streamable === false) {
    console.warn(`[Track] Non-streamable track: "${track.title}" (ID: ${track.id})`);
  }

  return {
    id: track.id,
    title: track.title,
    artist: track.artist || albumArtist || 'Unknown Artist',
    album: albumTitle || '',
    duration_secs: track.durationSeconds,
    artwork_url: albumArtwork || null,
    hires: track.hires ?? false,
    bit_depth: track.bitDepth ?? null,
    sample_rate: track.samplingRate ?? null,
    is_local: false,
    album_id: track.albumId || albumId || null,
    artist_id: track.artistId ?? artistId ?? null,
    streamable: track.streamable ?? true,
    source: 'qobuz',
    parental_warning: track.parental_warning ?? false
  };
}

export function buildQueueTrackFromPlaylistTrack(track: PlaylistTrack): BackendQueueTrack {
  // Log if track is not streamable
  if (track.streamable === false) {
    console.warn(`[Track] Non-streamable playlist track: "${track.title}" (ID: ${track.id})`);
  }

  return {
    id: track.id,
    title: track.title,
    artist: track.artist || 'Unknown Artist',
    album: track.album || 'Playlist',
    duration_secs: track.durationSeconds,
    artwork_url: track.albumArt || null,
    hires: track.hires ?? false,
    bit_depth: track.bitDepth ?? null,
    sample_rate: track.samplingRate ?? null,
    is_local: false,
    album_id: track.albumId || null,
    artist_id: track.artistId ?? null,
    streamable: track.streamable ?? true,
    source: 'qobuz',
    parental_warning: track.parental_warning ?? false
  };
}

export function buildQueueTrackFromLocalTrack(track: LocalLibraryTrack): BackendQueueTrack {
  const artwork = track.artwork_path
    ? (/^https?:\/\//i.test(track.artwork_path) ? track.artwork_path : convertFileSrc(track.artwork_path))
    : null;
  // Local tracks are hi-res if bit_depth > 16 or sample_rate > 44100
  const isHires = Boolean((track.bit_depth && track.bit_depth > 16) || (track.sample_rate && track.sample_rate > 44100));
  const isPlexTrack = track.source === 'plex';
  const source = isPlexTrack
    ? 'plex'
    : track.source === 'qobuz_download'
      ? 'qobuz_download'
      : 'local';
  return {
    id: track.id,
    title: track.title,
    artist: track.artist,
    album: track.album,
    duration_secs: track.duration_secs,
    artwork_url: artwork,
    hires: isHires,
    bit_depth: track.bit_depth ?? null,
    sample_rate: track.sample_rate ?? null,
    is_local: !isPlexTrack,
    album_id: null,  // Local tracks don't have Qobuz album IDs
    artist_id: null,  // Local tracks don't have Qobuz artist IDs
    streamable: true,  // Local tracks are always playable
    source
  };
}

// ============ Queue Actions ============

export async function queueTrackNext(
  queueTrack: BackendQueueTrack,
  isLocal = false,
  options: QueueTrackActionOptions = {}
): Promise<boolean> {
  const silent = options.silent === true;
  const qconnectConnected = await isQconnectConnected();
  console.log('[QConnect/PlayNext] connected=%s track=%d source=%s isLocal=%s', qconnectConnected, queueTrack.id, queueTrack.source, isLocal);

  // When QConnect is active, try to send to the remote queue.
  // If the track type is not eligible for remote (e.g. local library),
  // fall through to the local queue instead.
  let useLocalFallback = false;

  if (qconnectConnected) {
    const origin = resolveQconnectTrackOrigin(queueTrack, isLocal);
    console.log('[QConnect/PlayNext] origin=%s', origin);
    const admission = await evaluateQconnectAdmission(origin);
    console.log('[QConnect/PlayNext] admission=%o', admission);

    if (!admission) {
      console.warn('[QConnect/PlayNext] admission returned null (invoke failed)');
      if (!silent) {
        showToast(translate('qconnect.admissionCheckFailed'), 'error');
      }
      return false;
    }

    if (!admission.accepted) {
      console.warn('[QConnect/PlayNext] admission REJECTED: reason=%s handoff=%s', admission.reason, admission.handoff_intent);
      if (admission.handoff_intent === 'continue_locally') {
        // Track type not eligible for remote queue (local/plex) — use local queue
        console.log('[QConnect/PlayNext] handoff=continue_locally, falling through to local queue');
        useLocalFallback = true;
      } else {
        if (!silent) {
          showToast(translate(qconnectAdmissionReasonKey(admission.reason)), 'warning');
        }
        return false;
      }
    } else if (queueTrack.streamable === false) {
      console.warn('[QConnect/PlayNext] track not streamable');
      if (!silent) {
        showToast(translate('qconnect.streamNotEligible'), 'warning');
      }
      return false;
    }

    if (!useLocalFallback) {
      try {
        const playNextResolution = await resolveQconnectPlayNextInsertAfterFromSnapshots();
        const insertAfter = playNextResolution.insertAfter;
        console.log('[QConnect/PlayNext] insertAfter=%s strategy=%s', insertAfter, playNextResolution.strategy);
        const payload: Record<string, unknown> = {
          track_ids: [queueTrack.id],
          context_uuid: crypto.randomUUID(),
          autoplay_reset: false,
          autoplay_loading: false
        };
        if (typeof insertAfter === 'number') {
          payload.insert_after = insertAfter;
        }

        await emitQconnectDiagnostic('qconnect:play_next_anchor', 'info', {
          requested_track_id: queueTrack.id,
          origin,
          insert_after: insertAfter,
          strategy: playNextResolution.strategy,
          snapshot_error: playNextResolution.snapshotError,
          renderer_current: playNextResolution.rendererSnapshot?.current_track ?? null,
          renderer_next: playNextResolution.rendererSnapshot?.next_track ?? null,
          queue_preview: (playNextResolution.queueSnapshot?.queue_items ?? []).slice(0, 8),
          queue_length: playNextResolution.queueSnapshot?.queue_items.length ?? 0,
          autoplay_length: playNextResolution.queueSnapshot?.autoplay_items.length ?? 0
        });

        console.log('[QConnect/PlayNext] sending queue_insert_tracks payload=%o', payload);
        await sendQconnectQueueCommandWithAdmission('queue_insert_tracks', origin, payload);
        console.log('[QConnect/PlayNext] queue_insert_tracks SUCCESS');
        await emitQconnectDiagnostic('qconnect:play_next_sent', 'info', {
          requested_track_id: queueTrack.id,
          origin,
          insert_after: insertAfter,
          strategy: playNextResolution.strategy,
          payload
        });
        if (!silent) {
          showToast(translate('qconnect.remoteQueuedNext'), 'success');
        }
        return true;
      } catch (err) {
        console.error('[QConnect/PlayNext] FAILED:', err);
        await emitQconnectDiagnostic('qconnect:play_next_failed', 'error', {
          requested_track_id: queueTrack.id,
          origin,
          error: String(err)
        });
        if (!silent) {
          showToast(translate('qconnect.remoteQueueFailed'), 'error');
        }
        return false;
      }
    }
  }

  const success = await addToQueueNext(queueTrack, isLocal);
  if (!silent && success) {
    showToast('Queued to play next', 'success');
  } else if (!silent) {
    showToast('Failed to queue track', 'error');
  }
  return success;
}

export async function queueTrackLater(
  queueTrack: BackendQueueTrack,
  isLocal = false,
  options: QueueTrackActionOptions = {}
): Promise<boolean> {
  const silent = options.silent === true;
  const qconnectConnected = await isQconnectConnected();
  console.log('[QConnect/AddToQueue] connected=%s track=%d source=%s isLocal=%s', qconnectConnected, queueTrack.id, queueTrack.source, isLocal);
  let useLocalFallback = false;

  if (qconnectConnected) {
    const origin = resolveQconnectTrackOrigin(queueTrack, isLocal);
    console.log('[QConnect/AddToQueue] origin=%s', origin);
    const admission = await evaluateQconnectAdmission(origin);
    console.log('[QConnect/AddToQueue] admission=%o', admission);
    if (!admission) {
      console.warn('[QConnect/AddToQueue] admission returned null (invoke failed)');
      if (!silent) {
        showToast(translate('qconnect.admissionCheckFailed'), 'error');
      }
      return false;
    }

    if (!admission.accepted) {
      console.warn('[QConnect/AddToQueue] admission REJECTED: reason=%s handoff=%s', admission.reason, admission.handoff_intent);
      if (admission.handoff_intent === 'continue_locally') {
        console.log('[QConnect/AddToQueue] handoff=continue_locally, falling through to local queue');
        useLocalFallback = true;
      } else {
        if (!silent) {
          showToast(translate(qconnectAdmissionReasonKey(admission.reason)), 'warning');
        }
        return false;
      }
    } else if (queueTrack.streamable === false) {
      console.warn('[QConnect/AddToQueue] track not streamable');
      if (!silent) {
        showToast(translate('qconnect.streamNotEligible'), 'warning');
      }
      return false;
    }

    if (!useLocalFallback) {
      try {
        const payload = {
          track_ids: [queueTrack.id],
          context_uuid: crypto.randomUUID(),
          autoplay_reset: false,
          autoplay_loading: false
        };
        console.log('[QConnect/AddToQueue] sending queue_add_tracks payload=%o', payload);
        await sendQconnectQueueCommandWithAdmission('queue_add_tracks', origin, payload);
        console.log('[QConnect/AddToQueue] queue_add_tracks SUCCESS');
        if (!silent) {
          showToast(translate('qconnect.remoteQueuedLater'), 'success');
        }
        return true;
      } catch (err) {
        console.error('[QConnect/AddToQueue] FAILED:', err);
        if (!silent) {
          showToast(translate('qconnect.remoteQueueFailed'), 'error');
        }
        return false;
      }
    }
  }

  const success = await addToQueue(queueTrack, isLocal);
  if (!silent && success) {
    showToast('Added to queue', 'success');
  } else if (!silent) {
    showToast('Failed to add to queue', 'error');
  }
  return success;
}

// ============ QConnect Full Queue Load ============

/**
 * Replace the remote QConnect queue with a full set of track IDs.
 * Used when starting playback of an album or playlist from QBZ,
 * so the remote controllers see the same queue.
 *
 * @param trackIds Array of Qobuz track IDs to load
 * @param startIndex Index of the track to start playing (0-based)
 * @param shuffleMode Whether to enable shuffle on the remote
 * @returns true if the remote queue was loaded, false if not connected or rejected
 */
export async function loadQconnectQueue(
  trackIds: number[],
  startIndex: number = 0,
  shuffleMode: boolean = false
): Promise<boolean> {
  console.log('[QConnect/LoadQueue] called: trackCount=%d startIndex=%d', trackIds.length, startIndex);
  const qconnectConnected = await isQconnectConnected();
  if (!qconnectConnected) {
    console.warn('[QConnect/LoadQueue] skipped: transport not connected');
    await emitQconnectDiagnostic('qconnect:queue_load_skipped', 'warn', {
      reason: 'transport_not_connected',
      track_count: trackIds.length,
      start_index: startIndex
    });
    return false;
  }

  const origin: QconnectTrackOrigin = 'qobuz_online';
  const admission = await evaluateQconnectAdmission(origin);
  console.log('[QConnect/LoadQueue] admission=%o trackCount=%d startIndex=%d', admission, trackIds.length, startIndex);

  if (!admission?.accepted) {
    console.warn('[QConnect/LoadQueue] admission rejected: reason=%s', admission?.reason);
    await emitQconnectDiagnostic('qconnect:queue_load_rejected', 'warn', {
      reason: admission?.reason ?? 'unknown',
      track_count: trackIds.length,
      start_index: startIndex
    });
    return false;
  }

  try {
    const normalizedStartIndex = Math.max(0, Math.min(startIndex, Math.max(trackIds.length - 1, 0)));
    // NOSONAR: Math.random is used only as a shuffle-order seed for the
    // playback queue. Non-cryptographic by design.
    const shuffleSeed = shuffleMode
      ? Math.floor(Math.random() * 0x1_0000_0000)
      : undefined;
    const payload = {
      track_ids: trackIds,
      queue_position: shuffleMode ? 0 : normalizedStartIndex,
      shuffle_seed: shuffleSeed,
      shuffle_pivot_index: normalizedStartIndex,
      shuffle_mode: shuffleMode,
      context_uuid: crypto.randomUUID(),
      autoplay_reset: true,
      autoplay_loading: false
    };
    await emitQconnectDiagnostic('qconnect:queue_load_tracks', 'info', {
      track_count: trackIds.length,
      start_index: normalizedStartIndex,
      shuffle_mode: shuffleMode,
      preview_track_ids: trackIds.slice(0, 8),
      payload
    });
    console.log('[QConnect/LoadQueue] sending queue_load_tracks');
    await sendQconnectQueueCommandWithAdmission('queue_load_tracks', origin, payload);
    console.log('[QConnect/LoadQueue] SUCCESS');

    // Sync local autoplay preference to QConnect server after queue load
    try {
      const { isAutoplayEnabled } = await import('$lib/stores/playbackPreferencesStore');
      await invoke('v2_qconnect_set_autoplay_mode_if_remote', {
        enabled: isAutoplayEnabled()
      });
    } catch {
      // Best-effort sync
    }

    await emitQconnectDiagnostic('qconnect:queue_load_sent', 'info', {
      track_count: trackIds.length,
      start_index: normalizedStartIndex,
      shuffle_mode: shuffleMode,
      preview_track_ids: trackIds.slice(0, 8)
    });
    return true;
  } catch (err) {
    console.error('[QConnect/LoadQueue] FAILED:', err);
    await emitQconnectDiagnostic('qconnect:queue_load_failed', 'error', {
      track_count: trackIds.length,
      start_index: startIndex,
      shuffle_mode: shuffleMode,
      error: String(err)
    });
    return false;
  }
}

// ============ Convenience Queue Functions ============

export async function queueQobuzTrackNext(track: QobuzTrack): Promise<void> {
  await queueTrackNext(buildQueueTrackFromQobuz(track));
}

export async function queueQobuzTrackLater(track: QobuzTrack): Promise<void> {
  await queueTrackLater(buildQueueTrackFromQobuz(track));
}

export async function queuePlaylistTrackNext(track: PlaylistTrack): Promise<void> {
  await queueTrackNext(buildQueueTrackFromPlaylistTrack(track));
}

export async function queuePlaylistTrackLater(track: PlaylistTrack): Promise<void> {
  await queueTrackLater(buildQueueTrackFromPlaylistTrack(track));
}

export async function queueLocalTrackNext(track: LocalLibraryTrack): Promise<void> {
  const isPlexTrack = track.source === 'plex';
  await queueTrackNext(buildQueueTrackFromLocalTrack(track), !isPlexTrack);
}

export async function queueLocalTrackLater(track: LocalLibraryTrack): Promise<void> {
  const isPlexTrack = track.source === 'plex';
  await queueTrackLater(buildQueueTrackFromLocalTrack(track), !isPlexTrack);
}

export function buildQueueTrackFromDisplayTrack(track: DisplayTrack): BackendQueueTrack {
  return {
    id: track.id,
    title: track.title,
    artist: track.artist || 'Unknown Artist',
    album: track.album || '',
    duration_secs: track.durationSeconds,
    artwork_url: track.albumArt || null,
    hires: track.hires ?? false,
    bit_depth: track.bitDepth ?? null,
    sample_rate: track.samplingRate ?? null,
    is_local: track.isLocal ?? false,
    album_id: track.albumId || null,
    artist_id: track.artistId ?? null,
    source: track.isLocal ? 'local' : 'qobuz',
    parental_warning: track.parental_warning ?? false
  };
}

export async function queueDisplayTrackNext(track: DisplayTrack): Promise<void> {
  await queueTrackNext(buildQueueTrackFromDisplayTrack(track), track.isLocal ?? false);
}

export async function queueDisplayTrackLater(track: DisplayTrack): Promise<void> {
  await queueTrackLater(buildQueueTrackFromDisplayTrack(track), track.isLocal ?? false);
}

// ============ Favorites ============

export async function handleAddToFavorites(trackId: number): Promise<void> {
  const success = await addTrackToFavorites(trackId);
  if (success) {
    showToast('Added to favorites', 'success');
  } else {
    showToast('Failed to add to favorites', 'error');
  }
}

// ============ Playlist Actions ============

export function addToPlaylist(trackIds: number[]): void {
  openPlaylistModal('addTrack', trackIds);
}

// ============ Sharing ============

async function copyToClipboard(text: string, successMessage: string): Promise<void> {
  try {
    await writeText(text);
    showToast(successMessage, 'success');
  } catch (err) {
    console.error('Failed to copy to clipboard:', err);
    showToast('Failed to copy link', 'error');
  }
}

/** Extract a human-readable message from a Tauri RuntimeError or unknown throw. */
function errorMessage(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err && typeof err === 'object') {
    const obj = err as Record<string, unknown>;
    if (typeof obj.details === 'string') return obj.details;
    if (typeof obj.message === 'string') return obj.message;
  }
  return 'Unknown error';
}

export async function shareQobuzTrackLink(trackId: number): Promise<void> {
  try {
    const url = await invoke<string>('v2_get_qobuz_track_url', { trackId });
    await copyToClipboard(url, 'Qobuz link copied');
  } catch (err) {
    console.error('Failed to get Qobuz link:', err);
    showToast(`Failed to share Qobuz link: ${errorMessage(err)}`, 'error');
  }
}

interface SongLinkResponse {
  pageUrl: string;
  title?: string;
  artist?: string;
  thumbnailUrl?: string;
  platforms: Record<string, string>;
  identifier: string;
  contentType: string;
}

export async function shareSonglinkTrack(trackId: number, isrc?: string): Promise<void> {
  const qobuzUrl = `https://www.qobuz.com/track/${trackId}`;
  const resolvedIsrc = isrc?.trim();
  try {
    showToast('Fetching Song.link...', 'info');
    const response = await invoke<SongLinkResponse>('v2_share_track_songlink', {
      isrc: resolvedIsrc?.length ? resolvedIsrc : null,
      url: qobuzUrl,
      trackId
    });
    await copyToClipboard(response.pageUrl, 'Song.link copied');
  } catch (err) {
    console.error('Failed to get Song.link:', err);
    showToast(`Song.link error: ${errorMessage(err)}`, 'error');
  }
}
