<script lang="ts">
  import { onMount } from 'svelte';
  import { SlidersHorizontal, ChevronDown } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import GenreFilterPopup from './GenreFilterPopup.svelte';
  import {
    hasActiveFilter,
    getAvailableGenres,
    getSelectedGenreIds,
    getSelectedGenreNames,
    loadGenres,
    subscribe as subscribeGenre,
    type GenreInfo,
    type GenreFilterContext
  } from '$lib/stores/genreFilterStore';

  type ButtonVariant = 'default' | 'control';
  type DropdownAlign = 'left' | 'right';

  interface Props {
    onFilterChange?: () => void;
    context?: GenreFilterContext;
    variant?: ButtonVariant;
    align?: DropdownAlign;
  }

  let { onFilterChange, context = 'home', variant = 'default', align = 'left' }: Props = $props();

  let isOpen = $state(false);
  let buttonEl: HTMLButtonElement | null = null;
  let hasFilter = $state(false);
  let selectedGenreName = $state<string | null>(null);
  let initialized = false;

  onMount(() => {
    // Load genres once on mount
    loadGenres();

    // Initialize state
    hasFilter = hasActiveFilter(context);
    updateSelectedName();
    initialized = true;

    // Subscribe to filter changes for this context
    const unsubscribe = subscribeGenre(() => {
      hasFilter = hasActiveFilter(context);
      updateSelectedName();
      // Only notify parent after initialization to prevent infinite loops
      if (initialized) {
        onFilterChange?.();
      }
    }, context);

    return unsubscribe;
  });

  function updateSelectedName() {
    const selectedNames = getSelectedGenreNames(context);
    if (selectedNames.length === 1) {
      selectedGenreName = selectedNames[0];
    } else if (selectedNames.length > 1) {
      const translate = get(t);
      selectedGenreName = translate('genreFilter.genresCount', { values: { count: selectedNames.length } });
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
  {#if variant === 'control'}
    <button
      class="control-btn"
      class:active={hasFilter}
      bind:this={buttonEl}
      onclick={togglePopup}
      type="button"
    >
      <span>
        {#if selectedGenreName}
          {$t('genreFilter.genrePrefix')} {selectedGenreName}
        {:else}
          {$t('genreFilter.genreAll')}
        {/if}
      </span>
      <ChevronDown size={14} />
    </button>
  {:else}
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
        <span class="filter-label">{$t('genreFilter.title')}</span>
      {/if}
    </button>
  {/if}

  <GenreFilterPopup
    {isOpen}
    {context}
    {align}
    onClose={closePopup}
    anchorEl={buttonEl}
  />
</div>

<style>
  .genre-filter-wrapper {
    position: relative;
  }

  /* Default variant (Home style) */
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

  /* Control variant (Favorites style) */
  .control-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 12px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .control-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .control-btn.active {
    color: var(--accent-primary);
  }
</style>
