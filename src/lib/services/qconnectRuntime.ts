import { invoke } from '@tauri-apps/api/core';
import type {
  QconnectQueueSnapshot,
  QconnectRendererSnapshot
} from '$lib/services/qconnectRemoteQueue';

/**
 * Backend lifecycle state — kept in sync with `QconnectLifecycleState` in
 * `src-tauri/src/qconnect_service.rs`. The toggle's on/off display reads
 * `running` (derived from this), so a stuck reconnect loop is still visible as
 * "on" and the user can disable it (issue #358).
 */
export type QconnectLifecycleState =
  | 'off'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'exhausted';

export type QconnectConnectionStatus = {
  running: boolean;
  transport_connected: boolean;
  endpoint_url?: string | null;
  last_error?: string | null;
  state?: QconnectLifecycleState;
};

export type QconnectRendererInfo = {
  renderer_id: number;
  device_uuid?: string | null;
  friendly_name?: string | null;
  brand?: string | null;
  model?: string | null;
  device_type?: number | null;
};

export type QconnectSessionSnapshot = {
  session_uuid?: string | null;
  active_renderer_id?: number | null;
  local_renderer_id?: number | null;
  renderers: QconnectRendererInfo[];
};

export type QconnectAdmissionBlockedEvent = {
  command_type: string;
  origin: string;
  reason: string;
  handoff_intent: 'continue_locally' | 'send_to_connect';
};

export type QconnectDiagnosticsEntry = {
  ts: number;
  level: 'info' | 'warn' | 'error';
  channel: string;
  message: string;
};

export type QconnectPlaybackReportSkipResult = {
  shouldSkip: boolean;
  nextSkipSignature: string;
  diagnosticPayload: Record<string, unknown> | null;
};

export type QconnectSessionPersistenceDecision = {
  shouldPersist: boolean;
  nextSkipLogged: boolean;
  shouldLogSkip: boolean;
};

export type QconnectRuntimeStateSnapshot = {
  status: QconnectConnectionStatus;
  connected: boolean;
  queueSnapshot: QconnectQueueSnapshot | null;
  rendererSnapshot: QconnectRendererSnapshot | null;
  sessionSnapshot: QconnectSessionSnapshot | null;
  snapshotError: unknown | null;
};

export const DEFAULT_QCONNECT_CONNECTION_STATUS: QconnectConnectionStatus = {
  running: false,
  transport_connected: false,
  endpoint_url: null,
  last_error: null,
  state: 'off'
};

/**
 * The toggle's on/off reading. Returns true when the user has enabled
 * QConnect even if the WS is currently re-establishing. This is required so a
 * stuck reconnect loop still shows as "on" and the user can disable it from
 * the UI (issue #358).
 */
export function isQconnectToggleOn(status: QconnectConnectionStatus): boolean {
  return Boolean(status.running) || status.state === 'connecting' || status.state === 'reconnecting';
}

export const SHOW_QCONNECT_DEV_DIAGNOSTICS = import.meta.env.DEV;
export const QCONNECT_DIAGNOSTIC_LOG_LIMIT = 200;

export function qconnectAdmissionReasonKey(reason: string): string {
  if (reason === 'local_library_tracks_never_enter_remote_qconnect_queue') {
    return 'qconnect.admissionBlockedLocalLibrary';
  }
  if (reason === 'plex_tracks_never_enter_remote_qconnect_queue') {
    return 'qconnect.admissionBlockedPlex';
  }
  return 'qconnect.admissionBlockedUnknown';
}

function normalizeQconnectDiagnosticPayload(payload: unknown): string {
  if (typeof payload === 'string') {
    return payload;
  }

  try {
    return JSON.stringify(payload);
  } catch {
    return String(payload);
  }
}

export function appendQconnectDiagnosticEntry(
  logs: QconnectDiagnosticsEntry[],
  channel: string,
  level: QconnectDiagnosticsEntry['level'],
  payload: unknown,
  logLimit: number = QCONNECT_DIAGNOSTIC_LOG_LIMIT
): QconnectDiagnosticsEntry[] {
  return [
    {
      ts: Date.now(),
      level,
      channel,
      message: normalizeQconnectDiagnosticPayload(payload)
    },
    ...logs
  ].slice(0, logLimit);
}

export function logQconnectPlaybackReport(
  logs: QconnectDiagnosticsEntry[],
  source: 'interval' | 'player_transition',
  payload: Record<string, unknown>
): QconnectDiagnosticsEntry[] {
  return appendQconnectDiagnosticEntry(logs, `qconnect:report_playback_state:${source}`, 'info', payload);
}

export function evaluateQconnectPlaybackReportSkip(params: {
  currentTrackId: number | null | undefined;
  queueSnapshot: QconnectQueueSnapshot | null;
  rendererSnapshot: QconnectRendererSnapshot | null;
  lastSkipSignature: string;
}): QconnectPlaybackReportSkipResult {
  const {
    currentTrackId,
    queueSnapshot,
    rendererSnapshot,
    lastSkipSignature
  } = params;

  if (currentTrackId == null || !queueSnapshot) {
    return {
      shouldSkip: false,
      nextSkipSignature: '',
      diagnosticPayload: null
    };
  }

  const remoteQueueContainsTrack =
    queueSnapshot.queue_items.some((item) => item.track_id === currentTrackId) ||
    queueSnapshot.autoplay_items.some((item) => item.track_id === currentTrackId);

  const rendererCurrentTrackId = rendererSnapshot?.current_track?.track_id ?? null;

  if (remoteQueueContainsTrack || rendererCurrentTrackId === currentTrackId) {
    return {
      shouldSkip: false,
      nextSkipSignature: '',
      diagnosticPayload: null
    };
  }

  const skipSignature =
    `${currentTrackId}:${queueSnapshot.version?.major ?? 0}.${queueSnapshot.version?.minor ?? 0}`;

  return {
    shouldSkip: true,
    nextSkipSignature: skipSignature,
    diagnosticPayload: lastSkipSignature === skipSignature
      ? null
      : {
          reason: 'local_track_not_in_remote_queue',
          current_track_id: currentTrackId,
          renderer_current_track_id: rendererCurrentTrackId,
          remote_queue_preview: queueSnapshot.queue_items.slice(0, 6),
          renderer_current: rendererSnapshot?.current_track ?? null,
          renderer_next: rendererSnapshot?.next_track ?? null
        }
  };
}

export function isQconnectRemoteModeActive(
  connected: boolean,
  status: QconnectConnectionStatus
): boolean {
  return Boolean(connected || status.transport_connected);
}

export function isQconnectPeerRendererActive(
  sessionSnapshot: QconnectSessionSnapshot | null | undefined
): boolean {
  const activeRendererId = sessionSnapshot?.active_renderer_id ?? null;
  const localRendererId = sessionSnapshot?.local_renderer_id ?? null;

  if (activeRendererId == null || localRendererId == null || activeRendererId < 0) {
    return false;
  }

  return activeRendererId !== localRendererId;
}

export function shouldQconnectSuppressLocalPlaybackAutomation(
  connected: boolean,
  sessionSnapshot: QconnectSessionSnapshot | null | undefined
): boolean {
  return connected && isQconnectPeerRendererActive(sessionSnapshot);
}

export function resolveQconnectPlayNextAuthoritativeTrackId(params: {
  sessionSnapshot: QconnectSessionSnapshot | null | undefined;
  localCurrentTrackId: number | null | undefined;
}): number | null {
  const { sessionSnapshot, localCurrentTrackId } = params;

  if (
    typeof localCurrentTrackId !== 'number' ||
    !Number.isFinite(localCurrentTrackId) ||
    localCurrentTrackId <= 0
  ) {
    return null;
  }

  const activeRendererId = sessionSnapshot?.active_renderer_id ?? null;
  const localRendererId = sessionSnapshot?.local_renderer_id ?? null;

  if (activeRendererId == null || localRendererId == null || activeRendererId < 0) {
    return null;
  }

  return isQconnectPeerRendererActive(sessionSnapshot) ? null : localCurrentTrackId;
}

export function evaluateQconnectSessionPersistence(
  _remoteModeActive: boolean,
  _skipLogged: boolean
): QconnectSessionPersistenceDecision {
  // Local session state is persisted unconditionally so track-level
  // restore keeps working when Qobuz Connect is enabled (issue #304).
  // QConnect-vs-local priority is resolved at restore time, not here.
  return {
    shouldPersist: true,
    nextSkipLogged: false,
    shouldLogSkip: false
  };
}

export async function fetchQconnectRuntimeState(): Promise<QconnectRuntimeStateSnapshot> {
  try {
    const status = await invoke<QconnectConnectionStatus>('v2_qconnect_status');
    const connected = Boolean(status.transport_connected);

    if (!connected) {
      return {
        status,
        connected,
        queueSnapshot: null,
        rendererSnapshot: null,
        sessionSnapshot: null,
        snapshotError: null
      };
    }

    try {
      const [queueSnapshot, rendererSnapshot, sessionSnapshot] = await Promise.all([
        invoke<QconnectQueueSnapshot>('v2_qconnect_queue_snapshot'),
        invoke<QconnectRendererSnapshot>('v2_qconnect_renderer_snapshot'),
        invoke<QconnectSessionSnapshot>('v2_qconnect_session_snapshot')
      ]);

      return {
        status,
        connected,
        queueSnapshot,
        rendererSnapshot,
        sessionSnapshot,
        snapshotError: null
      };
    } catch (snapshotError) {
      return {
        status,
        connected,
        queueSnapshot: null,
        rendererSnapshot: null,
        sessionSnapshot: null,
        snapshotError
      };
    }
  } catch {
    return {
      status: DEFAULT_QCONNECT_CONNECTION_STATUS,
      connected: false,
      queueSnapshot: null,
      rendererSnapshot: null,
      sessionSnapshot: null,
      snapshotError: null
    };
  }
}

/**
 * Toggle QConnect on/off based on whether the runtime is currently considered
 * "on" by the user. We must NOT base this on `transport_connected`: when the
 * reconnect loop is stuck (Reconnecting / Connecting), `transport_connected`
 * is false but the runtime is alive, and only `disconnect()` will tear it
 * down. Calling `connect()` in that state used to error with "already
 * running" — now it's a no-op and we still want the toggle to "turn it off".
 * (issue #358)
 */
export async function toggleQconnectConnection(toggleOn: boolean): Promise<void> {
  if (toggleOn) {
    await invoke('v2_qconnect_disconnect');
    return;
  }

  await invoke('v2_qconnect_connect', { options: null });
}
