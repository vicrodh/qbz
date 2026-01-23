<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Disc3 } from 'lucide-svelte';
  import AlbumCard from './AlbumCard.svelte';

  // Types
  interface LocalAlbum {
    id: string;
    title: string;
    artist: string;
    year?: number;
    catalog_number?: string;
    artwork_path?: string;
    track_count: number;
    total_duration_secs: number;
    format: string;
    bit_depth?: number;
    sample_rate: number;
    directory_path: string;
  }

  interface AlbumGroup {
    key: string;
    id: string;
    albums: LocalAlbum[];
  }

  type VirtualItem =
    | { type: 'header'; key: string; id: string; height: number }
    | { type: 'album'; album: LocalAlbum; height: number }
    | { type: 'row'; albums: LocalAlbum[]; height: number }; // For grid mode

  interface Props {
    groups: AlbumGroup[];
    viewMode: 'grid' | 'list';
    showGroupHeaders: boolean;
    getArtworkUrl: (path?: string) => string;
    getQualityBadge: (album: LocalAlbum) => string;
    isHiRes: (album: LocalAlbum) => boolean;
    formatDuration: (secs: number) => string;
    onAlbumClick: (album: LocalAlbum) => void;
    onAlbumPlay: (album: LocalAlbum) => void;
    onAlbumQueueNext: (album: LocalAlbum) => void;
    onAlbumQueueLater: (album: LocalAlbum) => void;
    scrollToGroupId?: string;
  }

  let {
    groups,
    viewMode,
    showGroupHeaders,
    getArtworkUrl,
    getQualityBadge,
    isHiRes,
    formatDuration,
    onAlbumClick,
    onAlbumPlay,
    onAlbumQueueNext,
    onAlbumQueueLater,
    scrollToGroupId,
  }: Props = $props();

  // Constants
  const HEADER_HEIGHT = 44; // px
  const LIST_ROW_HEIGHT = 76; // px (52px art + padding + gap)
  const GRID_ROW_HEIGHT = 260; // px (approx for 180px card + info + gap)
  const GRID_MIN_CARD_WIDTH = 180; // px
  const GRID_GAP = 24; // px
  const BUFFER_ITEMS = 5; // Extra items to render above/below viewport

  // State
  let containerEl: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let containerHeight = $state(0);
  let containerWidth = $state(0);

  // Computed: number of columns in grid mode
  let gridColumns = $derived.by(() => {
    if (viewMode !== 'grid' || containerWidth === 0) return 1;
    return Math.max(1, Math.floor((containerWidth + GRID_GAP) / (GRID_MIN_CARD_WIDTH + GRID_GAP)));
  });

  // Computed: flatten groups into virtual items with cumulative positions
  let virtualItems = $derived.by(() => {
    const items: (VirtualItem & { top: number; groupId?: string })[] = [];
    let currentTop = 0;

    for (const group of groups) {
      // Add header if showing
      if (showGroupHeaders && group.key) {
        items.push({
          type: 'header',
          key: group.key,
          id: group.id,
          height: HEADER_HEIGHT,
          top: currentTop,
          groupId: group.id,
        });
        currentTop += HEADER_HEIGHT;
      }

      if (viewMode === 'list') {
        // List mode: one album per row
        for (const album of group.albums) {
          items.push({
            type: 'album',
            album,
            height: LIST_ROW_HEIGHT,
            top: currentTop,
          });
          currentTop += LIST_ROW_HEIGHT;
        }
      } else {
        // Grid mode: group albums into rows based on column count
        const cols = gridColumns;
        for (let i = 0; i < group.albums.length; i += cols) {
          const rowAlbums = group.albums.slice(i, i + cols);
          items.push({
            type: 'row',
            albums: rowAlbums,
            height: GRID_ROW_HEIGHT,
            top: currentTop,
          });
          currentTop += GRID_ROW_HEIGHT;
        }
      }
    }

    return items;
  });

  // Computed: total height of all items
  let totalHeight = $derived(
    virtualItems.length > 0
      ? virtualItems[virtualItems.length - 1].top + virtualItems[virtualItems.length - 1].height
      : 0
  );

  // Binary search to find first item at or after a given position
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

  // Binary search to find last item before a given position
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

  // Computed: visible items (those in viewport + buffer)
  let visibleItems = $derived.by(() => {
    if (virtualItems.length === 0) return [];

    const viewportTop = scrollTop;
    const viewportBottom = scrollTop + containerHeight;

    // Use binary search for O(log n) instead of O(n)
    const firstVisible = binarySearchStart(virtualItems, viewportTop);
    const lastVisible = binarySearchEnd(virtualItems, viewportBottom, firstVisible);

    const startIdx = Math.max(0, firstVisible - BUFFER_ITEMS);
    const endIdx = Math.min(virtualItems.length - 1, lastVisible + BUFFER_ITEMS);

    return virtualItems.slice(startIdx, endIdx + 1);
  });

  // Group ID to scroll position map
  let groupPositions = $derived.by(() => {
    const map = new Map<string, number>();
    for (const item of virtualItems) {
      if (item.groupId) {
        map.set(item.groupId, item.top);
      }
    }
    return map;
  });

  // Scroll handling
  function handleScroll(e: Event) {
    const target = e.target as HTMLDivElement;
    scrollTop = target.scrollTop;
  }

  // Resize observer for container dimensions
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

  // Public method to scroll to a specific group
  export function scrollToGroup(groupId: string) {
    const position = groupPositions.get(groupId);
    if (position !== undefined && containerEl) {
      containerEl.scrollTo({ top: position, behavior: 'smooth' });
    }
  }
</script>

<div
  class="virtual-container"
  bind:this={containerEl}
  onscroll={handleScroll}
>
  <div class="virtual-content" style="height: {totalHeight}px;">
    {#each visibleItems as item (item.type === 'header' ? `header-${item.id}` : item.type === 'album' ? `album-${item.album.id}` : `row-${item.albums[0]?.id}`)}
      <div
        class="virtual-item"
        style="transform: translateY({item.top}px); height: {item.height}px;"
      >
        {#if item.type === 'header'}
          <div class="group-header">
            <span class="group-title">{item.key}</span>
          </div>
        {:else if item.type === 'album'}
          <!-- List mode: single album row -->
          <div
            class="album-row"
            role="button"
            tabindex="0"
            onclick={() => onAlbumClick(item.album)}
            onkeydown={(e) => e.key === 'Enter' && onAlbumClick(item.album)}
          >
            <div class="album-row-art">
              {#if item.album.artwork_path}
                <img
                  src={getArtworkUrl(item.album.artwork_path)}
                  alt={item.album.title}
                  loading="lazy"
                  decoding="async"
                />
              {:else}
                <div class="artwork-placeholder">
                  <Disc3 size={28} />
                </div>
              {/if}
            </div>
            <div class="album-row-info">
              <div class="album-row-title truncate">{item.album.title}</div>
              <div class="album-row-meta">
                <span>{item.album.artist}</span>
                {#if item.album.year}<span>{item.album.year}</span>{/if}
                <span>{item.album.track_count} tracks</span>
                <span>{formatDuration(item.album.total_duration_secs)}</span>
              </div>
            </div>
            <div class="album-row-quality">
              <span class="quality-badge" class:hires={isHiRes(item.album)}>
                {getQualityBadge(item.album)}
              </span>
            </div>
          </div>
        {:else if item.type === 'row'}
          <!-- Grid mode: row of album cards -->
          <div class="album-grid-row">
            {#each item.albums as album (album.id)}
              <AlbumCard
                artwork={getArtworkUrl(album.artwork_path)}
                title={album.title}
                artist={album.artist}
                quality={getQualityBadge(album)}
                showFavorite={true}
                favoriteEnabled={false}
                onPlay={() => onAlbumPlay(album)}
                onPlayNext={() => onAlbumQueueNext(album)}
                onPlayLater={() => onAlbumQueueLater(album)}
                onclick={() => onAlbumClick(album)}
              />
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .virtual-container {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
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

  /* Group Header */
  .group-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 0;
  }

  .group-title {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  /* Album Row (List Mode) */
  .album-row {
    display: grid;
    grid-template-columns: 56px 1fr auto;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    background: var(--bg-secondary);
    border-radius: 10px;
    cursor: pointer;
    transition: background 150ms ease;
    height: 72px;
    box-sizing: border-box;
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
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
  }

  .album-row-info {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .album-row-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .truncate {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .album-row-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .album-row-meta span:not(:last-child)::after {
    content: '·';
    margin-left: 8px;
  }

  .album-row-quality {
    flex-shrink: 0;
  }

  .quality-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 4px 8px;
    border-radius: 6px;
    background: var(--bg-tertiary);
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .quality-badge.hires {
    background: var(--accent-gradient, linear-gradient(135deg, #667eea 0%, #764ba2 100%));
    color: white;
  }

  /* Album Grid Row (Grid Mode) */
  .album-grid-row {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(162px, 1fr));
    gap: 24px;
    padding: 0;
  }

  .album-grid-row :global(.album-card) {
    width: 100%;
    max-width: 200px;
  }
</style>
