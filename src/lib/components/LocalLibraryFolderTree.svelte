<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { ChevronRight, ChevronDown, Folder, FileMusic } from 'lucide-svelte';
  import type { SvelteSet } from 'svelte/reactivity';
  import type { FolderTreeEntry } from '$lib/types/folderTree';
  // Svelte 5 deprecates `<svelte:self>` in favor of self-imports
  // (https://svelte.dev/e/svelte_self_deprecated). Self-import is the
  // recommended replacement; behavior is equivalent.
  import Self from './LocalLibraryFolderTree.svelte';

  interface Props {
    node: FolderTreeEntry;
    depth?: number;
    selectedPath: string | null;
    expandedPaths: SvelteSet<string>;
    /**
     * When non-null, only folder children whose path is in this set are
     * rendered (track-kind children always render under a visible parent).
     * Drives the tree-mode search filter — caller computes the set of
     * matching folders + their ancestors. Null means "no active filter,
     * render everything" (steady state).
     */
    visiblePaths?: SvelteSet<string> | null;
    /**
     * Active tree-mode search query (lowercased, trimmed). When non-empty
     * and the row's segment contains a match, the matched substring is
     * highlighted. Empty string disables highlighting.
     */
    searchQuery?: string;
    onSelect: (path: string) => void;
    onToggleExpand: (path: string) => void;
    /**
     * When true, render a leading checkbox on folder + track rows. The
     * folder checkbox state is derived from `getFolderSelectionState`
     * (none / partial / all); clicking calls `onToggleFolderSelection`.
     * Track-kind rows render their own checkbox bound to the parent's
     * `selectedTrackIds` set; clicking calls `onToggleTrackSelection`.
     */
    selectionMode?: boolean;
    /**
     * Reference to the parent's reactive `selectedTrackIds` Set. Read
     * by the folder-row checkbox's $derived state so the indeterminate
     * / all / none computation updates when the parent reassigns the
     * set. The child does not mutate it — mutations happen via the
     * parent-supplied callbacks.
     */
    selectedTrackIds?: Set<number> | null;
    getFolderSelectionState?: (entry: FolderTreeEntry) => 'none' | 'partial' | 'all';
    isTrackPathSelected?: (trackPath: string) => boolean;
    onToggleFolderSelection?: (folderPath: string, currentState: 'none' | 'partial' | 'all') => void;
    onToggleTrackSelection?: (trackPath: string) => void;
    /**
     * When true, the lazy `v2_library_list_folder_children` invoke
     * forwards `excludeNetworkFolders: true` so descendants living on
     * a network mount are filtered out of the rail. Mirrors the
     * predicate used by flat-mode search and recursive playback so
     * tree visibility, flat list, and queueing all share one source
     * of truth. Caller computes via `shouldExcludeNetworkFolders()`.
     */
    excludeNetworkFolders?: boolean;
  }

  let {
    node,
    depth = 0,
    selectedPath,
    expandedPaths,
    visiblePaths = null,
    searchQuery = '',
    onSelect,
    onToggleExpand,
    selectionMode = false,
    selectedTrackIds = null,
    getFolderSelectionState,
    isTrackPathSelected,
    onToggleFolderSelection,
    onToggleTrackSelection,
    excludeNetworkFolders = false,
  }: Props = $props();

  const isFolder = $derived(node.kind === 'folder');
  const isExpanded = $derived(isFolder && expandedPaths.has(node.path));
  const isSelected = $derived(node.path === selectedPath);
  const trackCount = $derived(node.kind === 'folder' ? node.track_count_under : 0);

  let children = $state<FolderTreeEntry[] | null>(null);
  let loading = $state(false);
  let loadError = $state<string | null>(null);

  // Lazy-load children on first expand. Re-runs whenever isExpanded becomes
  // true and the children cache is still empty.
  $effect(() => {
    if (isExpanded && children === null && !loading && isFolder) {
      loading = true;
      loadError = null;
      const folderPath = node.path;
      invoke<FolderTreeEntry[]>('v2_library_list_folder_children', {
        parentPath: folderPath,
        excludeNetworkFolders,
      })
        .then((result) => {
          children = result;
        })
        .catch((e: unknown) => {
          loadError = String(e);
          // eslint-disable-next-line no-console
          console.error('[FolderTree] list_folder_children failed', e);
        })
        .finally(() => {
          loading = false;
        });
    }
  });

  function handleChevronClick(e: MouseEvent) {
    e.stopPropagation();
    if (isFolder) onToggleExpand(node.path);
  }

  function handleRowClick() {
    if (isFolder) onSelect(node.path);
    // Track-kind rows ignore the row click for now (selection is folder-only).
    // The frontend integration (Task 7) decides what happens when a user
    // clicks a track row.
  }

  function handleRowKey(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleRowClick();
    }
  }

  // Compute the highlight slice for the current row's segment. Returns
  // null when there is no active query or the segment doesn't match —
  // the template falls back to plain text. Keeping this as a regular
  // function (not $derived) because it returns objects with strings and
  // we want to avoid any chance of confusing the Svelte CSS extractor
  // (cf. ADR-001). The function is cheap (single .toLowerCase + indexOf
  // per call) and only runs once per row render.
  function getSegmentHighlight(
    segment: string,
    query: string,
  ): { before: string; match: string; after: string } | null {
    if (!query) return null;
    const lower = segment.toLowerCase();
    const idx = lower.indexOf(query);
    if (idx < 0) return null;
    return {
      before: segment.substring(0, idx),
      match: segment.substring(idx, idx + query.length),
      after: segment.substring(idx + query.length),
    };
  }

  // Folder-row selection state. Computed via the parent's helper which
  // walks selectedTrackIds for paths starting with `${node.path}/`. The
  // 'partial' state needs to map to the DOM `indeterminate` property
  // (no HTML attribute equivalent), set via the bind:this ref below.
  // We read `selectedTrackIds` here so Svelte tracks the prop ref —
  // the parent reassigns the set on every mutation, which propagates a
  // new identity here and re-runs the $derived. The actual computation
  // happens in the parent-supplied callback.
  const folderSelectionState = $derived.by((): 'none' | 'partial' | 'all' => {
    void selectedTrackIds;
    if (!selectionMode || !isFolder || !getFolderSelectionState) return 'none';
    return getFolderSelectionState(node);
  });

  let folderCheckboxRef: HTMLInputElement | null = $state(null);

  $effect(() => {
    if (folderCheckboxRef) {
      folderCheckboxRef.indeterminate = folderSelectionState === 'partial';
    }
  });

  function handleFolderCheckboxClick(e: MouseEvent) {
    e.stopPropagation();
    if (isFolder && onToggleFolderSelection) {
      onToggleFolderSelection(node.path, folderSelectionState);
    }
  }

  function handleTrackCheckboxClick(e: MouseEvent) {
    e.stopPropagation();
    if (!isFolder && onToggleTrackSelection) {
      onToggleTrackSelection(node.path);
    }
  }

  // Track-row checked state. Reads `selectedTrackIds` so the derivation
  // re-runs when the parent reassigns the set; the actual lookup goes
  // through the parent-supplied `isTrackPathSelected` callback because
  // the tree only knows file_path, not track id.
  const trackChecked = $derived.by((): boolean => {
    void selectedTrackIds;
    if (!selectionMode || isFolder || !isTrackPathSelected) return false;
    return isTrackPathSelected(node.path);
  });
</script>

<div
  class="folder-tree-row"
  class:selected={isSelected}
  class:folder={isFolder}
  class:track={!isFolder}
  style:padding-left="{depth * 12 + 6}px"
  role="treeitem"
  tabindex="0"
  aria-selected={isSelected}
  aria-expanded={isFolder ? isExpanded : undefined}
  onclick={handleRowClick}
  onkeydown={handleRowKey}
>
  {#if selectionMode && isFolder}
    <input
      bind:this={folderCheckboxRef}
      type="checkbox"
      class="row-checkbox"
      checked={folderSelectionState === 'all'}
      onclick={handleFolderCheckboxClick}
      aria-label={$t('actions.select')}
    />
  {:else if selectionMode && !isFolder}
    <input
      type="checkbox"
      class="row-checkbox"
      checked={trackChecked}
      onclick={handleTrackCheckboxClick}
      aria-label={$t('actions.select')}
    />
  {/if}

  {#if isFolder}
    <button
      type="button"
      class="chevron-btn"
      onclick={handleChevronClick}
      aria-label={isExpanded ? 'Collapse' : 'Expand'}
    >
      {#if isExpanded}
        <ChevronDown size={12} />
      {:else}
        <ChevronRight size={12} />
      {/if}
    </button>
  {:else}
    <span class="chevron-spacer"></span>
  {/if}

  {#if isFolder}
    <span class="row-icon folder-icon">
      <Folder size={14} />
    </span>
  {:else}
    <span class="row-icon track-icon">
      <FileMusic size={14} />
    </span>
  {/if}

  <span class="row-name" title={node.segment}>
    {#if searchQuery}
      {@const highlight = getSegmentHighlight(node.segment, searchQuery)}
      {#if highlight}
        {highlight.before}<mark class="row-name-highlight">{highlight.match}</mark>{highlight.after}
      {:else}
        {node.segment}
      {/if}
    {:else}
      {node.segment}
    {/if}
    {#if isFolder && trackCount > 0}
      <span class="row-count-inline">({trackCount})</span>
    {/if}
  </span>
</div>

{#if isFolder && isExpanded}
  {#if loading}
    <div class="folder-tree-loading" style:padding-left="{(depth + 1) * 12 + 6}px">
      {$t('library.foldersTree.treeLoading')}
    </div>
  {:else if loadError}
    <div
      class="folder-tree-error"
      style:padding-left="{(depth + 1) * 12 + 6}px"
      title={loadError}
    >
      {loadError}
    </div>
  {:else if children}
    {#if children.length === 0}
      <div class="folder-tree-empty" style:padding-left="{(depth + 1) * 12 + 6}px">
        {$t('library.foldersTree.treeEmpty')}
      </div>
    {:else}
      {#each children as child (child.path)}
        {#if visiblePaths === null || child.kind === 'track' || visiblePaths.has(child.path)}
          <Self
            node={child}
            depth={depth + 1}
            {selectedPath}
            {expandedPaths}
            {visiblePaths}
            {searchQuery}
            {onSelect}
            {onToggleExpand}
            {selectionMode}
            {selectedTrackIds}
            {getFolderSelectionState}
            {isTrackPathSelected}
            {onToggleFolderSelection}
            {onToggleTrackSelection}
            {excludeNetworkFolders}
          />
        {/if}
      {/each}
    {/if}
  {/if}
{/if}

<style>
  .folder-tree-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 6px;
    min-height: 22px;
    cursor: pointer;
    border-radius: 3px;
    user-select: none;
    color: var(--text-primary);
    transition: background 100ms ease;
  }
  .folder-tree-row:hover {
    background: var(--bg-hover);
  }
  .folder-tree-row.selected {
    background: var(--accent-soft, var(--bg-hover));
    color: var(--text-primary);
  }
  .folder-tree-row.track {
    color: var(--text-muted);
    cursor: default;
  }

  .chevron-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 3px;
    padding: 0;
  }
  .chevron-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }
  .chevron-spacer {
    width: 14px;
    flex-shrink: 0;
  }

  .row-checkbox {
    width: 13px;
    height: 13px;
    flex-shrink: 0;
    margin: 0;
    cursor: pointer;
    accent-color: var(--accent-primary);
    transform: scale(0.9);
    transform-origin: center;
  }

  .row-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 14px;
    height: 14px;
  }
  .row-icon.folder-icon {
    color: var(--accent-primary, var(--text-secondary));
  }
  .row-icon.track-icon {
    color: var(--text-muted);
  }

  .row-name {
    flex: 1;
    min-width: 0;
    font-size: 0.78rem;
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-name-highlight {
    background: var(--accent-soft, var(--bg-tertiary));
    color: var(--text-primary);
    font-weight: 600;
    padding: 0 1px;
    border-radius: 2px;
  }
  .row-count-inline {
    margin-left: 6px;
    font-size: 0.7rem;
    color: var(--text-tertiary, var(--text-muted, #888));
    opacity: 0.7;
    font-weight: normal;
  }

  .folder-tree-loading,
  .folder-tree-empty,
  .folder-tree-error {
    padding: 3px 6px;
    font-size: 0.72rem;
    color: var(--text-muted);
  }
  .folder-tree-error {
    color: var(--error, #f55);
  }
</style>
