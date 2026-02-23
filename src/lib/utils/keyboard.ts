/**
 * Keyboard utilities for keybindings system
 */

/** Display mappings for special keys */
const KEY_DISPLAY_MAP: Record<string, string> = {
  ArrowLeft: '←',
  ArrowRight: '→',
  ArrowUp: '↑',
  ArrowDown: '↓',
  Space: 'Space',
  Escape: 'Esc',
  Enter: '↵',
  Backspace: '⌫',
  Delete: 'Del',
  Tab: 'Tab',
};

/**
 * Converts a KeyboardEvent to a normalized shortcut string
 * @example eventToShortcut({ key: 'ArrowRight', ctrlKey: true }) // 'Ctrl+ArrowRight'
 */
export function eventToShortcut(event: KeyboardEvent): string {
  const parts: string[] = [];

  if (event.ctrlKey || event.metaKey) parts.push('Ctrl');
  if (event.altKey) parts.push('Alt');

  // Normalize key
  let key = event.key;

  // Ignore if it's only a modifier key
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(key)) {
    return '';
  }

  // Normalize space
  if (key === ' ') key = 'Space';

  // Only include Shift when the key is a letter, digit, space, or named key (Arrow*, F1-F12, etc.)
  // For symbol keys like ?, !, @, #, the Shift is already "consumed" by producing the symbol
  if (event.shiftKey) {
    const isLetter = key.length === 1 && /[a-zA-Z]/.test(key);
    const isDigit = key.length === 1 && /[0-9]/.test(key);
    const isNamedKey = key.length > 1; // ArrowLeft, Space, Tab, F1, etc.
    if (isLetter || isDigit || isNamedKey) {
      parts.push('Shift');
    }
  }

  parts.push(key);
  return parts.join('+');
}

/**
 * Parses a shortcut string into its components
 * @example parseShortcut('Ctrl+Shift+F') // { ctrl: true, alt: false, shift: true, key: 'F' }
 */
export function parseShortcut(shortcut: string): {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  key: string;
} {
  const parts = shortcut.split('+');
  const key = parts.pop() || '';

  return {
    ctrl: parts.includes('Ctrl'),
    alt: parts.includes('Alt'),
    shift: parts.includes('Shift'),
    key,
  };
}

/**
 * Formats a shortcut for display in UI (with platform-specific symbols)
 * @example formatShortcutDisplay('Ctrl+ArrowRight') // '⌘ →' on macOS, 'Ctrl + →' on others
 */
export function formatShortcutDisplay(shortcut: string): string {
  const isMac =
    typeof navigator !== 'undefined' &&
    navigator.platform.toLowerCase().includes('mac');

  const { ctrl, alt, shift, key } = parseShortcut(shortcut);
  const parts: string[] = [];

  if (ctrl) parts.push(isMac ? '⌘' : 'Ctrl');
  if (alt) parts.push(isMac ? '⌥' : 'Alt');
  if (shift) parts.push(isMac ? '⇧' : 'Shift');

  const displayKey = KEY_DISPLAY_MAP[key] || key.toUpperCase();
  parts.push(displayKey);

  return parts.join(isMac ? ' ' : ' + ');
}

/**
 * Checks if an event matches a shortcut string
 */
export function eventMatchesShortcut(
  event: KeyboardEvent,
  shortcut: string
): boolean {
  const eventShortcut = eventToShortcut(event);
  return eventShortcut === shortcut;
}

/**
 * Checks if the event target is an input element (to ignore shortcuts)
 */
export function isInputTarget(event: KeyboardEvent): boolean {
  const target = event.target;
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}
