<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { ArrowLeft, Search, X, Disc3, Loader2 } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import type { QobuzAlbum, LabelDetail } from '$lib/types';
  import type { OfflineCacheStatus } from '$lib/stores/offlineCacheState';

  interface Props {
    labelId: number;
    labelName?: string;
    onBack: () => void;
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    checkAlbumFullyDownloaded?: (albumId: string) => Promise<boolean>;
    downloadStateVersion?: number;
  }

  let {
    labelId,
    labelName = '',
    onBack,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAddAlbumToPlaylist,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    checkAlbumFullyDownloaded,
    downloadStateVersion
  }: Props = $props();

  // State
  let label = $state<LabelDetail | null>(null);
  let albums = $state<QobuzAlbum[]>([]);
  let loading = $state(false);
  let loadingMore = $state(false);
  let error = $state<string | null>(null);
  let searchQuery = $state('');
  let searchExpanded = $state(false);
  let totalAlbums = $state(0);
  let albumsFetched = $state(0);

  // Download status tracking
  let albumOfflineCacheStatuses = $state<Map<string, boolean>>(new Map());

  // Derived: filtered albums based on search
  let filteredAlbums = $derived(
    searchQuery.trim()
      ? albums.filter(album =>
          album.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
          album.artist?.name?.toLowerCase().includes(searchQuery.toLowerCase())
        )
      : albums
  );

  async function loadLabel() {
    loading = true;
    error = null;

    try {
      const result = await invoke<{
        id: number;
        name: string;
        description?: string;
        image?: { small?: string; thumbnail?: string; large?: string };
        albums?: { items: QobuzAlbum[]; total: number; offset: number; limit: number };
        albums_count?: number;
      }>('get_label', { labelId, limit: 100, offset: 0 });

      label = {
        id: result.id,
        name: result.name,
        description: result.description,
        image: result.image,
        albums: result.albums?.items ?? [],
        totalAlbums: result.albums?.total ?? result.albums_count ?? 0,
        albumsFetched: result.albums?.items?.length ?? 0
      };

      albums = label.albums;
      totalAlbums = label.totalAlbums;
      albumsFetched = label.albumsFetched;

      // Load download statuses
      await loadAllAlbumOfflineCacheStatuses(albums);
    } catch (e) {
      console.error('Failed to load label:', e);
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function loadMore() {
    if (loadingMore || albumsFetched >= totalAlbums) return;

    loadingMore = true;

    try {
      const result = await invoke<{
        id: number;
        name: string;
        albums?: { items: QobuzAlbum[]; total: number; offset: number; limit: number };
      }>('get_label', { labelId, limit: 100, offset: albumsFetched });

      const newAlbums = result.albums?.items ?? [];
      albums = [...albums, ...newAlbums];
      albumsFetched += newAlbums.length;

      // Load download statuses for new albums
      await loadAllAlbumOfflineCacheStatuses(newAlbums);
    } catch (e) {
      console.error('Failed to load more albums:', e);
    } finally {
      loadingMore = false;
    }
  }

  async function loadAlbumOfflineCacheStatus(albumId: string) {
    if (!checkAlbumFullyDownloaded) return false;
    try {
      const isDownloaded = await checkAlbumFullyDownloaded(albumId);
      albumOfflineCacheStatuses.set(albumId, isDownloaded);
      return isDownloaded;
    } catch {
      return false;
    }
  }

  async function loadAllAlbumOfflineCacheStatuses(albumList: QobuzAlbum[]) {
    if (!checkAlbumFullyDownloaded || albumList.length === 0) return;
    await Promise.all(albumList.map(album => loadAlbumOfflineCacheStatus(album.id)));
  }

  function isAlbumDownloaded(albumId: string): boolean {
    void downloadStateVersion;
    return albumOfflineCacheStatuses.get(albumId) || false;
  }

  function getQualityLabel(album: QobuzAlbum): string {
    if (album.hires_streamable) {
      const bitDepth = album.maximum_bit_depth || 24;
      const sampleRate = album.maximum_sampling_rate || 96;
      return `${bitDepth}-bit/${sampleRate}kHz`;
    }
    return '';
  }

  function getGenreLabel(album: QobuzAlbum): string {
    return album.genre?.name || '';
  }

  function clearSearch() {
    searchQuery = '';
  }

  function toggleSearch() {
    searchExpanded = !searchExpanded;
    if (!searchExpanded) {
      searchQuery = '';
    }
  }

  // Scroll handler for infinite loading
  function handleScroll(e: Event) {
    const target = e.target as HTMLElement;
    const scrollBottom = target.scrollHeight - target.scrollTop - target.clientHeight;

    if (scrollBottom < 400 && !loadingMore && albumsFetched < totalAlbums) {
      loadMore();
    }
  }

  onMount(() => {
    loadLabel();
  });
</script>

<div class="label-view" onscroll={handleScroll}>
  <!-- Header -->
  <header class="header">
    <button class="back-btn" onclick={onBack} title="Go back">
      <ArrowLeft size={20} />
    </button>
    <div class="header-icon">
      <Disc3 size={36} strokeWidth={1.5} />
    </div>
    <div class="header-content">
      <h1>{label?.name || labelName || 'Label'}</h1>
      <p class="subtitle">
        {#if loading}
          Loading...
        {:else if totalAlbums > 0}
          {totalAlbums} album{totalAlbums !== 1 ? 's' : ''}
        {/if}
      </p>
    </div>
  </header>

  <!-- Fixed Navigation/Search Bar -->
  <nav class="label-nav">
    <div class="nav-left">
      <span class="nav-title">Albums</span>
      {#if albumsFetched > 0 && albumsFetched < totalAlbums}
        <span class="nav-count">Showing {albumsFetched} of {totalAlbums}</span>
      {/if}
    </div>
    <div class="nav-right">
      {#if searchExpanded}
        <div class="search-expanded">
          <Search size={16} class="search-icon-inline" />
          <input
            type="text"
            class="search-input-inline"
            placeholder="Search albums..."
            bind:value={searchQuery}
            autofocus
          />
          {#if searchQuery}
            <button class="search-clear-btn" onclick={clearSearch}>
              <X size={14} />
            </button>
          {/if}
          <button class="search-close-btn" onclick={toggleSearch}>
            <X size={16} />
          </button>
        </div>
      {:else}
        <button class="search-icon-btn" onclick={toggleSearch} title="Search albums">
          <Search size={18} />
        </button>
      {/if}
    </div>
  </nav>

  <!-- Content -->
  <div class="content">
    {#if loading}
      <div class="loading">
        <div class="spinner"></div>
        <p>Loading label...</p>
      </div>
    {:else if error}
      <div class="error">
        <p>Failed to load label</p>
        <p class="error-detail">{error}</p>
        <button class="retry-btn" onclick={loadLabel}>Retry</button>
      </div>
    {:else if albums.length === 0}
      <div class="empty">
        <Disc3 size={48} />
        <p>No albums found for this label</p>
      </div>
    {:else if filteredAlbums.length === 0}
      <div class="empty">
        <Search size={48} />
        <p>No albums match "{searchQuery}"</p>
      </div>
    {:else}
      <div class="album-grid">
        {#each filteredAlbums as album (album.id)}
          <AlbumCard
            albumId={album.id}
            artwork={album.image?.large || album.image?.thumbnail || ''}
            title={album.title}
            artist={album.artist?.name || 'Unknown Artist'}
            genre={getGenreLabel(album)}
            releaseDate={album.release_date_original}
            quality={getQualityLabel(album)}
            size="large"
            onclick={() => onAlbumClick?.(album.id)}
            onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
            onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
            onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
            onAddAlbumToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist(album.id) : undefined}
            onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
            onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
            onDownload={onAlbumDownload ? () => onAlbumDownload(album.id) : undefined}
            isAlbumFullyDownloaded={isAlbumDownloaded(album.id)}
            onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(album.id) : undefined}
            onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(album.id) : undefined}
            {downloadStateVersion}
          />
        {/each}
      </div>

      {#if loadingMore}
        <div class="loading-more">
          <Loader2 size={20} class="spinner-icon" />
          <span>Loading more albums...</span>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .label-view {
    padding: 24px;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  /* Custom scrollbar */
  .label-view::-webkit-scrollbar {
    width: 6px;
  }

  .label-view::-webkit-scrollbar-track {
    background: transparent;
  }

  .label-view::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .label-view::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 32px;
  }

  .back-btn {
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .back-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .header-icon {
    width: 80px;
    height: 80px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, #6366f1 0%, #8b5cf6 100%);
    border-radius: 16px;
    color: white;
  }

  .header-content {
    flex: 1;
  }

  .header-content h1 {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 4px 0;
  }

  .subtitle {
    font-size: 14px;
    color: var(--text-muted);
    margin: 0;
  }

  .label-nav {
    position: sticky;
    top: -24px;
    z-index: 4;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 12px 24px;
    margin: 0 -8px 16px -24px;
    width: calc(100% + 32px);
    background: var(--bg-primary);
    border-bottom: 1px solid var(--alpha-6);
    box-shadow: 0 4px 8px -4px rgba(0, 0, 0, 0.5);
  }

  .nav-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .nav-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .nav-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .nav-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .search-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    transition: all 150ms ease;
  }

  .search-icon-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .search-expanded {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    min-width: 280px;
  }

  :global(.search-icon-inline) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input-inline {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 13px;
  }

  .search-input-inline::placeholder {
    color: var(--text-muted);
  }

  .search-clear-btn,
  .search-close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .search-clear-btn:hover,
  .search-close-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .content {
    min-height: 200px;
  }

  .loading,
  .error,
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px 24px;
    color: var(--text-muted);
    text-align: center;
  }

  .loading p,
  .error p,
  .empty p {
    margin: 16px 0 0 0;
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .error-detail {
    font-size: 12px;
    margin-top: 8px;
  }

  .retry-btn {
    margin-top: 16px;
    padding: 8px 24px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    cursor: pointer;
  }

  .retry-btn:hover {
    opacity: 0.9;
  }

  .album-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 24px 14px;
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

  :global(.spinner-icon) {
    animation: spin 1s linear infinite;
  }
</style>
