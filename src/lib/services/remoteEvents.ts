/**
 * Remote Events Service
 *
 * SSE client that connects to a remote qbzd daemon's /api/events endpoint.
 * When in remote mode, this replaces Tauri's event system for playback
 * and queue state updates.
 */
import { getTarget } from '$lib/stores/playbackTargetStore';

type EventHandler = (data: unknown) => void;

let eventSource: EventSource | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
const handlers: Map<string, EventHandler[]> = new Map();

/** Connect to the remote daemon's SSE stream */
export function connectRemoteEvents() {
  disconnectRemoteEvents();

  const target = getTarget();
  if (target.type !== 'qbzd' || !target.baseUrl || !target.token) return;

  const url = `${target.baseUrl}/api/events?token=${encodeURIComponent(target.token)}`;
  console.log('[RemoteEvents] Connecting to SSE:', target.baseUrl);

  eventSource = new EventSource(url);

  eventSource.onopen = () => {
    console.log('[RemoteEvents] SSE connected');
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  };

  eventSource.onerror = () => {
    console.warn('[RemoteEvents] SSE error, will reconnect...');
    disconnectRemoteEvents();
    reconnectTimer = setTimeout(connectRemoteEvents, 3000);
  };

  // Register handlers for each event type
  const eventTypes = [
    'playback',
    'queue',
    'track-started',
    'track-ended',
    'favorites-updated',
    'playlist-created',
    'playlist-updated',
    'error',
    'runtime',
    'logged-in',
    'logged-out',
  ];

  for (const type of eventTypes) {
    eventSource.addEventListener(type, (e: MessageEvent) => {
      try {
        const data = JSON.parse(e.data);
        const typeHandlers = handlers.get(type);
        if (typeHandlers) {
          for (const handler of typeHandlers) {
            handler(data);
          }
        }
      } catch (err) {
        console.warn(`[RemoteEvents] Failed to parse ${type} event:`, err);
      }
    });
  }
}

/** Disconnect from the remote SSE stream */
export function disconnectRemoteEvents() {
  if (eventSource) {
    eventSource.close();
    eventSource = null;
  }
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
}

/** Register a handler for a specific event type */
export function onRemoteEvent(type: string, handler: EventHandler): () => void {
  const existing = handlers.get(type) || [];
  existing.push(handler);
  handlers.set(type, existing);

  // Return unsubscribe function
  return () => {
    const list = handlers.get(type);
    if (list) {
      const idx = list.indexOf(handler);
      if (idx !== -1) list.splice(idx, 1);
    }
  };
}

/** Check if SSE is currently connected */
export function isRemoteEventsConnected(): boolean {
  return eventSource !== null && eventSource.readyState === EventSource.OPEN;
}
