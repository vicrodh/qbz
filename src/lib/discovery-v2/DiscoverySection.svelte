<script lang="ts" generics="T">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import { t } from '$lib/i18n';
  import { ArrowRight, ChevronLeft, ChevronRight } from 'lucide-svelte';

  interface Props<T> {
    title: string;
    items: T[];
    /** Approximate card width in CSS px including its own gap allowance. Drives
     *  `itemsPerPage = floor((containerWidth + gap) / (cardWidth + gap))`. Pass
     *  170 for circular artist tiles, 220 (default) for album/track/playlist. */
    cardWidth?: number;
    onSeeAll?: () => void;
    renderItem: Snippet<[T]>;
  }

  let {
    title,
    items,
    cardWidth = 220,
    onSeeAll,
    renderItem,
  }: Props<T> = $props();

  /**
   * Pagination by DOM slice. Only the current page's items are mounted; the
   * section is NOT a horizontal scroll container — there's no `overflow-x:
   * auto`, no transform, no extra backing surface under software compositing.
   * Chevrons advance the page; the visible cards slice updates instantly.
   *
   * The N cards per page is computed responsively from the container width,
   * so the same section renders 4 cards at 1280px and 11+ at 4K maximize.
   */
  let containerEl: HTMLDivElement | undefined = $state();
  let page = $state(0);
  let itemsPerPage = $state(1);
  /** Inter-card horizontal gap inside .row. Kept in JS for the
   *  itemsPerPage math; the matching CSS lives in .row's `gap` rule below. */
  const gap = 32;

  // Drag-to-paginate. We commit a page change on pointerup if the cursor
  // moved past PAGE_COMMIT_PX. The OS-native click that would otherwise
  // fire on a card under the cursor gets swallowed in capture phase when
  // a real drag happened (dragDistance > DRAG_THRESHOLD_PX). Cero efecto
  // visual durante el drag — no transform, no scrollLeft animation; the
  // page swap on release carries the smoothness via the fade transition.
  const DRAG_THRESHOLD_PX = 5;
  const PAGE_COMMIT_PX = 60;
  let pointerIsDown = false;
  let activePointerId = -1;
  let dragStartX = 0;
  let dragDistance = 0;

  function recompute() {
    if (!containerEl) return;
    const width = containerEl.clientWidth;
    if (width <= 0) return;
    itemsPerPage = Math.max(1, Math.floor((width + gap) / (cardWidth + gap)));
    const maxPage = Math.max(0, Math.ceil(items.length / itemsPerPage) - 1);
    if (page > maxPage) page = maxPage;
  }

  onMount(() => {
    recompute();
    if (!containerEl) return;
    const ro = new ResizeObserver(recompute);
    ro.observe(containerEl);
    return () => ro.disconnect();
  });

  // Re-compute when items array changes (length affects maxPage clamp).
  $effect(() => {
    void items.length;
    recompute();
  });

  const totalPages = $derived(Math.max(1, Math.ceil(items.length / itemsPerPage)));
  const canPrev = $derived(page > 0);
  const canNext = $derived(page < totalPages - 1);
  const visibleItems = $derived(
    items.slice(page * itemsPerPage, (page + 1) * itemsPerPage)
  );

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
    // A drag-then-release should not also click a card. Swallow the click
    // in capture phase before the card's onclick handler sees it.
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
    </div>
  </header>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="row-outer"
    bind:this={containerEl}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    onclickcapture={onClickCapture}
  >
    {#key page}
      <div class="row" in:fade={{ duration: 120 }}>
        {#each visibleItems as item, idx (idx)}
          {@render renderItem(item)}
        {/each}
      </div>
    {/key}
  </div>
</section>

<style>
  /* Pagination-by-slice. The .row is a plain flex container — no overflow,
     no transform, no scroll. Only `itemsPerPage` cards are mounted at any
     moment; chevrons swap which slice is rendered. */
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

  /* Outer wraps the row + receives the ResizeObserver and the pointer
     handlers. The inner .row holds the actual cards and gets fresh DOM
     on every page change via `{#key page}`, with a brief opacity fade-in
     for smoothness. The fade is opacity-only (no transform, no filter)
     so its paint cost stays trivial under software compositing. */
  .row-outer {
    position: relative;
    width: 100%;
    touch-action: pan-y;
    cursor: grab;
  }

  .row-outer:active {
    cursor: grabbing;
  }

  .row {
    display: flex;
    gap: 32px;
  }
</style>
