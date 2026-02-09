<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { X, Trash2, EyeOff, Eye, Folder } from 'lucide-svelte';
  import { logPlaylistAdd } from '$lib/services/recoService';
  import { subscribe as subscribeOffline, getStatus, createPendingPlaylist } from '$lib/stores/offlineStore';
  import { showToast } from '$lib/stores/toastStore';
  import { t } from '$lib/i18n';
  import PlaylistDuplicateConfirmModal from './PlaylistDuplicateConfirmModal.svelte';
  import type { PlaylistDuplicateResult } from '$lib/types/index';
  import {
    subscribe as subscribeFolders,
    getVisibleFolders,
    movePlaylistToFolder,
    type PlaylistFolder
  } from '$lib/stores/playlistFoldersStore';

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
  }

  interface Props {
    isOpen: boolean;
    mode: 'create' | 'edit' | 'addTrack';
    playlist?: Playlist;
    trackIds?: number[];
    userPlaylists?: Playlist[];
    onClose: () => void;
    onSuccess?: (playlist?: Playlist) => void;
    onDelete?: (playlistId: number) => void;
    isHidden?: boolean;
    isLocalTracks?: boolean;
    currentFolderId?: string | null;
  }

  let {
    isOpen,
    mode,
    playlist,
    trackIds = [],
    userPlaylists = [],
    onClose,
    onSuccess,
    onDelete,
    isHidden = false,
    isLocalTracks = false,
    currentFolderId = null
  }: Props = $props();

  // Form state
  let name = $state('');
  let description = $state('');
  let isPublic = $state(false);
  let hidden = $state(false);
  let folderId = $state<string | null>(null);
  let selectedPlaylistId = $state<number | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // Searchable playlist dropdown state
  let playlistSearchQuery = $state('');
  let isPlaylistDropdownOpen = $state(false);
  let highlightedIndex = $state(-1);
  let dropdownRef = $state<HTMLDivElement | null>(null);
  let inputRef = $state<HTMLInputElement | null>(null);

  // Folders state
  let folders = $state<PlaylistFolder[]>([]);

  // Offline state (reactive)
  let offlineMode = $state(false);

  // Special value for "create new playlist" option (won't collide with negative pending playlist IDs)
  const CREATE_NEW_PLAYLIST = -999999;

  // Local track counts for playlists
  let localTrackCounts = $state<Map<number, number>>(new Map());
  let showDeleteConfirm = $state(false);

  // State for duplicate checking
  let showDuplicateConfirm = $state(false);
  let duplicateResult = $state<PlaylistDuplicateResult | null>(null);

  // Load local track counts
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

  // Get total track count for a playlist
  function getTotalTrackCount(pl: Playlist): number {
    const localCount = localTrackCounts.get(pl.id) ?? 0;
    return pl.tracks_count + localCount;
  }

  // Filtered playlists based on search query
  const filteredPlaylists = $derived(
    playlistSearchQuery.trim()
      ? userPlaylists.filter(pl =>
          pl.name.toLowerCase().includes(playlistSearchQuery.toLowerCase())
        )
      : userPlaylists
  );

  // Get display text for selected playlist - regular function to avoid $t() in $derived()
  function getSelectedPlaylistDisplay(): string {
    if (selectedPlaylistId === null) return '';
    if (selectedPlaylistId === CREATE_NEW_PLAYLIST) return '+ ' + $t('playlist.createNewPlaylist');
    const pl = userPlaylists.find(p => p.id === selectedPlaylistId);
    return pl ? $t('playlist.playlistWithTracks', { values: { count: getTotalTrackCount(pl), name: pl.name } }) : '';
  }

  // Handle dropdown item click
  function selectPlaylist(id: number | null) {
    selectedPlaylistId = id;
    playlistSearchQuery = '';
    isPlaylistDropdownOpen = false;
    highlightedIndex = -1;
  }

  // Handle dropdown keyboard navigation
  function handleDropdownKeydown(e: KeyboardEvent) {
    if (!isPlaylistDropdownOpen) {
      if (e.key === 'ArrowDown' || e.key === 'Enter') {
        isPlaylistDropdownOpen = true;
        highlightedIndex = 0;
        e.preventDefault();
      }
      return;
    }

    const totalItems = filteredPlaylists.length + 1; // +1 for "Create new" option

    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        highlightedIndex = (highlightedIndex + 1) % totalItems;
        break;
      case 'ArrowUp':
        e.preventDefault();
        highlightedIndex = highlightedIndex <= 0 ? totalItems - 1 : highlightedIndex - 1;
        break;
      case 'Enter':
        e.preventDefault();
        if (highlightedIndex === 0) {
          selectPlaylist(CREATE_NEW_PLAYLIST);
        } else if (highlightedIndex > 0 && highlightedIndex <= filteredPlaylists.length) {
          selectPlaylist(filteredPlaylists[highlightedIndex - 1].id);
        }
        break;
      case 'Escape':
        e.preventDefault();
        isPlaylistDropdownOpen = false;
        highlightedIndex = -1;
        break;
    }
  }

  // Close dropdown when clicking outside
  function handleClickOutside(e: MouseEvent) {
    if (dropdownRef && !dropdownRef.contains(e.target as Node)) {
      isPlaylistDropdownOpen = false;
      highlightedIndex = -1;
    }
  }

  // Register click outside listener when dropdown opens
  $effect(() => {
    if (isPlaylistDropdownOpen) {
      document.addEventListener('click', handleClickOutside);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  });

  // Subscribe to offline state changes
  $effect(() => {
    const unsubscribe = subscribeOffline(() => {
      const status = getStatus();
      offlineMode = status.isOffline;
    });
    return unsubscribe;
  });

  // Subscribe to folders state changes
  $effect(() => {
    const unsubscribe = subscribeFolders(() => {
      folders = getVisibleFolders();
    });
    // Initialize folders
    folders = getVisibleFolders();
    return unsubscribe;
  });

  // Reset form when modal opens
  $effect(() => {
    if (isOpen) {
      error = null;
      loading = false;
      showDeleteConfirm = false;
      if (mode === 'edit' && playlist) {
        name = playlist.name;
        description = '';
        isPublic = false;
        hidden = isHidden;
        folderId = currentFolderId;
      } else if (mode === 'create') {
        name = '';
        description = '';
        isPublic = false;
        hidden = false;
        folderId = null;
      } else if (mode === 'addTrack') {
        selectedPlaylistId = null;
        folderId = null;
        playlistSearchQuery = '';
        isPlaylistDropdownOpen = false;
        highlightedIndex = -1;
        loadLocalTrackCounts();
      }
    }
  });

  async function handleCreate() {
    if (!name.trim()) {
      error = $t('playlist.enterName');
      return;
    }

    loading = true;
    error = null;

    try {
      if (offlineMode) {
        // Create pending playlist for sync when back online
        const pendingId = await createPendingPlaylist(
          name.trim(),
          description.trim() || null,
          isPublic,
          [], // No Qobuz tracks for empty playlist
          []  // No local tracks for empty playlist
        );
        showToast($t('toast.playlistCreatedOffline'), 'info');
        // Create a temporary playlist object for UI
        const tempPlaylist: Playlist = {
          id: -pendingId, // Negative ID to distinguish from real playlists
          name: name.trim(),
          tracks_count: 0
        };
        onSuccess?.(tempPlaylist);
        onClose();
      } else {
        // Create playlist normally via API
        const newPlaylist = await invoke<Playlist>('create_playlist', {
          name: name.trim(),
          description: description.trim() || null,
          isPublic
        });

        // Assign to folder if selected
        if (folderId) {
          await movePlaylistToFolder(newPlaylist.id, folderId);
        }

        onSuccess?.(newPlaylist);
        onClose();
      }
    } catch (err) {
      console.error('Failed to create playlist:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function handleUpdate() {
    if (!playlist) return;
    if (!name.trim()) {
      error = $t('playlist.enterName');
      return;
    }

    loading = true;
    error = null;

    try {
      // Update playlist on Qobuz
      const updatedPlaylist = await invoke<Playlist>('update_playlist', {
        playlistId: playlist.id,
        name: name.trim(),
        description: description.trim() || null,
        isPublic
      });

      // Update hidden status locally
      await invoke('playlist_set_hidden', {
        playlistId: playlist.id,
        hidden
      });

      // Update folder assignment if changed
      if (folderId !== currentFolderId) {
        await movePlaylistToFolder(playlist.id, folderId);
      }

      onSuccess?.(updatedPlaylist);
      onClose();
    } catch (err) {
      console.error('Failed to update playlist:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function handleDelete() {
    if (!playlist) return;

    loading = true;
    error = null;

    try {
      await invoke('delete_playlist', { playlistId: playlist.id });
      onDelete?.(playlist.id);
      onClose();
    } catch (err) {
      console.error('Failed to delete playlist:', err);
      error = String(err);
      showDeleteConfirm = false;
    } finally {
      loading = false;
    }
  }

  async function handleAddToPlaylist() {
    if (!selectedPlaylistId || trackIds.length === 0) {
      error = $t('playlist.selectPlaylist');
      return;
    }

    // For pending playlists and local tracks add directly
    if (selectedPlaylistId < 0 || isLocalTracks) {
      await handleDirectAddToPlaylist();
      return;
    }

    loading = true;
    error = null;

    try {
      // Check for duplicates in regular playlists with Qobuz tracks
      const result = await invoke<PlaylistDuplicateResult>('check_playlist_duplicates', {
        playlistId: selectedPlaylistId,
        trackIds
      });
      
      duplicateResult = {
        ...result,
        duplicate_track_ids: new Set(result.duplicate_track_ids)
      };

      if (duplicateResult.duplicate_count > 0) {
        // There are duplicates. Show confirmation dialog
        // to allow the user to decide on what action to take
        showDuplicateConfirm = true;
      } else {
        await executeAddTracks(false);
      }
    } catch (err) {
      console.error('Failed to check playlist duplicates:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function handleDirectAddToPlaylist() {
    if (!selectedPlaylistId || trackIds.length === 0) {
      error = $t('playlist.selectPlaylist');
      return;
    }

    loading = true;
    error = null;

    try {
      // Check if this is a pending playlist (negative ID)
      if (selectedPlaylistId < 0) {
        const pendingId = -selectedPlaylistId; // Convert back to positive
        const qobuzTrackIds = isLocalTracks ? [] : trackIds;

        // For local tracks, fetch file paths instead of using IDs
        let localTrackPaths: string[] = [];
        if (isLocalTracks && trackIds.length > 0) {
          // Fetch tracks to get their file paths
          const tracks = await invoke<Array<{ id: number; file_path: string }>>('library_get_tracks_by_ids', {
            trackIds
          });
          localTrackPaths = tracks.map(track => track.file_path);
        }

        await invoke('add_tracks_to_pending_playlist', {
          pendingId,
          qobuzTrackIds,
          localTrackPaths
        });
      } else if (isLocalTracks) {
        // Regular playlist with local tracks
        // Get current total count (Qobuz + local) to append at correct position
        const playlist = userPlaylists.find(p => p.id === selectedPlaylistId);
        const qobuzCount = playlist?.tracks_count ?? 0;
        const localCount = localTrackCounts.get(selectedPlaylistId!) ?? 0;
        const startPosition = qobuzCount + localCount;

        // Add local tracks at the end
        for (let i = 0; i < trackIds.length; i++) {
          await invoke('playlist_add_local_track', {
            playlistId: selectedPlaylistId,
            localTrackId: trackIds[i],
            position: startPosition + i
          });
        }
      } else {
        // Regular playlist with Qobuz tracks
        await executeAddTracks(false);
      }
      onSuccess?.();
      onClose();
    } catch (err) {
      console.error('Failed to add tracks to playlist:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function executeAddTracks(skipDuplicates: boolean) {
    try {
      let tracksToAdd = trackIds;

      if (skipDuplicates) {
        tracksToAdd = trackIds.filter(id => !duplicateResult?.duplicate_track_ids.has(id));
      }

      if (tracksToAdd.length === 0) {
        onSuccess?.();
        onClose();
        showDuplicateConfirm = false;
        return;
      }

      await invoke('add_tracks_to_playlist', {
        playlistId: selectedPlaylistId!,
        trackIds: tracksToAdd
      });
      void logPlaylistAdd(trackIds, selectedPlaylistId!);
      onSuccess?.();
      onClose();
      showDuplicateConfirm = false;
    } catch (err) {
      console.error('Failed to add tracks to playlist:', err);
      error = String(err);
    }
  }

  function handleDuplicateAddAll() {
    executeAddTracks(false);
  }

  function handleDuplicateSkipDuplicates() {
    executeAddTracks(true);
  }

  function handleDuplicateCancel() {
    showDuplicateConfirm = false;
    duplicateResult = null;
  }

  async function handleCreateAndAdd() {
    if (!name.trim()) {
      error = $t('playlist.enterName');
      return;
    }

    loading = true;
    error = null;

    try {
      if (offlineMode) {
        // In offline mode, create pending playlist with both Qobuz and local tracks
        const qobuzTrackIds = isLocalTracks ? [] : trackIds;

        // For local tracks, fetch file paths instead of using IDs
        let localTrackPaths: string[] = [];
        if (isLocalTracks && trackIds.length > 0) {
          // Fetch tracks to get their file paths
          const tracks = await invoke<Array<{ id: number; file_path: string }>>('library_get_tracks_by_ids', {
            trackIds
          });
          localTrackPaths = tracks.map(track => track.file_path);
        }

        // Create pending playlist for sync when back online
        const pendingId = await createPendingPlaylist(
          name.trim(),
          description.trim() || null,
          false,
          qobuzTrackIds,
          localTrackPaths
        );
        showToast($t('toast.playlistCreatedOffline'), 'info');

        // Create a temporary playlist object for UI
        const tempPlaylist: Playlist = {
          id: -pendingId, // Negative ID to distinguish from real playlists
          name: name.trim(),
          tracks_count: trackIds.length
        };
        onSuccess?.(tempPlaylist);
        onClose();
        return;
      }

      // Online mode - create the playlist first
      const newPlaylist = await invoke<Playlist>('create_playlist', {
        name: name.trim(),
        description: description.trim() || null,
        isPublic: false
      });

      // Then add tracks
      if (trackIds.length > 0) {
        if (isLocalTracks) {
          // Add local tracks one by one
          for (let i = 0; i < trackIds.length; i++) {
            await invoke('playlist_add_local_track', {
              playlistId: newPlaylist.id,
              localTrackId: trackIds[i],
              position: i
            });
          }
        } else {
          await invoke('add_tracks_to_playlist', {
            playlistId: newPlaylist.id,
            trackIds
          });
          void logPlaylistAdd(trackIds, newPlaylist.id);
        }
        // Update tracks_count to reflect added tracks (API returns 0 at creation)
        newPlaylist.tracks_count = isLocalTracks ? 0 : trackIds.length;
      }

      onSuccess?.(newPlaylist);
      onClose();
    } catch (err) {
      console.error('Failed to create playlist and add tracks:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  function handleSubmit() {
    if (mode === 'create') {
      handleCreate();
    } else if (mode === 'edit') {
      handleUpdate();
    } else if (mode === 'addTrack') {
      if (selectedPlaylistId === CREATE_NEW_PLAYLIST) {
        // Create new playlist option selected
        handleCreateAndAdd();
      } else {
        handleAddToPlaylist();
      }
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    } else if (e.key === 'Enter' && !e.shiftKey) {
      handleSubmit();
    }
  }
</script>

{#if isOpen}
  <div
    class="modal-overlay"
    onclick={() => onClose()}
    onkeydown={handleKeydown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>
          {#if mode === 'create'}
            {$t('playlist.newPlaylist')}
          {:else if mode === 'edit'}
            {$t('playlist.editPlaylist')}
          {:else}
            {$t('playlist.addToPlaylist')}
          {/if}
        </h2>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      <div class="modal-body">
        {#if error}
          <div class="error-message">{error}</div>
        {/if}

        {#if mode === 'addTrack'}
          <div class="form-group">
            <div class="label-row">
              <label for="playlist-search">{$t('playlist.choosePlaylist')}</label>
              <span class="track-info-inline">
                {$t('playlist.addingTracks', { values: { count: trackIds.length, type: isLocalTracks ? $t('playlist.local') : '' } })}
              </span>
            </div>
            <div class="playlist-dropdown" bind:this={dropdownRef}>
              <input
                type="text"
                id="playlist-search"
                bind:this={inputRef}
                bind:value={playlistSearchQuery}
                placeholder={selectedPlaylistId ? getSelectedPlaylistDisplay() : $t('placeholders.searchPlaylists')}
                disabled={loading}
                onfocus={() => isPlaylistDropdownOpen = true}
                onkeydown={handleDropdownKeydown}
                autocomplete="off"
              />
              {#if selectedPlaylistId !== null && !isPlaylistDropdownOpen}
                <button
                  type="button"
                  class="clear-selection"
                  onclick={() => selectPlaylist(null)}
                  disabled={loading}
                >
                  <X size={14} />
                </button>
              {/if}
              {#if isPlaylistDropdownOpen}
                <div class="playlist-dropdown-list">
                  <button
                    type="button"
                    class="playlist-dropdown-item create-new"
                    class:highlighted={highlightedIndex === 0}
                    onclick={() => selectPlaylist(CREATE_NEW_PLAYLIST)}
                  >
                    + {$t('playlist.createNewPlaylist')}
                  </button>
                  {#each filteredPlaylists.slice(0, 8) as pl, i (pl.id)}
                    <button
                      type="button"
                      class="playlist-dropdown-item"
                      class:highlighted={highlightedIndex === i + 1}
                      onclick={() => selectPlaylist(pl.id)}
                    >
                      <span class="playlist-name">{pl.name}</span>
                      <span class="playlist-count">{$t('playlist.trackCount', { values: { count: getTotalTrackCount(pl) } })}</span>
                    </button>
                  {/each}
                  {#if filteredPlaylists.length === 0 && playlistSearchQuery.trim()}
                    <div class="no-results">{$t('playlist.noPlaylistsFound')}</div>
                  {/if}
                  {#if filteredPlaylists.length > 8}
                    <div class="more-results">
                      {$t('playlist.morePlaylists', { values: { count: filteredPlaylists.length - 8 } })}
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          </div>

          {#if selectedPlaylistId === CREATE_NEW_PLAYLIST}
            <div class="form-group">
              <label for="name">{$t('playlist.playlistName')}</label>
              <input
                type="text"
                id="name"
                bind:value={name}
                placeholder={$t('placeholders.myPlaylist')}
                disabled={loading}
              />
            </div>
          {/if}
        {:else}
          <div class="form-group">
            <label for="name">{$t('playlist.name')}</label>
            <input
              type="text"
              id="name"
              bind:value={name}
              placeholder={$t('placeholders.myPlaylist')}
              disabled={loading}
            />
          </div>

          <div class="form-group">
            <label for="description">{$t('playlist.descriptionOptional')}</label>
            <textarea
              id="description"
              bind:value={description}
              placeholder={$t('placeholders.addDescription')}
              rows="3"
              disabled={loading}
            ></textarea>
          </div>

          {#if folders.length > 0}
            <div class="form-group">
              <label for="folder-select">
                <Folder size={14} class="icon-inline" />
                {$t('playlist.folder')}
              </label>
              <select
                id="folder-select"
                bind:value={folderId}
                disabled={loading}
              >
                <option value={null}>{$t('playlist.noFolder')}</option>
                {#each folders as folder (folder.id)}
                  <option value={folder.id}>{folder.name}</option>
                {/each}
              </select>
            </div>
          {/if}

          <div class="checkbox-row">
            <div class="form-group checkbox">
              <label>
                <input
                  type="checkbox"
                  bind:checked={isPublic}
                  disabled={loading}
                />
                <span>{$t('playlist.makePublic')}</span>
              </label>
            </div>

            {#if mode === 'edit'}
              <div class="form-group checkbox">
                <label>
                  <input
                    type="checkbox"
                    bind:checked={hidden}
                    disabled={loading}
                  />
                  <span class="hidden-label">
                    {#if hidden}
                      <EyeOff size={14} />
                    {:else}
                      <Eye size={14} />
                    {/if}
                    {$t('playlist.hideFromSidebar')}
                  </span>
                </label>
              </div>
            {/if}
          </div>
        {/if}
      </div>

      <div class="modal-footer">
        {#if mode === 'edit'}
          <div class="footer-left">
            {#if showDeleteConfirm}
              <div class="delete-confirm-inline">
                <span>{$t('actions.deleteConfirm')}</span>
                <button class="btn-delete-sm" onclick={handleDelete} disabled={loading}>
                  {$t('actions.yes')}
                </button>
                <button class="btn-cancel-sm" onclick={() => showDeleteConfirm = false} disabled={loading}>
                  {$t('actions.no')}
                </button>
              </div>
            {:else}
              <button class="btn-danger-sm" onclick={() => showDeleteConfirm = true} disabled={loading}>
                <Trash2 size={12} />
                {$t('actions.delete')}
              </button>
            {/if}
          </div>
        {/if}
        <div class="footer-right">
          <button class="btn btn-secondary" onclick={onClose} disabled={loading}>
            {$t('actions.cancel')}
          </button>
          <button class="btn btn-primary" onclick={handleSubmit} disabled={loading}>
            {#if loading}
              {$t('actions.saving')}
            {:else if mode === 'create'}
              {$t('actions.create')}
            {:else if mode === 'edit'}
              {$t('actions.save')}
            {:else if selectedPlaylistId === CREATE_NEW_PLAYLIST}
              {$t('playlist.createAndAdd')}
            {:else}
              {$t('actions.add')}
            {/if}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

<PlaylistDuplicateConfirmModal
  isOpen={showDuplicateConfirm}
  {duplicateResult}
  {loading}
  onAddAll={handleDuplicateAddAll}
  onSkipDuplicates={handleDuplicateSkipDuplicates}
  onCancel={handleDuplicateCancel}
/>

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }

  .modal {
    width: 100%;
    max-width: 490px;
    max-height: 90vh;
    overflow: visible;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border-radius: 16px;
    border: 1px solid var(--bg-tertiary);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
  }

  /* Taller modal for addTrack mode */
  .modal:has(.label-row) {
    min-height: 400px;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 20px 24px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .modal-header h2 {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    transition: color 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  .modal-body {
    padding: 24px;
    overflow: visible;
    flex: 1;
  }

  .error-message {
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: #ef4444;
    padding: 12px;
    border-radius: 8px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .label-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 8px;
  }

  .label-row label {
    margin-bottom: 0;
  }

  .track-info-inline {
    font-size: 14px;
    color: var(--text-muted);
  }

  .form-group {
    margin-bottom: 16px;
  }

  .form-group label {
    display: block;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    margin-bottom: 8px;
  }

  .form-group input[type="text"],
  .form-group textarea,
  .form-group select {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    color: var(--text-primary);
    transition: border-color 150ms ease;
  }

  .form-group select {
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23888888' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 12px center;
    padding-right: 36px;
    cursor: pointer;
  }

  .form-group select option {
    background: var(--bg-primary);
    color: var(--text-primary);
    padding: 10px;
  }

  /* Searchable playlist dropdown */
  .playlist-dropdown {
    position: relative;
  }

  .playlist-dropdown input {
    width: 100%;
    padding: 10px 36px 10px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    color: var(--text-primary);
    transition: border-color 150ms ease;
  }

  .playlist-dropdown input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .playlist-dropdown input::placeholder {
    color: var(--text-muted);
  }

  .clear-selection {
    position: absolute;
    right: 10px;
    top: 50%;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .clear-selection:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .playlist-dropdown-list {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    max-height: 320px;
    overflow-y: auto;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10001;
  }

  .playlist-dropdown-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 10px 12px;
    background: none;
    border: none;
    font-size: 14px;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    transition: background-color 150ms ease;
  }

  .playlist-dropdown-item:hover,
  .playlist-dropdown-item.highlighted {
    background: var(--bg-hover);
  }

  .playlist-dropdown-item.create-new {
    color: var(--accent-primary);
    font-weight: 500;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .playlist-dropdown-item .playlist-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-right: 8px;
  }

  .playlist-dropdown-item .playlist-count {
    font-size: 12px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .no-results,
  .more-results {
    padding: 10px 12px;
    font-size: 13px;
    color: var(--text-muted);
    text-align: center;
  }

  .more-results {
    border-top: 1px solid var(--bg-tertiary);
    font-style: italic;
  }

  .form-group input[type="text"]:focus,
  .form-group textarea:focus,
  .form-group select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .form-group textarea {
    resize: vertical;
    min-height: 80px;
  }

  .form-group.checkbox label {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
  }

  .form-group.checkbox input[type="checkbox"] {
    width: 18px;
    height: 18px;
    accent-color: var(--accent-primary);
  }

  .form-group.checkbox span {
    font-size: 14px;
    color: var(--text-primary);
  }

  .checkbox-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-top: 8px;
  }

  .checkbox-row .form-group {
    margin-bottom: 0;
  }

  .modal-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 16px 24px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .footer-left {
    display: flex;
    align-items: center;
  }

  .footer-right {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: auto;
  }

  .hidden-label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  /* Compact footer delete button */
  .btn-danger-sm {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 8px;
    font-size: 13px;
    color: #ef4444;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .btn-danger-sm:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.2);
    border-color: rgba(239, 68, 68, 0.5);
  }

  .btn-danger-sm:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .delete-confirm-inline {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .delete-confirm-inline span {
    font-size: 13px;
    color: #ef4444;
  }

  .btn-delete-sm,
  .btn-cancel-sm {
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
    transition: all 150ms ease;
    border: none;
  }

  .btn-cancel-sm {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .btn-cancel-sm:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .btn-delete-sm {
    background: #ef4444;
    color: white;
  }

  .btn-delete-sm:hover:not(:disabled) {
    background: #dc2626;
  }

  .btn-delete-sm:disabled,
  .btn-cancel-sm:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

</style>
