<script lang="ts" generics="TAlbum">
  import { onMount, untrack } from 'svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    /** Total number of items in the logical collection — drives the
     *  pre-allocated scroll height even when only a chunk is loaded.
     *  Caller passes the total count, not the loaded count. */
    totalCount: number;
    /** Returns the item at a given flat index, or `null` if the chunk
     *  containing that index hasn't loaded yet. (For Phase B this is a
     *  trivial array lookup; for Phase A it consults a chunked store.) */
    getItem: (index: number) => TAlbum | null;
    /** Caller bumps this whenever the underlying data changes in a way
     *  that should re-paint already-rendered slots (filter / sort
     *  change, chunk arrival, etc.). The pool re-reads via `getItem`
     *  for every slot when this changes. */
    dataVersion?: number;
    /** Card layout. Defaults match `AlbumCardLibraryLite`. */
    cardWidth?: number;
    cardHeight?: number;
    rowGap?: number;
    colGap?: number;
    /** Extra empty space below the last row so the player bar doesn't
     *  cover the bottom row. */
    bottomPadding?: number;
    /** Render the loaded item at `(item, index)`. */
    renderCell: Snippet<[TAlbum, number]>;
    /** Render a placeholder for an in-range index whose data is null.
     *  Use this to show a skeleton while the chunk is in flight. */
    renderPlaceholder?: Snippet<[number]>;
    /** Fires when a slot maps to an index whose data is null — caller
     *  can use this to trigger the chunk fetch (Phase A). Phase B
     *  callers ignore it because all data is loaded up front. */
    onNeedIndex?: (index: number) => void;
  }

  let {
    totalCount,
    getItem,
    dataVersion = 0,
    cardWidth = 210,
    cardHeight = 320,
    rowGap = 24,
    colGap = 22,
    bottomPadding = 100,
    renderCell,
    renderPlaceholder,
    onNeedIndex,
  }: Props = $props();

  /**
   * Recycling grid pool — Plex-style.
   *
   * Constant pool of N absolutely-positioned slots inside a pre-sized
   * spacer. As the user scrolls, slot **positions** and **bindings**
   * update; slot **DOM and component instances** do NOT mount/unmount.
   *
   * Why this matters: Svelte's `{#each items as item (item.id)}` pattern
   * keys by album id. Scrolling changes which items are visible, which
   * changes the keys, which causes Svelte to destroy and remount card
   * components row by row. Under software compositing each
   * mount/destroy is expensive (subscribe/unsubscribe to stores, action
   * lifecycles, etc.) and the churn dominates scroll cost.
   *
   * Here, `{#each Array(maxPool) as _, slotIdx (slotIdx)}` uses a stable
   * slot index as the key. Slot 0 always exists; what *album* it points
   * to changes as `firstSlotRow` advances. Svelte never destroys the
   * slot — it just updates the inner snippet's bound props. No
   * remount, no churn.
   *
   * Phase B: `getItem` reads from a flat array of all loaded items.
   * Phase A (future): `getItem` reads from a chunked store, returns
   * null for missing chunks, and `onNeedIndex` triggers chunk fetches.
   * The pool is identical in both phases.
   */

  let containerEl: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(0);
  let containerWidth = $state(0);

  const rowStep = $derived(cardHeight + rowGap);
  const colStep = $derived(cardWidth + colGap);

  /** Columns that fit in the container at the current width. */
  const cols = $derived(
    containerWidth > 0
      ? Math.max(1, Math.floor((containerWidth + colGap) / colStep))
      : 1
  );

  const totalRows = $derived(Math.ceil(totalCount / cols));
  const totalHeight = $derived(totalRows * rowStep + bottomPadding);

  /** Buffer rows above + below the viewport so a fast scroll doesn't
   *  paint into blank slots before the bindings update. */
  const bufferRows = 3;

  /** Rows that fit on screen at once (rounded up). */
  const visibleRows = $derived(
    rowStep > 0 ? Math.ceil(viewportHeight / rowStep) : 0
  );

  /** Pool slot row capacity — stable for a given container size, so the
   *  pool only resizes on resize, never on scroll. */
  const slotRows = $derived(visibleRows + 2 * bufferRows);

  /** Pool slot capacity. Constant for a given (viewport, cols). */
  const maxPool = $derived(slotRows * cols);

  /** Index of the topmost row currently occupied by the pool. As the
   *  user scrolls, this advances; the pool re-binds its slots so each
   *  slot points at a new album, but the slot DOM stays mounted. */
  const firstSlotRow = $derived(
    Math.max(0, Math.floor(scrollTop / rowStep) - bufferRows)
  );

  function albumIndexFor(slotIdx: number): number {
    return firstSlotRow * cols + slotIdx;
  }

  function slotTopPx(slotIdx: number): number {
    const rowOffset = Math.floor(slotIdx / cols);
    return (firstSlotRow + rowOffset) * rowStep;
  }

  function slotLeftPx(slotIdx: number): number {
    const colOffset = slotIdx % cols;
    return colOffset * colStep;
  }

  function handleScroll(): void {
    if (containerEl) scrollTop = containerEl.scrollTop;
  }

  onMount(() => {
    if (!containerEl) return;
    viewportHeight = containerEl.clientHeight;
    containerWidth = containerEl.clientWidth;
    const ro = new ResizeObserver(() => {
      if (!containerEl) return;
      viewportHeight = containerEl.clientHeight;
      containerWidth = containerEl.clientWidth;
    });
    ro.observe(containerEl);
    return () => ro.disconnect();
  });

  /** Notify caller about needed indices so it can fetch chunks ahead
   *  of the slot binding to them. Fires when the slot window shifts.
   *  Phase B callers can ignore — `getItem` already returns data. */
  $effect(() => {
    if (!onNeedIndex) return;
    void firstSlotRow;
    void maxPool;
    void totalCount;
    untrack(() => {
      for (let i = 0; i < maxPool; i++) {
        const albumIdx = firstSlotRow * cols + i;
        if (albumIdx >= 0 && albumIdx < totalCount) {
          onNeedIndex(albumIdx);
        }
      }
    });
  });
</script>

<div
  class="grid-pool"
  bind:this={containerEl}
  onscroll={handleScroll}
>
  <div class="grid-spacer" style="height: {totalHeight}px;">
    {#each Array(maxPool) as _, slotIdx (slotIdx)}
      {@const albumIdx = albumIndexFor(slotIdx)}
      {@const inRange = albumIdx >= 0 && albumIdx < totalCount}
      {@const item = inRange ? (void dataVersion, getItem(albumIdx)) : null}
      <div
        class="grid-slot"
        style="top: {slotTopPx(slotIdx)}px; left: {slotLeftPx(slotIdx)}px; width: {cardWidth}px; display: {inRange ? 'block' : 'none'};"
      >
        {#if inRange}
          {#if item !== null}
            {@render renderCell(item, albumIdx)}
          {:else if renderPlaceholder}
            {@render renderPlaceholder(albumIdx)}
          {/if}
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .grid-pool {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    position: relative;
  }

  .grid-spacer {
    position: relative;
    width: 100%;
    /* Pre-allocated to `totalRows * rowStep + bottomPadding` so the
       native scrollbar shows the full extent of the collection even
       when only a chunk is materialized in the DOM. */
  }

  .grid-slot {
    position: absolute;
    /* No `will-change: transform` — under software compositing the
       extra GPU-layer hint costs more than it saves. */
  }
</style>
