<script lang="ts">
  import { tick } from 'svelte';
  import { SlidersHorizontal, X, Minus, Check } from 'lucide-svelte';
  import {
    getAvailableGenres,
    getChildGenres,
    toggleGenre,
    clearSelection,
    hasActiveFilter,
    setRememberSelection,
    getGenreFilterState,
    subscribe as subscribeGenre,
    type GenreInfo,
    type GenreFilterContext
  } from '$lib/stores/genreFilterStore';

  type DropdownAlign = 'left' | 'right';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    anchorEl?: HTMLElement | null;
    context?: GenreFilterContext;
    align?: DropdownAlign;
  }

  let { isOpen, onClose, anchorEl = null, context = 'home', align = 'left' }: Props = $props();

  let parentGenres = $state<GenreInfo[]>([]);
  let allGenres = $state<GenreInfo[]>([]);
  let selectedIds = $state<Set<number>>(new Set());
  let rememberSelection = $state(true);
  let showAllGenres = $state(false);
  let popupEl: HTMLDivElement | null = null;
  let popupStyle = $state('');

  // Subscribe to store changes for this context
  $effect(() => {
    const unsubscribe = subscribeGenre(() => {
      const state = getGenreFilterState(context);
      parentGenres = state.availableGenres;
      allGenres = state.allGenres;
      selectedIds = state.selectedGenreIds;
      rememberSelection = state.rememberSelection;
    }, context);

    // Initial load
    const state = getGenreFilterState(context);
    parentGenres = state.availableGenres;
    allGenres = state.allGenres;
    selectedIds = state.selectedGenreIds;
    rememberSelection = state.rememberSelection;

    return unsubscribe;
  });

  // Position popup when opening
  $effect(() => {
    if (isOpen && anchorEl) {
      positionPopup();
    }
  });

  async function positionPopup() {
    await tick();
    if (!anchorEl || !popupEl) return;

    const anchorRect = anchorEl.getBoundingClientRect();
    const popupRect = popupEl.getBoundingClientRect();

    let left: number;
    let top = anchorRect.bottom + 8;

    if (align === 'right') {
      left = anchorRect.left;
      if (left + popupRect.width > window.innerWidth - 8) {
        left = window.innerWidth - popupRect.width - 8;
      }
    } else {
      left = anchorRect.right - popupRect.width;
      if (left < 8) left = 8;
    }

    if (top + popupRect.height > window.innerHeight - 8) {
      top = anchorRect.top - popupRect.height - 8;
    }

    popupStyle = `left: ${left}px; top: ${top}px;`;
  }

  function handleGenreClick(genreId: number) {
    toggleGenre(genreId, context);
  }

  // Get parent selection state: 'all' | 'none' | 'partial'
  function getParentState(parentId: number): 'all' | 'none' | 'partial' {
    const children = getChildGenres(parentId);
    if (children.length === 0) {
      // No children, just check parent itself
      return selectedIds.has(parentId) ? 'all' : 'none';
    }

    const selectedCount = children.filter(c => selectedIds.has(c.id)).length;
    if (selectedCount === 0) return 'none';
    if (selectedCount === children.length) return 'all';
    return 'partial';
  }

  // Toggle parent: if all selected, deselect all; otherwise select all
  function handleParentClick(parentId: number) {
    const children = getChildGenres(parentId);
    const currentState = getParentState(parentId);

    if (children.length === 0) {
      // No children, just toggle parent
      toggleGenre(parentId, context);
      return;
    }

    if (currentState === 'all') {
      // Deselect all children
      for (const child of children) {
        if (selectedIds.has(child.id)) {
          toggleGenre(child.id, context);
        }
      }
    } else {
      // Select all children
      for (const child of children) {
        if (!selectedIds.has(child.id)) {
          toggleGenre(child.id, context);
        }
      }
    }
  }

  function handleClearAll() {
    clearSelection(context);
    onClose();
  }

  function handleRememberToggle() {
    setRememberSelection(!rememberSelection, context);
  }

  function handleClickOutside(event: MouseEvent) {
    if (popupEl && !popupEl.contains(event.target as Node) &&
        anchorEl && !anchorEl.contains(event.target as Node)) {
      onClose();
    }
  }

  $effect(() => {
    if (isOpen) {
      document.addEventListener('click', handleClickOutside);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  });

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      onClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div
    class="genre-popup"
    class:expanded={showAllGenres}
    bind:this={popupEl}
    style={popupStyle}
  >
    <div class="popup-header">
      <div class="header-title">
        <SlidersHorizontal size={16} />
        <span>Filter by genre</span>
      </div>
      <button class="close-btn" onclick={onClose} type="button">
        <X size={16} />
      </button>
    </div>

    <div class="options-row">
      <div class="option-item">
        <span>Remember selection</span>
        <button
          class="toggle-switch"
          class:active={rememberSelection}
          onclick={handleRememberToggle}
          type="button"
          aria-pressed={rememberSelection}
        >
          <span class="toggle-thumb"></span>
        </button>
      </div>
      <div class="option-item">
        <span>Show all sub-genres</span>
        <button
          class="toggle-switch"
          class:active={showAllGenres}
          onclick={() => showAllGenres = !showAllGenres}
          type="button"
          aria-pressed={showAllGenres}
        >
          <span class="toggle-thumb"></span>
        </button>
      </div>
    </div>

    <div class="genres-container">
      {#if showAllGenres}
        <!-- Hierarchical view -->
        {#each parentGenres as parent (parent.id)}
          {@const children = getChildGenres(parent.id)}
          {@const parentState = getParentState(parent.id)}
          <div class="genre-group">
            <button
              class="parent-row"
              class:selected={parentState === 'all'}
              class:partial={parentState === 'partial'}
              onclick={() => handleParentClick(parent.id)}
              type="button"
            >
              <span class="check-box" class:checked={parentState === 'all'} class:partial={parentState === 'partial'}>
                {#if parentState === 'all'}
                  <Check size={10} strokeWidth={3} />
                {:else if parentState === 'partial'}
                  <Minus size={10} strokeWidth={3} />
                {/if}
              </span>
              <span class="parent-name">{parent.name}</span>
              {#if children.length > 0}
                <span class="child-count">{children.length}</span>
              {/if}
            </button>
            {#if children.length > 0}
              <div class="children-grid">
                {#each children as child (child.id)}
                  <button
                    class="child-card"
                    class:selected={selectedIds.has(child.id)}
                    onclick={() => handleGenreClick(child.id)}
                    type="button"
                  >
                    <span class="genre-name">{child.name}</span>
                    <span class="check-circle" class:checked={selectedIds.has(child.id)}></span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      {:else}
        <!-- Simple grid view (parents only) -->
        <div class="genres-grid">
          {#each parentGenres as genre (genre.id)}
            <button
              class="genre-card"
              class:selected={selectedIds.has(genre.id)}
              onclick={() => handleGenreClick(genre.id)}
              type="button"
            >
              <span class="genre-name">{genre.name}</span>
              <span class="check-circle" class:checked={selectedIds.has(genre.id)}></span>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="popup-footer">
      <button
        class="clear-btn"
        onclick={handleClearAll}
        type="button"
        disabled={!hasActiveFilter(context)}
      >
        Clear filter
      </button>
    </div>
  </div>
{/if}

<style>
  .genre-popup {
    position: fixed;
    z-index: 10000;
    width: 530px;
    max-height: 500px;
    background: var(--bg-primary);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .genre-popup.expanded {
    max-height: 650px;
  }

  .popup-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    width: 28px;
    height: 28px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 6px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .close-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .options-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 16px;
    gap: 24px;
    font-size: 12px;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border-subtle);
  }

  .option-item {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .toggle-switch {
    width: 36px;
    height: 20px;
    border-radius: 10px;
    background: var(--bg-tertiary);
    border: none;
    cursor: pointer;
    position: relative;
    transition: background 150ms ease;
  }

  .toggle-switch.active {
    background: var(--accent-primary);
  }

  .toggle-thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: white;
    transition: transform 150ms ease;
  }

  .toggle-switch.active .toggle-thumb {
    transform: translateX(16px);
  }

  .genres-container {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
  }

  /* Simple grid view */
  .genres-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }

  .genre-card {
    position: relative;
    height: 36px;
    border-radius: 6px;
    border: 1px solid var(--border-subtle);
    cursor: pointer;
    overflow: hidden;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 10px;
    transition: all 150ms ease;
  }

  .genre-card:hover {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  .genre-card.selected {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .genre-card.selected:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  .genre-name {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.2;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .genre-card.selected .genre-name,
  .child-card.selected .genre-name {
    color: white;
  }

  .check-circle {
    flex-shrink: 0;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1.5px solid var(--text-muted);
    background: transparent;
    transition: all 150ms ease;
    position: relative;
  }

  .check-circle.checked {
    border-color: white;
    background: white;
  }

  .check-circle.checked::after {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: 4px;
    height: 7px;
    border: solid var(--accent-primary);
    border-width: 0 1.5px 1.5px 0;
    transform: translate(-50%, -60%) rotate(45deg);
  }

  /* Hierarchical view */
  .genre-group {
    margin-bottom: 12px;
  }

  .genre-group:last-child {
    margin-bottom: 0;
  }

  .parent-row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .parent-row:hover {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  .parent-row.selected {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .parent-row.partial {
    border-color: var(--accent-primary);
  }

  .parent-row.selected:hover {
    background: var(--accent-hover);
  }

  .check-box {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    border-radius: 4px;
    border: 1.5px solid var(--text-muted);
    background: transparent;
    display: flex;
    align-items: center;
    justify-content: center;
    color: transparent;
    transition: all 150ms ease;
  }

  .check-box.checked {
    border-color: white;
    background: white;
    color: var(--accent-primary);
  }

  .check-box.partial {
    border-color: var(--accent-primary);
    background: var(--accent-primary);
    color: white;
  }

  .parent-name {
    flex: 1;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: left;
  }

  .parent-row.selected .parent-name {
    color: white;
  }

  .child-count {
    font-size: 11px;
    color: var(--text-muted);
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: 10px;
  }

  .parent-row.selected .child-count {
    background: rgba(255, 255, 255, 0.2);
    color: rgba(255, 255, 255, 0.8);
  }

  .children-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
    margin-top: 8px;
    padding-left: 26px;
  }

  .child-card {
    position: relative;
    height: 32px;
    border-radius: 6px;
    border: 1px solid var(--border-subtle);
    cursor: pointer;
    overflow: hidden;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px;
    transition: all 150ms ease;
  }

  .child-card:hover {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  .child-card.selected {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .child-card.selected:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  .child-card .genre-name {
    font-size: 10px;
  }

  .child-card .check-circle {
    width: 12px;
    height: 12px;
  }

  .child-card .check-circle.checked::after {
    width: 3px;
    height: 6px;
  }

  .popup-footer {
    padding: 12px 16px;
    border-top: 1px solid var(--border-subtle);
  }

  .clear-btn {
    width: 100%;
    padding: 8px 16px;
    border: none;
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease;
  }

  .clear-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .clear-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>
