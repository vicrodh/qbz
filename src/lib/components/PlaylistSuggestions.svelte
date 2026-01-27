<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { RefreshCw, Plus, X, Info, Sparkles } from 'lucide-svelte';
  import {
    type SuggestedTrack,
    type SuggestionResult,
    type PlaylistArtist,
    getPlaylistSuggestionsV2,
    getDismissedTrackIds,
    dismissTrack,
    formatDuration
  } from '$lib/services/playlistSuggestionsService';

  interface Props {
    playlistId: number;
    artists: PlaylistArtist[];
    excludeTrackIds: number[];
    onAddTrack?: (trackId: number) => Promise<void>;
    showReasons?: boolean;
  }

  let {
    playlistId,
    artists,
    excludeTrackIds,
    onAddTrack,
    showReasons = false
  }: Props = $props();

  // State
  let loading = $state(false);
  let loadingMore = $state(false);
  let error = $state<string | null>(null);
  let pool = $state<SuggestedTrack[]>([]);
  let currentPage = $state(0);
  let result = $state<SuggestionResult | null>(null);
  let hasLoadedOnce = $state(false);

  // Configuration
  const VISIBLE_COUNT = 6;
  const INITIAL_POOL = 18;  // 3 pages worth
  const EXPANDED_POOL = 60; // Full pool on demand

  // Derived state
  const dismissedIds = $derived(getDismissedTrackIds(playlistId));
  const filteredPool = $derived(
    pool.filter(t => !dismissedIds.has(t.track_id) && !excludeTrackIds.includes(t.track_id))
  );
  const totalPages = $derived(Math.ceil(filteredPool.length / VISIBLE_COUNT));
  const visibleTracks = $derived(
    filteredPool.slice(currentPage * VISIBLE_COUNT, (currentPage + 1) * VISIBLE_COUNT)
  );
  const hasMorePages = $derived(currentPage < totalPages - 1);
  const canLoadMore = $derived(hasLoadedOnce && pool.length < EXPANDED_POOL && !loadingMore);
  const isEmpty = $derived(filteredPool.length === 0 && !loading && hasLoadedOnce);

  // Track the last playlist we loaded for
  let lastLoadedPlaylistId = $state<number | null>(null);

  // Helper for timestamped logs
  function log(...args: unknown[]) {
    const ts = new Date().toISOString().slice(11, 23);
    console.log(`[${ts}] [Suggestions]`, ...args);
  }

  // Load suggestions when playlist/artists change
  $effect(() => {
    const artistCount = artists.length;
    const currentPlaylistId = playlistId;

    // Only load if we have artists and haven't loaded for this playlist yet
    if (artistCount > 0 && currentPlaylistId !== lastLoadedPlaylistId && !loading) {
      log('Effect triggered, playlist:', currentPlaylistId, 'artists:', artistCount);
      lastLoadedPlaylistId = currentPlaylistId;
      hasLoadedOnce = false;
      pool = [];
      void loadSuggestions(false);
    }
  });

  async function loadSuggestions(expanded: boolean) {
    if (loading) {
      log('Already loading, skipping');
      return;
    }

    const poolSize = expanded ? EXPANDED_POOL : INITIAL_POOL;
    log(`Starting load (expanded=${expanded}, poolSize=${poolSize}, artists=${artists.length})`);
    const startTime = performance.now();

    loading = true;
    error = null;

    try {
      log('Calling backend...');
      result = await getPlaylistSuggestionsV2(
        artists,
        excludeTrackIds,
        showReasons,
        { max_pool_size: poolSize, skip_vector_build: true }
      );
      const elapsed = ((performance.now() - startTime) / 1000).toFixed(2);
      log(`Backend returned in ${elapsed}s:`, {
        tracks: result.tracks.length,
        sourceArtists: result.source_artists.length,
        playlistArtists: result.playlist_artists_count,
        similarArtists: result.similar_artists_count
      });
      pool = result.tracks;
      currentPage = 0;
      hasLoadedOnce = true;
    } catch (err) {
      const elapsed = ((performance.now() - startTime) / 1000).toFixed(2);
      log(`Failed after ${elapsed}s:`, err);
      error = err instanceof Error ? err.message : 'Failed to load suggestions';
      pool = [];
    } finally {
      loading = false;
    }
  }

  async function handleLoadMore() {
    if (loadingMore || pool.length >= EXPANDED_POOL) return;

    loadingMore = true;
    try {
      const moreResult = await getPlaylistSuggestionsV2(
        artists,
        excludeTrackIds,
        showReasons,
        { max_pool_size: EXPANDED_POOL }
      );
      // Merge new tracks, avoiding duplicates
      const existingIds = new Set(pool.map(t => t.track_id));
      const newTracks = moreResult.tracks.filter(t => !existingIds.has(t.track_id));
      pool = [...pool, ...newTracks];
      result = moreResult;
    } catch (err) {
      console.error('Failed to load more suggestions:', err);
    } finally {
      loadingMore = false;
    }
  }

  function handleRefresh() {
    if (hasMorePages) {
      // Rotate to next page
      currentPage = (currentPage + 1) % totalPages;
    } else if (canLoadMore) {
      // Load more tracks
      void handleLoadMore();
    } else {
      // Reload from scratch
      hasLoadedOnce = false;
      void loadSuggestions(false);
    }
  }

  async function handleAddTrack(track: SuggestedTrack) {
    if (!onAddTrack) return;

    try {
      await onAddTrack(track.track_id);
      // Remove from pool locally (will be excluded on next load anyway)
      pool = pool.filter(t => t.track_id !== track.track_id);
    } catch (err) {
      console.error('Failed to add track:', err);
    }
  }

  function handleDismiss(track: SuggestedTrack) {
    dismissTrack(playlistId, track.track_id);
    // Force reactivity by reassigning pool
    pool = [...pool];
  }

  function getAlbumArt(albumId: string): string {
    if (!albumId) return '';
    // Qobuz album art URL pattern
    return `https://static.qobuz.com/images/covers/${albumId}_50.jpg`;
  }
</script>

{#if artists.length > 0 && !isEmpty}
  <div class="suggestions-section" id="suggestions-anchor">
    <div class="suggestions-header">
      <div class="header-left">
        <Sparkles size={18} class="sparkle-icon" />
        <h3>Suggested songs</h3>
        {#if result && !loading}
          <span class="stats">
            Based on {result.playlist_artists_count} artists
          </span>
        {/if}
      </div>
      <button
        class="refresh-btn"
        class:spinning={loading || loadingMore}
        onclick={handleRefresh}
        disabled={loading || loadingMore}
        title={hasMorePages ? 'Show more' : canLoadMore ? 'Load more suggestions' : 'Refresh suggestions'}
      >
        <RefreshCw size={16} />
      </button>
    </div>

    {#if loading && pool.length === 0}
      <div class="loading-state">
        <div class="spinner"></div>
        <p>Finding similar artists...</p>
      </div>
    {:else if error}
      <div class="error-state">
        <p>{error}</p>
        <button onclick={() => loadSuggestions(false)}>Retry</button>
      </div>
    {:else}
      <div class="suggestions-list">
        {#each visibleTracks as track (track.track_id)}
          <div class="suggestion-row">
            <div class="album-art">
              {#if track.album_id}
                <img
                  src={getAlbumArt(track.album_id)}
                  alt=""
                  loading="lazy"
                  onerror={(e) => {
                    const target = e.currentTarget as HTMLImageElement;
                    target.style.display = 'none';
                  }}
                />
              {/if}
            </div>

            <div class="track-info">
              <div class="track-title">{track.title}</div>
              <div class="track-meta">
                <span class="artist">{track.artist_name}</span>
                {#if track.album_title}
                  <span class="separator">·</span>
                  <span class="album">{track.album_title}</span>
                {/if}
              </div>
            </div>

            <div class="track-duration">
              {formatDuration(track.duration)}
            </div>

            {#if showReasons && track.reason}
              <button
                class="reason-btn"
                title={track.reason}
              >
                <Info size={14} />
              </button>
            {/if}

            <div class="actions">
              <button
                class="action-btn add"
                onclick={() => handleAddTrack(track)}
                title="Add to playlist"
              >
                <Plus size={16} />
              </button>
              <button
                class="action-btn dismiss"
                onclick={() => handleDismiss(track)}
                title="Not interested"
              >
                <X size={16} />
              </button>
            </div>
          </div>
        {/each}
      </div>

      {#if filteredPool.length > VISIBLE_COUNT || canLoadMore}
        <div class="pagination-info">
          {#if loadingMore}
            Loading more...
          {:else}
            Showing {currentPage * VISIBLE_COUNT + 1}-{Math.min((currentPage + 1) * VISIBLE_COUNT, filteredPool.length)} of {filteredPool.length}
            {#if canLoadMore}
              <button class="load-more-link" onclick={handleLoadMore}>Load more</button>
            {/if}
          {/if}
        </div>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .suggestions-section {
    margin-top: 32px;
    padding-top: 24px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .suggestions-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .header-left h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .header-left :global(.sparkle-icon) {
    color: var(--accent-primary);
  }

  .stats {
    font-size: 12px;
    color: var(--text-muted);
    margin-left: 8px;
  }

  .refresh-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .refresh-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .refresh-btn:disabled {
    cursor: not-allowed;
  }

  .refresh-btn.spinning {
    background: transparent;
    color: var(--text-muted);
  }

  .refresh-btn.spinning :global(svg) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .loading-state,
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 32px;
    text-align: center;
    color: var(--text-muted);
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
    margin-bottom: 12px;
  }

  .error-state button {
    margin-top: 12px;
    padding: 6px 16px;
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
  }

  .suggestions-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .suggestion-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 8px;
    transition: background-color 150ms ease;
  }

  .suggestion-row:hover {
    background-color: var(--bg-hover);
  }

  .album-art {
    width: 40px;
    height: 40px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .album-art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .track-info {
    flex: 1;
    min-width: 0;
  }

  .track-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-meta {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .separator {
    margin: 0 4px;
  }

  .track-duration {
    font-size: 13px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    width: 48px;
    text-align: right;
    flex-shrink: 0;
  }

  .reason-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: help;
    border-radius: 4px;
    opacity: 0.5;
    transition: opacity 150ms ease;
  }

  .suggestion-row:hover .reason-btn {
    opacity: 1;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .suggestion-row:hover .actions {
    opacity: 1;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .action-btn.add {
    color: var(--accent-primary);
  }

  .action-btn.add:hover {
    background: var(--accent-primary);
    color: white;
  }

  .action-btn.dismiss {
    color: var(--text-muted);
  }

  .action-btn.dismiss:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .pagination-info {
    margin-top: 12px;
    text-align: center;
    font-size: 12px;
    color: var(--text-muted);
  }

  .load-more-link {
    background: none;
    border: none;
    color: var(--accent-primary);
    cursor: pointer;
    font-size: 12px;
    margin-left: 8px;
    padding: 0;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .load-more-link:hover {
    color: var(--text-primary);
  }
</style>
