/**
 * Toast notification store
 *
 * Manages toast notifications across the app with auto-hide and queue support.
 */

export type ToastType = 'success' | 'error' | 'info';

export interface Toast {
  message: string;
  type: ToastType;
}

// Current toast state
let currentToast: Toast | null = null;

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
 * Show a toast notification
 * @param message The message to display
 * @param type The type of toast (success, error, info)
 * @param duration How long to show the toast in ms (default: varies by type)
 */
export function showToast(message: string, type: ToastType = 'info', duration?: number): void {
  // Clear existing timeout
  if (hideTimeout) {
    clearTimeout(hideTimeout);
    hideTimeout = null;
  }

  // Set the toast
  currentToast = { message, type };
  notifyListeners();

  // Auto-hide based on type
  const defaultDurations: Record<ToastType, number> = {
    success: 3000,
    error: 5000,
    info: 3000
  };

  const hideAfter = duration ?? defaultDurations[type];

  hideTimeout = setTimeout(() => {
    hideToast();
  }, hideAfter);
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
