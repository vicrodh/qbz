<script lang="ts">
  import { tick } from 'svelte';

  interface LyricsLine {
    text: string;
  }

  interface Props {
    lines: LyricsLine[];
    activeIndex?: number;
    activeProgress?: number;
    dimInactive?: boolean;
    center?: boolean;
    compact?: boolean;
    scrollToActive?: boolean;
    immersive?: boolean;
  }

  let {
    lines,
    activeIndex = -1,
    activeProgress = 0,
    dimInactive = true,
    center = false,
    compact = false,
    scrollToActive = true,
    immersive = false
  }: Props = $props();

  let container: HTMLDivElement | null = null;
  let prevActiveIndex = -1;

  // Calculate opacity based on distance from active line
  function getLineOpacity(index: number, active: number): number {
    if (!dimInactive || active < 0) return 1;
    if (index === active) return 1;

    const distance = Math.abs(index - active);
    if (distance === 1) return 0.5;
    if (distance === 2) return 0.35;
    if (distance === 3) return 0.25;
    return 0.15;
  }

  // Scroll active line into view (centered)
  async function scrollActiveIntoView(index: number) {
    if (!container || index < 0) return;

    await tick();

    const target = container.querySelector<HTMLElement>(`[data-line-index="${index}"]`);
    if (!target) return;

    const containerRect = container.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    const targetCenter = targetRect.top + targetRect.height / 2;
    const containerCenter = containerRect.top + containerRect.height / 2;
    const scrollOffset = targetCenter - containerCenter;

    container.scrollBy({
      top: scrollOffset,
      behavior: 'smooth'
    });
  }

  // React to activeIndex changes - always scroll when it changes
  $effect(() => {
    if (scrollToActive && activeIndex >= 0 && activeIndex !== prevActiveIndex) {
      prevActiveIndex = activeIndex;
      scrollActiveIntoView(activeIndex);
    }
  });
</script>

<div
  class="lyrics-lines"
  class:compact
  class:center
  class:immersive
  bind:this={container}
>
  {#if lines.length === 0}
    <div class="lyrics-empty">No lyrics available</div>
  {:else}
    <!-- Spacer at top to allow first lines to scroll to center -->
    <div class="lyrics-spacer"></div>

    {#each lines as line, index}
      {@const isActive = index === activeIndex}
      {@const isPast = index < activeIndex}
      {@const opacity = getLineOpacity(index, activeIndex)}
      <div
        class="lyrics-line"
        class:active={isActive}
        class:past={isPast}
        style="--line-opacity: {opacity}; {isActive ? `--line-progress: ${Math.max(0, Math.min(1, activeProgress))}` : ''}"
        data-line-index={index}
      >
        <span class="line-text">{line.text}</span>
      </div>
    {/each}

    <!-- Spacer at bottom to allow last lines to scroll to center -->
    <div class="lyrics-spacer"></div>
  {/if}
</div>

<style>
  .lyrics-lines {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 16px 20px;
    overflow-y: auto;
    height: 100%;
    scroll-behavior: smooth;
    scrollbar-width: thin;
    scrollbar-color: var(--bg-tertiary) transparent;
  }

  .lyrics-lines::-webkit-scrollbar {
    width: 6px;
  }

  .lyrics-lines::-webkit-scrollbar-track {
    background: transparent;
  }

  .lyrics-lines::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .lyrics-spacer {
    min-height: 40vh;
    flex-shrink: 0;
  }

  .lyrics-lines.center {
    text-align: center;
  }

  .lyrics-lines.compact {
    gap: 12px;
  }

  .lyrics-lines.compact .lyrics-line {
    font-size: 15px;
  }

  .lyrics-lines.compact .lyrics-line.active {
    font-size: 17px;
  }

  /* Immersive mode - larger text, center aligned */
  .lyrics-lines.immersive {
    gap: 20px;
    padding: 24px;
  }

  .lyrics-lines.immersive .lyrics-line {
    font-size: 22px;
    font-weight: 500;
  }

  .lyrics-lines.immersive .lyrics-line.active {
    font-size: 28px;
    font-weight: 700;
  }

  .lyrics-line {
    color: var(--text-secondary);
    font-family: var(--font-sans);
    font-size: 16px;
    font-weight: 500;
    line-height: 1.5;
    letter-spacing: 0.01em;
    opacity: var(--line-opacity, 1);
    transition:
      opacity 400ms cubic-bezier(0.4, 0, 0.2, 1),
      transform 400ms cubic-bezier(0.4, 0, 0.2, 1),
      font-size 300ms cubic-bezier(0.4, 0, 0.2, 1),
      font-weight 300ms cubic-bezier(0.4, 0, 0.2, 1),
      color 300ms cubic-bezier(0.4, 0, 0.2, 1);
    transform-origin: left center;
  }

  .lyrics-lines.center .lyrics-line {
    transform-origin: center center;
  }

  .lyrics-line.past {
    color: var(--text-muted);
  }

  .lyrics-line.active {
    color: var(--text-primary);
    font-size: 20px;
    font-weight: 700;
    opacity: 1;
    transform: scale(1.02);
    text-shadow: 0 0 30px rgba(255, 255, 255, 0.15);
  }

  .lyrics-lines.center .lyrics-line.active {
    transform: scale(1.05);
  }

  /* Karaoke progress effect on active line */
  .lyrics-line.active .line-text {
    background: linear-gradient(
      90deg,
      var(--accent-primary) calc(var(--line-progress, 0) * 100%),
      var(--text-primary) calc(var(--line-progress, 0) * 100%)
    );
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }

  .lyrics-empty {
    color: var(--text-muted);
    font-size: 14px;
    text-align: center;
    padding: 48px 0;
  }
</style>
