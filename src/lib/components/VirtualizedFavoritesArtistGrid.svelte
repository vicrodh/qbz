<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { Mic2 } from 'lucide-svelte';

  interface FavoriteArtist {
    id: number;
    name: string;
    image?: { small?: string; thumbnail?: string; large?: string };
    albums_count?: number;
  }

  interface ArtistGroup {
    key: string;
    id: string;
    artists: FavoriteArtist[];
  }

  type VirtualItem =
    | { type: 'header'; key: string; id: string; height: number }
    | { type: 'row'; artists: FavoriteArtist[]; height: number };

  interface Props {
    groups: ArtistGroup[];
    showGroupHeaders: boolean;
    selectedArtistId?: number | null;
    onArtistClick: (artistId: number) => void;
    scrollToGroupId?: string;
  }

  let {
    groups,
    showGroupHeaders,
    selectedArtistId = null,
    onArtistClick,
    scrollToGroupId,
  }: Props = $props();

  // Constants
  const CARD_WIDTH = 160;
  const CARD_HEIGHT = 200;
  const GAP = 24;
  const ROW_GAP = 24;
  const HEADER_HEIGHT = 44;
  const BUFFER_ITEMS = 5;

  // State
  let containerEl: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let containerHeight = $state(0);
  let containerWidth = $state(0);

  // Computed: number of columns
  let columns = $derived.by(() => {
    if (containerWidth === 0) return 1;
    return Math.max(1, Math.floor((containerWidth + GAP) / (CARD_WIDTH + GAP)));
  });

  // Computed: flatten groups into virtual items
  let virtualItems = $derived.by(() => {
    const items: (VirtualItem & { top: number; groupId?: string })[] = [];
    let currentTop = 0;
    const rowHeight = CARD_HEIGHT + ROW_GAP;

    for (const group of groups) {
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

      const cols = columns;
      for (let i = 0; i < group.artists.length; i += cols) {
        const rowArtists = group.artists.slice(i, i + cols);
        items.push({
          type: 'row',
          artists: rowArtists,
          height: CARD_HEIGHT,
          top: currentTop,
        });
        currentTop += rowHeight;
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
    scrollTop = (e.target as HTMLDivElement).scrollTop;
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
    return `row-${item.artists[0]?.id ?? item.top}`;
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
          <div class="group-header">
            <span class="group-title">{item.key}</span>
            <span class="group-count">{groups.find(grp => grp.id === item.id)?.artists.length ?? ''}</span>
          </div>
        {:else if item.type === 'row'}
          <div class="artist-row">
            {#each item.artists as artist (artist.id)}
              <button
                class="artist-card"
                class:selected={selectedArtistId === artist.id}
                onclick={() => onArtistClick(artist.id)}
              >
                <div class="artist-image">
                  {#if artist.image?.large || artist.image?.thumbnail}
                    <img src={artist.image?.large || artist.image?.thumbnail} alt={artist.name} loading="lazy" decoding="async" />
                  {:else}
                    <div class="artist-placeholder">
                      <Mic2 size={32} />
                    </div>
                  {/if}
                </div>
                <div class="artist-name">{artist.name}</div>
                {#if artist.albums_count}
                  <div class="artist-albums">{$t('library.albumCount', { values: { count: artist.albums_count } })}</div>
                {/if}
              </button>
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

  .group-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    color: var(--text-muted);
    font-size: 12px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 12px 0 4px;
  }

  .group-title {
    font-weight: 600;
  }

  .group-count {
    font-size: 12px;
  }

  .artist-row {
    display: flex;
    gap: 24px;
    padding: 0;
  }

  .artist-card {
    width: 160px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 16px;
    background-color: var(--bg-secondary);
    border: none;
    border-radius: 12px;
    cursor: pointer;
    transition: background-color 150ms ease;
    box-sizing: border-box;
  }

  .artist-card:hover {
    background-color: var(--bg-tertiary);
  }

  .artist-card.selected {
    background-color: var(--bg-tertiary);
    outline: 2px solid var(--accent-primary);
    outline-offset: -2px;
  }

  .artist-image {
    width: 120px;
    height: 120px;
    border-radius: 50%;
    overflow: hidden;
    margin-bottom: 12px;
    background-color: var(--bg-tertiary);
  }

  .artist-image img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artist-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .artist-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    width: 100%;
    line-height: 1.3;
  }

  .artist-albums {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
  }
</style>
