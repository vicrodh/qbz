<script lang="ts">
  import { X, Search, Heart, MoreVertical, Trash2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { invoke } from '@tauri-apps/api/core';

  interface QueueTrack {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    duration: string;
    available?: boolean;
    trackId?: number; // For favorite checking
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    currentTrack?: QueueTrack;
    upcomingTracks: QueueTrack[];
    historyTracks?: QueueTrack[];
    onPlayTrack?: (trackId: string) => void;
    onPlayHistoryTrack?: (trackId: string) => void;
    onClearQueue?: () => void;
    onSaveAsPlaylist?: () => void;
    onReorderTrack?: (fromIndex: number, toIndex: number) => void;
    onToggleInfinitePlay?: () => void;
    infinitePlayEnabled?: boolean;
    isPlaying?: boolean;
  }

  let {
    isOpen,
    onClose,
    currentTrack,
    upcomingTracks,
    historyTracks = [],
    onPlayTrack,
    onPlayHistoryTrack,
    onClearQueue,
    onSaveAsPlaylist,
    onReorderTrack,
    onToggleInfinitePlay,
    infinitePlayEnabled = false,
    isPlaying = false
  }: Props = $props();

  // Tab state
  let activeTab = $state<'queue' | 'history'>('queue');

  // Search state
  let searchOpen = $state(false);
  let searchQuery = $state('');

  // Favorite state for current track
  let currentTrackFavorite = $state(false);

  // Hover state for history tracks
  let hoveredHistoryTrack = $state<string | null>(null);

  // Drag state for queue
  let draggedIndex = $state<number | null>(null);
  let dragOverIndex = $state<number | null>(null);

  // Display limit
  const DISPLAY_LIMIT = 20;
  let displayCount = $state(DISPLAY_LIMIT);

  // Reset state when panel closes
  $effect(() => {
    if (!isOpen) {
      activeTab = 'queue';
      searchOpen = false;
      searchQuery = '';
      displayCount = DISPLAY_LIMIT;
    }
  });

  // Check favorite status when current track changes
  $effect(() => {
    if (currentTrack?.trackId) {
      checkFavoriteStatus(currentTrack.trackId);
    } else {
      currentTrackFavorite = false;
    }
  });

  async function checkFavoriteStatus(trackId: number) {
    try {
      const result = await invoke<boolean>('is_track_favorite', { trackId });
      currentTrackFavorite = result;
    } catch {
      currentTrackFavorite = false;
    }
  }

  async function toggleCurrentTrackFavorite() {
    if (!currentTrack?.trackId) return;
    try {
      if (currentTrackFavorite) {
        await invoke('remove_track_from_favorites', { trackId: currentTrack.trackId });
        currentTrackFavorite = false;
      } else {
        await invoke('add_track_to_favorites', { trackId: currentTrack.trackId });
        currentTrackFavorite = true;
      }
    } catch (err) {
      console.error('Failed to toggle favorite:', err);
    }
  }

  // Filter tracks based on search
  const filteredTracks = $derived.by(() => {
    if (!searchQuery.trim()) return upcomingTracks.slice(0, displayCount);
    const query = searchQuery.toLowerCase();
    return upcomingTracks
      .filter(track =>
        track.title.toLowerCase().includes(query) ||
        track.artist.toLowerCase().includes(query)
      )
      .slice(0, displayCount);
  });

  const totalTracks = $derived(upcomingTracks.length);
  const displayedTracks = $derived(filteredTracks.length);
  const hasMoreTracks = $derived(!searchQuery && upcomingTracks.length > displayCount);
  const canDrag = $derived(!searchQuery.trim());

  function loadMore() {
    displayCount += DISPLAY_LIMIT;
  }

  // Drag handlers
  function handleDragStart(e: DragEvent, index: number) {
    if (!canDrag) {
      e.preventDefault();
      return;
    }
    draggedIndex = index;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', String(index));
    }
  }

  function handleDragOver(e: DragEvent, index: number) {
    if (!canDrag || draggedIndex === null) return;
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = 'move';
    }
    dragOverIndex = index;
  }

  function handleDragLeave() {
    dragOverIndex = null;
  }

  function handleDrop(e: DragEvent, toIndex: number) {
    e.preventDefault();
    if (!canDrag || draggedIndex === null || draggedIndex === toIndex) {
      draggedIndex = null;
      dragOverIndex = null;
      return;
    }
    onReorderTrack?.(draggedIndex, toIndex);
    draggedIndex = null;
    dragOverIndex = null;
  }

  function handleDragEnd() {
    draggedIndex = null;
    dragOverIndex = null;
  }

  function handleTrackClick(track: QueueTrack) {
    if (track.available === false) return;
    onPlayTrack?.(track.id);
  }

  function handleHistoryTrackClick(track: QueueTrack) {
    onPlayHistoryTrack?.(track.id);
  }

  function handleClose() {
    onClose();
  }
</script>

{#if isOpen}
  <!-- Backdrop -->
  <div class="backdrop" onclick={handleClose} role="presentation"></div>

  <!-- Queue Panel -->
  <div class="queue-panel">
    <!-- Header with Tabs -->
    <div class="header">
      <div class="tabs">
        <button
          class="tab"
          class:active={activeTab === 'queue'}
          onclick={() => activeTab = 'queue'}
        >
          {$t('player.queue')}
        </button>
        <span class="tab-separator">|</span>
        <button
          class="tab"
          class:active={activeTab === 'history'}
          onclick={() => activeTab = 'history'}
        >
          {$t('player.history')}
        </button>
      </div>
      <button class="close-btn" onclick={handleClose}>
        <X size={20} />
      </button>
    </div>

    <!-- Content -->
    <div class="content">
      {#if activeTab === 'queue'}
        <!-- Now Playing Card -->
        {#if currentTrack}
          <div class="section">
            <div class="section-label">{$t('player.nowPlaying').toUpperCase()}</div>
            <div class="now-playing-card">
              <div class="np-artwork">
                <img src={currentTrack.artwork} alt={currentTrack.title} />
                {#if isPlaying}
                  <div class="spectrum-bars">
                    <div class="bar"></div>
                    <div class="bar"></div>
                    <div class="bar"></div>
                    <div class="bar"></div>
                  </div>
                {/if}
              </div>
              <div class="np-info">
                <div class="np-title">{currentTrack.title}</div>
                <div class="np-artist">{currentTrack.artist}</div>
              </div>
              <button
                class="np-favorite"
                class:active={currentTrackFavorite}
                onclick={toggleCurrentTrackFavorite}
                title={currentTrackFavorite ? $t('actions.removeFromFavorites') : $t('actions.addToFavorites')}
              >
                <Heart size={18} fill={currentTrackFavorite ? 'currentColor' : 'none'} />
              </button>
            </div>
          </div>
        {/if}

        <!-- Up Next Section -->
        {#if upcomingTracks.length > 0}
          <div class="section up-next-section">
            <div class="section-label">
              {$t('player.upNext').toUpperCase()} ({displayedTracks}/{totalTracks})
            </div>
            <div class="tracks-list">
              {#each filteredTracks as track, index}
                {@const originalIndex = upcomingTracks.findIndex(t => t.id === track.id)}
                {@const isUnavailable = track.available === false}
                <div
                  class="queue-track"
                  class:dragging={draggedIndex === originalIndex}
                  class:drag-over={dragOverIndex === originalIndex && draggedIndex !== originalIndex}
                  class:unavailable={isUnavailable}
                  draggable={canDrag && !isUnavailable}
                  onclick={() => handleTrackClick(track)}
                  ondragstart={(e) => handleDragStart(e, originalIndex)}
                  ondragover={(e) => handleDragOver(e, originalIndex)}
                  ondragleave={handleDragLeave}
                  ondrop={(e) => handleDrop(e, originalIndex)}
                  ondragend={handleDragEnd}
                  role="button"
                  tabindex={isUnavailable ? -1 : 0}
                >
                  <span class="track-number">{originalIndex + 1}</span>
                  <div class="track-info">
                    <div class="track-title">{track.title}</div>
                    <div class="track-artist">{track.artist}</div>
                  </div>
                  <span class="track-duration">{track.duration}</span>
                </div>
              {/each}
              {#if hasMoreTracks}
                <button class="load-more" onclick={loadMore}>
                  {$t('actions.loadMore')} ({upcomingTracks.length - displayCount} more)
                </button>
              {/if}
              {#if searchQuery && filteredTracks.length === 0}
                <div class="no-results">{$t('player.noTracksMatch', { values: { query: searchQuery } })}</div>
              {/if}
            </div>
          </div>
        {:else if !currentTrack}
          <div class="empty-state">
            <div class="emoji">🎵</div>
            <div class="empty-title">{$t('player.queueEmpty')}</div>
            <div class="empty-text">{$t('player.queueEmptyDescription')}</div>
          </div>
        {/if}

      {:else}
        <!-- History Tab -->
        <div class="section">
          <div class="section-label">{$t('player.recentlyPlayed').toUpperCase()}</div>
          {#if historyTracks.length > 0}
            <div class="history-list">
              {#each historyTracks.slice(0, DISPLAY_LIMIT) as track}
                <div
                  class="history-track"
                  onclick={() => handleHistoryTrackClick(track)}
                  onmouseenter={() => hoveredHistoryTrack = track.id}
                  onmouseleave={() => hoveredHistoryTrack = null}
                  role="button"
                  tabindex="0"
                >
                  <img src={track.artwork} alt={track.title} class="history-artwork" />
                  <div class="track-info">
                    <div class="track-title">{track.title}</div>
                    <div class="track-artist">{track.artist}</div>
                  </div>
                  <span class="track-duration">{track.duration}</span>
                  {#if hoveredHistoryTrack === track.id}
                    <button class="track-menu" onclick={(e) => e.stopPropagation()}>
                      <MoreVertical size={16} />
                    </button>
                  {/if}
                </div>
              {/each}
            </div>
          {:else}
            <div class="empty-state">
              <div class="empty-title">{$t('player.noHistoryYet')}</div>
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Footer (Queue tab only) -->
    {#if activeTab === 'queue' && (upcomingTracks.length > 0 || currentTrack)}
      <div class="footer">
        <div class="footer-left">
          <button
            class="footer-icon-btn"
            onclick={onClearQueue}
            title={$t('player.clearQueue')}
          >
            <img src="/trash-list.svg" alt="" class="footer-icon" />
          </button>
          <button
            class="footer-icon-btn"
            onclick={onSaveAsPlaylist}
            title={$t('player.saveQueue')}
          >
            <img src="/add-to-list.svg" alt="" class="footer-icon" />
          </button>
          <button
            class="footer-icon-btn"
            class:active={infinitePlayEnabled}
            onclick={onToggleInfinitePlay}
            title={$t('player.infinitePlay')}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="infinite-icon">
              <path d="M18.178 8c5.096 0 5.096 8 0 8-5.095 0-7.133-8-12.739-8-4.781 0-4.781 8 0 8 5.606 0 7.644-8 12.739-8z"/>
            </svg>
          </button>
        </div>
        <div class="footer-right">
          {#if searchOpen}
            <div class="search-bar">
              <Search size={14} />
              <input
                type="text"
                placeholder={$t('player.searchQueue')}
                bind:value={searchQuery}
                class="search-input"
              />
              <button class="search-close" onclick={() => { searchOpen = false; searchQuery = ''; }}>
                <X size={14} />
              </button>
            </div>
          {:else}
            <button
              class="footer-icon-btn"
              onclick={() => searchOpen = true}
              title={$t('player.searchQueue')}
            >
              <Search size={18} />
            </button>
          {/if}
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background-color: rgba(0, 0, 0, 0.4);
    z-index: 40;
  }

  .queue-panel {
    position: fixed;
    top: 32px;
    right: 0;
    bottom: 104px;
    width: 340px;
    z-index: 50;
    display: flex;
    flex-direction: column;
    animation: slideIn 200ms ease-out;
    background-color: var(--bg-secondary);
    border-left: 1px solid var(--bg-tertiary);
    box-shadow: -4px 0 24px rgba(0, 0, 0, 0.3);
  }

  @keyframes slideIn {
    from { transform: translateX(100%); }
    to { transform: translateX(0); }
  }

  /* Header */
  .header {
    padding: 12px 16px;
    border-bottom: 1px solid var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .tab {
    background: none;
    border: none;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0;
    transition: color 150ms ease;
  }

  .tab:hover {
    color: var(--text-secondary);
  }

  .tab.active {
    color: var(--text-primary);
  }

  .tab-separator {
    color: var(--text-disabled);
    font-size: 14px;
  }

  .close-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  /* Content */
  .content {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 12px 16px;
    min-height: 0;
    overscroll-behavior: contain;
  }

  .content::-webkit-scrollbar {
    width: 4px;
  }

  .content::-webkit-scrollbar-track {
    background: transparent;
  }

  .content::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.15);
    border-radius: 2px;
  }

  .content:hover::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.25);
  }

  /* Sections */
  .section {
    margin-bottom: 16px;
  }

  .section-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    letter-spacing: 0.05em;
    margin-bottom: 10px;
  }

  /* Now Playing Card */
  .now-playing-card {
    background: var(--bg-tertiary);
    border-radius: 8px;
    padding: 10px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .np-artwork {
    position: relative;
    width: 48px;
    height: 48px;
    flex-shrink: 0;
  }

  .np-artwork img {
    width: 100%;
    height: 100%;
    border-radius: 4px;
    object-fit: cover;
  }

  .spectrum-bars {
    position: absolute;
    bottom: 4px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: flex-end;
    gap: 2px;
    height: 16px;
  }

  .spectrum-bars .bar {
    width: 3px;
    background-color: #4285F4;
    border-radius: 1px;
    animation: spectrum 1s ease-in-out infinite;
  }

  .spectrum-bars .bar:nth-child(1) {
    height: 8px;
    animation-delay: 0s;
  }

  .spectrum-bars .bar:nth-child(2) {
    height: 12px;
    animation-delay: 0.15s;
  }

  .spectrum-bars .bar:nth-child(3) {
    height: 6px;
    animation-delay: 0.3s;
  }

  .spectrum-bars .bar:nth-child(4) {
    height: 10px;
    animation-delay: 0.45s;
  }

  @keyframes spectrum {
    0%, 100% {
      transform: scaleY(0.5);
      opacity: 0.7;
    }
    50% {
      transform: scaleY(1);
      opacity: 1;
    }
  }

  .np-info {
    flex: 1;
    min-width: 0;
  }

  .np-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .np-artist {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .np-favorite {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .np-favorite:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .np-favorite.active {
    color: #ef4444;
  }

  /* Queue Tracks */
  .tracks-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .queue-track {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 6px;
    border-radius: 6px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .queue-track:hover {
    background: var(--bg-tertiary);
  }

  .queue-track.dragging {
    opacity: 0.5;
  }

  .queue-track.drag-over {
    border-top: 2px solid var(--accent-primary);
    margin-top: -2px;
  }

  .queue-track.unavailable {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .track-number {
    width: 24px;
    font-size: 13px;
    color: var(--text-muted);
    text-align: center;
    flex-shrink: 0;
  }

  .track-info {
    flex: 1;
    min-width: 0;
  }

  .track-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-artist {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-duration {
    font-size: 12px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .load-more {
    padding: 12px;
    text-align: center;
    background: none;
    border: none;
    color: var(--accent-primary);
    font-size: 12px;
    cursor: pointer;
    width: 100%;
    border-radius: 6px;
    transition: background 150ms ease;
  }

  .load-more:hover {
    background: var(--bg-tertiary);
  }

  /* History */
  .history-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .history-track {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px;
    border-radius: 6px;
    cursor: pointer;
    transition: background-color 150ms ease;
    position: relative;
  }

  .history-track:hover {
    background: var(--bg-tertiary);
  }

  .history-artwork {
    width: 40px;
    height: 40px;
    border-radius: 4px;
    object-fit: cover;
    flex-shrink: 0;
  }

  .track-menu {
    position: absolute;
    right: 6px;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-secondary);
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
  }

  .track-menu:hover {
    color: var(--text-primary);
  }

  /* Empty State */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 0;
    text-align: center;
  }

  .emoji {
    font-size: 32px;
    margin-bottom: 12px;
  }

  .empty-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .empty-text {
    font-size: 13px;
    color: var(--text-muted);
    max-width: 200px;
  }

  .no-results {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 12px;
  }

  /* Footer */
  .footer {
    padding: 12px 16px;
    border-top: 1px solid var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
  }

  .footer-left,
  .footer-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .footer-icon-btn {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    transition: all 150ms ease;
  }

  .footer-icon-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .footer-icon {
    width: 18px;
    height: 18px;
    filter: brightness(0) saturate(100%) invert(50%) sepia(0%) saturate(0%) hue-rotate(0deg) brightness(100%) contrast(100%);
    transition: filter 150ms ease;
  }

  .footer-icon-btn:hover .footer-icon {
    filter: brightness(0) saturate(100%) invert(100%);
  }

  .footer-icon-btn.active {
    color: var(--accent-primary, #6366f1);
  }

  .footer-icon-btn.active .infinite-icon {
    stroke: var(--accent-primary, #6366f1);
  }

  /* Search Bar */
  .search-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    width: 180px;
    animation: expandSearch 150ms ease-out;
  }

  @keyframes expandSearch {
    from {
      width: 32px;
      opacity: 0;
    }
    to {
      width: 180px;
      opacity: 1;
    }
  }

  .search-bar :global(svg) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 12px;
    outline: none;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-close {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .search-close:hover {
    color: var(--text-primary);
  }
</style>
