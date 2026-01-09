<script lang="ts">
  import { Search, Home, HardDrive, Music, Disc3, Mic2, Plus, RefreshCw, ChevronDown, Heart, ListMusic } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import NavigationItem from './NavigationItem.svelte';
  import UserCard from './UserCard.svelte';

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
  }

  interface Props {
    activeView: string;
    selectedPlaylistId?: number | null;
    onNavigate: (view: string) => void;
    onPlaylistSelect?: (playlistId: number) => void;
    onCreatePlaylist?: () => void;
    onSettingsClick?: () => void;
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
    onSettingsClick,
    onLogout,
    userName = 'User',
    subscription = 'Qobuz'
  }: Props = $props();

  let userPlaylists = $state<Playlist[]>([]);
  let playlistsLoading = $state(false);

  // Expose playlists to parent via binding
  export function getPlaylists(): Playlist[] {
    return userPlaylists;
  }

  export function refreshPlaylists() {
    loadUserPlaylists();
  }

  onMount(() => {
    loadUserPlaylists();
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

    <!-- Playlists Section -->
    <div class="section playlists-section">
      <div class="playlists-header">
        <div class="section-header">Playlists</div>
        <div class="header-actions">
          <button class="icon-btn" onclick={onCreatePlaylist} title="New playlist">
            <Plus size={14} />
          </button>
          <button class="icon-btn" onclick={loadUserPlaylists} title="Refresh playlists">
            <RefreshCw size={14} />
          </button>
        </div>
      </div>

      <NavigationItem
        label="Favorites"
        active={activeView === 'favorites'}
        onclick={() => handleViewChange('favorites')}
      >
        {#snippet icon()}<Heart size={14} />{/snippet}
      </NavigationItem>

      {#if playlistsLoading}
        <div class="playlists-loading">Loading...</div>
      {:else if userPlaylists.length > 0}
        {#each userPlaylists as playlist (playlist.id)}
          <NavigationItem
            label={playlist.name}
            active={activeView === 'playlist' && selectedPlaylistId === playlist.id}
            onclick={() => handlePlaylistClick(playlist)}
          >
            {#snippet icon()}<ListMusic size={14} />{/snippet}
          </NavigationItem>
        {/each}
      {:else}
        <div class="no-playlists">No playlists yet</div>
      {/if}
    </div>

    <!-- Local Library Section -->
    <div class="section">
      <div class="section-header">Local Library</div>
      <NavigationItem
        label="Browse Library"
        active={activeView === 'library'}
        onclick={() => handleViewChange('library')}
      >
        {#snippet icon()}<HardDrive size={14} />{/snippet}
      </NavigationItem>
    </div>
  </div>

  <!-- Fixed User Profile at Bottom -->
  <div class="user-section">
    <UserCard
      username={userName}
      {subscription}
      onSettingsClick={onSettingsClick ?? (() => handleViewChange('settings'))}
      {onLogout}
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
    height: calc(100vh - 80px);
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
  }

  .icon-btn:hover {
    color: var(--text-primary);
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
