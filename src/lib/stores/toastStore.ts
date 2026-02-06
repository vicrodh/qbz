/**
 * Toast notification store
 *
 * Manages toast notifications across the app with auto-hide and queue support.
 */

import { getUserItem, setUserItem, removeUserItem } from '$lib/utils/userStorage';

export type ToastType = 'success' | 'error' | 'info' | 'warning' | 'buffering';

export interface Toast {
  message: string;
  type: ToastType;
  persistent?: boolean;
}

// Current toast state
let currentToast: Toast | null = null;

// Track buffering toast specifically so we can dismiss it
let bufferingToastActive = false;

// Global enable/disable for toasts (in-app notifications)
let toastsEnabled = true;

/**
 * Load toasts preference from localStorage
 */
export function loadToastsPreference(): void {
  const saved = getUserItem('qbz-toasts-enabled');
  if (saved !== null) {
    toastsEnabled = saved === 'true';
  } else {
    // Migrate from old key if exists
    const oldSaved = getUserItem('qbz-notifications-enabled');
    if (oldSaved !== null) {
      toastsEnabled = oldSaved === 'true';
      setUserItem('qbz-toasts-enabled', oldSaved);
      removeUserItem('qbz-notifications-enabled');
    }
  }
}

/**
 * Set toasts enabled/disabled
 */
export function setToastsEnabled(enabled: boolean): void {
  toastsEnabled = enabled;
  setUserItem('qbz-toasts-enabled', String(enabled));
  if (!enabled) {
    hideToast();
  }
}

/**
 * Get toasts enabled state
 */
export function getToastsEnabled(): boolean {
  return toastsEnabled;
}

// Auto-hide timeout
let hideTimeout: ReturnType<typeof setTimeout> | null = null;

// Listeners for state changes
const listeners = new Set<(toast: Toast | null) => void>();

/**
 * Get the current toast
 */
export function getToast(): Toast | null {
  return currentToast;
}

/**
 * Check if buffering toast is active
 */
export function isBufferingActive(): boolean {
  return bufferingToastActive;
}

/**
 * Show a toast notification
 * @param message The message to display
 * @param type The type of toast (success, error, info, buffering)
 * @param duration How long to show the toast in ms (default: varies by type)
 */
export function showToast(message: string, type: ToastType = 'info', duration?: number): void {
  // Skip if toasts are disabled (except errors which are always shown)
  if (!toastsEnabled && type !== 'error') {
    return;
  }

  // Clear existing timeout
  if (hideTimeout) {
    clearTimeout(hideTimeout);
    hideTimeout = null;
  }

  // Track buffering state
  if (type === 'buffering') {
    bufferingToastActive = true;
  }

  // Set the toast
  currentToast = {
    message,
    type,
    persistent: type === 'buffering'  // Buffering toasts don't auto-hide
  };
  notifyListeners();

  // Auto-hide based on type (buffering is persistent, no timeout)
  if (type !== 'buffering') {
    const defaultDurations: Record<ToastType, number> = {
      success: 3000,
      error: 5000,
      info: 3000,
      warning: 4000,
      buffering: 0  // Never used, but needed for type safety
    };

    const hideAfter = duration ?? defaultDurations[type];

    hideTimeout = setTimeout(() => {
      hideToast();
    }, hideAfter);
  }
}

/**
 * Dismiss buffering toast specifically (called when track loads)
 */
export function dismissBuffering(): void {
  if (bufferingToastActive) {
    bufferingToastActive = false;
    hideToast();
  }
}

/**
 * Hide the current toast
 */
export function hideToast(): void {
  if (hideTimeout) {
    clearTimeout(hideTimeout);
    hideTimeout = null;
  }
  currentToast = null;
  notifyListeners();
}

/**
 * Subscribe to toast changes
 * @param listener Callback function called when toast changes
 * @returns Unsubscribe function
 */
export function subscribe(listener: (toast: Toast | null) => void): () => void {
  listeners.add(listener);
  // Immediately notify with current state
  listener(currentToast);
  return () => listeners.delete(listener);
}

function notifyListeners(): void {
  for (const listener of listeners) {
    listener(currentToast);
  }
}
