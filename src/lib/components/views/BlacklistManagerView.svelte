<script lang="ts">
  import { onMount } from 'svelte';
  import { ArrowLeft, Search, X, Trash2, Ban, ToggleLeft, ToggleRight, AlertCircle } from 'lucide-svelte';
  import ViewTransition from '../ViewTransition.svelte';
  import {
    subscribe,
    loadBlacklist,
    removeFromBlacklist,
    clearBlacklist,
    setEnabled,
    isEnabled,
    getCachedBlacklist,
    getCount,
    type BlacklistedArtist
  } from '$lib/stores/artistBlacklistStore';

  interface Props {
    onBack?: () => void;
    onArtistSelect?: (artistId: number) => void;
  }

  let { onBack, onArtistSelect }: Props = $props();

  let artists = $state<BlacklistedArtist[]>([]);
  let enabled = $state(true);
  let loading = $state(true);
  let searchQuery = $state('');
  let confirmClearOpen = $state(false);

  // Filtered artists based on search
  const filteredArtists = $derived.by(() => {
    if (!searchQuery.trim()) return artists;
    const query = searchQuery.trim().toLowerCase();
    return artists.filter(a => a.artist_name.toLowerCase().includes(query));
  });

  onMount(() => {
    loadData();

    const unsubscribe = subscribe(() => {
      artists = getCachedBlacklist();
      enabled = isEnabled();
    });

    return unsubscribe;
  });

  async function loadData() {
    loading = true;
    try {
      await loadBlacklist();
      artists = getCachedBlacklist();
      enabled = isEnabled();
    } catch (err) {
      console.error('Failed to load blacklist:', err);
    } finally {
      loading = false;
    }
  }

  async function handleRemove(artistId: number) {
    try {
      await removeFromBlacklist(artistId);
    } catch (err) {
      console.error('Failed to remove artist from blacklist:', err);
    }
  }

  async function handleToggleEnabled() {
    try {
      await setEnabled(!enabled);
    } catch (err) {
      console.error('Failed to toggle blacklist:', err);
    }
  }

  async function handleClearAll() {
    try {
      await clearBlacklist();
      confirmClearOpen = false;
    } catch (err) {
      console.error('Failed to clear blacklist:', err);
    }
  }

  function formatDate(timestamp: number): string {
    return new Date(timestamp * 1000).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric'
    });
  }
</script>

<ViewTransition duration={200} distance={12} direction="down">
<div class="blacklist-manager">
  <!-- Header -->
  <div class="header">
    <button class="back-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>Back</span>
    </button>
    <div class="title-section">
      <Ban size={24} class="title-icon" />
      <h1>Artist Blacklist</h1>
    </div>
  </div>

  <!-- Description -->
  <p class="description">
    Blacklisted artists are hidden from search results, radio, suggestions, and similar artists.
  </p>

  <!-- Controls -->
  <div class="controls">
    <!-- Enable/Disable Toggle -->
    <button
      class="toggle-btn"
      class:enabled={enabled}
      onclick={handleToggleEnabled}
    >
      {#if enabled}
        <ToggleRight size={20} />
        <span>Enabled</span>
      {:else}
        <ToggleLeft size={20} />
        <span>Disabled</span>
      {/if}
    </button>

    <!-- Search bar -->
    <div class="search-container">
      <Search size={16} class="search-icon" />
      <input
        type="text"
        placeholder="Search blacklisted artists..."
        bind:value={searchQuery}
        class="search-input"
      />
      {#if searchQuery}
        <button class="clear-search" onclick={() => searchQuery = ''}>
          <X size={14} />
        </button>
      {/if}
    </div>

    <!-- Clear All Button -->
    {#if artists.length > 0}
      <button
        class="clear-all-btn"
        onclick={() => confirmClearOpen = true}
      >
        <Trash2 size={16} />
        <span>Clear All</span>
      </button>
    {/if}

    <span class="count">{getCount()} artists</span>
  </div>

  <!-- Disabled Warning -->
  {#if !enabled}
    <div class="warning-banner">
      <AlertCircle size={16} />
      <span>Blacklist is currently disabled. Artists below will appear in results.</span>
    </div>
  {/if}

  <!-- Content -->
  {#if loading}
    <div class="loading">
      <div class="spinner"></div>
      <p>Loading blacklist...</p>
    </div>
  {:else if artists.length === 0}
    <div class="empty">
      <Ban size={48} class="empty-icon" />
      <p>No blacklisted artists</p>
      <span class="empty-hint">To blacklist an artist, go to their page and click the ban icon.</span>
    </div>
  {:else if filteredArtists.length === 0}
    <div class="empty">
      <Search size={48} class="empty-icon" />
      <p>No results for "{searchQuery}"</p>
    </div>
  {:else}
    <div class="list">
      {#each filteredArtists as artist (artist.artist_id)}
        <div class="list-item">
          <div
            class="artist-info"
            role="button"
            tabindex="0"
            onclick={() => onArtistSelect?.(artist.artist_id)}
            onkeydown={(e) => e.key === 'Enter' && onArtistSelect?.(artist.artist_id)}
          >
            <div class="artist-avatar">
              <Ban size={20} />
            </div>
            <div class="artist-details">
              <span class="artist-name">{artist.artist_name}</span>
              <span class="artist-meta">Added {formatDate(artist.added_at)}</span>
              {#if artist.notes}
                <span class="artist-notes">{artist.notes}</span>
              {/if}
            </div>
          </div>
          <button
            class="remove-btn"
            onclick={() => handleRemove(artist.artist_id)}
            title="Remove from blacklist"
          >
            <X size={18} />
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>
</ViewTransition>

<!-- Confirm Clear Modal -->
{#if confirmClearOpen}
  <div
    class="modal-overlay"
    role="button"
    tabindex="0"
    onclick={() => confirmClearOpen = false}
    onkeydown={(e) => e.key === 'Escape' && (confirmClearOpen = false)}
  >
    <div
      class="modal-content"
      role="dialog"
      aria-modal="true"
      onclick={(e) => e.stopPropagation()}
    >
      <h2 class="modal-title">Clear Blacklist?</h2>
      <p class="modal-text">
        This will remove all {artists.length} artists from your blacklist. This action cannot be undone.
      </p>
      <div class="modal-actions">
        <button class="modal-btn cancel" onclick={() => confirmClearOpen = false}>
          Cancel
        </button>
        <button class="modal-btn danger" onclick={handleClearAll}>
          Clear All
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .blacklist-manager {
    padding: 24px;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    height: 100%;
    overflow-y: auto;
  }

  /* Custom scrollbar */
  .blacklist-manager::-webkit-scrollbar {
    width: 6px;
  }

  .blacklist-manager::-webkit-scrollbar-track {
    background: transparent;
  }

  .blacklist-manager::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .blacklist-manager::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 12px;
  }

  .title-section {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .title-section :global(.title-icon) {
    color: var(--text-muted);
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

  .description {
    color: var(--text-muted);
    font-size: 14px;
    margin: 0 0 20px 0;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
    flex-wrap: wrap;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .toggle-btn:hover {
    background: var(--bg-hover);
  }

  .toggle-btn.enabled {
    color: var(--accent-primary);
  }

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

  .clear-all-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: #ef4444;
    font-size: 13px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .clear-all-btn:hover {
    background: rgba(239, 68, 68, 0.15);
  }

  .count {
    font-size: 13px;
    color: var(--text-muted);
    margin-left: auto;
  }

  .warning-banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    background: rgba(251, 191, 36, 0.1);
    border: 1px solid rgba(251, 191, 36, 0.3);
    border-radius: 8px;
    color: #fbbf24;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .loading,
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px;
    color: var(--text-muted);
  }

  .empty :global(.empty-icon) {
    opacity: 0.3;
    margin-bottom: 16px;
  }

  .empty p {
    margin: 0 0 8px 0;
    font-size: 16px;
  }

  .empty-hint {
    font-size: 13px;
    opacity: 0.7;
    text-align: center;
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
    margin-bottom: 16px;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .list-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: var(--bg-secondary);
    border-radius: 8px;
    transition: background-color 150ms ease;
  }

  .list-item:hover {
    background: var(--bg-tertiary);
  }

  .artist-info {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
    min-width: 0;
  }

  .artist-avatar {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .artist-details {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .artist-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .artist-meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  .artist-notes {
    font-size: 12px;
    color: var(--text-muted);
    font-style: italic;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .remove-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .remove-btn:hover {
    background: rgba(239, 68, 68, 0.15);
    color: #ef4444;
  }

  /* Modal */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
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
    margin: 0 0 12px 0;
  }

  .modal-text {
    font-size: 14px;
    color: var(--text-secondary);
    margin: 0 0 20px 0;
    line-height: 1.5;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
  }

  .modal-btn {
    padding: 10px 20px;
    border: none;
    border-radius: 6px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .modal-btn.cancel {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .modal-btn.cancel:hover {
    background: var(--bg-hover);
  }

  .modal-btn.danger {
    background: #ef4444;
    color: white;
  }

  .modal-btn.danger:hover {
    background: #dc2626;
  }
</style>
