<script lang="ts">
  import { ChevronLeft, ChevronRight } from 'lucide-svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    title?: string;
    header?: Snippet;
    children: Snippet;
  }

  let { title, header, children }: Props = $props();

  const DRAG_THRESHOLD_PX = 5;

  let scrollContainer: HTMLDivElement;
  let isDragging = $state(false);
  let pointerIsDown = false;
  let activePointerId = -1;
  let dragStartX = 0;
  let dragStartScroll = 0;
  let dragDistance = 0;

  function scroll(direction: 'left' | 'right') {
    if (scrollContainer) {
      const scrollAmount = 600;
      const currentScroll = scrollContainer.scrollLeft;
      const targetScroll = direction === 'left'
        ? currentScroll - scrollAmount
        : currentScroll + scrollAmount;

      scrollContainer.scrollTo({
        left: targetScroll,
        behavior: 'smooth'
      });
    }
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    // Arm drag detection but do NOT setPointerCapture yet. Capturing here
    // redirects the subsequent click away from the card under the pointer,
    // which breaks card/play-button clicks inside carousels (issue #323).
    pointerIsDown = true;
    activePointerId = e.pointerId;
    dragStartX = e.clientX;
    dragStartScroll = scrollContainer.scrollLeft;
    dragDistance = 0;
    isDragging = false;
  }

  function onPointerMove(e: PointerEvent) {
    if (!pointerIsDown || e.pointerId !== activePointerId) return;
    const dx = e.clientX - dragStartX;
    dragDistance = Math.abs(dx);
    if (!isDragging && dragDistance > DRAG_THRESHOLD_PX) {
      // Cross the threshold → now it's a real drag. Capture the pointer so
      // we keep getting move events even if the cursor leaves the container.
      isDragging = true;
      try {
        scrollContainer.setPointerCapture(e.pointerId);
      } catch {
        // Some pointer types (e.g. synthesised mouse on old WebKit) may
        // reject capture — continue tracking via bubble events.
      }
    }
    if (isDragging) {
      scrollContainer.scrollLeft = dragStartScroll - dx;
    }
  }

  function onPointerUp(e: PointerEvent) {
    if (!pointerIsDown) return;
    pointerIsDown = false;
    if (isDragging) {
      isDragging = false;
      try {
        scrollContainer.releasePointerCapture(e.pointerId);
      } catch {
        // ignore
      }
    }
    activePointerId = -1;
  }

  function onClickCapture(e: MouseEvent) {
    // Only swallow the click that would otherwise follow a real drag.
    if (dragDistance > DRAG_THRESHOLD_PX) {
      e.preventDefault();
      e.stopPropagation();
    }
    // Reset so the next unrelated click passes through cleanly.
    dragDistance = 0;
  }

  const hasHeader = $derived(!!title || !!header);
</script>

<div class="scroll-row">
  <!-- Section Header -->
  {#if hasHeader}
    <div class="header">
      {#if header}
        <div class="header-content">
          {@render header()}
        </div>
      {:else if title}
        <h2 class="title">{title}</h2>
      {/if}
      <div class="nav-buttons">
        <button onclick={() => scroll('left')} class="nav-btn">
          <ChevronLeft size={24} />
        </button>
        <button onclick={() => scroll('right')} class="nav-btn">
          <ChevronRight size={24} />
        </button>
      </div>
    </div>
  {/if}

  <!-- Horizontal Scroll Container -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="scroll-container hide-scrollbar"
    class:dragging={isDragging}
    bind:this={scrollContainer}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    onclickcapture={onClickCapture}
  >
    <div class="content">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .scroll-row {
    margin-bottom: 32px;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  .header-content {
    display: flex;
    align-items: center;
    gap: 16px;
    flex: 1;
    min-width: 0;
  }

  .title {
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .nav-buttons {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .nav-btn {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: #666666;
    cursor: pointer;
    transition: color 150ms ease;
  }

  .nav-btn:hover {
    color: var(--text-primary);
  }

  .scroll-container {
    overflow-x: auto;
    overflow-y: hidden;
    cursor: grab;
  }

  .scroll-container.dragging {
    cursor: grabbing;
    user-select: none;
  }

  .content {
    display: flex;
    align-items: flex-start;
    gap: 22px;
    width: max-content;
  }
</style>
