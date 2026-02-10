<script lang="ts">
  import { ChevronLeft, ChevronRight } from 'lucide-svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    title?: string;
    header?: Snippet;
    children: Snippet;
  }

  let { title, header, children }: Props = $props();

  let scrollContainer: HTMLDivElement;

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
  <div class="scroll-container hide-scrollbar" bind:this={scrollContainer}>
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
    gap: 15px;
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
  }

  .content {
    display: flex;
    gap: 16px;
    width: max-content;
  }
</style>
