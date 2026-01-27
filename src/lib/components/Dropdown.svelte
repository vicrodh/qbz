<script lang="ts">
  import { ChevronDown } from 'lucide-svelte';
  import {
    openMenu as openGlobalMenu,
    closeMenu as closeGlobalMenu,
    subscribe as subscribeFloatingMenu,
    getActiveMenuId,
    MENU_INACTIVITY_TIMEOUT
  } from '$lib/stores/floatingMenuStore';

  interface Props {
    value: string;
    options: string[];
    onchange: (value: string) => void;
    wide?: boolean;        // For long device names
    expandLeft?: boolean;  // Expand menu to the left
    compact?: boolean;     // Smaller text in options
  }

  let { value, options, onchange, wide = false, expandLeft = false, compact = false }: Props = $props();

  let isOpen = $state(false);
  let isHovering = $state(false);
  let dropdownRef: HTMLDivElement;

  // Unique ID for this dropdown instance
  const menuId = `dropdown-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`;

  function handleClickOutside(event: MouseEvent) {
    if (dropdownRef && !dropdownRef.contains(event.target as Node)) {
      closeDropdown();
    }
  }

  function openDropdown() {
    openGlobalMenu(menuId);
    isOpen = true;
  }

  function closeDropdown() {
    isOpen = false;
    closeGlobalMenu(menuId);
  }

  // Subscribe to global floating menu store
  $effect(() => {
    const unsubscribe = subscribeFloatingMenu(() => {
      const activeId = getActiveMenuId();
      if (activeId !== null && activeId !== menuId && isOpen) {
        isOpen = false;
      }
    });
    return unsubscribe;
  });

  $effect(() => {
    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);

      // Inactivity timeout
      let idleTimer: ReturnType<typeof setTimeout> | null = null;

      const scheduleIdleClose = () => {
        if (idleTimer) clearTimeout(idleTimer);
        idleTimer = setTimeout(() => {
          if (isOpen && !isHovering) closeDropdown();
        }, MENU_INACTIVITY_TIMEOUT);
      };

      if (!isHovering) scheduleIdleClose();

      const onActivity = () => {
        if (!isHovering) scheduleIdleClose();
      };

      window.addEventListener('pointermove', onActivity, true);

      return () => {
        document.removeEventListener('mousedown', handleClickOutside);
        window.removeEventListener('pointermove', onActivity, true);
        if (idleTimer) clearTimeout(idleTimer);
      };
    }
  });
</script>

<div class="dropdown" class:wide bind:this={dropdownRef}>
  <button class="trigger" onclick={() => isOpen ? closeDropdown() : openDropdown()}>
    <span class="value-text">{value}</span>
    <ChevronDown size={16} class="chevron" />
  </button>

  {#if isOpen}
    <div
      class="menu"
      class:expand-left={expandLeft}
      class:compact
      onmouseenter={() => isHovering = true}
      onmouseleave={() => isHovering = false}
    >
      {#each options as option}
        <button
          class="option"
          class:selected={option === value}
          onclick={() => {
            onchange(option);
            closeDropdown();
          }}
          title={option}
        >
          {option}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .dropdown {
    position: relative;
  }

  .dropdown.wide {
    min-width: 280px;
  }

  .trigger {
    height: 40px;
    min-width: 160px;
    width: 100%;
    padding: 0 16px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    border: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    font-size: 14px;
    color: var(--text-primary);
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .dropdown.wide .trigger {
    min-width: 280px;
  }

  .value-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    text-align: left;
  }

  .trigger:hover {
    background-color: var(--bg-hover);
  }

  .trigger :global(.chevron) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    min-width: 100%;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    overflow: hidden;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    z-index: 10000;
    max-height: 300px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--text-muted) transparent;
  }

  .menu::-webkit-scrollbar {
    width: 8px;
  }

  .menu::-webkit-scrollbar-track {
    background: transparent;
  }

  .menu::-webkit-scrollbar-thumb {
    background: var(--text-muted);
    border-radius: 9999px;
  }

  .menu::-webkit-scrollbar-thumb:hover {
    background: var(--text-secondary);
  }

  .menu.expand-left {
    left: auto;
    right: 0;
    min-width: 320px;
  }

  .option {
    width: 100%;
    padding: 12px 16px;
    text-align: left;
    font-size: 14px;
    color: var(--text-secondary);
    background: none;
    border: none;
    cursor: pointer;
    transition: background-color 150ms ease;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .menu.compact .option {
    padding: 10px 14px;
    font-size: 12px;
  }

  .option:hover {
    background-color: var(--bg-hover);
  }

  .option.selected {
    background-color: rgba(66, 133, 244, 0.15);
    color: var(--text-primary);
  }

  [data-theme="light"] .option.selected,
  [data-theme="warm"] .option.selected {
    background-color: rgba(var(--accent-primary), 0.15);
    color: var(--accent-primary);
  }
</style>
