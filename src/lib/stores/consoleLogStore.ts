/**
 * Console log capture store
 *
 * Wraps console.log/warn/error/info to capture browser-side logs
 * for the Developer Mode "View Logs" modal. Original console methods
 * are still called — no breakage.
 */

interface ConsoleEntry {
  level: 'log' | 'warn' | 'error' | 'info';
  timestamp: string;
  message: string;
}

const MAX_ENTRIES = 2000;
let entries: ConsoleEntry[] = [];
let initialized = false;

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

export function initConsoleCapture() {
  if (initialized) return;
  initialized = true;

  console.log = (...args: unknown[]) => {
    capture('log', args);
    originalConsole.log(...args);
  };
  console.warn = (...args: unknown[]) => {
    capture('warn', args);
    originalConsole.warn(...args);
  };
  console.error = (...args: unknown[]) => {
    capture('error', args);
    originalConsole.error(...args);
  };
  console.info = (...args: unknown[]) => {
    capture('info', args);
    originalConsole.info(...args);
  };
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
