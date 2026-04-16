/**
 * diagnosticsSnapshot
 *
 * Collects the same payload the System Diagnostics panel exports, in a
 * text-block format suited for attaching to a paste-site upload alongside
 * the terminal log. Used by the "include diagnostics" toggle in the
 * Developer Mode > View Logs modal so users can share one link instead
 * of two.
 *
 * No sensitive data. Track fields exclude IDs/UUIDs; QConnect fields
 * exclude endpoint URLs and renderer UUIDs; cast devices expose only
 * friendly names (no hosts/ports).
 */

import { invoke } from '@tauri-apps/api/core';
import {
  getCurrentTrack,
  getIsPlaying,
  getCurrentTime,
  getDuration,
  getVolume,
} from '$lib/stores/playerStore';
import type {
  QconnectConnectionStatus,
  QconnectSessionSnapshot,
} from '$lib/services/qconnectRuntime';

function redactIdLike(value: string | null | undefined): string | null {
  if (!value) return null;
  return value
    .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, '<uuid>')
    .replace(/\b[0-9a-f]{32,}\b/gi, '<hex>');
}

function snapshotPlayback(): Record<string, unknown> {
  const track = getCurrentTrack();
  return {
    isPlaying: getIsPlaying(),
    volumePercent: Math.round(getVolume()),
    positionSecs: Math.round(getCurrentTime()),
    durationSecs: Math.round(getDuration()),
    hasTrack: !!track,
    trackTitle: track?.title ?? null,
    trackArtist: track?.artist ?? null,
    trackAlbum: track?.album ?? null,
    trackQuality: track?.quality ?? null,
    trackFormat: track?.format ?? null,
    trackBitDepth: track?.bitDepth ?? null,
    trackSamplingRate: track?.samplingRate ?? null,
    trackIsLocal: track?.isLocal ?? null,
    trackSource: track?.source ?? null,
  };
}

async function snapshotQconnect(): Promise<Record<string, unknown>> {
  try {
    const status = await invoke<QconnectConnectionStatus>('v2_qconnect_get_status');
    const session = await invoke<QconnectSessionSnapshot>('v2_qconnect_session_snapshot');
    const activeId = session?.active_renderer_id ?? null;
    const localId = session?.local_renderer_id ?? null;
    const active = activeId != null
      ? session?.renderers?.find((r) => r.renderer_id === activeId)
      : null;
    let role: 'none' | 'controller' | 'local-renderer' | 'observer' = 'none';
    if (activeId != null) {
      role = activeId === localId ? 'local-renderer' : 'controller';
    } else if (status.running || status.transport_connected) {
      role = 'observer';
    }
    return {
      running: !!status.running,
      transport_connected: !!status.transport_connected,
      hasEndpoint: !!status.endpoint_url,
      lastError: status.last_error ? redactIdLike(String(status.last_error)) : null,
      role,
      activeRendererName: active?.friendly_name ?? null,
      activeRendererBrand: active?.brand ?? null,
      activeRendererModel: active?.model ?? null,
      rendererCount: session?.renderers?.length ?? 0,
    };
  } catch {
    return { unavailable: true };
  }
}

/**
 * Run a brief multicast scan and report the friendly-name list per protocol.
 * Does NOT persist cast state; stops discovery before returning.
 */
async function scanCastDevices(durationMs: number): Promise<Record<string, unknown>> {
  const started = Date.now();
  try {
    await Promise.allSettled([
      invoke('v2_cast_start_discovery'),
      invoke('v2_dlna_start_discovery'),
    ]);
  } catch { /* ignore */ }
  await new Promise((resolve) => setTimeout(resolve, durationMs));
  let chromecast: { name?: string }[] = [];
  let dlna: { name?: string }[] = [];
  try {
    const [cc, dl] = await Promise.allSettled([
      invoke<{ name?: string }[]>('v2_cast_get_devices'),
      invoke<{ name?: string }[]>('v2_dlna_get_devices'),
    ]);
    if (cc.status === 'fulfilled') chromecast = cc.value ?? [];
    if (dl.status === 'fulfilled') dlna = dl.value ?? [];
  } catch { /* ignore */ }
  try {
    await Promise.allSettled([
      invoke('v2_cast_stop_discovery'),
      invoke('v2_dlna_stop_discovery'),
    ]);
  } catch { /* ignore */ }
  return {
    chromecastCount: chromecast.length,
    dlnaCount: dlna.length,
    chromecastDevices: chromecast.map((d) => d.name ?? '<unnamed>'),
    dlnaDevices: dlna.map((d) => d.name ?? '<unnamed>'),
    durationMs: Date.now() - started,
  };
}

export interface CollectOptions {
  includeCastScan?: boolean;
  castScanMs?: number;
}

/**
 * Collect all diagnostic data and return it as a pretty-printed JSON block
 * suitable for prepending to a paste upload.
 */
export async function collectDiagnosticsText(opts: CollectOptions = {}): Promise<string> {
  const castPromise = opts.includeCastScan
    ? scanCastDevices(opts.castScanMs ?? 10000)
    : Promise.resolve(null);

  const [runtime, system, qconnect, cast] = await Promise.all([
    invoke<unknown>('v2_get_runtime_diagnostics').catch((e) => ({ error: String(e) })),
    invoke<unknown>('v2_get_system_info').catch((e) => ({ error: String(e) })),
    snapshotQconnect(),
    castPromise,
  ]);

  const payload = {
    exportedAt: new Date().toISOString(),
    runtime,
    system,
    playback: snapshotPlayback(),
    qconnect,
    castScan: cast,
  };

  return JSON.stringify(payload, null, 2);
}
