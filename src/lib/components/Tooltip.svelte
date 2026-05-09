<script lang="ts">
  import { Info } from 'lucide-svelte';

  interface Props {
    text: string;
  }

  let { text }: Props = $props();

  let isVisible = $state(false);
</script>

<!-- The tooltip wrapper is a purely visual hit area; the inner popover
     carries the actual tooltip semantics. The linter doesn't recognise
     this pattern, hence the local ignore. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="tooltip-container"
  onmouseenter={() => (isVisible = true)}
  onmouseleave={() => (isVisible = false)}
>
  <Info size={14} class="icon" />
  {#if isVisible}
    <div class="tooltip">
      {text}
    </div>
  {/if}
</div>

<style>
  .tooltip-container {
    position: relative;
    display: inline-flex;
    align-items: center;
    cursor: help;
  }

  .tooltip-container :global(.icon) {
    color: var(--text-muted);
  }

  .tooltip {
    position: absolute;
    bottom: 100%;
    left: 50%;
    transform: translateX(-50%);
    margin-bottom: 8px;
    padding: 8px 12px;
    background-color: var(--bg-tertiary);
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    white-space: nowrap;
    max-width: 200px;
    white-space: normal;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    z-index: 100;
  }
</style>
