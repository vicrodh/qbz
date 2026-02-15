<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { ChevronLeft, Search, LayoutGrid, List, X, ListMusic, Play, Plus } from 'lucide-svelte';
  import QobuzPlaylistCard from '../QobuzPlaylistCard.svelte';
  import GenreFilterButton from '../GenreFilterButton.svelte';
  import {
    getSelectedGenreIds,
    type GenreFilterContext
  } from '$lib/stores/genreFilterStore';
  import type {
    DiscoverPlaylist,
    DiscoverPlaylistsResponse,
    PlaylistTag
  } from '$lib/types';

  interface Props {
    onBack: () => void;
    onPlaylistClick?: (playlistId: number) => void;
    onPlaylistPlay?: (playlistId: number) => void;
    onPlaylistPlayNext?: (playlistId: number) => void;
    onPlaylistPlayLater?: (playlistId: number) => void;
    onPlaylistCopyToLibrary?: (playlistId: number) => void;
    onPlaylistShareQobuz?: (playlistId: number) => void;
  }

  let {
    onBack,
    onPlaylistClick,
    onPlaylistPlay,
    onPlaylistPlayNext,
    onPlaylistPlayLater,
    onPlaylistCopyToLibrary,
    onPlaylistShareQobuz,
  }: Props = $props();

  const PAGE_SIZE = 50;
  const genreContext: GenreFilterContext = 'discover-playlists';

  // State
  let playlists = $state<DiscoverPlaylist[]>([]);
  let tags = $state<PlaylistTag[]>([]);
  let selectedTagSlug = $state<string | null>(null);
  let hasMore = $state(true);
  let offset = $state(0);
  let isLoading = $state(false);
  let isLoadingMore = $state(false);
  let searchQuery = $state('');
  let viewMode = $state<'grid' | 'list'>('grid');

  // Client-side search filter
  function getFilteredPlaylists(): DiscoverPlaylist[] {
    if (!searchQuery.trim()) return playlists;
    const q = searchQuery.toLowerCase();
    return playlists.filter(
      (pl) =>
        pl.name.toLowerCase().includes(q) ||
        (pl.owner?.name || '').toLowerCase().includes(q) ||
        (pl.description || '').toLowerCase().includes(q)
    );
  }

  function getGenreIdsForFetch(): number[] | undefined {
    const ids = getSelectedGenreIds(genreContext);
    return ids.size > 0 ? Array.from(ids) : undefined;
  }

  function getFirstTagName(pl: DiscoverPlaylist): string | undefined {
    return pl.tags?.[0]?.name;
  }

  async function fetchPlaylists(resetData: boolean = false) {
    if (resetData) {
      offset = 0;
      playlists = [];
      hasMore = true;
      isLoading = true;
    } else {
      isLoadingMore = true;
    }

    try {
      const genreIds = getGenreIdsForFetch();
      const response = await invoke<DiscoverPlaylistsResponse>('v2_get_discover_playlists', {
        tag: selectedTagSlug,
        genreIds,
        offset,
        limit: PAGE_SIZE,
      });

      const newPlaylists = response.items;

      if (resetData) {
        playlists = newPlaylists;
      } else {
        playlists = [...playlists, ...newPlaylists];
      }

      hasMore = response.has_more;
      offset += newPlaylists.length;
    } catch (err) {
      console.error('Failed to fetch discover playlists:', err);
    } finally {
      isLoading = false;
      isLoadingMore = false;
    }
  }

  async function fetchTags() {
    try {
      const result = await invoke<PlaylistTag[]>('get_playlist_tags');
      tags = result;
    } catch (err) {
      console.error('Failed to fetch playlist tags:', err);
    }
  }

  function handleLoadMore() {
    if (hasMore && !isLoadingMore && !isLoading) {
      fetchPlaylists(false);
    }
  }

  function handleGenreFilterChange() {
    fetchPlaylists(true);
  }

  function handleTagSelect(slug: string | null) {
    selectedTagSlug = slug;
    fetchPlaylists(true);
  }

  function clearSearch() {
    searchQuery = '';
  }

  function formatDuration(seconds: number): string {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    if (hours > 0) {
      return `${hours}h ${minutes}m`;
    }
    return `${minutes}m`;
  }

  // Scroll handling for infinite scroll
  let scrollContainer: HTMLDivElement | undefined;

  function handleScroll() {
    if (!scrollContainer || isLoadingMore || isLoading || !hasMore || searchQuery.trim()) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollContainer;
    if (scrollTop + clientHeight >= scrollHeight - 300) {
      handleLoadMore();
    }
  }

  onMount(() => {
    fetchTags();
    fetchPlaylists(true);
  });
</script>

<div class="discover-playlists">
  <!-- Top bar -->
  <div class="top-bar">
    <div class="top-bar-left">
      <button class="back-btn" onclick={onBack}>
        <ChevronLeft size={20} />
      </button>
      <h1 class="page-title">{$t('discover.qobuzPlaylists')}</h1>
    </div>
    <div class="top-bar-right">
      <div class="search-wrapper">
        <Search size={16} />
        <input
          type="text"
          class="search-input"
          placeholder={$t('discover.searchPlaylistsPlaceholder')}
          bind:value={searchQuery}
        />
        {#if searchQuery}
          <button class="clear-btn" onclick={clearSearch}>
            <X size={14} />
          </button>
        {/if}
      </div>
      <GenreFilterButton context={genreContext} variant="control" align="right" onFilterChange={handleGenreFilterChange} />
      <div class="view-toggle">
        <button
          class="toggle-btn"
          class:active={viewMode === 'grid'}
          onclick={() => viewMode = 'grid'}
        >
          <LayoutGrid size={18} />
        </button>
        <button
          class="toggle-btn"
          class:active={viewMode === 'list'}
          onclick={() => viewMode = 'list'}
        >
          <List size={18} />
        </button>
      </div>
    </div>
  </div>

  <!-- Tag Selector (pill/radio style like Search filter type) -->
  {#if tags.length > 0}
    <div class="tag-bar">
      <div class="tag-pills">
        <label class="tag-pill">
          <input
            type="radio"
            name="playlistTag"
            checked={selectedTagSlug === null}
            onchange={() => handleTagSelect(null)}
          />
          <span>{$t('home.allTags')}</span>
        </label>
        {#each tags as tag (tag.id)}
          <label class="tag-pill">
            <input
              type="radio"
              name="playlistTag"
              checked={selectedTagSlug === tag.slug}
              onchange={() => handleTagSelect(tag.slug)}
            />
            <span>{tag.name}</span>
          </label>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Content -->
  <div class="browse-content" bind:this={scrollContainer} onscroll={handleScroll}>
    {#if isLoading}
      <div class="loading-state">
        <div class="skeleton-grid" class:list-mode={viewMode === 'list'}>
          {#each { length: 12 } as _}
            {#if viewMode === 'grid'}
              <div class="skeleton-card">
                <div class="skeleton-art"></div>
                <div class="skeleton-text"></div>
                <div class="skeleton-text short"></div>
              </div>
            {:else}
              <div class="skeleton-list-row">
                <div class="skeleton-art-small"></div>
                <div class="skeleton-text-group">
                  <div class="skeleton-text"></div>
                  <div class="skeleton-text short"></div>
                </div>
              </div>
            {/if}
          {/each}
        </div>
      </div>
    {:else if getFilteredPlaylists().length === 0}
      <div class="empty-state">
        <p>{$t('discover.noPlaylistResults')}</p>
      </div>
    {:else if viewMode === 'grid'}
      <!-- Grid View -->
      <div class="playlist-grid">
        {#each getFilteredPlaylists() as playlist (playlist.id)}
          <div class="grid-item">
            <QobuzPlaylistCard
              playlistId={playlist.id}
              name={playlist.name}
              owner={playlist.owner?.name || 'Qobuz'}
              image={playlist.image?.rectangle || playlist.image?.covers?.[0]}
              trackCount={playlist.tracks_count}
              duration={playlist.duration}
              genre={playlist.genres?.[0]?.name}
              onclick={onPlaylistClick ? () => onPlaylistClick(playlist.id) : undefined}
              onPlay={onPlaylistPlay ? () => onPlaylistPlay(playlist.id) : undefined}
              onPlayNext={onPlaylistPlayNext ? () => onPlaylistPlayNext(playlist.id) : undefined}
              onPlayLater={onPlaylistPlayLater ? () => onPlaylistPlayLater(playlist.id) : undefined}
              onCopyToLibrary={onPlaylistCopyToLibrary ? () => onPlaylistCopyToLibrary(playlist.id) : undefined}
              onShareQobuz={onPlaylistShareQobuz ? () => onPlaylistShareQobuz(playlist.id) : undefined}
            />
            {#if getFirstTagName(playlist)}
              <div class="tag-label">{getFirstTagName(playlist)}</div>
            {/if}
          </div>
        {/each}
      </div>
    {:else}
      <!-- List View -->
      <div class="playlist-list">
        {#each getFilteredPlaylists() as playlist (playlist.id)}
          <button
            class="list-row"
            onclick={() => onPlaylistClick?.(playlist.id)}
          >
            <div class="list-artwork">
              {#if playlist.image?.rectangle || playlist.image?.covers?.[0]}
                <img
                  src={playlist.image?.rectangle || playlist.image?.covers?.[0]}
                  alt={playlist.name}
                  loading="lazy"
                  decoding="async"
                />
              {:else}
                <div class="list-artwork-placeholder">
                  <ListMusic size={20} />
                </div>
              {/if}
            </div>
            <div class="list-info">
              <div class="list-name">{playlist.name}</div>
              <div class="list-meta">
                <span class="list-owner">{playlist.owner?.name || 'Qobuz'}</span>
                {#if playlist.tracks_count}
                  <span>{playlist.tracks_count} {$t('playlist.tracks')}</span>
                {/if}
                {#if playlist.duration}
                  <span>{formatDuration(playlist.duration)}</span>
                {/if}
              </div>
            </div>
            {#if getFirstTagName(playlist)}
              <div class="list-tag">{getFirstTagName(playlist)}</div>
            {/if}
            <div class="list-actions">
              {#if onPlaylistPlay}
                <button
                  class="list-action-btn"
                  onclick={(e) => { e.stopPropagation(); onPlaylistPlay(playlist.id); }}
                  title={$t('actions.play')}
                >
                  <Play size={16} fill="currentColor" />
                </button>
              {/if}
              {#if onPlaylistCopyToLibrary}
                <button
                  class="list-action-btn"
                  onclick={(e) => { e.stopPropagation(); onPlaylistCopyToLibrary(playlist.id); }}
                  title={$t('playlist.copyToLibrary')}
                >
                  <Plus size={16} />
                </button>
              {/if}
            </div>
          </button>
        {/each}
      </div>
    {/if}

    {#if isLoadingMore}
      <div class="loading-more">
        <div class="spinner"></div>
        <span>{$t('discover.loadingMore')}</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .discover-playlists {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 24px;
    padding-left: 18px;
    padding-right: 8px;
  }

  .top-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 16px;
    flex-shrink: 0;
  }

  .top-bar-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border-radius: 8px;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .back-btn:hover {
    background: var(--bg-tertiary);
  }

  .page-title {
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
    white-space: nowrap;
  }

  .top-bar-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .search-wrapper {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-secondary);
    border-radius: 8px;
    padding: 6px 12px;
    color: var(--text-muted);
    min-width: 180px;
  }

  .search-input {
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 13px;
    outline: none;
    width: 100%;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    border-radius: 4px;
  }

  .clear-btn:hover {
    color: var(--text-primary);
  }

  .view-toggle {
    display: flex;
    background: var(--bg-secondary);
    border-radius: 8px;
    overflow: hidden;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .toggle-btn.active {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .toggle-btn:hover:not(.active) {
    color: var(--text-secondary);
  }

  /* Tag Bar - pill/radio style like SearchView filter type */
  .tag-bar {
    flex-shrink: 0;
    padding-bottom: 16px;
    overflow-x: auto;
  }

  .tag-pills {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .tag-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    font-size: 12px;
    color: var(--text-secondary);
    transition: color 150ms ease;
    white-space: nowrap;
  }

  .tag-pill input[type="radio"] {
    accent-color: var(--accent-primary);
    cursor: pointer;
    margin: 0;
  }

  .tag-pill:has(input:checked) {
    color: var(--text-primary);
  }

  .tag-pill:hover {
    color: var(--text-primary);
  }

  /* Content area */
  .browse-content {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  /* Grid view */
  .playlist-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 24px 14px;
  }

  .grid-item {
    width: 180px;
    position: relative;
  }

  .tag-label {
    font-size: 11px;
    font-weight: 600;
    color: #4caf50;
    text-transform: uppercase;
    letter-spacing: 0.3px;
    margin-top: 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* List view */
  .playlist-list {
    display: flex;
    flex-direction: column;
  }

  .list-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    background: none;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    transition: background 150ms ease;
    text-align: left;
    width: 100%;
    color: inherit;
  }

  .list-row:hover {
    background: var(--bg-hover);
  }

  .list-artwork {
    width: 48px;
    height: 48px;
    border-radius: 6px;
    overflow: hidden;
    flex-shrink: 0;
    background: var(--bg-tertiary);
  }

  .list-artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .list-artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .list-info {
    flex: 1;
    min-width: 0;
  }

  .list-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .list-meta {
    display: flex;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .list-meta span:not(:last-child)::after {
    content: '\2022';
    margin-left: 8px;
    opacity: 0.5;
  }

  .list-owner {
    color: var(--text-secondary);
  }

  .list-tag {
    font-size: 11px;
    font-weight: 600;
    color: #4caf50;
    text-transform: uppercase;
    letter-spacing: 0.3px;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .list-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .list-row:hover .list-actions {
    opacity: 1;
  }

  .list-action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 4px;
    cursor: pointer;
    transition: color 150ms ease, background 150ms ease;
  }

  .list-action-btn:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
  }

  /* Loading states */
  .loading-state {
    padding: 16px 0;
  }

  .skeleton-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 24px 14px;
  }

  .skeleton-grid.list-mode {
    flex-direction: column;
    gap: 4px;
  }

  .skeleton-card {
    width: 180px;
  }

  .skeleton-art {
    width: 180px;
    height: 180px;
    border-radius: 8px;
    background: var(--bg-secondary);
    animation: pulse 1.5s ease-in-out infinite;
    margin-bottom: 8px;
  }

  .skeleton-list-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
  }

  .skeleton-art-small {
    width: 48px;
    height: 48px;
    border-radius: 6px;
    background: var(--bg-secondary);
    animation: pulse 1.5s ease-in-out infinite;
    flex-shrink: 0;
  }

  .skeleton-text-group {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .skeleton-text {
    height: 14px;
    background: var(--bg-secondary);
    border-radius: 4px;
    animation: pulse 1.5s ease-in-out infinite;
    margin-bottom: 6px;
  }

  .skeleton-text.short {
    width: 60%;
  }

  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.7; }
  }

  .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 200px;
    color: var(--text-muted);
    font-size: 14px;
  }

  .loading-more {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
    color: var(--text-muted);
    font-size: 13px;
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
