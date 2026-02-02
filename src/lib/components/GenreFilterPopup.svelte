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

    let left = anchorRect.right - popupRect.width;
    let top = anchorRect.bottom + 8;

    // Keep within viewport
    if (left < 8) left = 8;
    if (left + popupRect.width > window.innerWidth - 8) {
      left = window.innerWidth - popupRect.width - 8;
    }
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

  // Default genre colors if not provided by API
  const defaultColors: Record<string, string> = {
    'Pop/Rock': '#e74c3c',
    'Jazz': '#f39c12',
    'Classical': '#9b59b6',
    'Electronic': '#3498db',
    'Soul/Funk/R&B': '#e91e63',
    'Hip-Hop/Rap': '#607d8b',
    'Metal': '#455a64',
    'Blues': '#1976d2',
    'Latin': '#ff9800',
    'Country': '#8d6e63',
    'World': '#00897b',
    'Soundtracks': '#5c6bc0',
    'Folk': '#689f38',
    'Reggae': '#4caf50',
    'R&B': '#ad1457',
    'Flamenco': '#d84315',
  };

  function getGenreColor(genre: GenreInfo): string {
    if (genre.color) return genre.color;
    // Try to find a matching default color
    for (const [key, color] of Object.entries(defaultColors)) {
      if (genre.name.toLowerCase().includes(key.toLowerCase())) {
        return color;
      }
    }
    return '#6b7280'; // Default gray
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
    width: 320px;
    max-height: 480px;
    background: var(--bg-primary);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
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
    gap: 8px;
    padding: 12px;
    overflow-y: auto;
    max-height: 320px;
  }

  .genre-card {
    position: relative;
    aspect-ratio: 1.2;
    border-radius: 8px;
    border: none;
    cursor: pointer;
    overflow: hidden;
    background: linear-gradient(135deg, var(--genre-color) 0%, color-mix(in srgb, var(--genre-color) 60%, black) 100%);
    display: flex;
    align-items: flex-end;
    padding: 8px;
    transition: transform 150ms ease, box-shadow 150ms ease;
  }

  .genre-card:hover {
    transform: scale(1.03);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .genre-card.selected {
    box-shadow: 0 0 0 2px var(--accent-primary), 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .genre-name {
    font-size: 11px;
    font-weight: 600;
    color: white;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
    line-height: 1.2;
    text-align: left;
  }

  .check-circle {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.6);
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
    top: 2px;
    left: 5px;
    width: 4px;
    height: 8px;
    border: solid var(--accent-primary);
    border-width: 0 2px 2px 0;
    transform: rotate(45deg);
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
