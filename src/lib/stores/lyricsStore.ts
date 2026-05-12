/**
 * Lyrics State Store
 *
 * Manages lyrics fetching, LRC parsing, and synced line tracking.
 */

import { invoke } from '@tauri-apps/api/core';
import { getCurrentTrack, getCurrentTime, subscribe as subscribePlayer } from './playerStore';
import { isOffline as checkIsOffline } from './offlineStore';

// ============ Types ============

export interface LyricsPayload {
  trackId: number | null;
  title: string;
  artist: string;
  album: string | null;
  durationSecs: number | null;
  plain: string | null;
  syncedLrc: string | null;
  provider: 'lrclib' | 'ovh';
  cached: boolean;
}

export interface LyricsLine {
  timeMs: number;
  text: string;
  // Optional end-of-vocal timestamp. Set from an empty-text LRC marker
  // following this line, when present. Lets us cap progress to the actual
  // sung portion and hold at 1.0 through an instrumental gap.
  endMs?: number;
}

export interface ParsedLyrics {
  lines: LyricsLine[];
  isSynced: boolean;
}

type LyricsStatus = 'idle' | 'loading' | 'loaded' | 'error' | 'not_found';

// ============ State ============

let status: LyricsStatus = 'idle';
let error: string | null = null;
let payload: LyricsPayload | null = null;
let parsedLyrics: ParsedLyrics = { lines: [], isSynced: false };
let activeIndex = -1;
let activeProgress = 0;
let sidebarVisible = false;

// Track the last fetched track to avoid duplicate fetches
let lastFetchedTrackId: number | null = null;

// Listeners
const listeners = new Set<() => void>();

function notifyListeners(): void {
  for (const listener of listeners) {
    listener();
  }
}

// ============ Subscribe ============

export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  listener();
  return () => listeners.delete(listener);
}

// ============ Getters ============

export function getStatus(): LyricsStatus {
  return status;
}

export function getError(): string | null {
  return error;
}

export function getPayload(): LyricsPayload | null {
  return payload;
}

export function getParsedLyrics(): ParsedLyrics {
  return parsedLyrics;
}

export function getActiveIndex(): number {
  return activeIndex;
}

export function getActiveProgress(): number {
  return activeProgress;
}

export function isSidebarVisible(): boolean {
  return sidebarVisible;
}

export interface LyricsState {
  status: LyricsStatus;
  error: string | null;
  payload: LyricsPayload | null;
  lines: LyricsLine[];
  isSynced: boolean;
  activeIndex: number;
  activeProgress: number;
  sidebarVisible: boolean;
}

export function getLyricsState(): LyricsState {
  return {
    status,
    error,
    payload,
    lines: parsedLyrics.lines,
    isSynced: parsedLyrics.isSynced,
    activeIndex,
    activeProgress,
    sidebarVisible
  };
}

// ============ LRC Parser ============

/**
 * Parse LRC format into array of lines with timestamps
 * Supports: [mm:ss.xx], [mm:ss.xxx], [mm:ss]
 */
function parseLRC(lrc: string): LyricsLine[] {
  // Two-pass: first collect every timestamp (incl. empty-text "gap" markers
  // like [02:34.00] used to mark end-of-vocal before an instrumental break),
  // then emit only the text-bearing lines with each one's endMs derived from
  // the following timestamp (whether text or gap).
  const stamps: { timeMs: number; text: string }[] = [];
  const regex = /\[(\d{1,2}):(\d{2})(?:[.:](\d{2,3}))?\](.*)/g;

  let match;
  while ((match = regex.exec(lrc)) !== null) {
    const minutes = Number.parseInt(match[1], 10);
    const seconds = Number.parseInt(match[2], 10);
    const ms = match[3] ? Number.parseInt(match[3].padEnd(3, '0'), 10) : 0;
    const text = match[4].trim();
    const timeMs = (minutes * 60 + seconds) * 1000 + ms;
    stamps.push({ timeMs, text });
  }

  stamps.sort((a, b) => a.timeMs - b.timeMs);

  // Cap any single line's sung duration. Anything beyond this is almost
  // certainly an instrumental gap the LRC didn't mark — capping prevents
  // the karaoke gradient from creeping across silence.
  const MAX_SUNG_MS = 8000;

  const lines: LyricsLine[] = [];
  for (let i = 0; i < stamps.length; i++) {
    const stamp = stamps[i];
    if (!stamp.text) continue; // gap marker — never displayed, only bounds previous line
    const nextStamp = stamps[i + 1];
    const cap = stamp.timeMs + MAX_SUNG_MS;
    const endMs = nextStamp ? Math.min(nextStamp.timeMs, cap) : undefined;
    lines.push({ timeMs: stamp.timeMs, text: stamp.text, endMs });
  }

  // Last line has no following stamp; the previous default of 5s was
  // visibly slow on songs that end with a short vocal phrase followed by
  // an instrumental coda. Estimate from the median of preceding lines'
  // sung durations — a more song-appropriate fallback.
  if (lines.length >= 2 && lines[lines.length - 1].endMs === undefined) {
    const durations: number[] = [];
    for (let i = 0; i < lines.length - 1; i++) {
      const d = (lines[i].endMs ?? lines[i + 1].timeMs) - lines[i].timeMs;
      if (d > 0) durations.push(d);
    }
    if (durations.length > 0) {
      durations.sort((a, b) => a - b);
      const median = durations[Math.floor(durations.length / 2)];
      const last = lines[lines.length - 1];
      last.endMs = last.timeMs + Math.min(median, MAX_SUNG_MS);
    }
  }

  return lines;
}

/**
 * Parse plain lyrics (no timestamps)
 */
function parsePlain(plain: string): LyricsLine[] {
  return plain
    .split('\n')
    .map(line => line.trim())
    .filter(line => line.length > 0)
    .map(text => ({ timeMs: 0, text }));
}

/**
 * Parse lyrics payload into lines
 */
function parsePayload(p: LyricsPayload): ParsedLyrics {
  if (p.syncedLrc && p.syncedLrc.trim()) {
    const lines = parseLRC(p.syncedLrc);
    if (lines.length > 0) {
      return { lines, isSynced: true };
    }
  }

  if (p.plain && p.plain.trim()) {
    return { lines: parsePlain(p.plain), isSynced: false };
  }

  return { lines: [], isSynced: false };
}

// ============ Active Line Tracking ============

/**
 * Find active line index using binary search
 */
function findActiveLineIndex(lines: LyricsLine[], currentTimeMs: number): number {
  if (lines.length === 0) return -1;

  let left = 0;
  let right = lines.length - 1;
  let result = -1;

  while (left <= right) {
    const mid = Math.floor((left + right) / 2);
    if (lines[mid].timeMs <= currentTimeMs) {
      result = mid;
      left = mid + 1;
    } else {
      right = mid - 1;
    }
  }

  return result;
}

/**
 * Calculate progress within current line (0-1)
 */
function calculateLineProgress(lines: LyricsLine[], index: number, currentTimeMs: number): number {
  if (index < 0 || index >= lines.length) return 0;

  const currentLine = lines[index];
  const nextLine = lines[index + 1];

  // Prefer endMs (sung-portion boundary from LRC gap marker) over next
  // line's start, so we hit 1.0 when the singer actually stops, and hold
  // there through any instrumental gap before the next line.
  const boundMs = currentLine.endMs ?? nextLine?.timeMs ?? currentLine.timeMs + 5000;
  const lineDuration = boundMs - currentLine.timeMs;
  if (lineDuration <= 0) return 0;

  const ratio = (currentTimeMs - currentLine.timeMs) / lineDuration;
  // Snap to 1 once we're effectively done — guards against any audio-time
  // reporting quirk that would otherwise keep the ratio at e.g. 0.998
  // forever and leave the karaoke gradient with a stuck 0.2% tail.
  if (ratio >= 0.99) return 1;
  if (ratio <= 0) return 0;
  return ratio;
}

/**
 * Update active line based on current playback time
 */
let lastLogTime = 0;
export function updateActiveLine(): void {
  if (!parsedLyrics.isSynced || parsedLyrics.lines.length === 0) {
    if (activeIndex !== -1 || activeProgress !== 0) {
      activeIndex = -1;
      activeProgress = 0;
      notifyListeners();
    }
    return;
  }

  const currentTimeMs = getCurrentTime() * 1000;
  const newIndex = findActiveLineIndex(parsedLyrics.lines, currentTimeMs);
  const newProgress = calculateLineProgress(parsedLyrics.lines, newIndex, currentTimeMs);

  // Debug log every 5 seconds (dev only)
  if (import.meta.env.DEV) {
    const now = Date.now();
    if (now - lastLogTime > 5000) {
      lastLogTime = now;
      console.log('[Lyrics] Update:', {
        currentTimeMs,
        newIndex,
        newProgress: newProgress.toFixed(2),
        activeLine: parsedLyrics.lines[newIndex]?.text?.substring(0, 30)
      });
    }
  }

  // When progress tracking is disabled (immersive mode), only notify on index change
  // This reduces re-renders by ~90% in immersive mode
  if (!trackProgressEnabled) {
    if (newIndex !== activeIndex) {
      activeIndex = newIndex;
      activeProgress = 0; // Don't track progress
      notifyListeners();
    }
    return;
  }

  // Full tracking: notify on any change. Threshold-based throttling made
  // sense with setInterval ticks at 80–200ms, but our rAF tick gives us a
  // natural 60Hz upper bound — and at any threshold > 0, a freshly-activated
  // line freezes visually until the threshold is crossed (reads as a
  // "hanging" pause at line start). Each notification is just an inline
  // style update downstream, so per-tick cost is negligible.
  if (newIndex !== activeIndex || newProgress !== activeProgress) {
    activeIndex = newIndex;
    activeProgress = newProgress;
    notifyListeners();
  }
}

// ============ Actions ============

/**
 * Fetch lyrics for current track
 */
export async function fetchLyrics(): Promise<void> {
  const track = getCurrentTrack();

  if (!track) {
    reset();
    return;
  }

  // Don't fetch lyrics when offline
  if (checkIsOffline()) {
    status = 'error';
    error = 'Lyrics unavailable offline';
    payload = null;
    parsedLyrics = { lines: [], isSynced: false };
    notifyListeners();
    return;
  }

  // Skip if already fetched for this track
  if (track.id === lastFetchedTrackId && status === 'loaded') {
    return;
  }

  lastFetchedTrackId = track.id;
  status = 'loading';
  error = null;
  notifyListeners();

  try {
    console.log(`[Lyrics] Fetching for: "${track.title}" by "${track.artist}"`);
    const result = await invoke<LyricsPayload | null>('v2_lyrics_get', {
      trackId: track.id,
      title: track.title,
      version: track.version ?? null,
      artist: track.artist,
      album: track.album || null,
      durationSecs: track.duration || null
    });

    // Explicit logging - no objects to expand
    console.log(`[Lyrics] Backend: hasResult=${!!result}, hasSyncedLrc=${!!result?.syncedLrc}, syncedLen=${result?.syncedLrc?.length ?? 0}, hasPlain=${!!result?.plain}, provider=${result?.provider}`);
    if (result?.syncedLrc) {
      console.log(`[Lyrics] Synced LRC preview: ${result.syncedLrc.substring(0, 150)}`);
    }

    if (result) {
      payload = result;
      parsedLyrics = parsePayload(result);
      console.log(`[Lyrics] Parsed: linesCount=${parsedLyrics.lines.length}, isSynced=${parsedLyrics.isSynced}, firstTimeMs=${parsedLyrics.lines[0]?.timeMs ?? 'N/A'}, firstText="${parsedLyrics.lines[0]?.text?.substring(0, 30) ?? 'N/A'}"`);
      status = 'loaded';
      activeIndex = -1;
      activeProgress = 0;

      // Immediately update active line
      if (parsedLyrics.isSynced) {
        updateActiveLine();
      }
    } else {
      payload = null;
      parsedLyrics = { lines: [], isSynced: false };
      status = 'not_found';
    }
  } catch (err) {
    console.error('Failed to fetch lyrics:', err);
    status = 'error';
    error = err instanceof Error ? err.message : String(err);
    payload = null;
    parsedLyrics = { lines: [], isSynced: false };
  }

  notifyListeners();
}

/**
 * Toggle sidebar visibility
 */
export function toggleSidebar(): void {
  sidebarVisible = !sidebarVisible;
  notifyListeners();
}

/**
 * Show sidebar
 */
export function showSidebar(): void {
  if (!sidebarVisible) {
    sidebarVisible = true;
    notifyListeners();
  }
}

/**
 * Hide sidebar
 */
export function hideSidebar(): void {
  if (sidebarVisible) {
    sidebarVisible = false;
    notifyListeners();
  }
}

/**
 * Clear lyrics cache (via backend)
 */
export async function clearCache(): Promise<void> {
  try {
    await invoke('v2_lyrics_clear_cache');
    console.log('Lyrics cache cleared');
  } catch (err) {
    console.error('Failed to clear lyrics cache:', err);
  }
}

/**
 * Reset store state
 */
export function reset(): void {
  status = 'idle';
  error = null;
  payload = null;
  parsedLyrics = { lines: [], isSynced: false };
  activeIndex = -1;
  activeProgress = 0;
  lastFetchedTrackId = null;
  notifyListeners();
}

// ============ Auto-update ============

let updateRafHandle: number | null = null;
let isUpdatesActive = false;
let trackProgressEnabled = true; // When false, only track index changes (no karaoke)

/**
 * Check if active line updates are currently running
 */
export function isActiveLineUpdatesRunning(): boolean {
  return updateRafHandle !== null;
}

/**
 * Enable/disable progress tracking.
 * When disabled, only line index changes are tracked (no karaoke progress).
 * This significantly reduces re-renders and CPU usage.
 */
export function setProgressTrackingEnabled(enabled: boolean): void {
  trackProgressEnabled = enabled;
  if (import.meta.env.DEV) {
    console.log(`[Lyrics] Progress tracking: ${enabled ? 'enabled' : 'disabled'}`);
  }
}

/**
 * Start auto-updating active line (call when lyrics are synced and playing).
 *
 * Driven by requestAnimationFrame so line-boundary transitions are detected
 * within one display frame (~16ms) instead of the 80–200ms a setInterval
 * could miss them by. updateActiveLine itself still gates notifications via
 * the progress threshold, so re-render rate downstream is unchanged — only
 * detection latency improves.
 *
 * rAF pauses in background tabs, which is the desired behavior: when the
 * user can't see the lyrics, there's no reason to keep updating them.
 */
export function startActiveLineUpdates(): void {
  if (updateRafHandle !== null) return;

  isUpdatesActive = true;

  if (import.meta.env.DEV) {
    console.log('[Lyrics] Starting active line updates (rAF)');
  }

  const tick = (): void => {
    if (!isUpdatesActive || !parsedLyrics.isSynced) {
      if (import.meta.env.DEV) {
        console.log('[Lyrics] Auto-stopping rAF (conditions no longer met)');
      }
      stopActiveLineUpdates();
      return;
    }
    updateActiveLine();
    updateRafHandle = requestAnimationFrame(tick);
  };

  updateRafHandle = requestAnimationFrame(tick);
}

/**
 * Stop auto-updating active line
 */
export function stopActiveLineUpdates(): void {
  isUpdatesActive = false;
  if (updateRafHandle !== null) {
    if (import.meta.env.DEV) {
      console.log('[Lyrics] Stopping active line updates');
    }
    cancelAnimationFrame(updateRafHandle);
    updateRafHandle = null;
  }
}

// ============ Player Integration ============

let playerUnsubscribe: (() => void) | null = null;
let lastTrackId: number | null = null;

/**
 * Start watching player state for track changes
 * Prefetches lyrics as soon as a new track starts playing
 */
export function startWatching(): void {
  if (playerUnsubscribe) return;

  console.log('[Lyrics] Starting track watcher');
  playerUnsubscribe = subscribePlayer(() => {
    const track = getCurrentTrack();
    const trackId = track?.id ?? null;

    // Track changed - always prefetch lyrics for new track
    if (trackId !== lastTrackId) {
      console.log('[Lyrics] Track changed:', { from: lastTrackId, to: trackId, title: track?.title });
      lastTrackId = trackId;
      if (trackId !== null) {
        // Always prefetch lyrics when a new track starts
        fetchLyrics();
      } else {
        reset();
      }
    }
  });
}

/**
 * Stop watching player state
 */
export function stopWatching(): void {
  if (playerUnsubscribe) {
    playerUnsubscribe();
    playerUnsubscribe = null;
  }
  stopActiveLineUpdates();
}
