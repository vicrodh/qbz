// Sleep timer for the QueuePanel toolbar.
// Frontend-only by design: a deadline is held in memory, a single setTimeout
// fires cmdPause() at expiry, and a 1Hz tick updates the visible countdown.
// Nothing persists across app restarts (per spec) so we don't touch storage.

import { writable, derived, get } from 'svelte/store';
import { cmdPause } from '$lib/services/commandRouter';

export const SLEEP_TIMER_PRESETS_MIN = [30, 60, 120, 180, 300] as const;
export const SLEEP_TIMER_CUSTOM_MIN_LIMIT = 15;
export const SLEEP_TIMER_CUSTOM_MAX_LIMIT = 24 * 60;

interface SleepTimerInternalState {
  active: boolean;
  deadlineMs: number | null;
  durationMin: number | null;
}

const _state = writable<SleepTimerInternalState>({
  active: false,
  deadlineMs: null,
  durationMin: null
});
// `_now` is a small ticker the countdown derives off of — keeps reactivity
// localized to subscribers of `sleepTimerRemainingSec` rather than poking
// every consumer of `_state` once per second.
const _now = writable<number>(Date.now());

let fireTimeout: ReturnType<typeof setTimeout> | null = null;
let tickInterval: ReturnType<typeof setInterval> | null = null;

function clearTimers() {
  if (fireTimeout) {
    clearTimeout(fireTimeout);
    fireTimeout = null;
  }
  if (tickInterval) {
    clearInterval(tickInterval);
    tickInterval = null;
  }
}

export const sleepTimer = { subscribe: _state.subscribe };

export const sleepTimerRemainingSec = derived([_state, _now], ([s, n]) => {
  if (!s.active || s.deadlineMs == null) return 0;
  return Math.max(0, Math.ceil((s.deadlineMs - n) / 1000));
});

export function setSleepTimer(durationMin: number): void {
  if (!Number.isFinite(durationMin) || durationMin <= 0) return;
  const clamped = Math.min(
    SLEEP_TIMER_CUSTOM_MAX_LIMIT,
    Math.max(1, Math.floor(durationMin))
  );
  clearTimers();
  const deadlineMs = Date.now() + clamped * 60_000;
  _state.set({ active: true, deadlineMs, durationMin: clamped });
  _now.set(Date.now());
  fireTimeout = setTimeout(() => {
    fireTimeout = null;
    void cmdPause().catch((err) => {
      console.warn('[sleepTimer] pause on expiry failed:', err);
    });
    cancelSleepTimer();
  }, clamped * 60_000);
  tickInterval = setInterval(() => _now.set(Date.now()), 1000);
}

export function cancelSleepTimer(): void {
  clearTimers();
  _state.set({ active: false, deadlineMs: null, durationMin: null });
}

export function isSleepTimerActive(): boolean {
  return get(_state).active;
}

export function formatSleepTimerRemaining(secs: number): string {
  if (secs <= 0) return '0s';
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return s > 0 && m < 5 ? `${m}m ${s}s` : `${m}m`;
  return `${s}s`;
}
