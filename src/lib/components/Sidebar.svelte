<script lang="ts">
  import { tick } from 'svelte';
  import { Search, Home, HardDrive, Plus, RefreshCw, ChevronDown, ChevronUp, Heart, ListMusic, Import, Settings, MoreHorizontal, ArrowUpDown, ChevronRight, ChevronLeft, Folder, FolderPlus } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import NavigationItem from './NavigationItem.svelte';
  import UserCard from './UserCard.svelte';
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
    isFolderExpanded,
    toggleFolderExpanded,
    loadFolders,
    createFolder,
    movePlaylistToFolder,
    type PlaylistFolder
  } from '$lib/stores/playlistFoldersStore';

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
    images?: string[];
    duration?: number;
  }

  type LocalContentStatus = 'unknown' | 'no' | 'some_local' | 'all_local';

  interface PlaylistSettings {
    qobuz_playlist_id: number;
    hidden: boolean;
    position: number;
    play_count?: number;
    hasLocalContent?: LocalContentStatus;
    folder_id?: string | null;
  }

  type SortOption = 'name' | 'recent' | 'tracks' | 'playcount' | 'custom';

  interface Props {
    activeView: string;
    selectedPlaylistId?: number | null;
    onNavigate: (view: string) => void;
    onPlaylistSelect?: (playlistId: number) => void;
    onCreatePlaylist?: () => void;
    onImportPlaylist?: () => void;
    onPlaylistManagerClick?: () => void;
    onSettingsClick?: () => void;
    onAboutClick?: () => void;
    onLogout?: () => void;
    userName?: string;
    subscription?: string;
    isExpanded?: boolean;
    onToggle?: () => void;
    showTitleBar?: boolean;
  }

  let {
    activeView,
    selectedPlaylistId = null,
    onNavigate,
    onPlaylistSelect,
    onCreatePlaylist,
    onImportPlaylist,
    onPlaylistManagerClick,
    onSettingsClick,
    onAboutClick,
    onLogout,
    userName = 'User',
    subscription = 'Qobuz™',
    isExpanded = true,
    onToggle,
    showTitleBar = true
  }: Props = $props();

  let userPlaylists = $state<Playlist[]>([]);
  let playlistSettings = $state<Map<number, PlaylistSettings>>(new Map());
  let localTrackCounts = $state<Map<number, number>>(new Map());
  let pendingPlaylistsMap = $state<Map<number, import('$lib/stores/offlineStore').PendingPlaylist>>(new Map());
  let playlistsLoading = $state(false);
  let playlistsCollapsed = $state(false);
  let localLibraryCollapsed = $state(false);

  // Folder state
  let folders = $state<PlaylistFolder[]>([]);
  let folderExpandState = $state<Map<string, boolean>>(new Map());

  // Create folder modal state
  let showCreateFolderModal = $state(false);
  let newFolderName = $state('');

  // Context menu state
  let contextMenu = $state<{
    visible: boolean;
    x: number;
    y: number;
    playlist: Playlist | null;
    currentFolderId: string | null;
  }>({
    visible: false,
    x: 0,
    y: 0,
    playlist: null,
    currentFolderId: null
  });
  let contextMenuSearch = $state('');
  const FOLDER_SEARCH_THRESHOLD = 8;

  // Filtered folders for context menu
  const filteredContextFolders = $derived.by(() => {
    const available = folders.filter(f => f.id !== contextMenu.currentFolderId);
    if (!contextMenuSearch.trim()) return available;
    const query = contextMenuSearch.toLowerCase();
    return available.filter(f => f.name.toLowerCase().includes(query));
  });

  // Offline state
  let offlineStatus = $state<OfflineStatus>(getOfflineStatus());
  let offlineSettings = $state<OfflineSettings>(getOfflineSettings());
  let isOffline = $derived(offlineStatus.isOffline);

  // Dropdown menu state
  let menuOpen = $state(false);
  let sortSubmenuOpen = $state(false);
  let submenuCloseTimeout: ReturnType<typeof setTimeout> | null = null;
  let menuRef: HTMLDivElement | null = null;
  let menuEl: HTMLDivElement | null = null;
  let triggerRef: HTMLButtonElement | null = null;
  let sortTriggerRef: HTMLDivElement | null = null;
  let submenuEl: HTMLDivElement | null = null;
  let menuStyle = $state('');
  let submenuStyle = $state('');

  function openSubmenu() {
    if (submenuCloseTimeout) {
      clearTimeout(submenuCloseTimeout);
      submenuCloseTimeout = null;
    }
    sortSubmenuOpen = true;
  }

  function closeSubmenuDelayed() {
    if (submenuCloseTimeout) {
      clearTimeout(submenuCloseTimeout);
    }
    submenuCloseTimeout = setTimeout(() => {
      sortSubmenuOpen = false;
    }, 150); // Small delay to allow mouse to move to submenu
  }

  // Sort state with localStorage persistence
  let sortOption = $state<SortOption>('name');

  // Tooltip cache for playlist artists (non-reactive for reads during render)
  const playlistTooltipCache = new Map<number, string>();
  const tooltipLoadingIds = new Set<number>();

  // Get total track count including local tracks
  function getTotalTrackCount(playlist: Playlist): number {
    const localCount = localTrackCounts.get(playlist.id) ?? 0;
    return playlist.tracks_count + localCount;
  }

  // Fetch playlist artists for tooltip
  async function fetchPlaylistArtists(playlistId: number, trackCount: number): Promise<string> {
    interface PlaylistDetails {
      tracks?: {
        items: Array<{
          performer?: { name: string };
        }>;
      };
    }

    try {
      const details = await invoke<PlaylistDetails>('get_playlist', { playlistId });
      if (details.tracks?.items) {
        // Extract unique artist names
        const artistNames = new Set<string>();
        for (const track of details.tracks.items) {
          if (track.performer?.name) {
            artistNames.add(track.performer.name);
            if (artistNames.size >= 5) break;
          }
        }

        const artists = Array.from(artistNames).slice(0, 5);
        const trackText = `${trackCount} ${trackCount === 1 ? 'Track' : 'Tracks'}`;

        if (artists.length > 0) {
          return `${artists.join('\n')}\n${trackText}`;
        }
        return trackText;
      }
    } catch (err) {
      console.debug('Failed to fetch playlist artists:', err);
    }

    return `${trackCount} ${trackCount === 1 ? 'Track' : 'Tracks'}`;
  }

  // Format track count text with proper plural
  function formatTrackCount(total: number, localCount: number): string {
    const plural = total === 1 ? 'Track' : 'Tracks';
    if (localCount > 0) {
      return `${total} ${plural} (${localCount} local)`;
    }
    return `${total} ${plural}`;
  }

  // Get basic tooltip (no state mutation during render)
  function getPlaylistTooltip(playlist: Playlist, sidebarExpanded: boolean): string {
    // When sidebar is collapsed, show playlist name + track count
    if (!sidebarExpanded) {
      const totalCount = getTotalTrackCount(playlist);
      const trackText = totalCount === 1 ? 'track' : 'tracks';
      return `${playlist.name}\n${totalCount} ${trackText}`;
    }

    // When sidebar is expanded, use the rich tooltip with artists
    const cached = playlistTooltipCache.get(playlist.id);
    if (cached) return cached;

    // Return basic tooltip with combined count
    const totalCount = getTotalTrackCount(playlist);
    const localCount = localTrackCounts.get(playlist.id) ?? 0;
    return formatTrackCount(totalCount, localCount);
  }

  // Load artist info for tooltip (called on hover, not during render)
  function loadPlaylistTooltip(playlist: Playlist) {
    if (playlistTooltipCache.has(playlist.id) || tooltipLoadingIds.has(playlist.id)) return;

    tooltipLoadingIds.add(playlist.id);

    const totalCount = getTotalTrackCount(playlist);
    const localCount = localTrackCounts.get(playlist.id) ?? 0;
    fetchPlaylistArtists(playlist.id, totalCount).then(baseTooltip => {
      // Replace the song count line with properly formatted one including local count
      const countText = formatTrackCount(totalCount, localCount);
      const finalTooltip = baseTooltip.replace(/\d+ (Track|Tracks)/, countText);
      playlistTooltipCache.set(playlist.id, finalTooltip);
      tooltipLoadingIds.delete(playlist.id);
    });
  }

  // Invalidate tooltip cache for a specific playlist (call when tracks change)
  function invalidatePlaylistTooltip(playlistId: number) {
    playlistTooltipCache.delete(playlistId);
    tooltipLoadingIds.delete(playlistId);
  }

  // Load sort preference from localStorage
  function loadSortPreference() {
    try {
      const saved = localStorage.getItem('sidebar-playlist-sort');
      if (saved && ['name', 'recent', 'tracks', 'playcount', 'custom'].includes(saved)) {
        sortOption = saved as SortOption;
      }
    } catch (e) {
      // localStorage not available
    }
  }

  function saveSortPreference(option: SortOption) {
    try {
      localStorage.setItem('sidebar-playlist-sort', option);
    } catch (e) {
      // localStorage not available
    }
  }

  // Visible and sorted playlists
  const visiblePlaylists = $derived.by(() => {
    let visible = userPlaylists.filter(p => {
      const settings = playlistSettings.get(p.id);
      return !settings?.hidden;
    });

    // Filter by local content when offline
    if (offlineStatus.isOffline) {
      visible = visible.filter(p => {
        // Calculate local content status from actual data
        const localCount = localTrackCounts.get(p.id) ?? 0;
        const qobuzCount = p.tracks_count ?? 0;

        // Determine availability status
        let localStatus: LocalContentStatus;
        if (localCount === 0) {
          localStatus = 'no';
        } else if (qobuzCount === 0) {
          // Only local tracks - fully available
          localStatus = 'all_local';
        } else {
          // Mixed: has both local and Qobuz tracks - partially available
          localStatus = 'some_local';
        }

        if (offlineSettings.showPartialPlaylists) {
          return localStatus === 'all_local' || localStatus === 'some_local';
        }
        return localStatus === 'all_local';
      });
    }

    // Sort based on selected option
    return [...visible].sort((a, b) => {
      switch (sortOption) {
        case 'name':
          return a.name.localeCompare(b.name);
        case 'recent':
          // For now, use reverse order (most recently added first)
          return userPlaylists.indexOf(b) - userPlaylists.indexOf(a);
        case 'tracks':
          return b.tracks_count - a.tracks_count;
        case 'playcount': {
          const aCount = playlistSettings.get(a.id)?.play_count ?? 0;
          const bCount = playlistSettings.get(b.id)?.play_count ?? 0;
          return bCount - aCount;
        }
        case 'custom': {
          const aPos = playlistSettings.get(a.id)?.position ?? 9999;
          const bPos = playlistSettings.get(b.id)?.position ?? 9999;
          return aPos - bPos;
        }
        default:
          return 0;
      }
    });
  });

  // Expose playlists to parent via binding
  export function getPlaylists(): Playlist[] {
    return userPlaylists;
  }

  export function refreshPlaylists() {
    playlistTooltipCache.clear();
    loadUserPlaylists();
    loadPlaylistSettings();
  }

  export function refreshPlaylistSettings() {
    loadPlaylistSettings();
  }

  export function refreshLocalTrackCounts() {
    loadLocalTrackCounts();
  }

  // Call this when tracks are added/removed from a playlist
  export function onPlaylistTracksChanged(playlistId: number) {
    invalidatePlaylistTooltip(playlistId);
    loadUserPlaylists();
    loadLocalTrackCounts();
  }

  // Update counts for a specific playlist (single source of truth from detail view)
  export function updatePlaylistCounts(playlistId: number, qobuzCount: number, localCount: number) {
    // Update Qobuz count in userPlaylists
    userPlaylists = userPlaylists.map(p =>
      p.id === playlistId ? { ...p, tracks_count: qobuzCount } : p
    );
    // Update local count
    localTrackCounts.set(playlistId, localCount);
    localTrackCounts = new Map(localTrackCounts); // Trigger reactivity
    // Invalidate tooltip cache for this playlist
    invalidatePlaylistTooltip(playlistId);
  }

  // Menu handling functions
  function closeMenu() {
    menuOpen = false;
    sortSubmenuOpen = false;
    if (submenuCloseTimeout) {
      clearTimeout(submenuCloseTimeout);
      submenuCloseTimeout = null;
    }
  }

  function handleClickOutside(event: MouseEvent) {
    if (menuRef && !menuRef.contains(event.target as Node)) {
      closeMenu();
    }
  }

  async function setMenuPosition() {
    await tick();
    if (!triggerRef || !menuEl) return;

    const triggerRect = triggerRef.getBoundingClientRect();
    const menuRect = menuEl.getBoundingClientRect();
    const padding = 8;

    let left = triggerRect.left;
    let top = triggerRect.bottom + 6;

    // Keep menu within bounds
    if (left + menuRect.width > window.innerWidth - padding) {
      left = Math.max(padding, window.innerWidth - menuRect.width - padding);
    }

    if (top + menuRect.height > window.innerHeight - padding) {
      top = triggerRect.top - menuRect.height - 6;
      if (top < padding) top = padding;
    }

    menuStyle = `left: ${left}px; top: ${top}px;`;
  }

  async function setSubmenuPosition() {
    await tick();
    if (!sortTriggerRef || !submenuEl) return;

    const triggerRect = sortTriggerRef.getBoundingClientRect();
    const submenuRect = submenuEl.getBoundingClientRect();
    const padding = 8;

    // Try to position to the right of the trigger
    let left = triggerRect.right + 4;
    let top = triggerRect.top;

    // If not enough space on right, position to left
    if (left + submenuRect.width > window.innerWidth - padding) {
      left = triggerRect.left - submenuRect.width - 4;
    }

    // Keep within vertical bounds
    if (top + submenuRect.height > window.innerHeight - padding) {
      top = Math.max(padding, window.innerHeight - submenuRect.height - padding);
    }

    submenuStyle = `left: ${left}px; top: ${top}px;`;
  }

  async function toggleMenu() {
    if (menuOpen) {
      closeMenu();
    } else {
      menuOpen = true;
      await setMenuPosition();
      document.addEventListener('click', handleClickOutside);
    }
  }

  function handleSortChange(option: SortOption) {
    sortOption = option;
    saveSortPreference(option);
    closeMenu();
  }

  function handleMenuAction(action: () => void) {
    action();
    closeMenu();
  }

  // Folder helpers
  function handleToggleFolder(folderId: string) {
    toggleFolderExpanded(folderId);
    // Update local state for reactivity
    folderExpandState = new Map(folderExpandState);
  }

  function openCreateFolderModal() {
    newFolderName = '';
    showCreateFolderModal = true;
    closeMenu();
  }

  async function handleCreateFolder() {
    if (!newFolderName.trim()) return;

    const folder = await createFolder(newFolderName.trim());
    if (folder) {
      showCreateFolderModal = false;
      newFolderName = '';
      // Refresh folders state
      folders = getVisibleFolders();
    }
  }

  function cancelCreateFolder() {
    showCreateFolderModal = false;
    newFolderName = '';
  }

  // Get playlists for a specific folder (or root if null)
  function getPlaylistsInFolder(folderId: string | null): Playlist[] {
    return visiblePlaylists.filter(p => {
      const settings = playlistSettings.get(p.id);
      const playlistFolderId = settings?.folder_id ?? null;
      return playlistFolderId === folderId;
    });
  }

  // Check if any playlists exist in root (no folder)
  let rootPlaylists = $derived(getPlaylistsInFolder(null));

  $effect(() => {
    if (!menuOpen) {
      document.removeEventListener('click', handleClickOutside);
      sortSubmenuOpen = false;
    }
  });

  $effect(() => {
    if (sortSubmenuOpen) {
      setSubmenuPosition();
    }
  });

  // Reload playlists when offline status changes
  $effect(() => {
    // Track isOffline to trigger reload when it changes
    if (isOffline !== undefined) {
      console.log('[Sidebar] Offline status changed, reloading playlists:', isOffline);
      loadUserPlaylists();
    }
  });

  onMount(() => {
    loadSortPreference();
    loadUserPlaylists();
    loadPlaylistSettings();
    loadLocalTrackCounts();
    loadFolders(); // Load playlist folders

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

  async function loadUserPlaylists() {
    playlistsLoading = true;
    try {
      if (isOffline) {
        // In offline mode, show only pending playlists (created offline)
        console.log('[Sidebar] Loading pending playlists in offline mode');
        const pendingPlaylists = await invoke<import('$lib/stores/offlineStore').PendingPlaylist[]>('get_pending_playlists');

        // Store pending playlists metadata and populate localTrackCounts
        const newPendingMap = new Map<number, import('$lib/stores/offlineStore').PendingPlaylist>();
        const newLocalTrackCounts = new Map<number, number>();

        // Convert pending playlists to Playlist format for UI compatibility
        userPlaylists = pendingPlaylists.map(p => {
          const negativeId = -p.id;
          newPendingMap.set(negativeId, p);
          newLocalTrackCounts.set(negativeId, p.localTrackIds.length); // Populate local track count

          return {
            id: negativeId, // Negative ID to distinguish from real playlists
            name: p.name,
            description: p.description || undefined,
            is_public: p.isPublic,
            tracks_count: p.trackIds.length, // Only Qobuz tracks for correct filtering
            duration: 0,
            users_count: 0,
            is_collaborative: false,
            timestamp_creation: p.createdAt,
            timestamp_update: p.createdAt,
            owner: {
              id: 0,
              name: 'You (Offline)',
              display_name: undefined
            }
          };
        });

        pendingPlaylistsMap = newPendingMap;
        localTrackCounts = newLocalTrackCounts;
        console.log(`[Sidebar] Loaded ${userPlaylists.length} pending playlists`);
      } else {
        // Online mode - load from Qobuz
        const playlists = await invoke<Playlist[]>('get_user_playlists');
        userPlaylists = playlists;
      }
    } catch (err) {
      console.error('Failed to load playlists:', err);
    } finally {
      playlistsLoading = false;
    }
  }

  async function loadPlaylistSettings() {
    try {
      const settings = await invoke<PlaylistSettings[]>('playlist_get_all_settings');
      const map = new Map<number, PlaylistSettings>();
      for (const s of settings) {
        map.set(s.qobuz_playlist_id, s);
      }
      playlistSettings = map;
    } catch (err) {
      // Command might not exist yet, that's ok
      console.debug('Failed to load playlist settings:', err);
    }
  }

  async function loadLocalTrackCounts() {
    try {
      const counts = await invoke<Record<string, number>>('playlist_get_all_local_track_counts');
      const map = new Map<number, number>();
      for (const [id, count] of Object.entries(counts)) {
        map.set(Number(id), count);
      }
      localTrackCounts = map;
    } catch (err) {
      console.debug('Failed to load local track counts:', err);
    }
  }

  function handleViewChange(view: string) {
    onNavigate(view);
  }

  function handlePlaylistClick(playlist: Playlist) {
    if (onPlaylistSelect) {
      onPlaylistSelect(playlist.id);
    }
  }

  // Context menu handlers
  function handlePlaylistContextMenu(e: MouseEvent, playlist: Playlist, folderId: string | null = null) {
    e.preventDefault();
    e.stopPropagation();

    // Get the current folder_id from settings
    const settings = playlistSettings.get(playlist.id);
    const currentFolderId = folderId ?? settings?.folder_id ?? null;

    contextMenu = {
      visible: true,
      x: e.clientX,
      y: e.clientY,
      playlist,
      currentFolderId
    };
  }

  function closeContextMenu() {
    contextMenu = { ...contextMenu, visible: false };
    contextMenuSearch = '';
  }

  async function handleMoveToFolder(folderId: string | null) {
    if (!contextMenu.playlist) return;

    const success = await movePlaylistToFolder(contextMenu.playlist.id, folderId);
    if (success) {
      // Update local settings
      const updated = new Map(playlistSettings);
      const existing = updated.get(contextMenu.playlist.id);
      if (existing) {
        updated.set(contextMenu.playlist.id, { ...existing, folder_id: folderId });
      } else {
        updated.set(contextMenu.playlist.id, {
          qobuz_playlist_id: contextMenu.playlist.id,
          hidden: false,
          position: 0,
          folder_id: folderId
        });
      }
      playlistSettings = updated;
    }
    closeContextMenu();
  }

  // Close context menu when clicking outside
  function handleGlobalClick(e: MouseEvent) {
    if (contextMenu.visible) {
      closeContextMenu();
    }
  }
</script>

<svelte:window onclick={handleGlobalClick} />

<aside class="sidebar" class:collapsed={!isExpanded} class:no-titlebar={!showTitleBar}>
  <!-- Scrollable Content Area -->
  <div class="content">
    <!-- Search Bar -->
    <button
      type="button"
      class="search-container"
      class:collapsed={!isExpanded}
      onclick={() => {
        console.log('Search button clicked!');
        handleViewChange('search');
      }}
      title={isExpanded ? undefined : $t('nav.search')}
    >
      <Search class="search-icon" size={16} />
      {#if isExpanded}
        <span class="search-placeholder">{$t('nav.search')}</span>
      {/if}
    </button>

    <!-- Navigation Section -->
    <nav class="nav-section">
      <NavigationItem
        label={$t('nav.home')}
        active={activeView === 'home'}
        onclick={() => handleViewChange('home')}
        showLabel={isExpanded}
      >
        {#snippet icon()}<Home size={14} />{/snippet}
      </NavigationItem>
    </nav>

    <!-- Favorites Section (standalone) -->
    <nav class="nav-section">
      <NavigationItem
        label={$t('nav.favorites')}
        active={activeView.startsWith('favorites-')}
        onclick={() => handleViewChange('favorites')}
        showLabel={isExpanded}
      >
        {#snippet icon()}<Heart size={14} />{/snippet}
      </NavigationItem>
    </nav>

    <!-- Playlists Section (hidden in offline mode) -->
    {#if !isOffline}
    <div class="section playlists-section">
      {#if isExpanded}
        <div class="playlists-header">
          <div class="section-header">{$t('nav.playlists')}</div>
          <div class="header-actions" bind:this={menuRef}>
            <button class="icon-btn" onclick={onCreatePlaylist} title={$t('playlist.createNew')}>
              <Plus size={14} />
            </button>
            <button
              class="icon-btn"
              bind:this={triggerRef}
              onclick={(e) => { e.stopPropagation(); toggleMenu(); }}
              title={$t('actions.more')}
            >
              <MoreHorizontal size={14} />
            </button>
            <button class="icon-btn" onclick={() => playlistsCollapsed = !playlistsCollapsed} title={playlistsCollapsed ? $t('actions.open') : $t('actions.close')}>
              {#if playlistsCollapsed}
                <ChevronDown size={14} />
              {:else}
                <ChevronUp size={14} />
              {/if}
            </button>
          </div>
        </div>
      {/if}

      <!-- Dropdown Menu -->
      {#if menuOpen}
        <div class="dropdown-menu" bind:this={menuEl} style={menuStyle}>
          <!-- Sort by submenu trigger -->
          <div
            class="menu-item has-submenu"
            bind:this={sortTriggerRef}
            role="button"
            tabindex="0"
            onmouseenter={openSubmenu}
            onmouseleave={closeSubmenuDelayed}
          >
            <ArrowUpDown size={14} />
            <span class="menu-label">{$t('library.sortBy')}</span>
            <ChevronRight size={14} class="submenu-arrow" />
          </div>

          <!-- Sort submenu (positioned outside trigger for better hover handling) -->
          {#if sortSubmenuOpen}
            <div
              class="submenu"
              bind:this={submenuEl}
              style={submenuStyle}
              onmouseenter={openSubmenu}
              onmouseleave={closeSubmenuDelayed}
            >
              <button class="menu-item" class:selected={sortOption === 'name'} onclick={() => handleSortChange('name')}>
                {$t('sort.nameAZ')}
              </button>
              <button class="menu-item" class:selected={sortOption === 'recent'} onclick={() => handleSortChange('recent')}>
                {$t('sort.recent')}
              </button>
              <button class="menu-item" class:selected={sortOption === 'tracks'} onclick={() => handleSortChange('tracks')}>
                {$t('sort.trackCount')}
              </button>
              <button class="menu-item" class:selected={sortOption === 'playcount'} onclick={() => handleSortChange('playcount')}>
                {$t('sort.playCount')}
              </button>
              <button class="menu-item" class:selected={sortOption === 'custom'} onclick={() => handleSortChange('custom')}>
                {$t('sort.custom')}
              </button>
            </div>
          {/if}

          <button class="menu-item" onclick={openCreateFolderModal}>
            <FolderPlus size={14} />
            <span>{$t('playlist.newFolder', { default: 'New Folder' })}</span>
          </button>

          <div class="menu-divider"></div>

          <button class="menu-item" onclick={() => handleMenuAction(refreshPlaylists)}>
            <RefreshCw size={14} />
            <span>{$t('actions.refresh')}</span>
          </button>

          <button
            class="menu-item"
            class:disabled={offlineStatus.isOffline}
            onclick={() => !offlineStatus.isOffline && handleMenuAction(onImportPlaylist ?? (() => {}))}
            title={offlineStatus.isOffline ? $t('offline.featureDisabled') : undefined}
          >
            <Import size={14} />
            <span>{$t('playlist.import')}</span>
          </button>

          <div class="menu-divider"></div>

          <button class="menu-item" onclick={() => handleMenuAction(onPlaylistManagerClick ?? (() => {}))}>
            <Settings size={14} />
            <span>{$t('playlist.manage')}</span>
          </button>
        </div>
      {/if}

      {#if !playlistsCollapsed || !isExpanded}
        <div class="playlists-scroll">
          {#if playlistsLoading}
            {#if isExpanded}
              <div class="playlists-loading">{$t('actions.loading')}</div>
            {/if}
          {:else if visiblePlaylists.length > 0 || folders.length > 0}
            <nav class="playlists-nav">
              <!-- Folders with their playlists -->
              {#each folders as folder (folder.id)}
                {@const folderPlaylists = getPlaylistsInFolder(folder.id)}
                {@const isExpanded_ = isFolderExpanded(folder.id)}
                {#if isExpanded}
                  <div class="folder-item">
                    <button
                      class="folder-header"
                      onclick={() => handleToggleFolder(folder.id)}
                    >
                      <Folder size={14} />
                      <span class="folder-name">{folder.name}</span>
                      <span class="folder-count">{folderPlaylists.length}</span>
                      <span class="folder-chevron" class:expanded={isExpanded_}>
                        <ChevronRight size={12} />
                      </span>
                    </button>
                    {#if isExpanded_}
                      <div class="folder-playlists">
                        {#each folderPlaylists as playlist (playlist.id)}
                          <NavigationItem
                            label={playlist.name}
                            tooltip={getPlaylistTooltip(playlist, true)}
                            active={activeView === 'playlist' && selectedPlaylistId === playlist.id}
                            onclick={() => handlePlaylistClick(playlist)}
                            onHover={() => loadPlaylistTooltip(playlist)}
                            oncontextmenu={(e) => handlePlaylistContextMenu(e, playlist, folder.id)}
                            showLabel={true}
                            indented={true}
                          >
                            {#snippet icon()}<ListMusic size={14} />{/snippet}
                          </NavigationItem>
                        {/each}
                      </div>
                    {/if}
                  </div>
                {:else}
                  <!-- Collapsed sidebar: show folder icon only -->
                  <button
                    class="collapsed-folder-btn"
                    onclick={() => handleToggleFolder(folder.id)}
                    title="{folder.name} ({folderPlaylists.length})"
                  >
                    <Folder size={14} />
                  </button>
                {/if}
              {/each}

              <!-- Root playlists (not in any folder) -->
              {#each rootPlaylists as playlist (playlist.id)}
                <NavigationItem
                  label={playlist.name}
                  tooltip={getPlaylistTooltip(playlist, isExpanded)}
                  active={activeView === 'playlist' && selectedPlaylistId === playlist.id}
                  onclick={() => handlePlaylistClick(playlist)}
                  onHover={() => loadPlaylistTooltip(playlist)}
                  oncontextmenu={(e) => handlePlaylistContextMenu(e, playlist, null)}
                  showLabel={isExpanded}
                >
                  {#snippet icon()}<ListMusic size={14} />{/snippet}
                </NavigationItem>
              {/each}
            </nav>
          {:else if userPlaylists.length > 0}
            {#if isExpanded}
              <div class="no-playlists">{$t('playlist.allHidden')}</div>
            {/if}
          {:else}
            {#if isExpanded}
              <div class="no-playlists">{$t('empty.noPlaylists')}</div>
            {/if}
          {/if}
        </div>
      {/if}
    </div>
    {/if}

    <!-- Local Library Section -->
    <div class="section local-library-section">
      {#if isExpanded}
        <button class="section-header-btn" onclick={() => localLibraryCollapsed = !localLibraryCollapsed}>
          <span class="section-header">{$t('library.title')}</span>
          {#if localLibraryCollapsed}
            <ChevronDown size={12} />
          {:else}
            <ChevronUp size={12} />
          {/if}
        </button>
      {/if}
      {#if !localLibraryCollapsed || !isExpanded}
        <NavigationItem
          label={$t('library.browse')}
          active={activeView === 'library'}
          onclick={() => handleViewChange('library')}
          showLabel={isExpanded}
        >
          {#snippet icon()}<HardDrive size={14} />{/snippet}
        </NavigationItem>
      {/if}
    </div>
  </div>

  <!-- Toggle Button (Edge position) -->
  <button
    class="toggle-btn"
    onclick={onToggle}
    title={isExpanded ? $t('actions.collapse') : $t('actions.expand')}
  >
    {#if isExpanded}
      <ChevronLeft size={16} />
    {:else}
      <ChevronRight size={16} />
    {/if}
  </button>

  <!-- Fixed User Profile at Bottom -->
  <div class="user-section" class:collapsed={!isExpanded}>
    {#if isExpanded}
      <UserCard
        username={userName}
        {subscription}
        onSettingsClick={onSettingsClick ?? (() => handleViewChange('settings'))}
        {onAboutClick}
      />
    {:else}
      <button
        class="collapsed-settings-btn"
        onclick={onSettingsClick ?? (() => handleViewChange('settings'))}
        title={$t('nav.settings')}
      >
        <Settings size={16} />
      </button>
    {/if}
  </div>
</aside>

<!-- Playlist Context Menu -->
{#if contextMenu.visible}
  {@const availableFolders = folders.filter(f => f.id !== contextMenu.currentFolderId)}
  {@const showSearch = availableFolders.length >= FOLDER_SEARCH_THRESHOLD}
  <div
    class="context-menu"
    class:has-search={showSearch}
    style="left: {contextMenu.x}px; top: {contextMenu.y}px;"
    onclick={(e) => e.stopPropagation()}
    role="menu"
  >
    {#if availableFolders.length > 0}
      <div class="context-menu-section">
        <span class="context-menu-label">Move to folder</span>
        {#if showSearch}
          <div class="context-menu-search">
            <Search size={14} />
            <input
              type="text"
              placeholder="Search folders..."
              bind:value={contextMenuSearch}
              onclick={(e) => e.stopPropagation()}
            />
          </div>
        {/if}
        <div class="context-menu-folders" class:scrollable={showSearch}>
          {#each filteredContextFolders as folder (folder.id)}
            <button
              class="context-menu-item"
              onclick={() => handleMoveToFolder(folder.id)}
            >
              <Folder size={14} />
              {folder.name}
            </button>
          {/each}
          {#if showSearch && filteredContextFolders.length === 0}
            <div class="context-menu-empty">
              No folders match
            </div>
          {/if}
        </div>
      </div>
    {/if}
    {#if contextMenu.currentFolderId}
      <button
        class="context-menu-item"
        onclick={() => handleMoveToFolder(null)}
      >
        <ChevronLeft size={14} />
        Move to root
      </button>
    {/if}
    {#if availableFolders.length === 0 && !contextMenu.currentFolderId}
      <div class="context-menu-empty">
        No folders yet
      </div>
    {/if}
  </div>
{/if}

<!-- Create Folder Modal -->
{#if showCreateFolderModal}
  <div class="modal-overlay" onclick={cancelCreateFolder} role="presentation">
    <div class="modal-content create-folder-modal" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
      <h2 class="modal-title">{$t('playlist.newFolder', { default: 'New Folder' })}</h2>
      <div class="form-group">
        <label for="folder-name">{$t('common.name', { default: 'Name' })}</label>
        <input
          id="folder-name"
          type="text"
          bind:value={newFolderName}
          placeholder={$t('playlist.folderNamePlaceholder', { default: 'Enter folder name' })}
          onkeydown={(e) => e.key === 'Enter' && handleCreateFolder()}
          autofocus
        />
      </div>
      <div class="modal-actions">
        <button class="btn-secondary" onclick={cancelCreateFolder}>
          {$t('actions.cancel')}
        </button>
        <button
          class="btn-primary"
          onclick={handleCreateFolder}
          disabled={!newFolderName.trim()}
        >
          {$t('actions.create', { default: 'Create' })}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .sidebar {
    width: 280px;
    min-width: 280px;
    flex-shrink: 0;
    background-color: var(--bg-secondary, #1a1a1a);
    position: relative;
    z-index: 3;
    display: flex;
    flex-direction: column;
    height: calc(100vh - 136px); /* 104px NowPlayingBar + 32px TitleBar */
    transition: width 200ms ease, min-width 200ms ease;
  }

  .sidebar.collapsed {
    width: 64px;
    min-width: 64px;
  }

  .sidebar.no-titlebar {
    height: calc(100vh - 104px); /* Only 104px NowPlayingBar, no title bar */
  }

  .content {
    flex: 1;
    overflow: hidden;
    padding: 12px;
    padding-bottom: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .search-container {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 32px;
    min-height: 32px;
    max-height: 32px;
    background-color: var(--bg-tertiary);
    border-radius: 6px;
    padding: 0 10px;
    border: none;
    cursor: pointer;
    transition: background-color 150ms ease;
    flex-shrink: 0;
  }

  .search-container:hover {
    background-color: var(--bg-hover);
  }

  .search-container.collapsed {
    width: 40px;
    height: 40px;
    padding: 0;
    justify-content: center;
    border-radius: 8px;
  }

  .search-container :global(.search-icon) {
    color: var(--text-muted);
    flex-shrink: 0;
    pointer-events: none;
  }

  .search-placeholder {
    font-size: 13px;
    color: var(--text-muted);
    pointer-events: none;
  }

  .nav-section {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .local-library-section {
    flex-shrink: 0;
    margin-bottom: 2px;
  }

  .section-header {
    font-size: 10px;
    text-transform: uppercase;
    color: var(--text-muted);
    font-weight: 600;
    letter-spacing: 0.05em;
    margin-bottom: 6px;
    padding: 0 8px;
  }

  .section-header-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0 8px;
    margin-bottom: 6px;
    color: var(--text-muted);
    transition: color 150ms ease;
  }

  .section-header-btn:hover {
    color: var(--text-primary);
  }

  .section-header-btn .section-header {
    margin-bottom: 0;
    padding: 0;
  }

  .playlists-section {
    flex: 1;
    padding-bottom: 12px;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .playlists-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 8px;
    margin-bottom: 6px;
  }

  .playlists-header .section-header {
    margin-bottom: 0;
    padding: 0;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .icon-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    transition: color 150ms ease;
    border-radius: 4px;
  }

  .icon-btn:hover {
    color: var(--text-primary);
  }

  /* Playlist list view */
  .playlists-nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .playlists-scroll {
    overflow-y: overlay;
    margin-right: 1px;
    min-height: 0;
    flex: 1;
  }

  /* Thin subtle scrollbar - always visible */
  .playlists-scroll::-webkit-scrollbar {
    width: 4px;
  }

  .playlists-scroll::-webkit-scrollbar-track {
    background: transparent;
  }

  .playlists-scroll::-webkit-scrollbar-thumb {
    background: var(--alpha-10);
    border-radius: 4px;
  }

  .playlists-scroll::-webkit-scrollbar-thumb:hover {
    background: var(--alpha-20);
  }


  /* Compensate margin-right in collapsed mode by shifting icons slightly right */
  .sidebar.collapsed .playlists-scroll .nav-item {
    padding-left: 1px;
  }

  .playlists-loading,
  .no-playlists {
    font-size: 12px;
    color: var(--text-muted);
    padding: 6px 8px;
  }

  /* Folder styles */
  .folder-item {
    display: flex;
    flex-direction: column;
  }

  .folder-header {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    color: var(--text-muted);
    transition: background-color 150ms ease;
  }

  .folder-header:hover {
    background: var(--bg-hover);
  }

  .folder-chevron {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    transition: transform 150ms ease;
  }

  .folder-chevron.expanded {
    transform: rotate(90deg);
  }

  .folder-name {
    flex: 1;
    font-size: 13px;
    font-weight: 400;
    color: var(--text-muted);
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-count {
    font-size: 11px;
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .folder-header:hover .folder-count {
    opacity: 1;
  }

  .folder-playlists {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-left: 12px;
  }

  .collapsed-folder-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    background: transparent;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .collapsed-folder-btn:hover {
    background: var(--bg-hover);
  }

  /* Create Folder Modal */
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
    margin-bottom: 20px;
  }

  .form-group label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-muted);
  }

  .form-group input {
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

  .form-group input:focus {
    border-color: var(--accent-primary);
  }

  .form-group input::placeholder {
    color: var(--text-muted);
  }

  .modal-actions {
    display: flex;
    gap: 12px;
    justify-content: flex-end;
  }

  .btn-secondary,
  .btn-primary {
    padding: 8px 16px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
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

  .toggle-btn {
    position: absolute;
    right: -10px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: var(--bg-tertiary);
    border: 1px solid var(--alpha-10);
    border-radius: 50%;
    color: var(--text-muted);
    cursor: pointer;
    box-shadow: 0 0 2px rgba(0, 0, 0, 0.4);
    transition: transform 150ms ease, background-color 150ms ease, color 150ms ease, box-shadow 150ms ease;
    z-index: 10;
  }

  .toggle-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
    transform: translateY(-50%) scale(1.1);
    box-shadow: 0 0 4px rgba(0, 0, 0, 0.5);
  }

  .user-section {
    border-top: 1px solid var(--bg-tertiary);
    padding: 8px;
  }

  .user-section.collapsed {
    display: flex;
    justify-content: center;
    padding: 8px;
  }

  .collapsed-settings-btn {
    width: 40px;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: 8px;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease;
  }

  .collapsed-settings-btn:hover {
    color: var(--text-primary);
    background-color: var(--bg-hover);
  }

  /* Dropdown menu styles */
  .dropdown-menu {
    position: fixed;
    min-width: 180px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10000;
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

  .menu-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
    text-align: left;
  }

  .menu-item:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .menu-item.selected {
    color: var(--accent-primary);
  }

  .menu-item.disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .menu-item.disabled:hover {
    background: none;
    color: var(--text-secondary);
  }

  .menu-item.has-submenu {
    position: relative;
  }

  .menu-item .menu-label {
    flex: 1;
  }

  .menu-item .submenu-arrow {
    flex-shrink: 0;
  }

  .menu-divider {
    height: 1px;
    background: var(--bg-tertiary);
    margin: 6px 0;
  }

  .submenu {
    position: fixed;
    min-width: 140px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10001;
  }

  .submenu .menu-item {
    gap: 8px;
  }

  /* Context Menu */
  .context-menu {
    position: fixed;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 6px;
    min-width: 180px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10002;
  }

  .context-menu.has-search {
    min-width: 220px;
  }

  .context-menu-section {
    display: flex;
    flex-direction: column;
  }

  .context-menu-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 6px 10px;
  }

  .context-menu-search {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    margin-bottom: 4px;
    color: var(--text-muted);
  }

  .context-menu-search input {
    flex: 1;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 4px;
    padding: 6px 8px;
    font-size: 12px;
    color: var(--text-primary);
    outline: none;
  }

  .context-menu-search input:focus {
    border-color: var(--accent-primary);
  }

  .context-menu-search input::placeholder {
    color: var(--text-muted);
  }

  .context-menu-folders {
    display: flex;
    flex-direction: column;
  }

  .context-menu-folders.scrollable {
    max-height: 200px;
    overflow-y: auto;
  }

  .context-menu-folders.scrollable::-webkit-scrollbar {
    width: 4px;
  }

  .context-menu-folders.scrollable::-webkit-scrollbar-track {
    background: transparent;
  }

  .context-menu-folders.scrollable::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 2px;
  }

  .context-menu-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    background: none;
    border: none;
    border-radius: 4px;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    transition: background-color 150ms ease;
  }

  .context-menu-item:hover {
    background: var(--bg-hover);
  }

  .context-menu-empty {
    padding: 12px;
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }
</style>
