<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, Filter, ArrowUpDown, LayoutGrid, List, GripVertical, EyeOff, Eye, BarChart2, Play, Pencil, Search, X, Cloud, CloudOff, Wifi, Heart, Folder, FolderPlus, ChevronRight, ChevronDown, Trash2 } from 'lucide-svelte';
  import PlaylistCollage from '../PlaylistCollage.svelte';
  import PlaylistModal from '../PlaylistModal.svelte';
  import { t } from '$lib/i18n';
  import {
    subscribe as subscribeOffline,
    getStatus as getOfflineStatus,
    getSettings as getOfflineSettings,
    type OfflineStatus,
    type OfflineSettings
  } from '$lib/stores/offlineStore';
  import {
    subscribe as subscribeFolders,
    getFolders,
    getVisibleFolders,
    loadFolders,
    createFolder,
    updateFolder,
    deleteFolder,
    movePlaylistToFolder,
    type PlaylistFolder
  } from '$lib/stores/playlistFoldersStore';

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
    images?: string[];
    duration: number;
    owner: { id: number; name: string };
  }

  type LocalContentStatus = 'unknown' | 'no' | 'some_local' | 'all_local';

  interface PlaylistSettings {
    qobuz_playlist_id: number;
    hidden: boolean;
    position: number;
    hasLocalContent?: LocalContentStatus;
    is_favorite?: boolean;
    folder_id?: string | null;
  }

  interface PlaylistStats {
    qobuz_playlist_id: number;
    play_count: number;
    last_played_at?: number;
  }

  type PlaylistFilter = 'all' | 'visible' | 'hidden' | 'offline_all' | 'offline_partial' | 'offline_unavailable';
  type PlaylistSort = 'name' | 'recent' | 'playcount' | 'tracks' | 'custom';
  type ViewMode = 'list' | 'grid';

  interface Props {
    onBack?: () => void;
    onPlaylistSelect?: (playlistId: number) => void;
    onPlaylistsChanged?: () => void;
  }

  let { onBack, onPlaylistSelect, onPlaylistsChanged }: Props = $props();

  let playlists = $state<Playlist[]>([]);
  let playlistSettings = $state<Map<number, PlaylistSettings>>(new Map());
  let playlistStats = $state<Map<number, PlaylistStats>>(new Map());
  let localTrackCounts = $state<Map<number, number>>(new Map());
  let pendingPlaylistsMap = $state<Map<number, import('$lib/stores/offlineStore').PendingPlaylist>>(new Map());
  let loading = $state(true);

  // Offline state
  let offlineStatus = $state<OfflineStatus>(getOfflineStatus());
  let offlineSettings = $state<OfflineSettings>(getOfflineSettings());

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
  let dragOverFolderId = $state<string | null>(null);

  // Folder state
  let folders = $state<PlaylistFolder[]>([]);
  let currentFolderId = $state<string | null>(null);
  let foldersCollapsed = $state(false);

  // Create/Edit folder modal state
  let showFolderModal = $state(false);
  let editingFolder = $state<PlaylistFolder | null>(null);
  let folderName = $state('');

  // Persist preferences
  $effect(() => { localStorage.setItem('qbz-pm-filter', filter); });
  $effect(() => { localStorage.setItem('qbz-pm-sort', sort); });
  $effect(() => { localStorage.setItem('qbz-pm-view', viewMode); });

  // Helper to get local content status for a playlist (calculated from actual data)
  function getLocalContentStatus(playlistId: number): LocalContentStatus {
    const playlist = playlists.find(p => p.id === playlistId);
    const localCount = localTrackCounts.get(playlistId) ?? 0;
    const qobuzCount = playlist?.tracks_count ?? 0;

    if (localCount === 0) {
      return 'no';
    } else if (qobuzCount === 0) {
      // Only local tracks - fully available offline
      return 'all_local';
    } else {
      // Mixed: has both local and Qobuz tracks - partially available
      return 'some_local';
    }
  }

  // Check if a playlist is available for interaction in offline mode
  function isPlaylistAvailableOffline(playlistId: number): boolean {
    if (!offlineStatus.isOffline) return true;
    const localStatus = getLocalContentStatus(playlistId);
    if (localStatus === 'all_local') return true;
    if (localStatus === 'some_local' && offlineSettings.showPartialPlaylists) return true;
    return false;
  }

  // Filtered and sorted playlists
  const displayPlaylists = $derived.by(() => {
    let result = [...playlists];

    // Apply search filter first
    if (searchQuery.trim()) {
      const query = searchQuery.trim().toLowerCase();
      result = result.filter(p => p.name.toLowerCase().includes(query));
    }

    // Apply filter based on mode (offline or regular)
    if (offlineStatus.isOffline) {
      // In offline mode, use offline-specific filters
      if (filter === 'offline_all' || filter === 'all') {
        // Show only playlists where ALL tracks are available offline
        result = result.filter(p => {
          const localStatus = getLocalContentStatus(p.id);
          return localStatus === 'all_local';
        });
      } else if (filter === 'offline_partial') {
        // Show only playlists with partial local content
        result = result.filter(p => {
          const localStatus = getLocalContentStatus(p.id);
          return localStatus === 'some_local';
        });
      } else if (filter === 'offline_unavailable') {
        // Show playlists with NO local content (view-only)
        result = result.filter(p => {
          const localStatus = getLocalContentStatus(p.id);
          return localStatus === 'no' || localStatus === 'unknown';
        });
      } else if (filter === 'visible') {
        result = result.filter(p => {
          const settings = playlistSettings.get(p.id);
          return !settings?.hidden && isPlaylistAvailableOffline(p.id);
        });
      } else if (filter === 'hidden') {
        result = result.filter(p => playlistSettings.get(p.id)?.hidden);
      }
    } else {
      // Regular online mode filters
      if (filter === 'visible') {
        result = result.filter(p => !playlistSettings.get(p.id)?.hidden);
      } else if (filter === 'hidden') {
        result = result.filter(p => playlistSettings.get(p.id)?.hidden);
      }
      // 'all' shows everything
    }

    // Filter by current folder
    result = result.filter(p => {
      const settings = playlistSettings.get(p.id);
      const playlistFolderId = settings?.folder_id ?? null;
      return playlistFolderId === currentFolderId;
    });

    // Apply sort
    if (sort === 'name') {
      result.sort((a, b) => a.name.localeCompare(b.name));
    } else if (sort === 'playcount') {
      result.sort((a, b) => {
        const countA = playlistStats.get(a.id)?.play_count ?? 0;
        const countB = playlistStats.get(b.id)?.play_count ?? 0;
        return countB - countA;
      });
    } else if (sort === 'tracks') {
      result.sort((a, b) => {
        const countA = getTotalTrackCount(a);
        const countB = getTotalTrackCount(b);
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

  // Get current folder info
  const currentFolder = $derived(
    currentFolderId ? folders.find(f => f.id === currentFolderId) : null
  );

  // Get playlist count for a folder
  function getPlaylistCountInFolder(folderId: string): number {
    return playlists.filter(p => {
      const settings = playlistSettings.get(p.id);
      return settings?.folder_id === folderId;
    }).length;
  }

  onMount(() => {
    loadData();
    loadFolders();

    // Subscribe to offline state changes
    const unsubscribeOffline = subscribeOffline(() => {
      offlineStatus = getOfflineStatus();
      offlineSettings = getOfflineSettings();
    });

    // Subscribe to folder changes
    const unsubscribeFolders = subscribeFolders(() => {
      folders = getVisibleFolders();
    });

    return () => {
      unsubscribeOffline();
      unsubscribeFolders();
    };
  });

  async function loadData() {
    loading = true;
    try {
      if (offlineStatus.isOffline) {
        // Offline mode: Load both regular playlists AND pending playlists
        const [playlistsResult, pendingPlaylistsResult, settingsResult, statsResult, localCountsResult] = await Promise.all([
          invoke<Playlist[]>('get_user_playlists'),
          invoke<import('$lib/stores/offlineStore').PendingPlaylist[]>('get_pending_playlists'),
          invoke<PlaylistSettings[]>('playlist_get_all_settings'),
          invoke<PlaylistStats[]>('playlist_get_all_stats'),
          invoke<Record<string, number>>('playlist_get_all_local_track_counts')
        ]);

        // Process regular playlists
        playlists = playlistsResult;

        // Process pending playlists and add them to the playlists array
        const newPendingMap = new Map<number, import('$lib/stores/offlineStore').PendingPlaylist>();
        const pendingAsPlaylists: Playlist[] = pendingPlaylistsResult.map(p => {
          const negativeId = -p.id;
          newPendingMap.set(negativeId, p);

          return {
            id: negativeId,
            name: p.name,
            tracks_count: p.trackIds.length, // Only Qobuz tracks for correct filtering
            images: [],
            duration: 0,
            owner: { id: 0, name: 'You (Offline)' }
          };
        });

        // Combine regular and pending playlists
        playlists = [...playlistsResult, ...pendingAsPlaylists];
        pendingPlaylistsMap = newPendingMap;

        // Process settings
        const settingsMap = new Map<number, PlaylistSettings>();
        for (const s of settingsResult) {
          settingsMap.set(s.qobuz_playlist_id, s);
        }
        playlistSettings = settingsMap;

        // Process stats
        const statsMap = new Map<number, PlaylistStats>();
        for (const s of statsResult) {
          statsMap.set(s.qobuz_playlist_id, s);
        }
        playlistStats = statsMap;

        // Process local track counts for regular playlists
        const localCountsMap = new Map<number, number>();
        for (const [id, count] of Object.entries(localCountsResult)) {
          localCountsMap.set(Number(id), count);
        }

        // Add local track counts for pending playlists
        for (const [negativeId, pending] of newPendingMap.entries()) {
          localCountsMap.set(negativeId, pending.localTrackIds.length);
        }

        localTrackCounts = localCountsMap;
      } else {
        // Online mode: Load only regular playlists
        const [playlistsResult, settingsResult, statsResult, localCountsResult] = await Promise.all([
          invoke<Playlist[]>('get_user_playlists'),
          invoke<PlaylistSettings[]>('playlist_get_all_settings'),
          invoke<PlaylistStats[]>('playlist_get_all_stats'),
          invoke<Record<string, number>>('playlist_get_all_local_track_counts')
        ]);

        playlists = playlistsResult;
        pendingPlaylistsMap = new Map(); // Clear pending playlists when online

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

        const localCountsMap = new Map<number, number>();
        for (const [id, count] of Object.entries(localCountsResult)) {
          localCountsMap.set(Number(id), count);
        }
        localTrackCounts = localCountsMap;
      }
    } catch (err) {
      console.error('Failed to load playlists:', err);
    } finally {
      loading = false;
    }
  }

  // Get total track count including local tracks
  function getTotalTrackCount(playlist: Playlist): number {
    const localCount = localTrackCounts.get(playlist.id) ?? 0;
    return playlist.tracks_count + localCount;
  }

  // Get local track count for a playlist
  function getLocalTrackCount(playlistId: number): number {
    return localTrackCounts.get(playlistId) ?? 0;
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
    onPlaylistsChanged?.();
  }

  function handleDelete(playlistId: number) {
    editModalOpen = false;
    editingPlaylist = null;
    loadData(); // Refresh
    onPlaylistsChanged?.();
  }

  async function toggleHidden(playlist: Playlist) {
    const current = playlistSettings.get(playlist.id);
    const newHidden = !current?.hidden;
    try {
      await invoke('playlist_set_hidden', { playlistId: playlist.id, hidden: newHidden });
      const updated = new Map(playlistSettings);
      updated.set(playlist.id, { ...current, qobuz_playlist_id: playlist.id, hidden: newHidden, position: current?.position ?? 0 });
      playlistSettings = updated;
      onPlaylistsChanged?.();
    } catch (err) {
      console.error('Failed to toggle hidden:', err);
    }
  }

  async function toggleFavorite(playlist: Playlist) {
    const current = playlistSettings.get(playlist.id);
    const newFavorite = !current?.is_favorite;
    try {
      await invoke('playlist_set_favorite', { playlistId: playlist.id, favorite: newFavorite });
      const updated = new Map(playlistSettings);
      updated.set(playlist.id, { ...current, qobuz_playlist_id: playlist.id, is_favorite: newFavorite, hidden: current?.hidden ?? false, position: current?.position ?? 0 });
      playlistSettings = updated;
      onPlaylistsChanged?.();
    } catch (err) {
      console.error('Failed to toggle favorite:', err);
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
      onPlaylistsChanged?.();
    } catch (err) {
      console.error('Failed to reorder playlists:', err);
    }

    draggedId = null;
    dragOverId = null;
  }

  function handleDragEnd() {
    draggedId = null;
    dragOverId = null;
    dragOverFolderId = null;
  }

  // === Folder Navigation ===

  function navigateToFolder(folderId: string | null) {
    currentFolderId = folderId;
  }

  function navigateToRoot() {
    currentFolderId = null;
  }

  // === Folder Drag & Drop ===

  function handleFolderDragOver(e: DragEvent, folderId: string) {
    e.preventDefault();
    if (draggedId) {
      dragOverFolderId = folderId;
    }
  }

  function handleFolderDragLeave() {
    dragOverFolderId = null;
  }

  async function handleFolderDrop(e: DragEvent, folderId: string) {
    e.preventDefault();
    if (!draggedId) return;

    // Move playlist to folder
    const success = await movePlaylistToFolder(draggedId, folderId);
    if (success) {
      // Update local settings
      const updated = new Map(playlistSettings);
      const existing = updated.get(draggedId);
      if (existing) {
        updated.set(draggedId, { ...existing, folder_id: folderId });
        playlistSettings = updated;
      }
      onPlaylistsChanged?.();
    }

    draggedId = null;
    dragOverId = null;
    dragOverFolderId = null;
  }

  // === Folder Modal ===

  function openCreateFolderModal() {
    editingFolder = null;
    folderName = '';
    showFolderModal = true;
  }

  function openEditFolderModal(folder: PlaylistFolder) {
    editingFolder = folder;
    folderName = folder.name;
    showFolderModal = true;
  }

  function closeFolderModal() {
    showFolderModal = false;
    editingFolder = null;
    folderName = '';
  }

  async function handleSaveFolder() {
    if (!folderName.trim()) return;

    if (editingFolder) {
      // Update existing folder (name only for now)
      await updateFolder(editingFolder.id, {
        name: folderName.trim()
      });
    } else {
      // Create new folder (simple, name only)
      await createFolder(folderName.trim());
    }

    folders = getVisibleFolders();
    closeFolderModal();
  }

  async function handleDeleteFolder() {
    if (!editingFolder) return;

    const confirmed = confirm(`Delete folder "${editingFolder.name}"? Playlists will be moved to root.`);
    if (!confirmed) return;

    await deleteFolder(editingFolder.id);
    folders = getVisibleFolders();

    // If we're inside the deleted folder, go back to root
    if (currentFolderId === editingFolder.id) {
      currentFolderId = null;
    }

    closeFolderModal();
    onPlaylistsChanged?.();
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

  <!-- Breadcrumb Navigation (when inside a folder) -->
  {#if currentFolderId && currentFolder}
    <div class="breadcrumb">
      <button class="breadcrumb-item" onclick={navigateToRoot}>
        All Playlists
      </button>
      <ChevronRight size={14} class="breadcrumb-separator" />
      <span class="breadcrumb-current">{currentFolder.name}</span>
    </div>
  {/if}

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
        {:else if filter === 'offline_unavailable'}
          <CloudOff size={16} />
        {:else}
          <Filter size={16} />
        {/if}
        <span>
          {#if offlineStatus.isOffline}
            {filter === 'all' || filter === 'offline_all' ? $t('offline.available') : filter === 'offline_partial' ? $t('offline.partiallyAvailable') : filter === 'offline_unavailable' ? $t('offline.notAvailableOffline') : filter === 'visible' ? 'Visible' : 'Hidden'}
          {:else}
            {filter === 'all' ? 'All' : filter === 'visible' ? 'Visible' : 'Hidden'}
          {/if}
        </span>
      </button>
      {#if showFilterMenu}
        <div class="dropdown-menu">
          {#if offlineStatus.isOffline}
            <button class="dropdown-item" class:selected={filter === 'all' || filter === 'offline_all'} onclick={() => { filter = 'offline_all'; showFilterMenu = false; }}>
              {$t('offline.available')}
            </button>
            <button class="dropdown-item" class:selected={filter === 'offline_partial'} onclick={() => { filter = 'offline_partial'; showFilterMenu = false; }}>
              {$t('offline.partiallyAvailable')}
            </button>
            <button class="dropdown-item" class:selected={filter === 'offline_unavailable'} onclick={() => { filter = 'offline_unavailable'; showFilterMenu = false; }}>
              {$t('offline.notAvailableOffline')}
            </button>
            <div class="dropdown-divider"></div>
          {/if}
          <button class="dropdown-item" class:selected={filter === 'all' && !offlineStatus.isOffline} onclick={() => { filter = 'all'; showFilterMenu = false; }}>
            {offlineStatus.isOffline ? $t('filter.all') : 'All'}
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
          {sort === 'name' ? 'Name' : sort === 'recent' ? 'Recent' : sort === 'playcount' ? 'Play Count' : sort === 'tracks' ? 'Track Count' : 'Custom'}
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
          <button class="dropdown-item" class:selected={sort === 'tracks'} onclick={() => { sort = 'tracks'; showSortMenu = false; }}>
            Track Count
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

    {#if !currentFolderId}
      <button class="control-btn" onclick={openCreateFolderModal}>
        <FolderPlus size={16} />
        <span>New Folder</span>
      </button>
    {/if}

    <span class="playlist-count">
      {#if !currentFolderId && folders.length > 0}
        {folders.length} folders, {displayPlaylists.length} playlists
      {:else}
        {displayPlaylists.length} playlists
      {/if}
    </span>
  </div>

  {#if sort === 'custom'}
    <p class="drag-hint">Drag playlists to reorder them{#if !currentFolderId && folders.length > 0}, or drop onto a folder to move{/if}</p>
  {/if}

  <!-- Content -->
  {#if loading}
    <div class="loading">
      <div class="spinner"></div>
      <p>Loading playlists...</p>
    </div>
  {:else}
    <!-- Folders Section (only at root level) -->
    {#if !currentFolderId && folders.length > 0}
      <div class="folders-section">
        <button
          class="section-header-btn"
          onclick={() => foldersCollapsed = !foldersCollapsed}
        >
          <span class="section-title">Folders ({folders.length})</span>
          {#if foldersCollapsed}
            <ChevronRight size={14} />
          {:else}
            <ChevronDown size={14} />
          {/if}
        </button>

        {#if !foldersCollapsed}
          <div class="folders-grid">
            {#each folders as folder (folder.id)}
              <div
                class="folder-card"
                class:drag-over={dragOverFolderId === folder.id}
                ondragover={(e) => handleFolderDragOver(e, folder.id)}
                ondragleave={handleFolderDragLeave}
                ondrop={(e) => handleFolderDrop(e, folder.id)}
              >
                <div
                  class="folder-card-content"
                  role="button"
                  tabindex="0"
                  onclick={() => navigateToFolder(folder.id)}
                  onkeydown={(e) => e.key === 'Enter' && navigateToFolder(folder.id)}
                >
                  <div class="folder-icon">
                    <Folder size={32} />
                  </div>
                  <span class="folder-name">{folder.name}</span>
                  <span class="folder-count">{getPlaylistCountInFolder(folder.id)} playlists</span>
                </div>
                <button
                  class="folder-edit-btn"
                  onclick={(e) => { e.stopPropagation(); openEditFolderModal(folder); }}
                  title="Edit folder"
                >
                  <Pencil size={12} />
                </button>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Playlists Section -->
    {#if displayPlaylists.length === 0 && (currentFolderId || folders.length === 0)}
      <div class="empty">
        <p>{filter === 'hidden' ? 'No hidden playlists' : filter === 'visible' ? 'No visible playlists' : currentFolderId ? 'No playlists in this folder' : 'No playlists yet'}</p>
      </div>
    {:else if displayPlaylists.length > 0}
      {#if !currentFolderId && folders.length > 0}
        <div class="section-header-btn playlists-section-header">
          <span class="section-title">Playlists ({displayPlaylists.length})</span>
        </div>
      {/if}

      {#if viewMode === 'grid'}
    <!-- Grid View -->
    <div class="grid">
      {#each displayPlaylists as playlist (playlist.id)}
        {@const isHidden = playlistSettings.get(playlist.id)?.hidden}
        {@const isFavorite = playlistSettings.get(playlist.id)?.is_favorite}
        {@const localStatus = getLocalContentStatus(playlist.id)}
        {@const isUnavailable = offlineStatus.isOffline && !isPlaylistAvailableOffline(playlist.id)}
        <div
          class="grid-item"
          class:hidden={isHidden}
          class:unavailable={isUnavailable}
          class:dragging={draggedId === playlist.id}
          class:drag-over={dragOverId === playlist.id}
          draggable={sort === 'custom' && !isUnavailable}
          ondragstart={(e) => !isUnavailable && handleDragStart(e, playlist.id)}
          ondragover={(e) => !isUnavailable && handleDragOver(e, playlist.id)}
          ondragleave={handleDragLeave}
          ondrop={(e) => !isUnavailable && handleDrop(e, playlist.id)}
          ondragend={handleDragEnd}
        >
          <!-- Top row: drag handle only (when in custom sort mode) -->
          {#if sort === 'custom' && !isUnavailable}
            <div class="grid-item-header">
              <div class="drag-handle">
                <GripVertical size={14} />
              </div>
            </div>
          {/if}

          <!-- Clickable area: artwork + info -->
          <div
            class="grid-item-content"
            role="button"
            tabindex="0"
            onclick={() => onPlaylistSelect?.(playlist.id)}
            title={isUnavailable ? $t('offline.viewOnly') : undefined}
          >
            <div class="artwork">
              <PlaylistCollage artworks={playlist.images ?? []} size={140} />
              {#if localStatus === 'all_local'}
                <div class="local-badge all" title={$t('offline.allLocal')}>
                  <Wifi size={12} />
                </div>
              {:else if localStatus === 'some_local'}
                <div class="local-badge partial" title={$t('offline.someLocal')}>
                  <Cloud size={12} />
                </div>
              {/if}
            </div>
            <div class="info">
              <span class="name">{playlist.name}</span>
            </div>
          </div>

          <!-- Footer: meta + action buttons inline -->
          <div class="grid-item-footer">
            <span class="meta">{getTotalTrackCount(playlist)} tracks{#if getLocalTrackCount(playlist.id) > 0} <span class="local-count">({getLocalTrackCount(playlist.id)} local)</span>{/if}</span>
            {#if !isUnavailable}
              <div class="footer-actions">
                <button
                  class="favorite-btn"
                  class:is-active={isFavorite}
                  onclick={(e) => { e.stopPropagation(); toggleFavorite(playlist); }}
                  title={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
                >
                  <Heart size={12} fill={isFavorite ? 'var(--accent-primary)' : 'none'} color={isFavorite ? 'var(--accent-primary)' : 'currentColor'} />
                </button>
                <button
                  class="visibility-btn"
                  class:is-hidden={isHidden}
                  onclick={(e) => { e.stopPropagation(); toggleHidden(playlist); }}
                  title={isHidden ? 'Show in sidebar' : 'Hide from sidebar'}
                >
                  {#if isHidden}
                    <EyeOff size={12} />
                  {:else}
                    <Eye size={12} />
                  {/if}
                </button>
                <button
                  class="edit-btn"
                  onclick={(e) => { e.stopPropagation(); openEditModal(playlist); }}
                  title="Edit playlist"
                >
                  <Pencil size={12} />
                </button>
              </div>
            {:else}
              <span class="view-only-badge" title={$t('offline.viewOnly')}>
                <CloudOff size={12} />
              </span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <!-- List View -->
    <div class="list">
      {#each displayPlaylists as playlist (playlist.id)}
        {@const isHidden = playlistSettings.get(playlist.id)?.hidden}
        {@const isFavorite = playlistSettings.get(playlist.id)?.is_favorite}
        {@const stats = playlistStats.get(playlist.id)}
        {@const localStatus = getLocalContentStatus(playlist.id)}
        {@const isUnavailable = offlineStatus.isOffline && !isPlaylistAvailableOffline(playlist.id)}
        <div
          class="list-item"
          class:hidden={isHidden}
          class:unavailable={isUnavailable}
          class:dragging={draggedId === playlist.id}
          class:drag-over={dragOverId === playlist.id}
          draggable={sort === 'custom' && !isUnavailable}
          ondragstart={(e) => !isUnavailable && handleDragStart(e, playlist.id)}
          ondragover={(e) => !isUnavailable && handleDragOver(e, playlist.id)}
          ondragleave={handleDragLeave}
          ondrop={(e) => !isUnavailable && handleDrop(e, playlist.id)}
          ondragend={handleDragEnd}
          role="button"
          tabindex="0"
          onclick={() => onPlaylistSelect?.(playlist.id)}
          title={isUnavailable ? $t('offline.viewOnly') : undefined}
        >
          {#if sort === 'custom' && !isUnavailable}
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
              {getTotalTrackCount(playlist)} tracks{#if getLocalTrackCount(playlist.id) > 0} <span class="local-count">({getLocalTrackCount(playlist.id)} local)</span>{/if}
              {#if playlist.duration > 0}
                <span class="dot">.</span>
                {formatDuration(playlist.duration)}
              {/if}
            </span>
          </div>
          {#if isUnavailable}
            <span class="unavailable-badge" title={$t('offline.viewOnly')}>
              <CloudOff size={14} />
            </span>
          {:else if localStatus === 'all_local'}
            <span class="local-indicator all" title={$t('offline.allLocal')}>
              <Wifi size={14} />
            </span>
          {:else if localStatus === 'some_local'}
            <span class="local-indicator partial" title={$t('offline.someLocal')}>
              <Cloud size={14} />
            </span>
          {/if}
          {#if stats && stats.play_count > 0}
            <span class="play-count-badge" title="Play count">
              <BarChart2 size={12} />
              {stats.play_count}
            </span>
          {/if}
          {#if !isUnavailable}
            <button
              class="favorite-btn"
              class:is-active={isFavorite}
              onclick={(e) => { e.stopPropagation(); toggleFavorite(playlist); }}
              title={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
            >
              <Heart size={14} fill={isFavorite ? 'var(--accent-primary)' : 'none'} color={isFavorite ? 'var(--accent-primary)' : 'currentColor'} />
            </button>
            <button
              class="visibility-btn"
              class:is-hidden={isHidden}
              onclick={(e) => { e.stopPropagation(); toggleHidden(playlist); }}
              title={isHidden ? 'Show in sidebar' : 'Hide from sidebar'}
            >
              {#if isHidden}
                <EyeOff size={14} />
              {:else}
                <Eye size={14} />
              {/if}
            </button>
            <button
              class="edit-btn"
              onclick={(e) => { e.stopPropagation(); openEditModal(playlist); }}
              title="Edit playlist"
            >
              <Pencil size={14} />
            </button>
          {/if}
        </div>
      {/each}
    </div>
      {/if}
    {/if}
  {/if}
</div>

<!-- Folder Modal -->
{#if showFolderModal}
  <div class="modal-overlay" onclick={closeFolderModal} role="presentation">
    <div class="modal-content folder-modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
      <h2 class="modal-title">{editingFolder ? 'Edit Folder' : 'New Folder'}</h2>
      <div class="form-group">
        <label for="folder-name-input">Name</label>
        <input
          id="folder-name-input"
          type="text"
          bind:value={folderName}
          placeholder="Enter folder name"
          onkeydown={(e) => e.key === 'Enter' && handleSaveFolder()}
        />
      </div>
            <div class="modal-actions">
        {#if editingFolder}
          <button class="btn-danger" onclick={handleDeleteFolder}>
            <Trash2 size={14} />
            Delete
          </button>
        {/if}
        <div class="modal-actions-right">
          <button class="btn-secondary" onclick={closeFolderModal}>
            Cancel
          </button>
          <button
            class="btn-primary"
            onclick={handleSaveFolder}
            disabled={!folderName.trim()}
          >
            {editingFolder ? 'Save' : 'Create'}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

<!-- Edit Modal -->
{#if editingPlaylist}
  <PlaylistModal
    isOpen={editModalOpen}
    mode="edit"
    playlist={{ id: editingPlaylist.id, name: editingPlaylist.name, tracks_count: editingPlaylist.tracks_count }}
    isHidden={playlistSettings.get(editingPlaylist.id)?.hidden ?? false}
    currentFolderId={playlistSettings.get(editingPlaylist.id)?.folder_id ?? null}
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

  /* Breadcrumb */
  .breadcrumb {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 16px;
    font-size: 14px;
  }

  .breadcrumb-item {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0;
    transition: color 150ms ease;
  }

  .breadcrumb-item:hover {
    color: var(--text-primary);
    text-decoration: underline;
  }

  .breadcrumb :global(.breadcrumb-separator) {
    color: var(--text-muted);
  }

  .breadcrumb-current {
    color: var(--text-primary);
    font-weight: 500;
  }

  /* Folders Section */
  .folders-section {
    margin-bottom: 24px;
  }

  .section-header-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    background: none;
    border: none;
    padding: 8px 0;
    cursor: pointer;
    color: var(--text-secondary);
    transition: color 150ms ease;
  }

  .section-header-btn:hover {
    color: var(--text-primary);
  }

  .playlists-section-header {
    margin-top: 16px;
    margin-bottom: 8px;
    cursor: default;
  }

  .section-title {
    font-size: 14px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .folders-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 12px;
    margin-top: 12px;
  }

  .folder-card {
    position: relative;
    background: var(--bg-tertiary);
    border-radius: 10px;
    padding: 16px;
    transition: background-color 150ms ease, transform 150ms ease, box-shadow 150ms ease;
  }

  .folder-card:hover {
    background: var(--bg-hover);
  }

  .folder-card.drag-over {
    background: var(--accent-primary);
    transform: scale(1.02);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
  }

  .folder-card-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }

  .folder-icon {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .folder-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .folder-card .folder-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .folder-edit-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: var(--bg-secondary);
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity 150ms ease, background-color 150ms ease;
  }

  .folder-card:hover .folder-edit-btn {
    opacity: 1;
  }

  .folder-edit-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  /* Folder Modal */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-content {
    background: var(--bg-secondary);
    border-radius: 12px;
    padding: 24px;
    min-width: 320px;
    max-width: 400px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .modal-title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 20px 0;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 16px;
  }

  .form-group label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-muted);
  }

  .form-group input[type="text"] {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--alpha-10);
    border-radius: 8px;
    font-size: 14px;
    color: var(--text-primary);
    outline: none;
    transition: border-color 150ms ease;
  }

  .form-group input[type="text"]:focus {
    border-color: var(--accent-primary);
  }

  .modal-actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 20px;
  }

  .modal-actions-right {
    display: flex;
    gap: 12px;
  }

  .btn-secondary,
  .btn-primary,
  .btn-danger {
    padding: 8px 16px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
    transition: background-color 150ms ease, opacity 150ms ease;
  }

  .btn-secondary {
    background: var(--bg-tertiary);
    border: 1px solid var(--alpha-10);
    color: var(--text-primary);
  }

  .btn-secondary:hover {
    background: var(--bg-hover);
  }

  .btn-primary {
    background: var(--accent-primary);
    border: none;
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--accent-secondary);
  }

  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-danger {
    background: transparent;
    border: 1px solid var(--error);
    color: var(--error);
  }

  .btn-danger:hover {
    background: var(--error);
    color: white;
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
    max-height: 260px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--text-muted) transparent;
  }

  .dropdown-menu::-webkit-scrollbar {
    width: 8px;
  }

  .dropdown-menu::-webkit-scrollbar-track {
    background: transparent;
  }

  .dropdown-menu::-webkit-scrollbar-thumb {
    background: var(--text-muted);
    border-radius: 9999px;
  }

  .dropdown-menu::-webkit-scrollbar-thumb:hover {
    background: var(--text-secondary);
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

  .dropdown-divider {
    height: 1px;
    background: var(--bg-tertiary);
    margin: 4px 0;
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
    grid-template-columns: repeat(auto-fill, 160px);
    gap: 16px;
    justify-content: start;
  }

  .grid-item {
    width: 160px;
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

  /* Grid item header: drag handle only (when in custom sort) */
  .grid-item-header {
    display: flex;
    justify-content: flex-start;
    align-items: center;
    margin-bottom: 4px;
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

  .visibility-btn {
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

  .visibility-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .visibility-btn.is-hidden {
    color: var(--text-muted);
    opacity: 0.4;
  }

  .visibility-btn.is-hidden:hover {
    opacity: 1;
    color: var(--text-primary);
  }

  .favorite-btn {
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

  .favorite-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .favorite-btn.is-active {
    color: var(--accent-primary);
  }

  /* Grid item footer: meta + actions inline */
  .grid-item-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 8px;
    width: 140px;
    margin-left: auto;
    margin-right: auto;
  }

  .grid-item-footer .meta {
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
  }

  .grid-item-footer .local-count {
    color: var(--text-muted);
    opacity: 0.8;
  }

  .footer-actions {
    display: flex;
    align-items: center;
    gap: 0;
  }

  .grid-item-footer .favorite-btn,
  .grid-item-footer .visibility-btn,
  .grid-item-footer .edit-btn {
    padding: 2px;
  }

  /* Clickable content area */
  .grid-item-content {
    cursor: pointer;
    display: flex;
    flex-direction: column;
  }

  .grid-item .artwork {
    position: relative;
    width: 140px;
    height: 140px;
    margin: 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border-radius: 4px;
  }

  .local-badge {
    position: absolute;
    bottom: 4px;
    left: 4px;
    background: rgba(0, 0, 0, 0.7);
    border-radius: 4px;
    padding: 3px;
  }

  .local-badge.all {
    color: #4ade80;
  }

  .local-badge.partial {
    color: #fbbf24;
  }

  .local-indicator {
    display: flex;
    align-items: center;
    margin-right: 8px;
  }

  .local-indicator.all {
    color: #4ade80;
  }

  .local-indicator.partial {
    color: #fbbf24;
  }

  .grid-item .info {
    display: flex;
    flex-direction: column;
    margin-top: 8px;
    width: 140px;
    margin-left: auto;
    margin-right: auto;
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
    height: 34px; /* Fixed 2-line height: 13px * 1.3 * 2 */
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

  .local-count {
    color: var(--text-muted);
    opacity: 0.8;
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

  .list-item .favorite-btn,
  .list-item .visibility-btn,
  .list-item .edit-btn {
    flex-shrink: 0;
  }

  /* Unavailable playlist styles (offline mode) */
  .grid-item.unavailable,
  .list-item.unavailable {
    opacity: 0.5;
  }

  .grid-item.unavailable .artwork,
  .list-item.unavailable .artwork-small {
    filter: grayscale(100%);
  }

  .view-only-badge {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 2px;
    color: var(--text-muted);
  }

  .unavailable-badge {
    display: flex;
    align-items: center;
    color: var(--text-muted);
    margin-right: 8px;
    flex-shrink: 0;
  }
</style>
