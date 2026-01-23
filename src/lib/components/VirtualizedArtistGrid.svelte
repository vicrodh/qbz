<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Mic2, Upload } from 'lucide-svelte';

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  type Artist = any;

  interface ArtistGroup {
    key: string;
    id: string;
    artists: Artist[];
  }

  type VirtualItem =
    | { type: 'header'; key: string; id: string; height: number }
    | { type: 'row'; artists: Artist[]; height: number };

  interface Props {
    groups: ArtistGroup[];
    artistImages: Map<string, string>;
    showSettings: boolean;
    showGroupHeaders: boolean;
    onArtistClick: (name: string) => void;
    onUploadImage: (name: string, e: MouseEvent) => void;
    scrollToGroupId?: string;
  }

  let {
    groups,
    artistImages,
    showSettings,
    showGroupHeaders,
    onArtistClick,
    onUploadImage,
    scrollToGroupId,
  }: Props = $props();

  // Constants
  const CARD_WIDTH = 160;
  const CARD_HEIGHT = 200; // Fixed card height
  const GAP = 24; // Horizontal gap between cards
  const ROW_GAP = 24; // Vertical gap between rows
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

      // Group artists into rows
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
    return `row-${item.artists[0]?.name ?? item.top}`;
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
          </div>
        {:else if item.type === 'row'}
          <div class="artist-row">
            {#each item.artists as artist (artist.name)}
              {@const artistImage = artistImages.get(artist.name)}
              <div
                class="artist-card"
                role="button"
                tabindex="0"
                onclick={() => onArtistClick(artist.name)}
                onkeydown={(e) => e.key === 'Enter' && onArtistClick(artist.name)}
              >
                <div class="artist-icon" class:has-image={!!artistImage}>
                  {#if artistImage}
                    <img src={artistImage} alt={artist.name} class="artist-image" loading="lazy" />
                  {:else}
                    <Mic2 size={32} />
                  {/if}
                </div>
                {#if showSettings}
                  <button
                    class="artist-image-btn"
                    onclick={(e) => { e.stopPropagation(); onUploadImage(artist.name, e); }}
                    title="Upload custom image"
                  >
                    <Upload size={14} />
                  </button>
                {/if}
                <div class="artist-name">{artist.name}</div>
                <div class="artist-stats">
                  {artist.album_count} albums &bull; {artist.track_count} tracks
                </div>
              </div>
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

  .artist-row {
    display: flex;
    gap: 24px;
    padding: 0;
  }

  .artist-card {
    width: 160px;
    height: 200px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 16px;
    background: var(--bg-secondary);
    border-radius: 12px;
    cursor: pointer;
    transition: background 150ms ease;
    position: relative;
    box-sizing: border-box;
  }

  .artist-card:hover {
    background: var(--bg-tertiary);
  }

  .artist-icon {
    width: 80px;
    height: 80px;
    flex-shrink: 0;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
    overflow: hidden;
  }

  .artist-icon.has-image {
    background: none;
  }

  .artist-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artist-image-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    border: none;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .artist-card:hover .artist-image-btn {
    opacity: 1;
  }

  .artist-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .artist-stats {
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }
</style>
