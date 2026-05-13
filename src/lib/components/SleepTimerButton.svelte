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
  let popoverPos = $state<{ top: number; left: number } | null>(null);

  // Width is fixed; height is estimated for the worst case (active timer
  // and the Custom row both fit comfortably under it). Used only to decide
  // whether the popover fits above the trigger; final size is determined
  // by content.
  const POPOVER_WIDTH = 232;
  const POPOVER_HEIGHT_ESTIMATE = 180;
  const POPOVER_GAP = 8;
  const VIEWPORT_PAD = 12;

  function computePopoverPos() {
    if (!wrapEl) return;
    const rect = wrapEl.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    // Vertical: prefer above. If there's not enough headroom, flip below.
    // If neither side fits, anchor to whichever has more space and clamp.
    const spaceAbove = rect.top - VIEWPORT_PAD;
    const spaceBelow = vh - rect.bottom - VIEWPORT_PAD;
    let top: number;
    if (spaceAbove >= POPOVER_HEIGHT_ESTIMATE + POPOVER_GAP) {
      top = rect.top - POPOVER_HEIGHT_ESTIMATE - POPOVER_GAP;
    } else if (spaceBelow >= POPOVER_HEIGHT_ESTIMATE + POPOVER_GAP) {
      top = rect.bottom + POPOVER_GAP;
    } else if (spaceAbove >= spaceBelow) {
      top = Math.max(VIEWPORT_PAD, rect.top - POPOVER_HEIGHT_ESTIMATE - POPOVER_GAP);
    } else {
      top = Math.min(vh - POPOVER_HEIGHT_ESTIMATE - VIEWPORT_PAD, rect.bottom + POPOVER_GAP);
    }

    // Horizontal: prefer left-aligned to the trigger. Shift inward if it
    // would overflow either edge.
    let left = rect.left;
    if (left + POPOVER_WIDTH > vw - VIEWPORT_PAD) {
      left = vw - POPOVER_WIDTH - VIEWPORT_PAD;
    }
    if (left < VIEWPORT_PAD) left = VIEWPORT_PAD;

    popoverPos = { top, left };
  }

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
    if (!popoverOpen) {
      computePopoverPos();
    }
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

  function handleViewportChange() {
    if (!popoverOpen) return;
    computePopoverPos();
  }

  $effect(() => {
    if (!popoverOpen) return;
    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleKeydown);
    window.addEventListener('resize', handleViewportChange);
    // Capture-phase scroll catches scroll on any ancestor (queue list,
    // page body, etc) so the popover stays anchored to its trigger.
    window.addEventListener('scroll', handleViewportChange, true);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleKeydown);
      window.removeEventListener('resize', handleViewportChange);
      window.removeEventListener('scroll', handleViewportChange, true);
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

  {#if popoverOpen && popoverPos}
    <div
      class="popover"
      bind:this={popoverEl}
      role="dialog"
      aria-label={$t('player.sleepTimer.title')}
      style="top: {popoverPos.top}px; left: {popoverPos.left}px;"
      transition:fade={{ duration: 120 }}
    >
      {#if isActive}
        <div class="popover-section">
          <span class="popover-eyebrow">{$t('player.sleepTimer.activeLabel')}</span>
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
        <div class="popover-section">
          <span class="popover-eyebrow">{$t('player.sleepTimer.stopAfter')}</span>
          <Dropdown
            value={currentPresetLabel}
            options={presetOptions}
            onchange={handlePresetChange}
            expandLeft
            compact
          />
        </div>
        {#if selectedKey === CUSTOM_KEY}
          <label class="popover-section custom-section">
            <span class="popover-eyebrow">
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
    pointer-events: none;
  }

  .popover {
    position: fixed;
    z-index: 100;
    width: 232px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.32);
  }

  /* Stacked label-above-control rhythm; both rows share the same scale so
     no single element dominates. Eyebrow (small caps-ish, muted) marks
     section context, control sits underneath at body size. */
  .popover-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .popover-eyebrow {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    letter-spacing: 0.02em;
    line-height: 1.2;
  }

  .popover-countdown {
    font-size: 15px;
    font-weight: 600;
    color: var(--color-success, #22c55e);
    font-variant-numeric: tabular-nums;
    line-height: 1.2;
  }

  .custom-section {
    align-items: stretch;
  }

  .custom-input {
    height: 30px;
    padding: 0 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    text-align: right;
    width: 100%;
    box-sizing: border-box;
  }

  .custom-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .popover-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 2px;
  }

  .action-primary,
  .action-secondary {
    height: 28px;
    padding: 0 12px;
    border-radius: 4px;
    border: 1px solid transparent;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    transition: background-color 120ms ease, border-color 120ms ease, color 120ms ease;
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

  /* Bring the Dropdown trigger down to the popover's body scale so it
     stops being the visual peak of the stack. The default Dropdown is
     designed for settings pages where it's the primary control; here it
     shares space with eyebrow labels and a primary button. */
  .popover :global(.dropdown .trigger) {
    width: 100%;
    height: 30px;
    padding: 0 10px;
    font-size: 13px;
    font-weight: 500;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 4px;
  }

  .popover :global(.dropdown .trigger:hover) {
    background: var(--bg-tertiary);
  }

  .popover :global(.dropdown.wide .trigger) {
    width: 100%;
  }

  .popover :global(.dropdown .value-text) {
    font-weight: 500;
  }
</style>
