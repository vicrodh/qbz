<script lang="ts" generics="T">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import { t } from '$lib/i18n';
  import { ArrowRight, ChevronLeft, ChevronRight } from 'lucide-svelte';

  interface Props<T> {
    title: string;
    items: T[];
    /** Number of columns at the default viewport. Used as the fixed
     *  column count for the CSS grid template. */
    columns?: number;
    /** Number of rows per page. itemsPerPage = columns * rows. */
    rows?: number;
    onSeeAll?: () => void;
    /** Receives `(item, globalIndex)` where globalIndex = page * itemsPerPage
     *  + local index. Snippets that ignore the second arg work too. */
    renderItem: Snippet<[T, number]>;
  }

  let {
    title,
    items,
    columns = 4,
    rows = 3,
    onSeeAll,
    renderItem,
  }: Props<T> = $props();

  /**
   * Compact-grid variant of DiscoverySection. Fixed-cell pagination:
   *   itemsPerPage = columns × rows (default 4 × 3 = 12).
   *
   * Unlike the album row pagination (which adapts itemsPerPage to the
   * viewport width), this grid is deliberately constant — the compact
   * track entries don't benefit from wider viewports the way 220px
   * album cards do, and a fixed 4×3 grid keeps the visual consistent
   * across FullHD and 4K. CSS grid handles the layout via
   * `grid-template-columns: repeat(N, 1fr)`.
   */
  let containerEl: HTMLDivElement | undefined = $state();
  let page = $state(0);

  const itemsPerPage = $derived(columns * rows);
  const totalPages = $derived(Math.max(1, Math.ceil(items.length / itemsPerPage)));
  const canPrev = $derived(page > 0);
  const canNext = $derived(page < totalPages - 1);
  const visibleItems = $derived(
    items.slice(page * itemsPerPage, (page + 1) * itemsPerPage)
  );

  // Re-clamp if items array shrinks below current page.
  $effect(() => {
    void items.length;
    if (page > totalPages - 1) page = Math.max(0, totalPages - 1);
  });

  // Drag-to-paginate (same gesture as DiscoverySection).
  const DRAG_THRESHOLD_PX = 5;
  const PAGE_COMMIT_PX = 60;
  let pointerIsDown = false;
  let activePointerId = -1;
  let dragStartX = 0;
  let dragDistance = 0;

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    pointerIsDown = true;
    activePointerId = e.pointerId;
    dragStartX = e.clientX;
    dragDistance = 0;
  }

  function onPointerMove(e: PointerEvent) {
    if (!pointerIsDown || e.pointerId !== activePointerId) return;
    dragDistance = e.clientX - dragStartX;
  }

  function onPointerUp(e: PointerEvent) {
    if (!pointerIsDown) return;
    pointerIsDown = false;
    activePointerId = -1;
    if (Math.abs(dragDistance) >= PAGE_COMMIT_PX) {
      if (dragDistance < 0 && canNext) page = page + 1;
      else if (dragDistance > 0 && canPrev) page = page - 1;
    }
  }

  function onClickCapture(e: MouseEvent) {
    if (Math.abs(dragDistance) > DRAG_THRESHOLD_PX) {
      e.preventDefault();
      e.stopPropagation();
    }
    dragDistance = 0;
  }
</script>

<section class="section">
  <header class="head">
    <h2 class="title">{title}</h2>
    <div class="actions">
      {#if onSeeAll}
        <button class="see-all" type="button" onclick={onSeeAll}>
          {$t('discovery.seeAll')}
          <ArrowRight size={14} />
        </button>
      {/if}
      {#if totalPages > 1}
        <button
          class="nav-btn"
          type="button"
          aria-label="Previous page"
          disabled={!canPrev}
          onclick={() => { if (canPrev) page = page - 1; }}
        >
          <ChevronLeft size={18} />
        </button>
        <button
          class="nav-btn"
          type="button"
          aria-label="Next page"
          disabled={!canNext}
          onclick={() => { if (canNext) page = page + 1; }}
        >
          <ChevronRight size={18} />
        </button>
      {/if}
    </div>
  </header>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="grid-outer"
    bind:this={containerEl}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    onclickcapture={onClickCapture}
  >
    {#key page}
      <div
        class="grid"
        style="grid-template-columns: repeat({columns}, 1fr)"
        in:fade={{ duration: 120 }}
      >
        {#each visibleItems as item, idx (idx)}
          {@render renderItem(item, page * itemsPerPage + idx)}
        {/each}
      </div>
    {/key}
  </div>
</section>

<style>
  .section {
    margin-bottom: 48px;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 12px;
  }

  .title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .see-all {
    display: flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    padding: 4px 8px;
    font-family: inherit;
    margin-right: 4px;
  }

  .nav-btn {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: none;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .nav-btn:disabled {
    opacity: 0.4;
    cursor: default;
    color: var(--text-muted);
  }

  .grid-outer {
    position: relative;
    width: 100%;
    touch-action: pan-y;
    cursor: grab;
  }

  .grid-outer:active {
    cursor: grabbing;
  }

  .grid {
    display: grid;
    gap: 8px 16px;
    user-select: none;
    -webkit-user-select: none;
  }
</style>
