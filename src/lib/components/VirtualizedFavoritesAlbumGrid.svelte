<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { Disc3, Loader2 } from 'lucide-svelte';
  import AlbumCard from './AlbumCard.svelte';
  import QualityBadge from './QualityBadge.svelte';

  interface FavoriteAlbum {
    id: string;
    title: string;
    artist: { id: number; name: string };
    genre?: { name: string };
    image: { small?: string; thumbnail?: string; large?: string };
    release_date_original?: string;
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
  }

  interface AlbumGroup {
    key: string;
    id: string;
    albums: FavoriteAlbum[];
  }

  type VirtualItem =
    | { type: 'header'; key: string; id: string; albumCount: number; height: number }
    | { type: 'grid-row'; albums: FavoriteAlbum[]; startIndex: number; height: number }
    | { type: 'list-row'; album: FavoriteAlbum; globalIndex: number; height: number };

  interface Props {
    groups: AlbumGroup[];
    showGroupHeaders: boolean;
    viewMode: 'grid' | 'list';
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    downloadStateVersion?: number;
    isAlbumDownloaded?: (albumId: string) => boolean;
    onAlbumClicked?: (albumId: string) => void;
    scrollToGroupId?: string;
    getQualityLabel?: (item: { hires?: boolean; maximum_bit_depth?: number; maximum_sampling_rate?: number }) => string;
    getGenreLabel?: (album: FavoriteAlbum) => string;
    getAlbumYear?: (album: FavoriteAlbum) => string | null;
    showRanking?: boolean;
    onLoadMore?: () => void;
    isLoadingMore?: boolean;
    onAddAlbumToPlaylist?: (albumId: string) => void;
  }

  let {
    groups,
    showGroupHeaders,
    viewMode,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    downloadStateVersion,
    isAlbumDownloaded,
    onAlbumClicked,
    scrollToGroupId,
    getQualityLabel,
    getGenreLabel,
    getAlbumYear,
    showRanking = false,
    onLoadMore,
    isLoadingMore = false,
    onAddAlbumToPlaylist,
  }: Props = $props();

  // Constants
  const GRID_CARD_WIDTH = 180;
  const GRID_CARD_HEIGHT = 290;
  const GRID_GAP_X = 14;
  const GRID_GAP_Y = 24;
  const LIST_ROW_HEIGHT = 76; // 52px art + padding
  const LIST_ROW_GAP = 8;
  const HEADER_HEIGHT = 44;
  const BUFFER_ITEMS = 5;

  // State
  let containerEl: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let containerHeight = $state(0);
  let containerWidth = $state(0);

  // Computed: number of grid columns
  let gridColumns = $derived.by(() => {
    if (containerWidth === 0) return 1;
    return Math.max(1, Math.floor((containerWidth + GRID_GAP_X) / (GRID_CARD_WIDTH + GRID_GAP_X)));
  });

  // Computed: flatten groups into virtual items
  let virtualItems = $derived.by(() => {
    const items: (VirtualItem & { top: number; groupId?: string })[] = [];
    let currentTop = 0;
    let albumIndex = 0;

    for (const group of groups) {
      if (showGroupHeaders && group.key) {
        items.push({
          type: 'header',
          key: group.key,
          id: group.id,
          albumCount: group.albums.length,
          height: HEADER_HEIGHT,
          top: currentTop,
          groupId: group.id,
        });
        currentTop += HEADER_HEIGHT;
      }

      if (viewMode === 'grid') {
        const rowHeight = GRID_CARD_HEIGHT + GRID_GAP_Y;
        const cols = gridColumns;
        for (let i = 0; i < group.albums.length; i += cols) {
          const rowAlbums = group.albums.slice(i, i + cols);
          items.push({
            type: 'grid-row',
            albums: rowAlbums,
            startIndex: albumIndex + i,
            height: GRID_CARD_HEIGHT,
            top: currentTop,
          });
          currentTop += rowHeight;
        }
        albumIndex += group.albums.length;
      } else {
        const rowHeight = LIST_ROW_HEIGHT + LIST_ROW_GAP;
        for (const album of group.albums) {
          items.push({
            type: 'list-row',
            album,
            globalIndex: albumIndex,
            height: LIST_ROW_HEIGHT,
            top: currentTop,
          });
          albumIndex++;
          currentTop += rowHeight;
        }
      }
    }

    return items;
  });

  // Computed: total height
  let totalHeight = $derived(
    virtualItems.length > 0
      ? virtualItems[virtualItems.length - 1].top + virtualItems[virtualItems.length - 1].height
      : 0
  );

  // Binary search for first visible item
  function binarySearchStart(items: typeof virtualItems, targetTop: number): number {
    let low = 0;
    let high = items.length - 1;
    let result = 0;

    while (low <= high) {
      const mid = Math.floor((low + high) / 2);
      const item = items[mid];
      if (item.top + item.height > targetTop) {
        result = mid;
        high = mid - 1;
      } else {
        low = mid + 1;
      }
    }
    return result;
  }

  // Binary search for last visible item
  function binarySearchEnd(items: typeof virtualItems, targetBottom: number, startFrom: number): number {
    let low = startFrom;
    let high = items.length - 1;
    let result = high;

    while (low <= high) {
      const mid = Math.floor((low + high) / 2);
      const item = items[mid];
      if (item.top > targetBottom) {
        result = mid;
        high = mid - 1;
      } else {
        low = mid + 1;
      }
    }
    return result;
  }

  // Computed: visible items
  let visibleItems = $derived.by(() => {
    if (virtualItems.length === 0) return [];

    const viewportTop = scrollTop;
    const viewportBottom = scrollTop + containerHeight;

    const firstVisible = binarySearchStart(virtualItems, viewportTop);
    const lastVisible = binarySearchEnd(virtualItems, viewportBottom, firstVisible);

    const startIdx = Math.max(0, firstVisible - BUFFER_ITEMS);
    const endIdx = Math.min(virtualItems.length - 1, lastVisible + BUFFER_ITEMS);

    return virtualItems.slice(startIdx, endIdx + 1);
  });

  // Group positions for scroll-to
  let groupPositions = $derived.by(() => {
    const map = new Map<string, number>();
    for (const item of virtualItems) {
      if (item.groupId) {
        map.set(item.groupId, item.top);
      }
    }
    return map;
  });

  function handleScroll(e: Event) {
    const el = e.target as HTMLDivElement;
    scrollTop = el.scrollTop;

    // Infinite scroll: fire onLoadMore when near bottom
    if (onLoadMore && !isLoadingMore) {
      const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
      if (distanceFromBottom < 300) {
        onLoadMore();
      }
    }
  }

  let resizeObserver: ResizeObserver | null = null;

  onMount(() => {
    if (containerEl) {
      containerHeight = containerEl.clientHeight;
      containerWidth = containerEl.clientWidth;

      resizeObserver = new ResizeObserver((entries) => {
        for (const entry of entries) {
          containerHeight = entry.contentRect.height;
          containerWidth = entry.contentRect.width;
        }
      });
      resizeObserver.observe(containerEl);
    }
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
  });

  // Scroll to group when requested
  $effect(() => {
    if (scrollToGroupId && containerEl) {
      const position = groupPositions.get(scrollToGroupId);
      if (position !== undefined) {
        containerEl.scrollTo({ top: position, behavior: 'smooth' });
      }
    }
  });

  export function scrollToGroup(groupId: string) {
    const position = groupPositions.get(groupId);
    if (position !== undefined && containerEl) {
      containerEl.scrollTo({ top: position, behavior: 'smooth' });
    }
  }

  function getItemKey(item: typeof virtualItems[0]): string {
    if (item.type === 'header') return `header-${item.id}`;
    if (item.type === 'grid-row') return `grid-row-${item.albums[0]?.id ?? item.top}`;
    return `list-row-${item.album.id}`;
  }

  function handleAlbumClickEvent(albumId: string) {
    onAlbumClick?.(albumId);
    onAlbumClicked?.(albumId);
  }
</script>

<div class="virtual-container" bind:this={containerEl} onscroll={handleScroll}>
  <div class="virtual-content" style="height: {totalHeight}px;">
    {#each visibleItems as item (getItemKey(item))}
      <div
        class="virtual-item"
        style="transform: translateY({item.top}px); height: {item.height}px;"
      >
        {#if item.type === 'header'}
          <div class="album-group-header">
            <span class="album-group-title">{item.key}</span>
            <span class="album-group-count">{item.albumCount}</span>
          </div>
        {:else if item.type === 'grid-row'}
          <div class="album-grid-row">
            {#each item.albums as album, idx (album.id)}
              <div class="grid-card-wrapper">
                {#if showRanking}
                  <div class="rank-badge">{item.startIndex + idx + 1}</div>
                {/if}
                <AlbumCard
                  albumId={album.id}
                  artwork={album.image?.large || album.image?.thumbnail || ''}
                  title={album.title}
                  artist={album.artist.name}
                  genre={getGenreLabel?.(album) ?? album.genre?.name ?? ''}
                  releaseDate={album.release_date_original}
                  size="large"
                  quality={getQualityLabel?.(album) ?? ''}
                  onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
                  onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
                  onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
                  onAddAlbumToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist(album.id) : undefined}
                  onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
                  onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
                  onDownload={onAlbumDownload ? () => onAlbumDownload(album.id) : undefined}
                  isAlbumFullyDownloaded={isAlbumDownloaded?.(album.id) ?? false}
                  onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(album.id) : undefined}
                  onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(album.id) : undefined}
                  {downloadStateVersion}
                  onclick={() => handleAlbumClickEvent(album.id)}
                />
              </div>
            {/each}
          </div>
        {:else if item.type === 'list-row'}
          {@const album = item.album}
          <div class="album-row" class:has-rank={showRanking} role="button" tabindex="0" onclick={() => handleAlbumClickEvent(album.id)}>
            {#if showRanking}
              <div class="rank-number">#{item.globalIndex + 1}</div>
            {/if}
            <div class="album-row-art">
              {#if album.image?.thumbnail || album.image?.small || album.image?.large}
                <img src={album.image?.thumbnail || album.image?.small || album.image?.large} alt={album.title} loading="lazy" decoding="async" />
              {:else}
                <div class="artwork-placeholder">
                  <Disc3 size={28} />
                </div>
              {/if}
            </div>
            <div class="album-row-info">
              <div class="album-row-title truncate">{album.title}</div>
              <div class="album-row-meta">
                <span>{album.artist.name}</span>
                {#if getAlbumYear?.(album)}<span>{getAlbumYear(album)}</span>{/if}
              </div>
            </div>
            <div class="album-row-quality">
              <QualityBadge
                bitDepth={album.maximum_bit_depth}
                samplingRate={album.maximum_sampling_rate}
                compact
              />
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </div>
  {#if isLoadingMore}
    <div class="loading-more">
      <Loader2 size={20} class="spin" />
      <span>{$t('discover.loadingMore')}</span>
    </div>
  {/if}
</div>

<style>
  .virtual-container {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
  }

  .virtual-container::-webkit-scrollbar {
    width: 6px;
  }

  .virtual-container::-webkit-scrollbar-track {
    background: transparent;
  }

  .virtual-container::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .virtual-container::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .virtual-content {
    position: relative;
    width: 100%;
  }

  .virtual-item {
    position: absolute;
    left: 0;
    right: 0;
    will-change: transform;
  }

  .album-group-header {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 12px 0 4px;
  }

  .album-group-title {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .album-group-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .album-grid-row {
    display: flex;
    flex-wrap: wrap;
    gap: 24px 14px;
  }

  /* List mode styles */
  .album-row {
    display: grid;
    grid-template-columns: 56px 1fr auto;
    gap: 12px;
    align-items: center;
    padding: 10px 12px;
    background: var(--bg-secondary);
    border-radius: 10px;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .album-row:hover {
    background: var(--bg-tertiary);
  }

  .album-row-art {
    width: 52px;
    height: 52px;
    border-radius: 8px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .album-row-art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    color: var(--text-muted);
  }

  .album-row-info {
    min-width: 0;
  }

  .album-row-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .album-row-meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    font-size: 12px;
    color: var(--text-muted);
  }

  .album-row-meta span + span::before {
    content: "\2022";
    margin: 0 8px;
    color: var(--text-muted);
  }

  .album-row-quality {
    display: flex;
    justify-content: flex-end;
  }

  /* Ranking styles */
  .grid-card-wrapper {
    position: relative;
  }

  .rank-badge {
    position: absolute;
    top: 4px;
    right: 4px;
    z-index: 2;
    background: rgba(0, 0, 0, 0.70);
    color: #fff;
    font-family: Impact, 'Arial Black', sans-serif;
    font-size: 35px;
    font-weight: 700;
    padding: 2px 8px;
    border-radius: 8px;
    pointer-events: none;
    line-height: 1;
  }

  .rank-number {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-muted);
    min-width: 36px;
    text-align: right;
    flex-shrink: 0;
  }

  .album-row.has-rank {
    grid-template-columns: 36px 56px 1fr auto;
  }

  /* Loading more spinner */
  .loading-more {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px 0;
    color: var(--text-muted);
    font-size: 13px;
  }

  .loading-more :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
