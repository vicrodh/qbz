/**
 * Console log capture store
 *
 * Wraps console.log/warn/error/info to capture browser-side logs
 * for the Developer Mode "View Logs" modal. Original console methods
 * are still called — no breakage.
 *
 * Performance optimization:
 * - error/warn: Always captured (low frequency, high value for bug reports)
 * - log/info: Only captured in dev builds OR when verbose mode is enabled
 *   (reduces GC/CPU overhead in production)
 */

import { getUserItem, setUserItem } from '$lib/utils/userStorage';

interface ConsoleEntry {
  level: 'log' | 'warn' | 'error' | 'info';
  timestamp: string;
  message: string;
}

const MAX_ENTRIES = 2000;
let entries: ConsoleEntry[] = [];
let initialized = false;

const VERBOSE_CAPTURE_KEY = 'qbz-verbose-capture';

// Verbose capture mode - when true, captures log/info even in production
// Can be enabled via Developer Mode for debugging production issues
let verboseCapture = getUserItem(VERBOSE_CAPTURE_KEY) === 'true';

// Keep references to the real console methods
const originalConsole = {
  log: console.log.bind(console),
  warn: console.warn.bind(console),
  error: console.error.bind(console),
  info: console.info.bind(console),
};

function formatArgs(args: unknown[]): string {
  return args.map(arg => {
    if (typeof arg === 'string') return arg;
    try {
      return JSON.stringify(arg);
    } catch {
      return String(arg);
    }
  }).join(' ');
}

function capture(level: ConsoleEntry['level'], args: unknown[]) {
  const entry: ConsoleEntry = {
    level,
    timestamp: new Date().toISOString(),
    message: formatArgs(args),
  };

  entries.push(entry);
  if (entries.length > MAX_ENTRIES) {
    entries = entries.slice(-MAX_ENTRIES);
  }
}

// Check if we should capture log/info (verbose levels)
// In dev builds: always capture everything
// In production: only capture if verbose mode is explicitly enabled
function shouldCaptureVerbose(): boolean {
  return import.meta.env.DEV || verboseCapture;
}

export function initConsoleCapture() {
  if (initialized) return;
  initialized = true;

  // log/info: Only capture in dev OR when verbose mode is enabled
  console.log = (...args: unknown[]) => {
    if (shouldCaptureVerbose()) {
      capture('log', args);
    }
    originalConsole.log(...args);
  };
  console.info = (...args: unknown[]) => {
    if (shouldCaptureVerbose()) {
      capture('info', args);
    }
    originalConsole.info(...args);
  };

  // warn/error: Always capture (valuable for bug reports)
  console.warn = (...args: unknown[]) => {
    capture('warn', args);
    originalConsole.warn(...args);
  };
  console.error = (...args: unknown[]) => {
    capture('error', args);
    originalConsole.error(...args);
  };
}

/** Enable verbose capture (log/info) in production - for debugging */
export function enableVerboseCapture() {
  verboseCapture = true;
  setUserItem(VERBOSE_CAPTURE_KEY, 'true');
  originalConsole.info('[ConsoleCapture] Verbose capture enabled');
}

/** Disable verbose capture (log/info) in production */
export function disableVerboseCapture() {
  verboseCapture = false;
  setUserItem(VERBOSE_CAPTURE_KEY, 'false');
  originalConsole.info('[ConsoleCapture] Verbose capture disabled');
}

/** Check if verbose capture is currently enabled */
export function isVerboseCaptureEnabled(): boolean {
  return verboseCapture;
}

export function getConsoleLogs(): ConsoleEntry[] {
  return [...entries];
}

export function getConsoleLogsAsText(): string {
  return entries
    .map(e => `[${e.timestamp}] [${e.level.toUpperCase()}] ${e.message}`)
    .join('\n');
}

export function clearConsoleLogs() {
  entries = [];
}
