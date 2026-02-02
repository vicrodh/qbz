<script lang="ts">
  import { SlidersHorizontal } from 'lucide-svelte';
  import GenreFilterPopup from './GenreFilterPopup.svelte';
  import {
    hasActiveFilter,
    getAvailableGenres,
    getSelectedGenreIds,
    loadGenres,
    subscribe as subscribeGenre,
    type GenreInfo
  } from '$lib/stores/genreFilterStore';

  interface Props {
    onFilterChange?: () => void;
  }

  let { onFilterChange }: Props = $props();

  let isOpen = $state(false);
  let buttonEl: HTMLButtonElement | null = null;
  let hasFilter = $state(false);
  let selectedGenreName = $state<string | null>(null);

  // Subscribe to filter changes
  $effect(() => {
    const unsubscribe = subscribeGenre(() => {
      hasFilter = hasActiveFilter();
      updateSelectedName();
      onFilterChange?.();
    });

    hasFilter = hasActiveFilter();
    updateSelectedName();

    return unsubscribe;
  });

  // Load genres on mount
  $effect(() => {
    loadGenres();
  });

  function updateSelectedName() {
    const selectedIds = getSelectedGenreIds();
    if (selectedIds.size === 1) {
      const genres = getAvailableGenres();
      const selectedId = Array.from(selectedIds)[0];
      const genre = genres.find(g => g.id === selectedId);
      selectedGenreName = genre?.name ?? null;
    } else {
      selectedGenreName = null;
    }
  }

  function togglePopup() {
    isOpen = !isOpen;
  }

  function closePopup() {
    isOpen = false;
  }
</script>

<div class="genre-filter-wrapper">
  <button
    class="genre-filter-btn"
    class:active={hasFilter}
    bind:this={buttonEl}
    onclick={togglePopup}
    type="button"
  >
    <SlidersHorizontal size={14} />
    {#if selectedGenreName}
      <span class="filter-label">{selectedGenreName}</span>
    {:else}
      <span class="filter-label">Filter by genre</span>
    {/if}
  </button>

  <GenreFilterPopup
    {isOpen}
    onClose={closePopup}
    anchorEl={buttonEl}
  />
</div>

<style>
  .genre-filter-wrapper {
    position: relative;
  }

  .genre-filter-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: transparent;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .genre-filter-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: var(--border-subtle);
  }

  .genre-filter-btn.active {
    background: var(--accent-primary);
    color: white;
    border-color: var(--accent-primary);
  }

  .genre-filter-btn.active:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }

  .filter-label {
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
