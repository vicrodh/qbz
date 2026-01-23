<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    icon: Snippet;
    label: string;
    badge?: string;
    tooltip?: string;
    active?: boolean;
    onclick?: () => void;
    onHover?: () => void;
    class?: string;
    showLabel?: boolean;
  }

  let { icon, label, badge, tooltip, active = false, onclick, onHover, class: className = '', showLabel = true }: Props = $props();

  // When label is hidden, always show tooltip on hover
  const effectiveTooltip = $derived(showLabel ? tooltip : (tooltip || label));

  let showTooltip = $state(false);
  let tooltipTimeout: ReturnType<typeof setTimeout> | null = null;
  let buttonRef: HTMLButtonElement | null = null;
  let tooltipStyle = $state('');

  function handleMouseEnter() {
    // Call hover callback immediately (for lazy loading)
    onHover?.();

    if (!effectiveTooltip) return;
    tooltipTimeout = setTimeout(() => {
      updateTooltipPosition();
      showTooltip = true;
    }, 500); // Delay before showing tooltip
  }

  function handleMouseLeave() {
    if (tooltipTimeout) {
      clearTimeout(tooltipTimeout);
      tooltipTimeout = null;
    }
    showTooltip = false;
  }

  function updateTooltipPosition() {
    if (!buttonRef) return;
    const rect = buttonRef.getBoundingClientRect();
    tooltipStyle = `left: ${rect.right + 8}px; top: ${rect.top + rect.height / 2}px; transform: translateY(-50%);`;
  }
</script>

<button
  bind:this={buttonRef}
  {onclick}
  class="nav-item {className}"
  class:active
  class:collapsed={!showLabel}
  title={effectiveTooltip ? undefined : label}
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
>
  <div class="icon-container">
    {@render icon()}
  </div>
  {#if showLabel}
    <span class="label">{label}</span>
    {#if badge}
      <span class="badge">{badge}</span>
    {/if}
  {/if}
</button>

{#if showTooltip && effectiveTooltip}
  <div class="custom-tooltip" class:bold-first-line={!showLabel} style={tooltipStyle}>
    {effectiveTooltip}
  </div>
{/if}

<style>
  .nav-item {
    position: relative;
    width: 100%;
    height: 32px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px;
    border-radius: 6px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
    text-align: left;
  }

  .nav-item:hover {
    background-color: var(--bg-hover);
  }

  .nav-item.collapsed {
    justify-content: center;
    padding: 0;
  }

  .nav-item.active {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .icon-container {
    width: 14px;
    height: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .label {
    font-size: 13px;
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .badge {
    font-size: 11px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .custom-tooltip {
    position: fixed;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 8px 12px;
    font-size: 12px;
    color: var(--text-secondary);
    white-space: pre-line;
    line-height: 1.5;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    z-index: 10000;
    pointer-events: none;
    min-width: 120px;
    max-width: 200px;
  }

  .custom-tooltip.bold-first-line::first-line {
    font-weight: 600;
    color: var(--text-primary);
  }
</style>
