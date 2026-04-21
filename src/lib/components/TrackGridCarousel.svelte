<script lang="ts">
  import { ChevronLeft, ChevronRight } from 'lucide-svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    title?: string;
    children: Snippet;
    rows?: number;
    gap?: number;
  }

  let { title, children, rows = 3, gap = 12 }: Props = $props();

  let viewport: HTMLDivElement;
  let columnWidth = $state(240);
  let visibleColumns = $state(4);

  // Responsive breakpoints: how many columns fit in the current viewport.
  function columnsFor(width: number): number {
    if (width >= 1100) return 4;
    if (width >= 820) return 3;
    if (width >= 540) return 2;
    return 1;
  }

  function measure() {
    if (!viewport) return;
    const w = viewport.clientWidth;
    const cols = columnsFor(w);
    visibleColumns = cols;
    columnWidth = Math.floor((w - gap * (cols - 1)) / cols);
  }

  function scrollBy(direction: -1 | 1) {
    if (!viewport) return;
    viewport.scrollBy({ left: direction * (columnWidth + gap), behavior: 'smooth' });
  }

  $effect(() => {
    if (!viewport) return;
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(viewport);
    return () => ro.disconnect();
  });
</script>

<section class="track-grid-section">
  {#if title}
    <div class="track-grid-header">
      <h2 class="track-grid-title">{title}</h2>
      <div class="track-grid-nav">
        <button class="track-grid-nav-btn" onclick={() => scrollBy(-1)} aria-label="Previous">
          <ChevronLeft size={20} />
        </button>
        <button class="track-grid-nav-btn" onclick={() => scrollBy(1)} aria-label="Next">
          <ChevronRight size={20} />
        </button>
      </div>
    </div>
  {/if}

  <div
    class="track-grid-viewport hide-scrollbar"
    bind:this={viewport}
    style="--col-width: {columnWidth}px; --col-gap: {gap}px; --rows: {rows};"
  >
    <div class="track-grid">
      {@render children()}
    </div>
  </div>
</section>

<style>
  .track-grid-section {
    margin-bottom: 32px;
  }

  .track-grid-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .track-grid-title {
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .track-grid-nav {
    display: flex;
    gap: 8px;
  }

  .track-grid-nav-btn {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    transition: color 150ms ease, background-color 150ms ease;
  }

  .track-grid-nav-btn:hover {
    color: var(--text-primary);
    background-color: var(--bg-hover);
  }

  .track-grid-viewport {
    overflow-x: auto;
    overflow-y: hidden;
    scroll-snap-type: x proximity;
  }

  .track-grid {
    display: grid;
    grid-template-rows: repeat(var(--rows), auto);
    grid-auto-flow: column;
    grid-auto-columns: var(--col-width);
    column-gap: var(--col-gap);
    row-gap: 4px;
  }

  .track-grid :global(> *) {
    scroll-snap-align: start;
  }

  .hide-scrollbar {
    scrollbar-width: none;
  }

  .hide-scrollbar::-webkit-scrollbar {
    display: none;
  }
</style>
