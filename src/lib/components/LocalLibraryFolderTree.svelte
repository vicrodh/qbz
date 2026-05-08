<script lang="ts">
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { ChevronRight, ChevronDown, Folder, FileMusic, Play } from 'lucide-svelte';
  import type { SvelteSet } from 'svelte/reactivity';
  // Svelte 5 deprecates `<svelte:self>` in favor of self-imports
  // (https://svelte.dev/e/svelte_self_deprecated). Self-import is the
  // recommended replacement; behavior is equivalent.
  import Self from './LocalLibraryFolderTree.svelte';

  type FolderEntry = {
    kind: 'folder';
    path: string;
    segment: string;
    track_count_under: number;
    artwork: string | null;
  };
  type TrackEntry = {
    kind: 'track';
    path: string;
    segment: string;
  };
  type FolderTreeEntry = FolderEntry | TrackEntry;

  interface Props {
    node: FolderTreeEntry;
    depth?: number;
    selectedPath: string | null;
    expandedPaths: SvelteSet<string>;
    onSelect: (path: string) => void;
    onToggleExpand: (path: string) => void;
    onPlayRecursive: (path: string) => void;
  }

  let {
    node,
    depth = 0,
    selectedPath,
    expandedPaths,
    onSelect,
    onToggleExpand,
    onPlayRecursive,
  }: Props = $props();

  const isFolder = $derived(node.kind === 'folder');
  const isExpanded = $derived(isFolder && expandedPaths.has(node.path));
  const isSelected = $derived(node.path === selectedPath);
  const trackCount = $derived(node.kind === 'folder' ? node.track_count_under : 0);
  const artworkUrl = $derived(
    node.kind === 'folder' && node.artwork ? convertFileSrc(node.artwork) : null
  );

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
      invoke<FolderTreeEntry[]>('v2_library_list_folder_children', { parentPath: folderPath })
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

  function handlePlayClick(e: MouseEvent) {
    e.stopPropagation();
    if (isFolder) onPlayRecursive(node.path);
  }
</script>

<div
  class="folder-tree-row"
  class:selected={isSelected}
  class:folder={isFolder}
  class:track={!isFolder}
  style:padding-left="{depth * 16 + 8}px"
  role="treeitem"
  tabindex="0"
  aria-selected={isSelected}
  aria-expanded={isFolder ? isExpanded : undefined}
  onclick={handleRowClick}
  onkeydown={handleRowKey}
>
  {#if isFolder}
    <button
      type="button"
      class="chevron-btn"
      onclick={handleChevronClick}
      aria-label={isExpanded ? 'Collapse' : 'Expand'}
    >
      {#if isExpanded}
        <ChevronDown size={14} />
      {:else}
        <ChevronRight size={14} />
      {/if}
    </button>
  {:else}
    <span class="chevron-spacer"></span>
  {/if}

  {#if isFolder && artworkUrl}
    <img class="row-thumb" src={artworkUrl} alt="" loading="lazy" decoding="async" />
  {:else if isFolder}
    <span class="row-icon folder-icon">
      <Folder size={16} />
    </span>
  {:else}
    <span class="row-icon track-icon">
      <FileMusic size={16} />
    </span>
  {/if}

  <div class="row-meta">
    <span class="row-name" title={node.segment}>{node.segment}</span>
    {#if isFolder}
      <span class="row-subtitle">
        {$t('library.foldersTree.treeFolderTracks', { values: { count: trackCount } })}
      </span>
    {/if}
  </div>

  {#if isFolder}
    <button
      type="button"
      class="play-btn"
      onclick={handlePlayClick}
      aria-label={$t('library.foldersTree.playAllRecursive')}
      title={$t('library.foldersTree.playAllRecursive')}
    >
      <Play size={12} fill="currentColor" />
    </button>
  {/if}
</div>

{#if isFolder && isExpanded}
  {#if loading}
    <div class="folder-tree-loading" style:padding-left="{(depth + 1) * 16 + 8}px">
      {$t('library.foldersTree.treeLoading')}
    </div>
  {:else if loadError}
    <div
      class="folder-tree-error"
      style:padding-left="{(depth + 1) * 16 + 8}px"
      title={loadError}
    >
      {loadError}
    </div>
  {:else if children}
    {#if children.length === 0}
      <div class="folder-tree-empty" style:padding-left="{(depth + 1) * 16 + 8}px">
        {$t('library.foldersTree.treeEmpty')}
      </div>
    {:else}
      {#each children as child (child.path)}
        <Self
          node={child}
          depth={depth + 1}
          {selectedPath}
          {expandedPaths}
          {onSelect}
          {onToggleExpand}
          {onPlayRecursive}
        />
      {/each}
    {/if}
  {/if}
{/if}

<style>
  .folder-tree-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    cursor: pointer;
    border-radius: 4px;
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
    width: 16px;
    height: 16px;
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
    width: 16px;
    flex-shrink: 0;
  }

  .row-thumb {
    width: 24px;
    height: 24px;
    border-radius: 3px;
    object-fit: cover;
    flex-shrink: 0;
  }
  .row-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .row-icon.folder-icon {
    color: var(--accent-primary, var(--text-secondary));
  }
  .row-icon.track-icon {
    color: var(--text-muted);
  }

  .row-meta {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .row-name {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-subtitle {
    font-size: 11px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .play-btn {
    display: none;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex-shrink: 0;
    border: none;
    background: var(--accent-primary);
    color: var(--btn-primary-text, white);
    cursor: pointer;
    border-radius: 50%;
    padding: 0;
  }
  .folder-tree-row:hover .play-btn,
  .folder-tree-row:focus-within .play-btn {
    display: inline-flex;
  }

  .folder-tree-loading,
  .folder-tree-empty,
  .folder-tree-error {
    padding: 4px 8px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .folder-tree-error {
    color: var(--error, #f55);
  }
</style>
