<script lang="ts">
  import { ChevronDown, Search } from 'lucide-svelte';
  import { t } from '$lib/i18n';
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
  let menuRef = $state<HTMLDivElement | null>(null);
  let searchInputRef = $state<HTMLInputElement | null>(null);
  let searchQuery = $state('');

  // Unique ID for this dropdown instance
  const menuId = `dropdown-${Date.now()}-${Math.random().toString(36).substring(2, 9)}`;

  // Show search only when >5 options
  const showSearch = $derived(options.length > 5);

  // Filtered options based on search query
  const filteredOptions = $derived(
    searchQuery.trim() === ''
      ? options
      : options.filter(opt => opt.toLowerCase().includes(searchQuery.toLowerCase()))
  );

  // Fixed position in pixels (for position: fixed)
  let fixedPosition = $state<{ top: number; left: number; width: number } | null>(null);

  // Item height depends on compact mode
  const ITEM_HEIGHT_NORMAL = 40;
  const ITEM_HEIGHT_COMPACT = 36;
  const itemHeight = $derived(compact ? ITEM_HEIGHT_COMPACT : ITEM_HEIGHT_NORMAL);
  const SEARCH_HEIGHT = 48;
  const MENU_PADDING = 8;
  const MAX_VISIBLE_ITEMS = 4;
  const MENU_GAP = 4; // gap between trigger and menu

  // Calculate expected menu height before render
  const expectedMenuHeight = $derived(
    showSearch
      ? SEARCH_HEIGHT + (MAX_VISIBLE_ITEMS * itemHeight) + MENU_PADDING
      : Math.min(options.length, 8) * itemHeight + MENU_PADDING
  );

  function calculatePosition() {
    if (!dropdownRef) return;

    const triggerRect = dropdownRef.getBoundingClientRect();
    const viewportHeight = window.innerHeight;
    const viewportWidth = window.innerWidth;

    // Player height (bottom bar)
    const playerHeight = 104;
    const safeBottom = viewportHeight - playerHeight;

    // Get actual menu dimensions if available, otherwise use expected
    const menuHeight = menuRef?.offsetHeight || expectedMenuHeight;
    const menuWidth = menuRef?.offsetWidth || (wide ? 280 : 170);

    // Calculate available space
    const spaceBelow = safeBottom - triggerRect.bottom - MENU_GAP;
    const spaceAbove = triggerRect.top - MENU_GAP;

    // Determine vertical position
    let top: number;
    if (spaceBelow >= menuHeight) {
      // Fits below
      top = triggerRect.bottom + MENU_GAP;
    } else if (spaceAbove >= menuHeight) {
      // Fits above
      top = triggerRect.top - menuHeight - MENU_GAP;
    } else {
      // Not enough space, pick best option
      if (spaceBelow >= spaceAbove) {
        top = triggerRect.bottom + MENU_GAP;
      } else {
        top = triggerRect.top - menuHeight - MENU_GAP;
      }
    }

    // Determine horizontal position
    let left: number;
    if (expandLeft) {
      // Align right edge of menu with right edge of trigger
      left = triggerRect.right - menuWidth;
      // Clamp to viewport
      if (left < 8) left = 8;
    } else {
      // Align left edge of menu with left edge of trigger
      left = triggerRect.left;
      // Clamp to viewport
      if (left + menuWidth > viewportWidth - 8) {
        left = viewportWidth - menuWidth - 8;
      }
    }

    fixedPosition = { top, left, width: triggerRect.width };
  }

  function handleClickOutside(event: MouseEvent) {
    if (dropdownRef && !dropdownRef.contains(event.target as Node) &&
        menuRef && !menuRef.contains(event.target as Node)) {
      closeDropdown();
    }
  }

  function openDropdown() {
    // Calculate position BEFORE opening to prevent any layout shift
    calculatePosition();

    openGlobalMenu(menuId);
    isOpen = true;
    searchQuery = '';

    // Focus search input and recalculate after render
    requestAnimationFrame(() => {
      if (showSearch) {
        searchInputRef?.focus();
      }
      // Recalculate with actual menu dimensions
      calculatePosition();
    });
  }

  function closeDropdown() {
    isOpen = false;
    searchQuery = '';
    fixedPosition = null;
    closeGlobalMenu(menuId);
  }

  function handleOptionClick(option: string) {
    onchange(option);
    closeDropdown();
  }

  function handleKeyDown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      closeDropdown();
    } else if (event.key === 'Enter' && filteredOptions.length === 1) {
      handleOptionClick(filteredOptions[0]);
    }
  }

  // Subscribe to global floating menu store
  $effect(() => {
    const unsubscribe = subscribeFloatingMenu(() => {
      const activeId = getActiveMenuId();
      if (activeId !== null && activeId !== menuId && isOpen) {
        isOpen = false;
        searchQuery = '';
        fixedPosition = null;
      }
    });
    return unsubscribe;
  });

  $effect(() => {
    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);

      // Recalculate position on scroll/resize
      const recalc = () => calculatePosition();
      window.addEventListener('scroll', recalc, true);
      window.addEventListener('resize', recalc);

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
        window.removeEventListener('scroll', recalc, true);
        window.removeEventListener('resize', recalc);
        window.removeEventListener('pointermove', onActivity, true);
        if (idleTimer) clearTimeout(idleTimer);
      };
    }
  });

  // Menu max-height
  const menuMaxHeight = $derived(`${expectedMenuHeight}px`);
</script>

<div class="dropdown" class:wide bind:this={dropdownRef}>
  <button class="trigger" onclick={() => isOpen ? closeDropdown() : openDropdown()}>
    <span class="value-text">{value}</span>
    <ChevronDown size={16} class="chevron" />
  </button>
</div>

{#if isOpen && fixedPosition}
  <div
    class="menu"
    class:compact
    class:searchable={showSearch}
    bind:this={menuRef}
    onmouseenter={() => isHovering = true}
    onmouseleave={() => isHovering = false}
    style:top="{fixedPosition.top}px"
    style:left="{fixedPosition.left}px"
    style:min-width="{fixedPosition.width}px"
    style:max-height={menuMaxHeight}
  >
    {#if showSearch}
      <div class="search-container">
        <Search size={14} class="search-icon" />
        <input
          bind:this={searchInputRef}
          type="text"
          class="search-input"
          placeholder={$t('placeholders.search')}
          bind:value={searchQuery}
          onkeydown={handleKeyDown}
        />
      </div>
    {/if}
    <div
      class="options-container"
      class:with-search={showSearch}
      style:max-height={showSearch ? `${MAX_VISIBLE_ITEMS * itemHeight}px` : undefined}
    >
      {#each filteredOptions as option}
        <button
          class="option"
          class:selected={option === value}
          onclick={() => handleOptionClick(option)}
          title={option}
        >
          {option}
        </button>
      {:else}
        <div class="no-results">No matches found</div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .dropdown {
    position: relative;
  }

  .dropdown.wide {
    min-width: 280px;
  }

  .trigger {
    height: 40px;
    width: 170px;
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
    width: 280px;
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
    position: fixed;
    width: max-content;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10000;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .search-container {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
    flex-shrink: 0;
  }

  .search-container :global(.search-icon) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 13px;
    padding: 0;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .options-container {
    overflow-y: auto;
    overflow-x: hidden;
    scroll-snap-type: y mandatory;
    scrollbar-width: thin;
    scrollbar-color: var(--text-muted) transparent;
    flex: 1;
    min-height: 0;
  }

  .options-container::-webkit-scrollbar {
    width: 6px;
  }

  .options-container::-webkit-scrollbar-track {
    background: transparent;
  }

  .options-container::-webkit-scrollbar-thumb {
    background: var(--text-muted);
    border-radius: 9999px;
  }

  .options-container::-webkit-scrollbar-thumb:hover {
    background: var(--text-secondary);
  }

  .option {
    width: 100%;
    height: 40px;
    padding: 0 16px;
    display: flex;
    align-items: center;
    text-align: left;
    font-size: 13px;
    color: var(--text-secondary);
    background: none;
    border: none;
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
    white-space: nowrap;
    scroll-snap-align: start;
    flex-shrink: 0;
  }

  .menu.compact .option {
    height: 36px;
    padding: 0 12px;
    font-size: 12px;
  }

  .option:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .option.selected {
    background-color: rgba(66, 133, 244, 0.15);
    color: var(--text-primary);
  }

  .no-results {
    padding: 12px 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
