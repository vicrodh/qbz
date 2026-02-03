<script lang="ts">
  import { RotateCcw } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    eventToShortcut,
    formatShortcutDisplay,
    getConflictingAction,
    type KeybindingAction,
  } from '$lib/stores/keybindingsStore';

  interface Props {
    action: KeybindingAction;
    currentShortcut: string;
    defaultShortcut: string;
    onShortcutChange: (newShortcut: string) => void;
    onReset: () => void;
  }

  let {
    action,
    currentShortcut,
    defaultShortcut,
    onShortcutChange,
    onReset,
  }: Props = $props();

  let isRecording = $state(false);
  let recordedShortcut = $state('');
  let conflict = $state<KeybindingAction | null>(null);
  let inputElement: HTMLButtonElement | null = $state(null);

  const isModified = $derived(currentShortcut !== defaultShortcut);
  const displayShortcut = $derived(
    isRecording
      ? (recordedShortcut || $t('keybindings.pressKeys') || 'Press keys...')
      : formatShortcutDisplay(currentShortcut)
  );

  function startRecording() {
    isRecording = true;
    recordedShortcut = '';
    conflict = null;
    inputElement?.focus();
  }

  function stopRecording() {
    isRecording = false;
    recordedShortcut = '';
    conflict = null;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!isRecording) return;

    e.preventDefault();
    e.stopPropagation();

    // Ignore modifier keys alone
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) {
      return;
    }

    const shortcut = eventToShortcut(e);
    if (!shortcut) return;

    recordedShortcut = formatShortcutDisplay(shortcut);

    // Check for conflict
    const conflicting = getConflictingAction(shortcut, action.id);
    if (conflicting) {
      conflict = conflicting;
    } else {
      conflict = null;
      // Apply the change
      onShortcutChange(shortcut);
      stopRecording();
    }
  }

  function handleBlur() {
    // Small delay to allow button clicks
    setTimeout(() => {
      if (isRecording) {
        stopRecording();
      }
    }, 150);
  }

  function handleReset() {
    onReset();
    conflict = null;
  }

  // Get i18n action label
  function getActionLabel(): string {
    const parts = action.id.split('.');
    if (parts.length === 2) {
      const key = parts[0] + parts[1].charAt(0).toUpperCase() + parts[1].slice(1);
      const translated = $t(`keybindings.actions.${key}`);
      if (translated && !translated.startsWith('keybindings.actions.')) {
        return translated;
      }
    }
    return action.label;
  }
</script>

<div class="shortcut-input-wrapper">
  <div class="action-info">
    <span class="action-label">{getActionLabel()}</span>
  </div>

  <div class="input-row">
    <button
      bind:this={inputElement}
      class="shortcut-input"
      class:recording={isRecording}
      class:conflict={conflict !== null}
      class:modified={isModified && !isRecording}
      onclick={startRecording}
      onkeydown={handleKeydown}
      onblur={handleBlur}
    >
      <span class="shortcut-text">{displayShortcut}</span>
      {#if isModified && !isRecording}
        <span class="modified-indicator" title={$t('keybindings.modified') || 'Modified'}>*</span>
      {/if}
    </button>

    <button
      class="reset-btn"
      onclick={handleReset}
      disabled={!isModified}
      title={$t('keybindings.resetToDefault') || `Reset to default (${formatShortcutDisplay(defaultShortcut)})`}
    >
      <RotateCcw size={14} />
    </button>
  </div>

  {#if conflict}
    <div class="conflict-message">
      {$t('keybindings.conflict', { values: { shortcut: recordedShortcut, action: conflict.label } }) ||
       `"${recordedShortcut}" is already used by "${conflict.label}"`}
    </div>
  {/if}
</div>

<style>
  .shortcut-input-wrapper {
    margin-bottom: 16px;
  }

  .action-info {
    margin-bottom: 6px;
  }

  .action-label {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .input-row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .shortcut-input {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: var(--bg-tertiary);
    border: 2px solid transparent;
    border-radius: 8px;
    font-family: var(--font-mono, monospace);
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
    transition: all 150ms ease;
    min-height: 42px;
  }

  .shortcut-input:hover {
    background: var(--bg-hover);
    border-color: var(--border-subtle);
  }

  .shortcut-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .shortcut-input.recording {
    border-color: var(--accent-primary);
    background: var(--bg-hover);
    animation: pulse 1.5s ease-in-out infinite;
  }

  .shortcut-input.conflict {
    border-color: var(--color-error, #ef4444);
  }

  .shortcut-input.modified {
    border-color: var(--accent-primary);
    border-style: dashed;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.7; }
  }

  .shortcut-text {
    color: inherit;
  }

  .modified-indicator {
    color: var(--accent-primary);
    font-size: 14px;
    font-weight: bold;
  }

  .reset-btn {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .reset-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .reset-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .conflict-message {
    margin-top: 6px;
    padding: 8px 12px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 6px;
    font-size: 12px;
    color: var(--color-error, #ef4444);
  }
</style>
