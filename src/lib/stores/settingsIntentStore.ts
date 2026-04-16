/**
 * settingsIntentStore
 *
 * Cross-component cue for navigating to a specific Settings section and
 * optionally triggering an action on arrival (e.g. open the Logs modal
 * from the "Report Issue" pop-up).
 *
 * The intent is consumed (cleared) as soon as SettingsView applies it,
 * so subsequent visits to Settings use their normal default section.
 */

export interface SettingsIntent {
  section?: string;
  openLogs?: boolean;
}

let pending: SettingsIntent | null = null;
const listeners = new Set<() => void>();

export function setSettingsIntent(intent: SettingsIntent): void {
  pending = intent;
  for (const listener of listeners) listener();
}

export function consumeSettingsIntent(): SettingsIntent | null {
  const intent = pending;
  pending = null;
  return intent;
}

export function peekSettingsIntent(): SettingsIntent | null {
  return pending;
}

export function subscribeSettingsIntent(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
