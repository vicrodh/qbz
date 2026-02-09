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
    | { type: 'artist'; artist: FavoriteArtist; height: number };

  interface Props {
    groups: ArtistGroup[];
    showGroupHeaders: boolean;
    selectedArtistId?: number | null;
    onArtistSelect: (artist: FavoriteArtist) => void;
    scrollToGroupId?: string;
  }

  let {
    groups,
    showGroupHeaders,
    selectedArtistId = null,
    onArtistSelect,
    scrollToGroupId,
  }: Props = $props();

  // Constants
  const HEADER_HEIGHT = 32;
  const ROW_HEIGHT = 64; // 48px image + padding
  const BUFFER_ITEMS = 10;

  // State
  let containerEl: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let containerHeight = $state(0);

  // Ticker animation state
  let hoveredArtistId = $state<number | null>(null);
  let artistNameOverflows = $state<Map<number, number>>(new Map());
  const tickerSpeed = 40; // px per second

  function measureArtistNameOverflow(artistId: number, element: HTMLElement | null) {
    if (!element) return;
    const textSpan = element.querySelector('.artist-name-text') as HTMLElement | null;
    if (!textSpan) return;
    const overflow = textSpan.scrollWidth - element.clientWidth;
    if (overflow > 0) {
      artistNameOverflows.set(artistId, overflow);
    } else {
      artistNameOverflows.delete(artistId);
    }
  }

  function getArtistNameTickerStyle(artistId: number): string {
    if (hoveredArtistId !== artistId) return '';
    const overflow = artistNameOverflows.get(artistId);
    if (!overflow || overflow <= 0) return '';
    const duration = (overflow + 16) / tickerSpeed;
    return `--ticker-offset: -${overflow + 16}px; --ticker-duration: ${duration}s;`;
  }

  // Computed: flatten groups into virtual items
  let virtualItems = $derived.by(() => {
    const items: (VirtualItem & { top: number; groupId?: string })[] = [];
    let currentTop = 0;

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

      for (const artist of group.artists) {
        items.push({
          type: 'artist',
          artist,
          height: ROW_HEIGHT,
          top: currentTop,
        });
        currentTop += ROW_HEIGHT;
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

      resizeObserver = new ResizeObserver((entries) => {
        for (const entry of entries) {
          containerHeight = entry.contentRect.height;
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
    return `artist-${item.artist.id}`;
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
          <div class="group-header">{item.key}</div>
        {:else if item.type === 'artist'}
          {@const artist = item.artist}
          {@const hasOverflow = artistNameOverflows.has(artist.id)}
          {@const isHovered = hoveredArtistId === artist.id}
          <button
            class="artist-list-item"
            class:selected={selectedArtistId === artist.id}
            onclick={() => onArtistSelect(artist)}
            onmouseenter={(e) => {
              hoveredArtistId = artist.id;
              const info = (e.currentTarget as HTMLElement).querySelector('.artist-list-name');
              measureArtistNameOverflow(artist.id, info as HTMLElement);
            }}
            onmouseleave={() => { hoveredArtistId = null; }}
          >
            <div class="artist-list-image">
              {#if artist.image?.thumbnail || artist.image?.small}
                <img src={artist.image?.thumbnail || artist.image?.small} alt={artist.name} loading="lazy" decoding="async" />
              {:else}
                <div class="artist-list-placeholder">
                  <Mic2 size={20} />
                </div>
              {/if}
            </div>
            <div class="artist-list-info">
              <div
                class="artist-list-name"
                class:scrollable={hasOverflow}
                style={getArtistNameTickerStyle(artist.id)}
              >
                <span class="artist-name-text" class:animating={isHovered && hasOverflow}>{artist.name}</span>
              </div>
              {#if artist.albums_count}
                <div class="artist-list-meta">{$t('library.albumCount', { values: { count: artist.albums_count } })}</div>
              {/if}
            </div>
          </button>
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
    padding: 4px 12px 4px 0;
    -webkit-overflow-scrolling: touch;
    scroll-behavior: smooth;
    overscroll-behavior: contain;
  }

  .virtual-container::-webkit-scrollbar {
    width: 4px;
  }

  .virtual-container::-webkit-scrollbar-track {
    background: transparent;
  }

  .virtual-container::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 2px;
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
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    padding: 12px 8px 4px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .artist-list-item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 8px;
    border: none;
    background: transparent;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
    text-align: left;
  }

  .artist-list-item:hover {
    background: var(--bg-hover);
  }

  .artist-list-item.selected {
    background: var(--bg-tertiary);
  }

  .artist-list-image {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    overflow: hidden;
    flex-shrink: 0;
    background: var(--bg-tertiary);
  }

  .artist-list-image img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artist-list-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .artist-list-info {
    flex: 1;
    min-width: 0;
  }

  .artist-list-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist-list-name.scrollable {
    text-overflow: clip;
  }

  .artist-list-name .artist-name-text {
    display: inline-block;
  }

  .artist-list-name .artist-name-text.animating {
    animation: artist-name-ticker var(--ticker-duration, 0s) linear infinite;
  }

  @keyframes artist-name-ticker {
    0%, 20% { transform: translateX(0); }
    70%, 80% { transform: translateX(var(--ticker-offset, 0)); }
    90%, 100% { transform: translateX(0); }
  }

  .artist-list-meta {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 2px;
  }
</style>
