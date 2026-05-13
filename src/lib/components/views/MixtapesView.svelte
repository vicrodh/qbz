<script lang="ts">
  import { onMount } from 'svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import {
    Plus, ArrowLeft, Search, X, ArrowUpDown,
    RotateCcw, LayoutGrid, List,
  } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    collectionsStore,
    loadCollections,
    type MixtapeCollection,
  } from '$lib/stores/mixtapeCollectionsStore';
  import CollectionMosaic from '../CollectionMosaic.svelte';
  import { getUserItem, setUserItem } from '$lib/utils/userStorage';

  function coverUrlFor(col: MixtapeCollection): string | null {
    return col.custom_artwork_path ? convertFileSrc(col.custom_artwork_path) : null;
  }

  interface Props {
    onOpen?: (id: string) => void;
    onCreate?: () => void;
    onBack?: () => void;
  }
  let { onOpen, onCreate, onBack }: Props = $props();

  type ViewMode = 'grid' | 'list';
  type SortBy = 'position' | 'name' | 'items' | 'updated';
  type SortDir = 'asc' | 'desc';

  const PREFS_KEY = 'mixtapes-index-prefs';
  interface IndexPrefs {
    viewMode?: ViewMode;
    sortBy?: SortBy;
    sortDir?: SortDir;
  }
  function loadPrefs(): IndexPrefs {
    try {
      const raw = getUserItem(PREFS_KEY);
      return raw ? (JSON.parse(raw) as IndexPrefs) : {};
    } catch {
      return {};
    }
  }
  function savePrefs(p: IndexPrefs): void {
    try {
      setUserItem(PREFS_KEY, JSON.stringify(p));
    } catch {
      // storage disabled / quota — non-critical.
    }
  }
  const initial = loadPrefs();

  let viewMode = $state<ViewMode>(initial.viewMode ?? 'grid');
  let sortBy = $state<SortBy>(initial.sortBy ?? 'position');
  let sortDir = $state<SortDir>(initial.sortDir ?? 'asc');
  let searchQuery = $state('');
  // Mutually-exclusive toolbar dropdown state.
  type OpenMenu = 'sort' | null;
  let openMenu = $state<OpenMenu>(null);

  $effect(() => {
    if (openMenu === null) return;

    function handleClickOutside(event: MouseEvent) {
      const target = event.target as HTMLElement | null;
      if (!target) return;
      if (target.closest('.control-btn') || target.closest('.control-menu')) return;
      openMenu = null;
    }

    setTimeout(() => {
      document.addEventListener('click', handleClickOutside);
    }, 0);

    return () => {
      document.removeEventListener('click', handleClickOutside);
    };
  });

  const normalizedSearch = $derived(searchQuery.trim().toLowerCase());

  const mixtapes = $derived(
    $collectionsStore.filter((mc) => mc.kind === 'mixtape'),
  );

  const visibleMixtapes = $derived.by(() => {
    let out = mixtapes.slice();
    if (normalizedSearch) {
      out = out.filter(
        (m) =>
          m.name.toLowerCase().includes(normalizedSearch) ||
          (m.description ?? '').toLowerCase().includes(normalizedSearch),
      );
    }
    const dir = sortDir === 'asc' ? 1 : -1;
    out.sort((a, b) => {
      switch (sortBy) {
        case 'name':
          return a.name.localeCompare(b.name) * dir;
        case 'items':
          return (a.items.length - b.items.length) * dir;
        case 'updated':
          return ((a.updated_at ?? 0) - (b.updated_at ?? 0)) * dir;
        default:
          return (a.position - b.position) * dir;
      }
    });
    return out;
  });

  const hasActiveFilters = $derived(
    sortBy !== 'position' || sortDir !== 'asc',
  );

  function selectSort(value: SortBy) {
    if (sortBy === value) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortBy = value;
      sortDir = 'asc';
    }
    openMenu = null;
  }

  function resetFilters() {
    sortBy = 'position';
    sortDir = 'asc';
    searchQuery = '';
  }

  $effect(() => {
    savePrefs({ viewMode, sortBy, sortDir });
  });

  const sortOptions: Array<{ value: SortBy; label: string }> = $derived([
    { value: 'position', label: $t('collectionDetail.sortByPosition') },
    { value: 'name', label: $t('collectionDetail.sortByName') },
    { value: 'items', label: $t('collections.sortByItems') },
    { value: 'updated', label: $t('collections.sortByUpdated') },
  ]);

  onMount(() => {
    loadCollections();
  });
</script>

<div class="mixtapes-view">
  {#if onBack}
    <button class="back-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>{$t('actions.back')}</span>
    </button>
  {/if}
  <header class="view-header">
    <h1>{$t('mixtapes.nav')}</h1>
    <button
      type="button"
      class="primary-cta"
      onclick={() => onCreate?.()}
    >
      <Plus size={16} />
      <span>{$t('mixtapes.empty.cta')}</span>
    </button>
  </header>

  {#if mixtapes.length === 0}
    <div class="empty-state">
      <CollectionMosaic items={[]} size={160} kind="mixtape" />
      <h2>{$t('mixtapes.empty.title')}</h2>
      <button
        type="button"
        class="primary-cta"
        onclick={() => onCreate?.()}
      >
        {$t('mixtapes.empty.cta')}
      </button>
    </div>
  {:else}
    <!-- Toolbar — search / sort / view toggle. No kind filter (all
         items are kind='mixtape'); filter dropdown would just be noise. -->
    <div class="list-controls">
      <div class="search-box" class:has-query={normalizedSearch.length > 0}>
        <Search size={14} />
        <input
          type="text"
          class="search-input"
          placeholder={$t('mixtapes.searchPlaceholder')}
          bind:value={searchQuery}
          aria-label={$t('mixtapes.searchPlaceholder')}
        />
        {#if normalizedSearch}
          <button
            type="button"
            class="search-clear"
            onclick={() => (searchQuery = '')}
            aria-label={$t('actions.clearSearch') || 'Clear search'}
            title={$t('actions.clearSearch') || 'Clear'}
          >
            <X size={12} />
          </button>
        {/if}
      </div>

      <div class="dropdown-container">
        <button
          type="button"
          class="control-btn"
          onclick={() => (openMenu = openMenu === 'sort' ? null : 'sort')}
          title={$t('collectionDetail.sort')}
        >
          <ArrowUpDown size={14} />
          <span>{sortOptions.find((o) => o.value === sortBy)?.label}</span>
          <span class="sort-indicator">{sortDir === 'asc' ? '↑' : '↓'}</span>
        </button>
        {#if openMenu === 'sort'}
          <div class="control-menu">
            {#each sortOptions as option}
              <button
                type="button"
                class="control-menu-item"
                class:selected={sortBy === option.value}
                onclick={() => selectSort(option.value)}
              >
                <span>{option.label}</span>
                {#if sortBy === option.value}
                  <span class="sort-indicator">{sortDir === 'asc' ? '↑' : '↓'}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      {#if hasActiveFilters}
        <button
          type="button"
          class="control-btn subtle"
          onclick={resetFilters}
          title={$t('discographyBuilder.typeOverrideReset') || 'Reset'}
        >
          <RotateCcw size={12} />
        </button>
      {/if}

      <div class="view-mode-group" role="radiogroup" aria-label={$t('collectionDetail.viewMode')}>
        <button
          type="button"
          class="control-btn seg"
          class:active={viewMode === 'list'}
          onclick={() => (viewMode = 'list')}
          title={$t('collectionDetail.viewList')}
          aria-label={$t('collectionDetail.viewList')}
          aria-pressed={viewMode === 'list'}
        >
          <List size={14} />
        </button>
        <button
          type="button"
          class="control-btn seg"
          class:active={viewMode === 'grid'}
          onclick={() => (viewMode = 'grid')}
          title={$t('collectionDetail.viewGrid')}
          aria-label={$t('collectionDetail.viewGrid')}
          aria-pressed={viewMode === 'grid'}
        >
          <LayoutGrid size={14} />
        </button>
      </div>
    </div>

    {#if visibleMixtapes.length === 0}
      <div class="empty-results">{$t('search.noResults')}</div>
    {:else if viewMode === 'grid'}
      <div class="grid">
        {#each visibleMixtapes as mc (mc.id)}
          <button
            type="button"
            class="card"
            onclick={() => onOpen?.(mc.id)}
          >
            <CollectionMosaic
              items={mc.items}
              size={184}
              kind={mc.kind}
              customCoverUrl={coverUrlFor(mc)}
            />
            <div class="card-label">{$t('mixtapes.label')}</div>
            <div class="card-name">{mc.name}</div>
            <div class="card-meta">
              {$t('mixtapes.albumCount', { values: { count: mc.items.length } })}
            </div>
          </button>
        {/each}
      </div>
    {:else}
      <div class="list">
        {#each visibleMixtapes as mc (mc.id)}
          <button
            type="button"
            class="list-row"
            onclick={() => onOpen?.(mc.id)}
          >
            <div class="list-cover">
              <CollectionMosaic
                items={mc.items}
                size={48}
                kind={mc.kind}
                customCoverUrl={coverUrlFor(mc)}
              />
            </div>
            <div class="list-meta">
              <div class="list-name">{mc.name}</div>
              <div class="list-sub">
                <span class="list-label">{$t('mixtapes.label')}</span>
                <span class="list-dot">·</span>
                <span>{$t('mixtapes.albumCount', { values: { count: mc.items.length } })}</span>
              </div>
            </div>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  /* Padding matches FavoritesView / canonical root-view pattern so the
     left edge lines up with other sidebar-accessed views. */
  .mixtapes-view {
    padding: 8px 8px 100px 18px;
    color: var(--text-primary);
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    margin-top: 8px;
    margin-bottom: 24px;
    font-family: inherit;
    transition: color 150ms ease;
  }
  .back-btn:hover {
    color: var(--text-secondary);
  }

  .view-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }
  .view-header h1 {
    margin: 0;
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .primary-cta {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px 20px;
    background: var(--accent-primary);
    color: var(--btn-primary-text);
    border: none;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
    transition: background 150ms ease;
  }
  .primary-cta:hover {
    filter: brightness(1.1);
  }

  /* Toolbar vocabulary identical to CollectionsView + detail view. */
  .list-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 8px 0 16px;
    flex-wrap: wrap;
  }

  .search-box {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    min-width: 220px;
    color: var(--text-muted);
    transition: border-color 150ms ease, background 150ms ease;
  }
  .search-box:focus-within,
  .search-box.has-query {
    border-color: var(--accent-primary);
    background: var(--bg-primary);
  }
  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 13px;
  }
  .search-clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: transparent;
    border: none;
    border-radius: 50%;
    color: var(--text-muted);
    cursor: pointer;
  }
  .search-clear:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .dropdown-container {
    position: relative;
  }

  .control-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    cursor: pointer;
    transition: background 150ms ease, border-color 150ms ease;
  }
  .control-btn:hover {
    background: var(--bg-hover);
  }
  .control-btn.subtle {
    padding: 0 8px;
    color: var(--text-muted);
  }
  .control-btn.seg {
    padding: 0 8px;
    border-radius: 0;
    border-right-width: 0;
  }
  .sort-indicator {
    color: var(--text-muted);
    font-size: 11px;
  }

  .control-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 180px;
    padding: 4px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 210;
  }
  .control-menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 8px 10px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 13px;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }
  .control-menu-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .control-menu-item.selected {
    color: var(--accent-primary);
  }

  .view-mode-group {
    display: inline-flex;
    align-items: center;
    margin-left: auto;
  }
  .view-mode-group .control-btn.seg:first-child {
    border-top-left-radius: 6px;
    border-bottom-left-radius: 6px;
  }
  .view-mode-group .control-btn.seg:last-child {
    border-top-right-radius: 6px;
    border-bottom-right-radius: 6px;
    border-right-width: 1px;
  }
  .view-mode-group .control-btn.seg.active {
    background: var(--accent-primary);
    color: var(--btn-primary-text);
    border-color: var(--accent-primary);
  }

  .empty-results {
    padding: 40px 0;
    color: var(--text-muted);
    text-align: center;
    font-size: 13px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, 208px);
    gap: 20px;
    justify-content: start;
  }

  .card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
    padding: 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition: background 150ms ease, border-color 150ms ease;
  }
  .card:hover {
    background: var(--bg-hover);
    border-color: var(--bg-hover);
  }

  .card-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--accent-primary);
    margin-top: 4px;
  }

  .card-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.3;
    width: 100%;
    min-height: 2.6em;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    word-wrap: break-word;
  }

  .card-meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .list-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    background: transparent;
    border: none;
    border-radius: 6px;
    width: 100%;
    text-align: left;
    color: var(--text-primary);
    font-family: inherit;
    cursor: pointer;
    transition: background 150ms ease;
  }
  .list-row:hover {
    background: var(--bg-hover);
  }
  .list-cover {
    flex-shrink: 0;
  }
  .list-meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 2px;
  }
  .list-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .list-sub {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .list-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--accent-primary);
  }
  .list-dot {
    color: var(--text-muted);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 80px 0;
    color: var(--text-muted);
  }
  .empty-state h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    color: var(--text-secondary);
  }
</style>
