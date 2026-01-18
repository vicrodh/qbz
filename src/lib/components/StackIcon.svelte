<script lang="ts">
  import { Layers } from 'lucide-svelte';
  import { subscribe, getCurrentContext, getContextDisplayInfo } from '$lib/stores/playbackContextStore';
  import { subscribe as subscribePrefs, getCachedPreferences } from '$lib/stores/playbackPreferencesStore';
  
  interface Props {
    size?: number;
    class?: string;
    onClick?: () => void;
  }
  
  let { size = 16, class: className = '', onClick }: Props = $props();
  
  let context = $state(getCurrentContext());
  let displayInfo = $state(context ? getContextDisplayInfo() : null);
  let showIcon = $state(getCachedPreferences().show_context_icon);
  let isHovering = $state(false);

  function handleClick(e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    console.log('[StackIcon] Clicked, onClick=', onClick, 'typeof=', typeof onClick);
    if (onClick) {
      console.log('[StackIcon] Calling onClick callback');
      onClick();
    } else {
      console.warn('[StackIcon] No onClick callback provided');
    }
  }

  // Subscribe to context changes
  $effect(() => {
    const unsubscribe = subscribe(() => {
      const newContext = getCurrentContext();
      const newDisplayInfo = newContext ? getContextDisplayInfo() : null;
      context = newContext;
      displayInfo = newDisplayInfo;
    });

    return () => {
      unsubscribe();
    };
  });

  // Subscribe to preferences changes
  $effect(() => {
    const unsubscribe = subscribePrefs(() => {
      showIcon = getCachedPreferences().show_context_icon;
    });

    return () => {
      unsubscribe();
    };
  });
</script>

{#if context && displayInfo && showIcon}
  <button
    class="stack-icon-wrapper {className}"
    onclick={handleClick}
    onmouseenter={() => isHovering = true}
    onmouseleave={() => isHovering = false}
    type="button"
  >
    <Layers size={size} strokeWidth={2} />
    
    {#if isHovering}
      <div class="context-tooltip">
        <div class="tooltip-text">Playing from</div>
        <div class="tooltip-context">{displayInfo}</div>
        <div class="tooltip-hint">Click to navigate</div>
      </div>
    {/if}
  </button>
{/if}

<style>
  .stack-icon-wrapper {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-secondary);
    opacity: 0.7;
    transition: all 0.2s;
    flex-shrink: 0;
    background: none;
    border: none;
    padding: 4px;
    border-radius: 4px;
    cursor: pointer;
    position: relative;
  }
  
  .stack-icon-wrapper:hover {
    opacity: 1;
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-primary);
  }

  .stack-icon-wrapper:active {
    transform: scale(0.95);
  }

  .context-tooltip {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    background: rgba(20, 20, 20, 0.95);
    backdrop-filter: blur(10px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    padding: 8px 12px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    white-space: nowrap;
    z-index: 1000;
    pointer-events: none;
  }

  .tooltip-text {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.6);
    margin-bottom: 2px;
  }

  .tooltip-context {
    font-size: 13px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.9);
    margin-bottom: 2px;
  }

  .tooltip-hint {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.5);
    font-style: italic;
  }
</style>
