<script lang="ts">
  import { tick } from 'svelte';
  import { SlidersHorizontal, X } from 'lucide-svelte';
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
    type GenreInfo
  } from '$lib/stores/genreFilterStore';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    anchorEl?: HTMLElement | null;
  }

  let { isOpen, onClose, anchorEl = null }: Props = $props();

  let genres = $state<GenreInfo[]>([]);
  let selectedIds = $state<Set<number>>(new Set());
  let rememberSelection = $state(true);
  let popupEl: HTMLDivElement | null = null;
  let popupStyle = $state('');

  // Subscribe to store changes
  $effect(() => {
    const unsubscribe = subscribeGenre(() => {
      const state = getGenreFilterState();
      genres = state.availableGenres;
      selectedIds = state.selectedGenreIds;
      rememberSelection = state.rememberSelection;
    });

    // Initial load
    const state = getGenreFilterState();
    genres = state.availableGenres;
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

    // Align right edge of popup with right edge of anchor (extends to the left)
    let left = anchorRect.right - popupRect.width;
    let top = anchorRect.bottom + 8;

    // Only adjust left if it goes off the LEFT edge of the screen
    if (left < 8) left = 8;

    // Handle vertical overflow
    if (top + popupRect.height > window.innerHeight - 8) {
      top = anchorRect.top - popupRect.height - 8;
    }

    popupStyle = `left: ${left}px; top: ${top}px;`;
  }

  function handleGenreClick(genreId: number) {
    toggleGenre(genreId);
  }

  function handleClearAll() {
    clearSelection();
    onClose();
  }

  function handleRememberToggle() {
    setRememberSelection(!rememberSelection);
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

    <div class="remember-row">
      <span>Remember my selection</span>
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

    <div class="genres-grid">
      {#each genres as genre (genre.id)}
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

    {#if hasActiveFilter()}
      <div class="popup-footer">
        <button class="clear-btn" onclick={handleClearAll} type="button">
          Clear filter
        </button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .genre-popup {
    position: fixed;
    z-index: 10000;
    width: 480px;
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

  .remember-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 16px;
    font-size: 12px;
    color: var(--text-secondary);
    border-bottom: 1px solid var(--border-subtle);
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

  .clear-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
</style>
