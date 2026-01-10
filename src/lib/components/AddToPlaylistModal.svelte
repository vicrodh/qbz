<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { X, Plus, ListMusic, Loader2 } from 'lucide-svelte';

  interface Playlist {
    id: number;
    name: string;
    tracks_count: number;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onSelect: (playlistId: number) => Promise<void>;
    trackTitle?: string;
  }

  let { isOpen, onClose, onSelect, trackTitle }: Props = $props();

  let playlists = $state<Playlist[]>([]);
  let loading = $state(true);
  let adding = $state<number | null>(null);
  let error = $state<string | null>(null);

  onMount(() => {
    if (isOpen) {
      loadPlaylists();
    }
  });

  $effect(() => {
    if (isOpen && playlists.length === 0) {
      loadPlaylists();
    }
  });

  async function loadPlaylists() {
    loading = true;
    error = null;
    try {
      const result = await invoke<{ items: Playlist[] }>('get_user_playlists', { limit: 100, offset: 0 });
      playlists = result.items || [];
    } catch (err) {
      console.error('Failed to load playlists:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function handleSelect(playlistId: number) {
    adding = playlistId;
    try {
      await onSelect(playlistId);
      onClose();
    } catch (err) {
      console.error('Failed to add to playlist:', err);
      error = String(err);
    } finally {
      adding = null;
    }
  }
</script>

{#if isOpen}
  <div class="overlay" onclick={onClose} onkeydown={(e) => e.key === 'Escape' && onClose()} role="presentation">
    <div class="modal" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()} role="dialog" tabindex="-1">
      <div class="header">
        <h3>Add to Playlist</h3>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      {#if trackTitle}
        <div class="track-info">
          Adding: <span class="track-name">{trackTitle}</span>
        </div>
      {/if}

      <div class="content">
        {#if loading}
          <div class="loading">
            <Loader2 size={24} class="spin" />
            <p>Loading playlists...</p>
          </div>
        {:else if error}
          <div class="error">
            <p>{error}</p>
            <button class="retry-btn" onclick={loadPlaylists}>Retry</button>
          </div>
        {:else if playlists.length === 0}
          <div class="empty">
            <ListMusic size={32} />
            <p>No playlists found</p>
            <p class="hint">Create a playlist first in Qobuz</p>
          </div>
        {:else}
          <div class="playlist-list">
            {#each playlists as playlist}
              <button
                class="playlist-item"
                disabled={adding !== null}
                onclick={() => handleSelect(playlist.id)}
              >
                <div class="playlist-icon">
                  <ListMusic size={20} />
                </div>
                <div class="playlist-info">
                  <span class="playlist-name">{playlist.name}</span>
                  <span class="playlist-count">{playlist.tracks_count} tracks</span>
                </div>
                {#if adding === playlist.id}
                  <Loader2 size={16} class="spin" />
                {:else}
                  <Plus size={16} class="add-icon" />
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    background-color: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .modal {
    width: 400px;
    max-height: 500px;
    background-color: var(--bg-secondary);
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .header h3 {
    font-size: 16px;
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
    border-radius: 4px;
  }

  .close-btn:hover {
    color: var(--text-primary);
    background-color: rgba(255, 255, 255, 0.1);
  }

  .track-info {
    padding: 12px 20px;
    background-color: var(--bg-tertiary);
    font-size: 13px;
    color: var(--text-muted);
  }

  .track-name {
    color: var(--text-primary);
    font-weight: 500;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }

  .loading, .empty, .error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 32px;
    gap: 12px;
    color: var(--text-muted);
  }

  .loading :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .hint {
    font-size: 12px;
    color: #666666;
  }

  .error {
    color: #ff6b6b;
  }

  .retry-btn {
    margin-top: 8px;
    padding: 8px 16px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 6px;
    cursor: pointer;
  }

  .playlist-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .playlist-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: none;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
    width: 100%;
    text-align: left;
  }

  .playlist-item:hover:not(:disabled) {
    background-color: var(--bg-tertiary);
  }

  .playlist-item:disabled {
    opacity: 0.6;
    cursor: wait;
  }

  .playlist-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    background-color: var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-muted);
  }

  .playlist-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .playlist-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playlist-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .add-icon {
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .playlist-item:hover .add-icon {
    opacity: 1;
    color: var(--accent-primary);
  }

  .playlist-item :global(.spin) {
    color: var(--accent-primary);
  }
</style>
