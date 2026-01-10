/**
 * Lyrics State Store
 *
 * Manages lyrics fetching, LRC parsing, and synced line tracking.
 */

import { invoke } from '@tauri-apps/api/core';
import { getCurrentTrack, getCurrentTime, subscribe as subscribePlayer } from './playerStore';

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
  const lines: LyricsLine[] = [];
  const regex = /\[(\d{1,2}):(\d{2})(?:[.:](\d{2,3}))?\](.*)/g;

  let match;
  while ((match = regex.exec(lrc)) !== null) {
    const minutes = parseInt(match[1], 10);
    const seconds = parseInt(match[2], 10);
    const ms = match[3] ? parseInt(match[3].padEnd(3, '0'), 10) : 0;
    const text = match[4].trim();

    if (text) {
      const timeMs = (minutes * 60 + seconds) * 1000 + ms;
      lines.push({ timeMs, text });
    }
  }

  // Sort by time
  lines.sort((a, b) => a.timeMs - b.timeMs);

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

  if (!nextLine) {
    // Last line - assume 5 seconds duration
    const duration = 5000;
    return Math.min(1, (currentTimeMs - currentLine.timeMs) / duration);
  }

  const lineDuration = nextLine.timeMs - currentLine.timeMs;
  if (lineDuration <= 0) return 0;

  return Math.min(1, (currentTimeMs - currentLine.timeMs) / lineDuration);
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

  // Debug log every 2 seconds
  const now = Date.now();
  if (now - lastLogTime > 2000) {
    lastLogTime = now;
    console.log('[Lyrics] Update:', {
      currentTimeMs,
      newIndex,
      newProgress: newProgress.toFixed(2),
      activeLine: parsedLyrics.lines[newIndex]?.text?.substring(0, 30)
    });
  }

  if (newIndex !== activeIndex || Math.abs(newProgress - activeProgress) > 0.01) {
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
    const result = await invoke<LyricsPayload | null>('lyrics_get', {
      trackId: track.id,
      title: track.title,
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
    await invoke('lyrics_clear_cache');
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

let updateInterval: number | null = null;

/**
 * Start auto-updating active line (call when lyrics are synced and playing)
 */
export function startActiveLineUpdates(): void {
  if (updateInterval !== null) return;

  console.log('[Lyrics] Starting active line updates, isSynced:', parsedLyrics.isSynced);
  updateInterval = window.setInterval(() => {
    if (parsedLyrics.isSynced) {
      updateActiveLine();
    }
  }, 50); // 50ms for smooth progress
}

/**
 * Stop auto-updating active line
 */
export function stopActiveLineUpdates(): void {
  if (updateInterval !== null) {
    clearInterval(updateInterval);
    updateInterval = null;
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
