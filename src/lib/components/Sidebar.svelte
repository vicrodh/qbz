<script lang="ts">
  import { Search, Home, HardDrive, Plus, RefreshCw, ChevronDown, ChevronUp, Heart, ListMusic, Download, Filter, ArrowUpDown, LayoutGrid, List, GripVertical, EyeOff, Eye } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import NavigationItem from './NavigationItem.svelte';
  import UserCard from './UserCard.svelte';
  import PlaylistCollage from './PlaylistCollage.svelte';

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

  type PlaylistFilter = 'all' | 'visible' | 'hidden';
  type PlaylistSort = 'name' | 'recent' | 'custom';
  type PlaylistViewMode = 'list' | 'grid';

  interface Props {
    activeView: string;
    selectedPlaylistId?: number | null;
    onNavigate: (view: string) => void;
    onPlaylistSelect?: (playlistId: number) => void;
    onCreatePlaylist?: () => void;
    onImportPlaylist?: () => void;
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

  // Playlist management state
  let playlistFilter = $state<PlaylistFilter>(
    (localStorage.getItem('qbz-playlist-filter') as PlaylistFilter) || 'visible'
  );
  let playlistSort = $state<PlaylistSort>(
    (localStorage.getItem('qbz-playlist-sort') as PlaylistSort) || 'name'
  );
  let playlistViewMode = $state<PlaylistViewMode>(
    (localStorage.getItem('qbz-playlist-view') as PlaylistViewMode) || 'list'
  );
  let showFilterMenu = $state(false);
  let showSortMenu = $state(false);

  // Persist preferences
  $effect(() => {
    localStorage.setItem('qbz-playlist-filter', playlistFilter);
  });
  $effect(() => {
    localStorage.setItem('qbz-playlist-sort', playlistSort);
  });
  $effect(() => {
    localStorage.setItem('qbz-playlist-view', playlistViewMode);
  });

  // Filtered and sorted playlists
  const filteredPlaylists = $derived.by(() => {
    let result = [...userPlaylists];

    // Apply filter
    if (playlistFilter === 'visible') {
      result = result.filter(p => {
        const settings = playlistSettings.get(p.id);
        return !settings?.hidden;
      });
    } else if (playlistFilter === 'hidden') {
      result = result.filter(p => {
        const settings = playlistSettings.get(p.id);
        return settings?.hidden;
      });
    }

    // Apply sort
    if (playlistSort === 'name') {
      result.sort((a, b) => a.name.localeCompare(b.name));
    } else if (playlistSort === 'custom') {
      result.sort((a, b) => {
        const posA = playlistSettings.get(a.id)?.position ?? 999;
        const posB = playlistSettings.get(b.id)?.position ?? 999;
        return posA - posB;
      });
    }
    // 'recent' keeps original order from API (already sorted by recent)

    return result;
  });

  // Expose playlists to parent via binding
  export function getPlaylists(): Playlist[] {
    return userPlaylists;
  }

  export function refreshPlaylists() {
    loadUserPlaylists();
  }

  onMount(() => {
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

  async function togglePlaylistHidden(playlistId: number) {
    const current = playlistSettings.get(playlistId);
    const newHidden = !current?.hidden;
    try {
      await invoke('playlist_set_hidden', { playlistId, hidden: newHidden });
      // Update local state
      const updated = new Map(playlistSettings);
      updated.set(playlistId, { ...current, qobuz_playlist_id: playlistId, hidden: newHidden, position: current?.position ?? 0 });
      playlistSettings = updated;
    } catch (err) {
      console.error('Failed to toggle playlist hidden:', err);
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
        <div class="header-actions">
          <!-- Filter dropdown -->
          <div class="dropdown-container">
            <button
              class="icon-btn"
              class:active={playlistFilter !== 'visible'}
              onclick={() => showFilterMenu = !showFilterMenu}
              title="Filter playlists"
            >
              {#if playlistFilter === 'hidden'}
                <EyeOff size={14} />
              {:else}
                <Filter size={14} />
              {/if}
            </button>
            {#if showFilterMenu}
              <div class="dropdown-menu" role="menu">
                <button class="dropdown-item" class:selected={playlistFilter === 'all'} onclick={() => { playlistFilter = 'all'; showFilterMenu = false; }}>
                  All
                </button>
                <button class="dropdown-item" class:selected={playlistFilter === 'visible'} onclick={() => { playlistFilter = 'visible'; showFilterMenu = false; }}>
                  Visible
                </button>
                <button class="dropdown-item" class:selected={playlistFilter === 'hidden'} onclick={() => { playlistFilter = 'hidden'; showFilterMenu = false; }}>
                  Hidden
                </button>
              </div>
            {/if}
          </div>

          <!-- Sort dropdown -->
          <div class="dropdown-container">
            <button class="icon-btn" onclick={() => showSortMenu = !showSortMenu} title="Sort playlists">
              <ArrowUpDown size={14} />
            </button>
            {#if showSortMenu}
              <div class="dropdown-menu" role="menu">
                <button class="dropdown-item" class:selected={playlistSort === 'name'} onclick={() => { playlistSort = 'name'; showSortMenu = false; }}>
                  Name (A-Z)
                </button>
                <button class="dropdown-item" class:selected={playlistSort === 'recent'} onclick={() => { playlistSort = 'recent'; showSortMenu = false; }}>
                  Recent
                </button>
                <button class="dropdown-item" class:selected={playlistSort === 'custom'} onclick={() => { playlistSort = 'custom'; showSortMenu = false; }}>
                  Custom Order
                </button>
              </div>
            {/if}
          </div>

          <!-- View toggle -->
          <button
            class="icon-btn"
            onclick={() => playlistViewMode = playlistViewMode === 'list' ? 'grid' : 'list'}
            title={playlistViewMode === 'list' ? 'Grid view' : 'List view'}
          >
            {#if playlistViewMode === 'list'}
              <LayoutGrid size={14} />
            {:else}
              <List size={14} />
            {/if}
          </button>

          <button class="icon-btn" onclick={onCreatePlaylist} title="New playlist">
            <Plus size={14} />
          </button>
          <button class="icon-btn" onclick={onImportPlaylist} title="Import playlist">
            <Download size={14} />
          </button>
          <button class="icon-btn" onclick={loadUserPlaylists} title="Refresh playlists">
            <RefreshCw size={14} />
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

      {#if !playlistsCollapsed}
        {#if playlistsLoading}
          <div class="playlists-loading">Loading...</div>
        {:else if filteredPlaylists.length > 0}
          {#if playlistViewMode === 'list'}
            <!-- List View -->
            <div class="playlists-list">
              {#each filteredPlaylists as playlist (playlist.id)}
                {@const isHidden = playlistSettings.get(playlist.id)?.hidden}
                <button
                  class="playlist-list-item"
                  class:active={activeView === 'playlist' && selectedPlaylistId === playlist.id}
                  class:hidden-playlist={isHidden}
                  onclick={() => handlePlaylistClick(playlist)}
                >
                  <PlaylistCollage artworks={playlist.images ?? []} size={36} />
                  <div class="playlist-info">
                    <span class="playlist-name">{playlist.name}</span>
                    <span class="playlist-count">{playlist.tracks_count} tracks</span>
                  </div>
                  {#if playlistFilter === 'all' && isHidden}
                    <EyeOff size={12} class="hidden-indicator" />
                  {/if}
                </button>
              {/each}
            </div>
          {:else}
            <!-- Grid View -->
            <div class="playlists-grid">
              {#each filteredPlaylists as playlist (playlist.id)}
                {@const isHidden = playlistSettings.get(playlist.id)?.hidden}
                <button
                  class="playlist-grid-item"
                  class:active={activeView === 'playlist' && selectedPlaylistId === playlist.id}
                  class:hidden-playlist={isHidden}
                  onclick={() => handlePlaylistClick(playlist)}
                >
                  <div class="grid-artwork">
                    <PlaylistCollage artworks={playlist.images ?? []} size={100} />
                    {#if playlistFilter === 'all' && isHidden}
                      <div class="hidden-badge">
                        <EyeOff size={12} />
                      </div>
                    {/if}
                  </div>
                  <span class="playlist-name">{playlist.name}</span>
                </button>
              {/each}
            </div>
          {/if}
        {:else if userPlaylists.length > 0}
          <div class="no-playlists">
            {playlistFilter === 'hidden' ? 'No hidden playlists' : 'No visible playlists'}
          </div>
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

  .icon-btn.active {
    color: var(--accent-primary);
  }

  /* Dropdown menus */
  .dropdown-container {
    position: relative;
  }

  .dropdown-menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 4px;
    min-width: 120px;
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .dropdown-item {
    display: block;
    width: 100%;
    padding: 6px 10px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 12px;
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

  /* Playlist list view */
  .playlists-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .playlist-list-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    background: none;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    transition: background-color 150ms ease;
    text-align: left;
    width: 100%;
  }

  .playlist-list-item:hover {
    background: var(--bg-tertiary);
  }

  .playlist-list-item.active {
    background: var(--bg-hover);
  }

  .playlist-list-item.hidden-playlist {
    opacity: 0.5;
  }

  .playlist-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .playlist-name {
    font-size: 13px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .playlist-count {
    font-size: 11px;
    color: var(--text-muted);
  }

  .playlist-list-item :global(.hidden-indicator) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  /* Playlist grid view */
  .playlists-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 8px;
    padding: 0 4px;
  }

  .playlist-grid-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    background: none;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
    text-align: center;
  }

  .playlist-grid-item:hover {
    background: var(--bg-tertiary);
  }

  .playlist-grid-item.active {
    background: var(--bg-hover);
  }

  .playlist-grid-item.hidden-playlist {
    opacity: 0.5;
  }

  .grid-artwork {
    position: relative;
    width: 100%;
    display: flex;
    justify-content: center;
  }

  .hidden-badge {
    position: absolute;
    top: 4px;
    right: 4px;
    background: rgba(0, 0, 0, 0.7);
    border-radius: 4px;
    padding: 4px;
    color: var(--text-muted);
  }

  .playlist-grid-item .playlist-name {
    font-size: 12px;
    line-height: 1.3;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
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
</style>
