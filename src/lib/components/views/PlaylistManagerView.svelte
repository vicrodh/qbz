<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, Filter, ArrowUpDown, LayoutGrid, List, GripVertical, EyeOff, Eye, BarChart2, Play, Pencil, Search, X } from 'lucide-svelte';
  import PlaylistCollage from '../PlaylistCollage.svelte';
  import PlaylistModal from '../PlaylistModal.svelte';

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
    images?: string[];
    duration: number;
    owner: { id: number; name: string };
  }

  interface PlaylistSettings {
    qobuz_playlist_id: number;
    hidden: boolean;
    position: number;
  }

  interface PlaylistStats {
    qobuz_playlist_id: number;
    play_count: number;
    last_played_at?: number;
  }

  type PlaylistFilter = 'all' | 'visible' | 'hidden';
  type PlaylistSort = 'name' | 'recent' | 'playcount' | 'custom';
  type ViewMode = 'list' | 'grid';

  interface Props {
    onBack?: () => void;
    onPlaylistSelect?: (playlistId: number) => void;
  }

  let { onBack, onPlaylistSelect }: Props = $props();

  let playlists = $state<Playlist[]>([]);
  let playlistSettings = $state<Map<number, PlaylistSettings>>(new Map());
  let playlistStats = $state<Map<number, PlaylistStats>>(new Map());
  let loading = $state(true);

  // Filter and sort state (persisted)
  let filter = $state<PlaylistFilter>(
    (localStorage.getItem('qbz-pm-filter') as PlaylistFilter) || 'all'
  );
  let sort = $state<PlaylistSort>(
    (localStorage.getItem('qbz-pm-sort') as PlaylistSort) || 'name'
  );
  let viewMode = $state<ViewMode>(
    (localStorage.getItem('qbz-pm-view') as ViewMode) || 'grid'
  );

  // Search state
  let searchQuery = $state('');

  // Dropdown state
  let showFilterMenu = $state(false);
  let showSortMenu = $state(false);

  // Edit modal state
  let editModalOpen = $state(false);
  let editingPlaylist = $state<Playlist | null>(null);

  // Drag state
  let draggedId = $state<number | null>(null);
  let dragOverId = $state<number | null>(null);

  // Persist preferences
  $effect(() => { localStorage.setItem('qbz-pm-filter', filter); });
  $effect(() => { localStorage.setItem('qbz-pm-sort', sort); });
  $effect(() => { localStorage.setItem('qbz-pm-view', viewMode); });

  // Filtered and sorted playlists
  const displayPlaylists = $derived.by(() => {
    let result = [...playlists];

    // Apply search filter
    if (searchQuery.trim()) {
      const query = searchQuery.trim().toLowerCase();
      result = result.filter(p => p.name.toLowerCase().includes(query));
    }

    // Apply visibility filter
    if (filter === 'visible') {
      result = result.filter(p => !playlistSettings.get(p.id)?.hidden);
    } else if (filter === 'hidden') {
      result = result.filter(p => playlistSettings.get(p.id)?.hidden);
    }

    // Apply sort
    if (sort === 'name') {
      result.sort((a, b) => a.name.localeCompare(b.name));
    } else if (sort === 'playcount') {
      result.sort((a, b) => {
        const countA = playlistStats.get(a.id)?.play_count ?? 0;
        const countB = playlistStats.get(b.id)?.play_count ?? 0;
        return countB - countA;
      });
    } else if (sort === 'custom') {
      result.sort((a, b) => {
        const posA = playlistSettings.get(a.id)?.position ?? 999;
        const posB = playlistSettings.get(b.id)?.position ?? 999;
        return posA - posB;
      });
    }
    // 'recent' keeps original order from API

    return result;
  });

  onMount(() => {
    loadData();
  });

  async function loadData() {
    loading = true;
    try {
      const [playlistsResult, settingsResult, statsResult] = await Promise.all([
        invoke<Playlist[]>('get_user_playlists'),
        invoke<PlaylistSettings[]>('playlist_get_all_settings'),
        invoke<PlaylistStats[]>('playlist_get_all_stats')
      ]);

      playlists = playlistsResult;

      const settingsMap = new Map<number, PlaylistSettings>();
      for (const s of settingsResult) {
        settingsMap.set(s.qobuz_playlist_id, s);
      }
      playlistSettings = settingsMap;

      const statsMap = new Map<number, PlaylistStats>();
      for (const s of statsResult) {
        statsMap.set(s.qobuz_playlist_id, s);
      }
      playlistStats = statsMap;
    } catch (err) {
      console.error('Failed to load playlists:', err);
    } finally {
      loading = false;
    }
  }

  function formatDuration(seconds: number): string {
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    if (hours > 0) {
      return `${hours}h ${mins}m`;
    }
    return `${mins}m`;
  }

  function openEditModal(playlist: Playlist) {
    editingPlaylist = playlist;
    editModalOpen = true;
  }

  function handleEditSuccess() {
    editModalOpen = false;
    editingPlaylist = null;
    loadData(); // Refresh
  }

  function handleDelete(playlistId: number) {
    editModalOpen = false;
    editingPlaylist = null;
    loadData(); // Refresh
  }

  async function toggleHidden(playlist: Playlist) {
    const current = playlistSettings.get(playlist.id);
    const newHidden = !current?.hidden;
    try {
      await invoke('playlist_set_hidden', { playlistId: playlist.id, hidden: newHidden });
      const updated = new Map(playlistSettings);
      updated.set(playlist.id, { ...current, qobuz_playlist_id: playlist.id, hidden: newHidden, position: current?.position ?? 0 });
      playlistSettings = updated;
    } catch (err) {
      console.error('Failed to toggle hidden:', err);
    }
  }

  // Drag and drop handlers
  function handleDragStart(e: DragEvent, playlistId: number) {
    if (sort !== 'custom') return;
    draggedId = playlistId;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', playlistId.toString());
    }
  }

  function handleDragOver(e: DragEvent, playlistId: number) {
    if (sort !== 'custom' || !draggedId) return;
    e.preventDefault();
    dragOverId = playlistId;
  }

  function handleDragLeave() {
    dragOverId = null;
  }

  async function handleDrop(e: DragEvent, targetId: number) {
    e.preventDefault();
    if (sort !== 'custom' || !draggedId || draggedId === targetId) {
      draggedId = null;
      dragOverId = null;
      return;
    }

    // Reorder the playlists array
    const currentOrder = displayPlaylists.map(p => p.id);
    const draggedIndex = currentOrder.indexOf(draggedId);
    const targetIndex = currentOrder.indexOf(targetId);

    if (draggedIndex === -1 || targetIndex === -1) return;

    // Remove dragged item and insert at target position
    currentOrder.splice(draggedIndex, 1);
    currentOrder.splice(targetIndex, 0, draggedId);

    // Save new order
    try {
      await invoke('playlist_reorder', { playlistIds: currentOrder });
      // Update local settings
      const updated = new Map(playlistSettings);
      currentOrder.forEach((id, index) => {
        const existing = updated.get(id);
        updated.set(id, { ...existing, qobuz_playlist_id: id, hidden: existing?.hidden ?? false, position: index });
      });
      playlistSettings = updated;
    } catch (err) {
      console.error('Failed to reorder playlists:', err);
    }

    draggedId = null;
    dragOverId = null;
  }

  function handleDragEnd() {
    draggedId = null;
    dragOverId = null;
  }
</script>

<div class="playlist-manager">
  <!-- Header -->
  <div class="header">
    <button class="back-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>Back</span>
    </button>
    <h1>Playlist Manager</h1>
  </div>

  <!-- Controls -->
  <div class="controls">
    <!-- Search bar -->
    <div class="search-container">
      <Search size={16} class="search-icon" />
      <input
        type="text"
        placeholder="Search playlists..."
        bind:value={searchQuery}
        class="search-input"
      />
      {#if searchQuery}
        <button class="clear-search" onclick={() => searchQuery = ''}>
          <X size={14} />
        </button>
      {/if}
    </div>

    <!-- Filter dropdown -->
    <div class="dropdown-container">
      <button class="control-btn" onclick={() => { showFilterMenu = !showFilterMenu; showSortMenu = false; }}>
        {#if filter === 'hidden'}
          <EyeOff size={16} />
        {:else}
          <Filter size={16} />
        {/if}
        <span>
          {filter === 'all' ? 'All' : filter === 'visible' ? 'Visible' : 'Hidden'}
        </span>
      </button>
      {#if showFilterMenu}
        <div class="dropdown-menu">
          <button class="dropdown-item" class:selected={filter === 'all'} onclick={() => { filter = 'all'; showFilterMenu = false; }}>
            All
          </button>
          <button class="dropdown-item" class:selected={filter === 'visible'} onclick={() => { filter = 'visible'; showFilterMenu = false; }}>
            Visible
          </button>
          <button class="dropdown-item" class:selected={filter === 'hidden'} onclick={() => { filter = 'hidden'; showFilterMenu = false; }}>
            Hidden
          </button>
        </div>
      {/if}
    </div>

    <!-- Sort dropdown -->
    <div class="dropdown-container">
      <button class="control-btn" onclick={() => { showSortMenu = !showSortMenu; showFilterMenu = false; }}>
        <ArrowUpDown size={16} />
        <span>
          {sort === 'name' ? 'Name' : sort === 'recent' ? 'Recent' : sort === 'playcount' ? 'Play Count' : 'Custom'}
        </span>
      </button>
      {#if showSortMenu}
        <div class="dropdown-menu">
          <button class="dropdown-item" class:selected={sort === 'name'} onclick={() => { sort = 'name'; showSortMenu = false; }}>
            Name (A-Z)
          </button>
          <button class="dropdown-item" class:selected={sort === 'recent'} onclick={() => { sort = 'recent'; showSortMenu = false; }}>
            Recent
          </button>
          <button class="dropdown-item" class:selected={sort === 'playcount'} onclick={() => { sort = 'playcount'; showSortMenu = false; }}>
            Play Count
          </button>
          <button class="dropdown-item" class:selected={sort === 'custom'} onclick={() => { sort = 'custom'; showSortMenu = false; }}>
            Custom Order
          </button>
        </div>
      {/if}
    </div>

    <!-- View toggle -->
    <button class="control-btn icon-only" onclick={() => viewMode = viewMode === 'list' ? 'grid' : 'list'}>
      {#if viewMode === 'list'}
        <LayoutGrid size={16} />
      {:else}
        <List size={16} />
      {/if}
    </button>

    <span class="playlist-count">{displayPlaylists.length} playlists</span>
  </div>

  {#if sort === 'custom'}
    <p class="drag-hint">Drag playlists to reorder them</p>
  {/if}

  <!-- Content -->
  {#if loading}
    <div class="loading">
      <div class="spinner"></div>
      <p>Loading playlists...</p>
    </div>
  {:else if displayPlaylists.length === 0}
    <div class="empty">
      <p>{filter === 'hidden' ? 'No hidden playlists' : filter === 'visible' ? 'No visible playlists' : 'No playlists yet'}</p>
    </div>
  {:else if viewMode === 'grid'}
    <!-- Grid View -->
    <div class="grid">
      {#each displayPlaylists as playlist (playlist.id)}
        {@const isHidden = playlistSettings.get(playlist.id)?.hidden}
        <div
          class="grid-item"
          class:hidden={isHidden}
          class:dragging={draggedId === playlist.id}
          class:drag-over={dragOverId === playlist.id}
          draggable={sort === 'custom'}
          ondragstart={(e) => handleDragStart(e, playlist.id)}
          ondragover={(e) => handleDragOver(e, playlist.id)}
          ondragleave={handleDragLeave}
          ondrop={(e) => handleDrop(e, playlist.id)}
          ondragend={handleDragEnd}
        >
          <!-- Top row: drag handle (left) and edit button (right) -->
          <div class="grid-item-header">
            {#if sort === 'custom'}
              <div class="drag-handle">
                <GripVertical size={14} />
              </div>
            {:else}
              <div class="drag-handle-placeholder"></div>
            {/if}
            <button
              class="edit-btn"
              onclick={(e) => { e.stopPropagation(); openEditModal(playlist); }}
              title="Edit playlist"
            >
              <Pencil size={14} />
            </button>
          </div>

          <!-- Clickable area: artwork + info -->
          <div
            class="grid-item-content"
            role="button"
            tabindex="0"
            onclick={() => onPlaylistSelect?.(playlist.id)}
          >
            <div class="artwork">
              <PlaylistCollage artworks={playlist.images ?? []} size={140} />
              {#if isHidden}
                <div class="hidden-badge">
                  <EyeOff size={12} />
                </div>
              {/if}
            </div>
            <div class="info">
              <span class="name">{playlist.name}</span>
              <span class="meta">{playlist.tracks_count} tracks</span>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <!-- List View -->
    <div class="list">
      {#each displayPlaylists as playlist (playlist.id)}
        {@const isHidden = playlistSettings.get(playlist.id)?.hidden}
        {@const stats = playlistStats.get(playlist.id)}
        <div
          class="list-item"
          class:hidden={isHidden}
          class:dragging={draggedId === playlist.id}
          class:drag-over={dragOverId === playlist.id}
          draggable={sort === 'custom'}
          ondragstart={(e) => handleDragStart(e, playlist.id)}
          ondragover={(e) => handleDragOver(e, playlist.id)}
          ondragleave={handleDragLeave}
          ondrop={(e) => handleDrop(e, playlist.id)}
          ondragend={handleDragEnd}
          role="button"
          tabindex="0"
          onclick={() => onPlaylistSelect?.(playlist.id)}
        >
          {#if sort === 'custom'}
            <div class="drag-handle">
              <GripVertical size={16} />
            </div>
          {/if}
          <div class="artwork-small">
            <PlaylistCollage artworks={playlist.images ?? []} size={48} />
          </div>
          <div class="info">
            <span class="name">{playlist.name}</span>
            <span class="meta">
              {playlist.tracks_count} tracks
              {#if playlist.duration > 0}
                <span class="dot">.</span>
                {formatDuration(playlist.duration)}
              {/if}
            </span>
          </div>
          {#if stats && stats.play_count > 0}
            <span class="play-count-badge" title="Play count">
              <BarChart2 size={12} />
              {stats.play_count}
            </span>
          {/if}
          {#if isHidden}
            <span class="hidden-indicator" title="Hidden from sidebar">
              <EyeOff size={14} />
            </span>
          {/if}
          <button
            class="edit-btn"
            onclick={(e) => { e.stopPropagation(); openEditModal(playlist); }}
            title="Edit playlist"
          >
            <Pencil size={14} />
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>

<!-- Edit Modal -->
{#if editingPlaylist}
  <PlaylistModal
    isOpen={editModalOpen}
    mode="edit"
    playlist={{ id: editingPlaylist.id, name: editingPlaylist.name, tracks_count: editingPlaylist.tracks_count }}
    isHidden={playlistSettings.get(editingPlaylist.id)?.hidden ?? false}
    onClose={() => { editModalOpen = false; editingPlaylist = null; }}
    onSuccess={handleEditSuccess}
    onDelete={handleDelete}
  />
{/if}

<style>
  .playlist-manager {
    padding: 24px;
    padding-right: 8px;
    padding-bottom: 100px;
    height: 100%;
    overflow-y: auto;
  }

  /* Custom scrollbar */
  .playlist-manager::-webkit-scrollbar {
    width: 6px;
  }

  .playlist-manager::-webkit-scrollbar-track {
    background: transparent;
  }

  .playlist-manager::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .playlist-manager::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 24px;
  }

  .header h1 {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-primary);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
    flex-wrap: wrap;
  }

  .dropdown-container {
    position: relative;
  }

  .control-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .control-btn:hover {
    background: var(--bg-hover);
  }

  .control-btn.icon-only {
    padding: 8px;
  }

  .dropdown-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 4px;
    min-width: 140px;
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .dropdown-item {
    display: block;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    border-radius: 4px;
    transition: background-color 150ms ease;
  }

  .dropdown-item:hover {
    background: var(--bg-tertiary);
  }

  .dropdown-item.selected {
    color: var(--accent-primary);
  }

  .playlist-count {
    font-size: 13px;
    color: var(--text-muted);
    margin-left: auto;
  }

  /* Search bar */
  .search-container {
    position: relative;
    display: flex;
    align-items: center;
    flex: 1;
    max-width: 280px;
  }

  .search-container :global(.search-icon) {
    position: absolute;
    left: 10px;
    color: var(--text-muted);
    pointer-events: none;
  }

  .search-input {
    width: 100%;
    padding: 8px 32px 8px 34px;
    background: var(--bg-tertiary);
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    outline: none;
    transition: border-color 150ms ease;
  }

  .search-input:focus {
    border-color: var(--accent-primary);
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .clear-search {
    position: absolute;
    right: 6px;
    padding: 4px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    transition: all 150ms ease;
  }

  .clear-search:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .drag-hint {
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 16px;
  }

  .loading,
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px;
    color: var(--text-muted);
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Grid View */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, 180px);
    gap: 16px;
    justify-content: start;
  }

  .grid-item {
    width: 180px;
    display: flex;
    flex-direction: column;
    padding: 10px;
    background: var(--bg-secondary);
    border-radius: 8px;
    transition: all 150ms ease;
  }

  .grid-item:hover {
    background: var(--bg-tertiary);
  }

  .grid-item.hidden {
    opacity: 0.6;
  }

  .grid-item.dragging {
    opacity: 0.5;
    transform: scale(0.98);
  }

  .grid-item.drag-over {
    border: 2px dashed var(--accent-primary);
  }

  /* Grid item header: drag handle left, edit button right */
  .grid-item-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    height: 24px;
    margin-bottom: 6px;
  }

  .grid-item .drag-handle {
    color: var(--text-muted);
    cursor: grab;
    padding: 2px;
  }

  .grid-item .drag-handle:active {
    cursor: grabbing;
  }

  .drag-handle-placeholder {
    width: 18px;
  }

  .edit-btn {
    padding: 4px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 150ms ease;
  }

  .edit-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* Clickable content area */
  .grid-item-content {
    cursor: pointer;
    display: flex;
    flex-direction: column;
  }

  .grid-item .artwork {
    position: relative;
    width: 160px;
    height: 160px;
    margin: 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border-radius: 4px;
  }

  .hidden-badge {
    position: absolute;
    bottom: 4px;
    right: 4px;
    background: rgba(0, 0, 0, 0.7);
    border-radius: 4px;
    padding: 3px;
    color: var(--text-muted);
  }

  .grid-item .info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: 8px;
  }

  .grid-item .name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    line-height: 1.3;
  }

  .grid-item .meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  /* List View */
  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .list-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border-radius: 6px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .list-item:hover {
    background: var(--bg-tertiary);
  }

  .list-item.hidden {
    opacity: 0.6;
  }

  .list-item.dragging {
    opacity: 0.5;
  }

  .list-item.drag-over {
    border: 2px dashed var(--accent-primary);
  }

  .list-item .drag-handle {
    color: var(--text-muted);
    cursor: grab;
    flex-shrink: 0;
  }

  .artwork-small {
    flex-shrink: 0;
  }

  .list-item .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .list-item .name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .list-item .meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  .dot {
    margin: 0 4px;
  }

  .play-count-badge {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    background: var(--bg-tertiary);
    border-radius: 12px;
    font-size: 11px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .hidden-indicator {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .list-item .edit-btn {
    flex-shrink: 0;
  }
</style>
