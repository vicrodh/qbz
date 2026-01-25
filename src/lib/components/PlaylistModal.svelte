<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { X, Trash2, EyeOff, Eye, Folder } from 'lucide-svelte';
  import { logPlaylistAdd } from '$lib/services/recoService';
  import { subscribe as subscribeOffline, getStatus, createPendingPlaylist } from '$lib/stores/offlineStore';
  import { showToast } from '$lib/stores/toastStore';
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

  // Folders state
  let folders = $state<PlaylistFolder[]>([]);

  // Offline state (reactive)
  let offlineMode = $state(false);

  // Special value for "create new playlist" option (won't collide with negative pending playlist IDs)
  const CREATE_NEW_PLAYLIST = -999999;

  // Local track counts for playlists
  let localTrackCounts = $state<Map<number, number>>(new Map());
  let showDeleteConfirm = $state(false);

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
        loadLocalTrackCounts();
      }
    }
  });

  async function handleCreate() {
    if (!name.trim()) {
      error = 'Please enter a playlist name';
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
        showToast('Playlist created offline - will sync when back online', 'info');
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
      error = 'Please enter a playlist name';
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
      error = 'Please select a playlist';
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
          localTrackPaths = tracks.map(t => t.file_path);
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
        await invoke('add_tracks_to_playlist', {
          playlistId: selectedPlaylistId,
          trackIds
        });
        void logPlaylistAdd(trackIds, selectedPlaylistId);
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

  async function handleCreateAndAdd() {
    if (!name.trim()) {
      error = 'Please enter a playlist name';
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
          localTrackPaths = tracks.map(t => t.file_path);
        }

        // Create pending playlist for sync when back online
        const pendingId = await createPendingPlaylist(
          name.trim(),
          description.trim() || null,
          false,
          qobuzTrackIds,
          localTrackPaths
        );
        showToast(`Playlist "${name.trim()}" created offline - will sync when back online`, 'info');

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
            New Playlist
          {:else if mode === 'edit'}
            Edit Playlist
          {:else}
            Add to Playlist
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
          <div class="track-info">
            Adding {trackIds.length} {isLocalTracks ? 'local ' : ''}track{trackIds.length !== 1 ? 's' : ''}
          </div>

          <div class="form-group">
            <label for="playlist-select">Choose playlist</label>
            <select
              id="playlist-select"
              bind:value={selectedPlaylistId}
              disabled={loading}
            >
              <option value={null}>Select a playlist...</option>
              <option value={CREATE_NEW_PLAYLIST}>+ Create new playlist</option>
              {#each userPlaylists as pl (pl.id)}
                <option value={pl.id}>{pl.name} ({getTotalTrackCount(pl)} tracks)</option>
              {/each}
            </select>
          </div>

          {#if selectedPlaylistId === CREATE_NEW_PLAYLIST}
            <div class="form-group">
              <label for="name">Playlist name</label>
              <input
                type="text"
                id="name"
                bind:value={name}
                placeholder="My Playlist"
                disabled={loading}
              />
            </div>
          {/if}
        {:else}
          <div class="form-group">
            <label for="name">Name</label>
            <input
              type="text"
              id="name"
              bind:value={name}
              placeholder="My Playlist"
              disabled={loading}
            />
          </div>

          <div class="form-group">
            <label for="description">Description (optional)</label>
            <textarea
              id="description"
              bind:value={description}
              placeholder="Add a description..."
              rows="3"
              disabled={loading}
            ></textarea>
          </div>

          {#if folders.length > 0}
            <div class="form-group">
              <label for="folder-select">
                <Folder size={14} style="display: inline; vertical-align: middle; margin-right: 4px;" />
                Folder
              </label>
              <select
                id="folder-select"
                bind:value={folderId}
                disabled={loading}
              >
                <option value={null}>No folder</option>
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
                <span>Make playlist public</span>
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
                    Hide from sidebar
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
                <span>Delete?</span>
                <button class="btn-delete-sm" onclick={handleDelete} disabled={loading}>
                  Yes
                </button>
                <button class="btn-cancel-sm" onclick={() => showDeleteConfirm = false} disabled={loading}>
                  No
                </button>
              </div>
            {:else}
              <button class="btn-danger-sm" onclick={() => showDeleteConfirm = true} disabled={loading}>
                <Trash2 size={12} />
                Delete
              </button>
            {/if}
          </div>
        {/if}
        <div class="footer-right">
          <button class="btn-secondary" onclick={onClose} disabled={loading}>
            Cancel
          </button>
          <button class="btn-primary" onclick={handleSubmit} disabled={loading}>
            {#if loading}
              Saving...
            {:else if mode === 'create'}
              Create
            {:else if mode === 'edit'}
              Save
            {:else if selectedPlaylistId === CREATE_NEW_PLAYLIST}
              Create & Add
            {:else}
              Add
            {/if}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

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
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border-radius: 16px;
    border: 1px solid var(--bg-tertiary);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
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
    overflow-y: auto;
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

  .track-info {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 16px;
    padding: 12px;
    background: var(--bg-secondary);
    border-radius: 8px;
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

  .btn-secondary,
  .btn-primary {
    padding: 10px 20px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .btn-secondary {
    background: transparent;
    border: 1px solid var(--text-muted);
    color: var(--text-primary);
  }

  .btn-secondary:hover:not(:disabled) {
    border-color: var(--text-primary);
  }

  .btn-primary {
    background: var(--accent-primary);
    border: none;
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .btn-secondary:disabled,
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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

  .danger-zone {
    margin-top: 24px;
    padding-top: 20px;
    border-top: 1px solid rgba(239, 68, 68, 0.2);
  }

  .danger-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #ef4444;
    margin-bottom: 12px;
  }

  .btn-danger {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 8px;
    font-size: 13px;
    color: #ef4444;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .btn-danger:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.2);
    border-color: rgba(239, 68, 68, 0.5);
  }

  .btn-danger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .delete-confirm {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 8px;
  }

  .delete-confirm span {
    font-size: 13px;
    color: #ef4444;
  }

  .delete-actions {
    display: flex;
    gap: 8px;
  }

  .btn-cancel {
    flex: 1;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    font-size: 13px;
    color: var(--text-primary);
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .btn-cancel:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .btn-delete {
    flex: 1;
    padding: 8px 12px;
    background: #ef4444;
    border: none;
    border-radius: 6px;
    font-size: 13px;
    color: white;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .btn-delete:hover:not(:disabled) {
    background: #dc2626;
  }

  .btn-cancel:disabled,
  .btn-delete:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
