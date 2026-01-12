<script lang="ts">
  import { tick } from 'svelte';
  import { Search, Home, HardDrive, Plus, RefreshCw, ChevronDown, ChevronUp, Heart, ListMusic, Import, Settings, MoreHorizontal, ArrowUpDown, ChevronRight } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import NavigationItem from './NavigationItem.svelte';
  import UserCard from './UserCard.svelte';

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
    images?: string[];
    duration?: number;
  }

  interface PlaylistSettings {
    qobuz_playlist_id: number;
    hidden: boolean;
    position: number;
    play_count?: number;
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
    subscription = 'Qobuz'
  }: Props = $props();

  let userPlaylists = $state<Playlist[]>([]);
  let playlistSettings = $state<Map<number, PlaylistSettings>>(new Map());
  let playlistsLoading = $state(false);
  let playlistsCollapsed = $state(false);
  let localLibraryCollapsed = $state(false);

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
        const songText = `${trackCount} ${trackCount === 1 ? 'Song' : 'Songs'}`;

        if (artists.length > 0) {
          return `${artists.join('\n')}\n${songText}`;
        }
        return songText;
      }
    } catch (err) {
      console.debug('Failed to fetch playlist artists:', err);
    }

    return `${trackCount} ${trackCount === 1 ? 'Song' : 'Songs'}`;
  }

  // Get basic tooltip (no state mutation during render)
  function getPlaylistTooltip(playlist: Playlist): string {
    const cached = playlistTooltipCache.get(playlist.id);
    if (cached) return cached;

    // Return basic tooltip - fetch will happen on hover
    return `${playlist.tracks_count} ${playlist.tracks_count === 1 ? 'Song' : 'Songs'}`;
  }

  // Load artist info for tooltip (called on hover, not during render)
  function loadPlaylistTooltip(playlist: Playlist) {
    if (playlistTooltipCache.has(playlist.id) || tooltipLoadingIds.has(playlist.id)) return;

    tooltipLoadingIds.add(playlist.id);

    fetchPlaylistArtists(playlist.id, playlist.tracks_count).then(tooltip => {
      playlistTooltipCache.set(playlist.id, tooltip);
      tooltipLoadingIds.delete(playlist.id);
    });
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
    const visible = userPlaylists.filter(p => {
      const settings = playlistSettings.get(p.id);
      return !settings?.hidden;
    });

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
    loadUserPlaylists();
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

  onMount(() => {
    loadSortPreference();
    loadUserPlaylists();
    loadPlaylistSettings();
  });

  async function loadUserPlaylists() {
    playlistsLoading = true;
    try {
      const playlists = await invoke<Playlist[]>('get_user_playlists');
      userPlaylists = playlists;
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

  function handleViewChange(view: string) {
    onNavigate(view);
  }

  function handlePlaylistClick(playlist: Playlist) {
    if (onPlaylistSelect) {
      onPlaylistSelect(playlist.id);
    }
  }
</script>

<aside class="sidebar">
  <!-- Scrollable Content Area -->
  <div class="content">
    <!-- Search Bar -->
    <button
      type="button"
      class="search-container"
      onclick={() => {
        console.log('Search button clicked!');
        handleViewChange('search');
      }}
    >
      <Search class="search-icon" size={16} />
      <span class="search-placeholder">Search</span>
    </button>

    <!-- Navigation Section -->
    <nav class="nav-section">
      <NavigationItem
        label="Home"
        active={activeView === 'home'}
        onclick={() => handleViewChange('home')}
      >
        {#snippet icon()}<Home size={14} />{/snippet}
      </NavigationItem>
    </nav>

    <!-- Favorites Section (standalone) -->
    <nav class="nav-section">
      <NavigationItem
        label="Favorites"
        active={activeView === 'favorites'}
        onclick={() => handleViewChange('favorites')}
      >
        {#snippet icon()}<Heart size={14} />{/snippet}
      </NavigationItem>
    </nav>

    <!-- Playlists Section -->
    <div class="section playlists-section">
      <div class="playlists-header">
        <div class="section-header">Playlists</div>
        <div class="header-actions" bind:this={menuRef}>
          <button class="icon-btn" onclick={onCreatePlaylist} title="New playlist">
            <Plus size={14} />
          </button>
          <button
            class="icon-btn"
            bind:this={triggerRef}
            onclick={(e) => { e.stopPropagation(); toggleMenu(); }}
            title="Options"
          >
            <MoreHorizontal size={14} />
          </button>
          <button class="icon-btn" onclick={() => playlistsCollapsed = !playlistsCollapsed} title={playlistsCollapsed ? "Expand" : "Collapse"}>
            {#if playlistsCollapsed}
              <ChevronDown size={14} />
            {:else}
              <ChevronUp size={14} />
            {/if}
          </button>
        </div>
      </div>

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
            <span class="menu-label">Sort by</span>
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
                Name (A-Z)
              </button>
              <button class="menu-item" class:selected={sortOption === 'recent'} onclick={() => handleSortChange('recent')}>
                Recent
              </button>
              <button class="menu-item" class:selected={sortOption === 'tracks'} onclick={() => handleSortChange('tracks')}>
                # of Tracks
              </button>
              <button class="menu-item" class:selected={sortOption === 'playcount'} onclick={() => handleSortChange('playcount')}>
                Play Count
              </button>
              <button class="menu-item" class:selected={sortOption === 'custom'} onclick={() => handleSortChange('custom')}>
                Custom
              </button>
            </div>
          {/if}

          <button class="menu-item" onclick={() => handleMenuAction(loadUserPlaylists)}>
            <RefreshCw size={14} />
            <span>Refresh</span>
          </button>

          <button class="menu-item" onclick={() => handleMenuAction(onImportPlaylist ?? (() => {}))}>
            <Import size={14} />
            <span>Import</span>
          </button>

          <div class="menu-divider"></div>

          <button class="menu-item" onclick={() => handleMenuAction(onPlaylistManagerClick ?? (() => {}))}>
            <Settings size={14} />
            <span>Manage playlists</span>
          </button>
        </div>
      {/if}

      {#if !playlistsCollapsed}
        {#if playlistsLoading}
          <div class="playlists-loading">Loading...</div>
        {:else if visiblePlaylists.length > 0}
          <nav class="playlists-nav">
            {#each visiblePlaylists as playlist (playlist.id)}
              <NavigationItem
                label={playlist.name}
                tooltip={getPlaylistTooltip(playlist)}
                active={activeView === 'playlist' && selectedPlaylistId === playlist.id}
                onclick={() => handlePlaylistClick(playlist)}
                onHover={() => loadPlaylistTooltip(playlist)}
              >
                {#snippet icon()}<ListMusic size={14} />{/snippet}
              </NavigationItem>
            {/each}
          </nav>
        {:else if userPlaylists.length > 0}
          <div class="no-playlists">All playlists are hidden</div>
        {:else}
          <div class="no-playlists">No playlists yet</div>
        {/if}
      {/if}
    </div>

    <!-- Local Library Section -->
    <div class="section">
      <button class="section-header-btn" onclick={() => localLibraryCollapsed = !localLibraryCollapsed}>
        <span class="section-header">Local Library</span>
        {#if localLibraryCollapsed}
          <ChevronDown size={12} />
        {:else}
          <ChevronUp size={12} />
        {/if}
      </button>
      {#if !localLibraryCollapsed}
        <NavigationItem
          label="Browse Library"
          active={activeView === 'library'}
          onclick={() => handleViewChange('library')}
        >
          {#snippet icon()}<HardDrive size={14} />{/snippet}
        </NavigationItem>
      {/if}
    </div>
  </div>

  <!-- Fixed User Profile at Bottom -->
  <div class="user-section">
    <UserCard
      username={userName}
      {subscription}
      onSettingsClick={onSettingsClick ?? (() => handleViewChange('settings'))}
      {onAboutClick}
    />
  </div>
</aside>

<style>
  .sidebar {
    width: 280px;
    min-width: 280px;
    flex-shrink: 0;
    background-color: var(--bg-secondary);
    display: flex;
    flex-direction: column;
    height: calc(100vh - 136px); /* 104px NowPlayingBar + 32px TitleBar */
  }

  .content {
    flex: 1;
    overflow-y: auto;
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

  .playlists-loading,
  .no-playlists {
    font-size: 12px;
    color: var(--text-muted);
    padding: 6px 8px;
  }

  .user-section {
    border-top: 1px solid var(--bg-tertiary);
    padding: 8px;
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
</style>
