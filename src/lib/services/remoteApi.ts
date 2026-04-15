/**
 * Remote API Service
 *
 * HTTP client for communicating with a remote qbzd daemon.
 * Used when playbackTarget.type === 'qbzd'.
 *
 * All methods throw if not in remote mode — callers must check first.
 */
import { getTarget, type PlaybackTarget } from '$lib/stores/playbackTargetStore';

export class RemoteApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
    this.name = 'RemoteApiError';
  }
}

function requireRemote(): PlaybackTarget & { type: 'qbzd'; baseUrl: string } {
  const target = getTarget();
  if (target.type !== 'qbzd' || !target.baseUrl) {
    throw new Error('Not in remote mode');
  }
  return target as PlaybackTarget & { type: 'qbzd'; baseUrl: string };
}

/** Make a GET request to the remote daemon */
export async function remoteGet<T = unknown>(path: string): Promise<T> {
  const target = requireRemote();
  const headers: Record<string, string> = {};
  if (target.token) headers['X-API-Key'] = target.token;

  const response = await fetch(`${target.baseUrl}${path}`, { headers });
  if (!response.ok) {
    const body = await response.text().catch(() => '');
    throw new RemoteApiError(response.status, 'request_failed', body || response.statusText);
  }
  return response.json();
}

/** Make a POST request to the remote daemon */
export async function remotePost<T = unknown>(path: string, body?: unknown): Promise<T> {
  const target = requireRemote();
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (target.token) headers['X-API-Key'] = target.token;

  const response = await fetch(`${target.baseUrl}${path}`, {
    method: 'POST',
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new RemoteApiError(response.status, 'request_failed', text || response.statusText);
  }
  // Some endpoints return plain text ("ok"), not JSON
  const text = await response.text();
  try {
    return JSON.parse(text);
  } catch {
    return text as unknown as T;
  }
}

/** Check if a remote daemon is reachable */
export async function pingRemote(baseUrl: string, token?: string): Promise<boolean> {
  try {
    const headers: Record<string, string> = {};
    if (token) headers['X-API-Key'] = token;
    const response = await fetch(`${baseUrl}/api/ping`, {
      headers,
      signal: AbortSignal.timeout(5000),
    });
    return response.ok;
  } catch {
    return false;
  }
}

/** Get daemon info (name, version, capabilities) */
export async function getRemoteInfo(
  baseUrl: string,
  token?: string,
): Promise<{ name: string; version: string; cache: Record<string, number> }> {
  const headers: Record<string, string> = {};
  if (token) headers['X-API-Key'] = token;
  const response = await fetch(`${baseUrl}/api/info`, {
    headers,
    signal: AbortSignal.timeout(5000),
  });
  if (!response.ok) throw new Error('Failed to get daemon info');
  return response.json();
}
