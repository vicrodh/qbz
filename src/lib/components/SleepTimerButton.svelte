<script lang="ts">
  import { Clock } from 'lucide-svelte';
  import { t } from 'svelte-i18n';
  import { fade } from 'svelte/transition';
  import Dropdown from './Dropdown.svelte';
  import {
    sleepTimer,
    sleepTimerRemainingSec,
    setSleepTimer,
    cancelSleepTimer,
    formatSleepTimerRemaining,
    SLEEP_TIMER_PRESETS_MIN,
    SLEEP_TIMER_CUSTOM_MIN_LIMIT,
    SLEEP_TIMER_CUSTOM_MAX_LIMIT
  } from '$lib/stores/sleepTimerStore';

  let popoverOpen = $state(false);
  let popoverEl: HTMLDivElement | null = $state(null);
  let wrapEl: HTMLDivElement | null = $state(null);

  const CUSTOM_KEY = 'custom';

  // Preset options live as objects with stable keys; the Dropdown receives
  // localized labels and we resolve the selection back to the key by
  // matching the label string. Avoids storing $t() calls inside reactive
  // expressions (per ADR-001 / global i18n rule).
  type PresetEntry = { key: string; minutes: number | null; labelKey: string };
  const presetEntries: PresetEntry[] = [
    ...SLEEP_TIMER_PRESETS_MIN.map((minutes) => ({
      key: `m${minutes}`,
      minutes,
      labelKey:
        minutes < 60
          ? 'player.sleepTimer.preset.minutes'
          : 'player.sleepTimer.preset.hours'
    })),
    { key: CUSTOM_KEY, minutes: null, labelKey: 'player.sleepTimer.preset.custom' }
  ];

  function presetLabel(entry: PresetEntry): string {
    if (entry.key === CUSTOM_KEY) return $t(entry.labelKey);
    if (entry.minutes == null) return '';
    if (entry.minutes < 60) {
      return $t('player.sleepTimer.preset.minutes', { values: { minutes: entry.minutes } });
    }
    const hours = entry.minutes / 60;
    return $t('player.sleepTimer.preset.hours', { values: { hours } });
  }

  let selectedKey = $state<string>(presetEntries[0].key);
  let customMinutes = $state<number>(60);

  function handlePresetChange(label: string) {
    const match = presetEntries.find((entry) => presetLabel(entry) === label);
    if (match) selectedKey = match.key;
  }

  function commitTimer() {
    let minutes: number;
    if (selectedKey === CUSTOM_KEY) {
      minutes = Math.min(
        SLEEP_TIMER_CUSTOM_MAX_LIMIT,
        Math.max(SLEEP_TIMER_CUSTOM_MIN_LIMIT, Math.floor(customMinutes || 0))
      );
    } else {
      const entry = presetEntries.find((e) => e.key === selectedKey);
      if (!entry || entry.minutes == null) return;
      minutes = entry.minutes;
    }
    setSleepTimer(minutes);
    popoverOpen = false;
  }

  function handleCancel() {
    cancelSleepTimer();
    popoverOpen = false;
  }

  function handleEdit() {
    // When editing an active timer, prefill the form with its current duration.
    const current = $sleepTimer.durationMin;
    if (current != null) {
      const matchedPreset = presetEntries.find(
        (entry) => entry.minutes === current
      );
      if (matchedPreset) {
        selectedKey = matchedPreset.key;
      } else {
        selectedKey = CUSTOM_KEY;
        customMinutes = current;
      }
    }
    // The popover stays open; the active-timer view is gated on $sleepTimer.active,
    // so cancelling the active timer first lets the form render.
    cancelSleepTimer();
  }

  function togglePopover() {
    popoverOpen = !popoverOpen;
  }

  function handleClickOutside(event: MouseEvent) {
    if (!popoverOpen) return;
    const target = event.target as Node;
    if (popoverEl && popoverEl.contains(target)) return;
    if (wrapEl && wrapEl.contains(target)) return;
    popoverOpen = false;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!popoverOpen) return;
    if (event.key === 'Escape') {
      event.stopPropagation();
      popoverOpen = false;
    }
  }

  $effect(() => {
    if (!popoverOpen) return;
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleKeydown);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleKeydown);
    };
  });

  const remainingLabel = $derived(formatSleepTimerRemaining($sleepTimerRemainingSec));
  const isActive = $derived($sleepTimer.active);

  // Compute the dropdown's current label from the selected key.
  const currentPresetLabel = $derived.by(() => {
    const entry = presetEntries.find((e) => e.key === selectedKey) ?? presetEntries[0];
    return presetLabel(entry);
  });

  const presetOptions = $derived(presetEntries.map((entry) => presetLabel(entry)));
</script>

<div class="sleep-timer-wrap" bind:this={wrapEl}>
  <button
    type="button"
    class="footer-icon-btn"
    class:active={isActive}
    onclick={togglePopover}
    aria-haspopup="dialog"
    aria-expanded={popoverOpen}
    title={isActive
      ? $t('player.sleepTimer.tooltipActive', { values: { remaining: remainingLabel } })
      : $t('player.sleepTimer.tooltipIdle')}
  >
    <Clock size={18} />
  </button>
  {#if isActive}
    <span class="countdown" aria-live="polite">{remainingLabel}</span>
  {/if}

  {#if popoverOpen}
    <div
      class="popover"
      bind:this={popoverEl}
      role="dialog"
      aria-label={$t('player.sleepTimer.title')}
      transition:fade={{ duration: 120 }}
    >
      {#if isActive}
        <div class="popover-row">
          <span class="popover-label">{$t('player.sleepTimer.activeLabel')}</span>
          <span class="popover-countdown">{remainingLabel}</span>
        </div>
        <div class="popover-actions">
          <button type="button" class="action-secondary" onclick={handleEdit}>
            {$t('player.sleepTimer.edit')}
          </button>
          <button type="button" class="action-primary" onclick={handleCancel}>
            {$t('player.sleepTimer.cancel')}
          </button>
        </div>
      {:else}
        <div class="popover-row">
          <span class="popover-label">{$t('player.sleepTimer.stopAfter')}</span>
        </div>
        <Dropdown
          value={currentPresetLabel}
          options={presetOptions}
          onchange={handlePresetChange}
          expandLeft
          compact
        />
        {#if selectedKey === CUSTOM_KEY}
          <label class="custom-row">
            <span class="popover-label custom-label">
              {$t('player.sleepTimer.customMinutes')}
            </span>
            <input
              type="number"
              class="custom-input"
              min={SLEEP_TIMER_CUSTOM_MIN_LIMIT}
              max={SLEEP_TIMER_CUSTOM_MAX_LIMIT}
              step="5"
              bind:value={customMinutes}
            />
          </label>
        {/if}
        <div class="popover-actions single">
          <button type="button" class="action-primary" onclick={commitTimer}>
            {$t('player.sleepTimer.set')}
          </button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .sleep-timer-wrap {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .footer-icon-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 6px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background-color 120ms ease, color 120ms ease;
  }

  .footer-icon-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .footer-icon-btn.active {
    color: var(--color-success, #22c55e);
  }

  .countdown {
    font-size: 11px;
    font-weight: 500;
    color: var(--color-success, #22c55e);
    font-variant-numeric: tabular-nums;
    line-height: 1;
    user-select: none;
    pointer-events: none;
  }

  .popover {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 100;
    width: 220px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
  }

  .popover-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .popover-label {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .popover-countdown {
    font-size: 16px;
    font-weight: 600;
    color: var(--color-success, #22c55e);
    font-variant-numeric: tabular-nums;
  }

  .custom-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .custom-label {
    flex: 1;
  }

  .custom-input {
    width: 80px;
    padding: 6px 8px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .custom-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .popover-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .popover-actions.single {
    justify-content: flex-end;
  }

  .action-primary,
  .action-secondary {
    padding: 6px 12px;
    border-radius: 4px;
    border: 1px solid transparent;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 120ms ease, border-color 120ms ease;
  }

  .action-primary {
    background: var(--accent-primary);
    color: var(--bg-primary);
  }

  .action-primary:hover {
    background: var(--accent-hover);
  }

  .action-secondary {
    background: transparent;
    border-color: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .action-secondary:hover {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }
</style>
