<script lang="ts">
  import { tick } from 'svelte';
  import { SlidersHorizontal, X, ChevronRight } from 'lucide-svelte';
  import {
    getAvailableGenres,
    getSelectedGenreIds,
    isGenreSelected,
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

  // Derived: which genres to display based on mode
  let displayGenres = $derived(showAllGenres ? allGenres : parentGenres);

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
      // Align left edge of popup with left edge of anchor (extends to the right)
      left = anchorRect.left;
      // Adjust if it goes off the RIGHT edge
      if (left + popupRect.width > window.innerWidth - 8) {
        left = window.innerWidth - popupRect.width - 8;
      }
    } else {
      // Align right edge of popup with right edge of anchor (extends to the left)
      left = anchorRect.right - popupRect.width;
      // Only adjust left if it goes off the LEFT edge of the screen
      if (left < 8) left = 8;
    }

    // Handle vertical overflow
    if (top + popupRect.height > window.innerHeight - 8) {
      top = anchorRect.top - popupRect.height - 8;
    }

    popupStyle = `left: ${left}px; top: ${top}px;`;
  }

  function handleGenreClick(genreId: number) {
    toggleGenre(genreId, context);
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

  // Close on click outside
  $effect(() => {
    if (isOpen) {
      document.addEventListener('click', handleClickOutside);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  });

  // Close on escape
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      onClose();
    }
  }

  // Use neutral dark colors for all genres
  function getGenreColor(_genre: GenreInfo): string {
    return 'var(--bg-tertiary)';
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div class="genre-popup" bind:this={popupEl} style={popupStyle}>
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

    <div class="genres-grid">
      {#each displayGenres as genre (genre.id)}
        <button
          class="genre-card"
          class:selected={selectedIds.has(genre.id)}
          style="--genre-color: {getGenreColor(genre)}"
          onclick={() => handleGenreClick(genre.id)}
          type="button"
        >
          <span class="genre-name">{genre.name}</span>
          <span class="check-circle" class:checked={selectedIds.has(genre.id)}></span>
        </button>
      {/each}
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

  .genres-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
    padding: 12px;
    overflow-y: auto;
    max-height: 400px;
  }

  .genre-card {
    position: relative;
    height: 36px;
    border-radius: 6px;
    border: 1px solid var(--border-subtle);
    cursor: pointer;
    overflow: hidden;
    background: var(--genre-color);
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

  .genre-card.selected .genre-name {
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
  }

  .check-circle.checked {
    border-color: white;
    background: white;
  }

  .check-circle.checked::after {
    content: '';
    position: absolute;
    top: 50%;
    right: 10px;
    width: 4px;
    height: 7px;
    border: solid var(--accent-primary);
    border-width: 0 1.5px 1.5px 0;
    transform: translateY(-60%) rotate(45deg);
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
