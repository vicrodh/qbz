<script lang="ts">
  import { tick } from 'svelte';
  import { SlidersHorizontal, X, Minus, Check, ChevronRight, ChevronDown, Search, Loader2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    getChildGenres,
    getAllDescendantIds,
    countDescendants,
    toggleGenre,
    setGenresSelected,
    clearSelection,
    hasActiveFilter,
    setRememberSelection,
    getGenreFilterState,
    loadChildren,
    areChildrenLoaded,
    subscribe as subscribeGenre,
    type GenreInfo,
    type GenreTreeNode,
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
  let genreTree = $state<GenreTreeNode[]>([]);
  let selectedIds = $state<Set<number>>(new Set());
  let rememberSelection = $state(true);
  let showAllGenres = $state(false);
  let searchQuery = $state('');
  let expandedNodes = $state<Set<number>>(new Set());
  let loadingNodes = $state<Set<number>>(new Set());
  let popupEl: HTMLDivElement | null = null;
  let popupStyle = $state('');

  // Subscribe to store changes
  $effect(() => {
    const unsubscribe = subscribeGenre(() => {
      const state = getGenreFilterState(context);
      parentGenres = state.availableGenres;
      genreTree = state.genreTree;
      selectedIds = state.selectedGenreIds;
      rememberSelection = state.rememberSelection;
    }, context);

    const state = getGenreFilterState(context);
    parentGenres = state.availableGenres;
    genreTree = state.genreTree;
    selectedIds = state.selectedGenreIds;
    rememberSelection = state.rememberSelection;

    return unsubscribe;
  });

  // Position popup when opening or when size changes
  $effect(() => {
    if (isOpen && anchorEl) {
      positionPopup();
    }
  });

  // Reposition when toggling advanced view (size changes)
  $effect(() => {
    if (isOpen && anchorEl && showAllGenres !== undefined) {
      // Small delay to let CSS transition complete
      setTimeout(positionPopup, 10);
    }
  });

  function genreMatchesSearch(genre: GenreInfo): boolean {
    if (!searchQuery.trim()) return true;
    return genre.name.toLowerCase().includes(searchQuery.toLowerCase());
  }

  function nodeMatchesSearch(node: GenreTreeNode): boolean {
    if (genreMatchesSearch(node.genre)) return true;
    return node.children.some(child => nodeMatchesSearch(child));
  }

  async function positionPopup() {
    await tick();
    if (!anchorEl || !popupEl) return;

    const anchorRect = anchorEl.getBoundingClientRect();
    // Use expected width based on expanded state (CSS values)
    const popupWidth = showAllGenres ? 630 : 530;
    const popupRect = popupEl.getBoundingClientRect();

    let left: number;
    let top = anchorRect.bottom + 8;

    if (align === 'right') {
      left = anchorRect.left;
      if (left + popupWidth > window.innerWidth - 8) {
        left = window.innerWidth - popupWidth - 8;
      }
    } else {
      // Align right edge of popup with right edge of anchor (extends to the left)
      left = anchorRect.right - popupWidth;
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

  // Get selection state for a node: 'all' | 'none' | 'partial'
  function getNodeState(genreId: number): 'all' | 'none' | 'partial' {
    const allIds = [genreId, ...getAllDescendantIds(genreId)];
    const selectedCount = allIds.filter(id => selectedIds.has(id)).length;

    if (selectedCount === 0) return 'none';
    if (selectedCount === allIds.length) return 'all';
    return 'partial';
  }

  // Toggle a parent node: select/deselect all loaded descendants
  function handleNodeToggle(genreId: number) {
    const currentState = getNodeState(genreId);
    const allIds = [genreId, ...getAllDescendantIds(genreId)];

    if (currentState === 'all') {
      setGenresSelected(allIds, false, context);
    } else {
      setGenresSelected(allIds, true, context);
    }
  }

  async function toggleExpanded(genreId: number) {
    const newExpanded = new Set(expandedNodes);

    if (newExpanded.has(genreId)) {
      newExpanded.delete(genreId);
    } else {
      newExpanded.add(genreId);

      // Lazy load children if not loaded
      if (!areChildrenLoaded(genreId)) {
        loadingNodes = new Set([...loadingNodes, genreId]);
        await loadChildren(genreId);
        loadingNodes = new Set([...loadingNodes].filter(id => id !== genreId));
      }
    }

    expandedNodes = newExpanded;
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

  // Filter tree based on search (only works on loaded nodes)
  function filterTree(nodes: GenreTreeNode[]): GenreTreeNode[] {
    if (!searchQuery.trim()) return nodes;

    return nodes
      .filter(node => nodeMatchesSearch(node))
      .map(node => ({
        ...node,
        children: node.children
          .filter(child => nodeMatchesSearch(child))
          .map(child => ({
            ...child,
            children: child.children.filter(gc => genreMatchesSearch(gc.genre))
          }))
      }));
  }

  let filteredTree = $derived(filterTree(genreTree));

  function getChildCount(genreId: number): number {
    const loaded = countDescendants(genreId);
    if (loaded > 0) return loaded;
    // If not loaded, we don't know the count
    return 0;
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
        <span>{$t('genreFilter.title')}</span>
      </div>
      <button class="close-btn" onclick={onClose} type="button" title={$t('actions.close')}>
        <X size={16} />
      </button>
    </div>

    <div class="options-row">
      <div class="option-item">
        <span>{$t('genreFilter.rememberSelection')}</span>
        <button
          class="toggle-switch"
          class:active={rememberSelection}
          onclick={handleRememberToggle}
          type="button"
          aria-pressed={rememberSelection}
          title={$t('genreFilter.rememberSelection')}
        >
          <span class="toggle-thumb"></span>
        </button>
      </div>
      <div class="option-item">
        <span>{$t('genreFilter.advancedView')}</span>
        <button
          class="toggle-switch"
          class:active={showAllGenres}
          onclick={() => showAllGenres = !showAllGenres}
          type="button"
          aria-pressed={showAllGenres}
          title={$t('genreFilter.toggleAdvancedView')}
        >
          <span class="toggle-thumb"></span>
        </button>
      </div>
    </div>

    {#if showAllGenres}
      <div class="search-row">
        <Search size={14} />
        <input
          type="text"
          placeholder={$t('placeholders.searchGenres')}
          bind:value={searchQuery}
          class="search-input"
        />
        {#if searchQuery}
          <button class="clear-search" onclick={() => searchQuery = ''} type="button" title={$t('genreFilter.clearSearch')}>
            <X size={12} />
          </button>
        {/if}
      </div>
    {/if}

    <div class="genres-container">
      {#if showAllGenres}
        <!-- Hierarchical tree view -->
        <div class="genre-tree">
          {#each filteredTree as parentNode (parentNode.genre.id)}
            {@const parentState = getNodeState(parentNode.genre.id)}
            {@const isExpanded = expandedNodes.has(parentNode.genre.id)}
            {@const isLoading = loadingNodes.has(parentNode.genre.id)}
            {@const childCount = getChildCount(parentNode.genre.id)}
            {@const hasLoadedChildren = parentNode.children.length > 0}

            <div class="tree-node">
              <div class="node-row level-0" class:selected={parentState === 'all'} class:partial={parentState === 'partial'}>
                {#if hasLoadedChildren || !areChildrenLoaded(parentNode.genre.id)}
                  <button class="expand-btn" onclick={(e) => { e.stopPropagation(); toggleExpanded(parentNode.genre.id); }} type="button" title={isExpanded ? $t('genreFilter.collapse') : $t('genreFilter.expand')}>
                    {#if isLoading}
                      <Loader2 size={14} class="animate-spin" />
                    {:else if isExpanded}
                      <ChevronDown size={14} />
                    {:else}
                      <ChevronRight size={14} />
                    {/if}
                  </button>
                {:else}
                  <span class="expand-placeholder"></span>
                {/if}

                <button class="node-content" onclick={() => handleNodeToggle(parentNode.genre.id)} type="button">
                  <span class="check-box" class:checked={parentState === 'all'} class:partial={parentState === 'partial'}>
                    {#if parentState === 'all'}
                      <Check size={10} strokeWidth={3} />
                    {:else if parentState === 'partial'}
                      <Minus size={10} strokeWidth={3} />
                    {/if}
                  </span>
                  <span class="node-name">{parentNode.genre.name}</span>
                  {#if childCount > 0}
                    <span class="descendant-count">{childCount}</span>
                  {/if}
                </button>
              </div>

              {#if isExpanded && hasLoadedChildren}
                <div class="children-container">
                  {#each parentNode.children as childNode (childNode.genre.id)}
                    {@const childState = getNodeState(childNode.genre.id)}
                    {@const childExpanded = expandedNodes.has(childNode.genre.id)}
                    {@const childLoading = loadingNodes.has(childNode.genre.id)}
                    {@const grandchildCount = getChildCount(childNode.genre.id)}
                    {@const hasGrandchildren = childNode.children.length > 0 || !areChildrenLoaded(childNode.genre.id)}

                    <div class="tree-node">
                      <div class="node-row level-1" class:selected={childState === 'all'} class:partial={childState === 'partial'}>
                        {#if hasGrandchildren}
                          <button class="expand-btn" onclick={(e) => { e.stopPropagation(); toggleExpanded(childNode.genre.id); }} type="button" title={childExpanded ? $t('genreFilter.collapse') : $t('genreFilter.expand')}>
                            {#if childLoading}
                              <Loader2 size={12} class="animate-spin" />
                            {:else if childExpanded}
                              <ChevronDown size={12} />
                            {:else}
                              <ChevronRight size={12} />
                            {/if}
                          </button>
                        {:else}
                          <span class="expand-placeholder"></span>
                        {/if}

                        <button class="node-content" onclick={() => handleNodeToggle(childNode.genre.id)} type="button">
                          <span class="check-box small" class:checked={childState === 'all' || selectedIds.has(childNode.genre.id)} class:partial={childState === 'partial'}>
                            {#if childState === 'all' || selectedIds.has(childNode.genre.id)}
                              <Check size={8} strokeWidth={3} />
                            {:else if childState === 'partial'}
                              <Minus size={8} strokeWidth={3} />
                            {/if}
                          </span>
                          <span class="node-name">{childNode.genre.name}</span>
                          {#if grandchildCount > 0}
                            <span class="descendant-count">{grandchildCount}</span>
                          {/if}
                        </button>
                      </div>

                      {#if childExpanded && childNode.children.length > 0}
                        <div class="children-container">
                          {#each childNode.children as grandchildNode (grandchildNode.genre.id)}
                            <div class="node-row level-2" class:selected={selectedIds.has(grandchildNode.genre.id)}>
                              <span class="expand-placeholder"></span>
                              <button class="node-content" onclick={() => handleGenreClick(grandchildNode.genre.id)} type="button">
                                <span class="check-box small" class:checked={selectedIds.has(grandchildNode.genre.id)}>
                                  {#if selectedIds.has(grandchildNode.genre.id)}
                                    <Check size={8} strokeWidth={3} />
                                  {/if}
                                </span>
                                <span class="node-name">{grandchildNode.genre.name}</span>
                              </button>
                            </div>
                          {/each}
                        </div>
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
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
        {$t('genreFilter.clearFilter')}
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
    width: 630px;
    max-height: 700px;
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

  .search-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text-muted);
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    outline: none;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .clear-search {
    width: 20px;
    height: 20px;
    border: none;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    border-radius: 50%;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .clear-search:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
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

  /* Tree view */
  .genre-tree {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .tree-node {
    display: flex;
    flex-direction: column;
  }

  .node-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 8px;
    border-radius: 6px;
    transition: background 150ms ease;
  }

  .node-row:hover {
    background: var(--bg-hover);
  }

  .node-row.selected {
    background: var(--accent-primary);
  }

  .node-row.selected:hover {
    background: var(--accent-hover);
  }

  .node-row.partial {
    background: rgba(var(--accent-primary-rgb, 59, 130, 246), 0.15);
  }

  .node-row.level-0 {
    font-weight: 600;
  }

  .node-row.level-1 {
    margin-left: 20px;
    font-size: 12px;
  }

  .node-row.level-2 {
    margin-left: 40px;
    font-size: 11px;
  }

  .expand-btn {
    width: 20px;
    height: 20px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 4px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .expand-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .node-row.selected .expand-btn {
    color: rgba(255, 255, 255, 0.7);
  }

  .node-row.selected .expand-btn:hover {
    background: rgba(255, 255, 255, 0.1);
    color: white;
  }

  .expand-placeholder {
    width: 20px;
    flex-shrink: 0;
  }

  .node-content {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    padding: 0;
    min-width: 0;
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

  .check-box.small {
    width: 14px;
    height: 14px;
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

  .node-row.selected .check-box {
    border-color: white;
  }

  .node-row.selected .check-box.checked {
    background: white;
    color: var(--accent-primary);
  }

  .node-name {
    flex: 1;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .node-row.selected .node-name {
    color: white;
  }

  .descendant-count {
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: 10px;
    flex-shrink: 0;
  }

  .node-row.selected .descendant-count {
    background: rgba(255, 255, 255, 0.2);
    color: rgba(255, 255, 255, 0.8);
  }

  .children-container {
    display: flex;
    flex-direction: column;
    gap: 2px;
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

  /* Spinner animation */
  :global(.animate-spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
