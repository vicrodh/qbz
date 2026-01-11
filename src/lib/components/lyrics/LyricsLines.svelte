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
    isSynced?: boolean;
  }

  let {
    lines,
    activeIndex = -1,
    activeProgress = 0,
    dimInactive = true,
    center = false,
    compact = false,
    scrollToActive = true,
    immersive = false,
    isSynced = false
  }: Props = $props();

  let container: HTMLDivElement | null = null;
  let prevActiveIndex = $state(-1);
  let hasInitialSync = $state(false);

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
  // instant: true for catch-up sync, false for normal progression
  async function scrollActiveIntoView(index: number, instant: boolean = false) {
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
      behavior: instant ? 'instant' : 'smooth'
    });
  }

  // React to activeIndex changes
  $effect(() => {
    if (scrollToActive && activeIndex >= 0 && activeIndex !== prevActiveIndex) {
      // Determine if we need instant scroll (catch-up sync)
      const needsInstantSync = !hasInitialSync && activeIndex > 0;
      const isLargeJump = prevActiveIndex >= 0 && Math.abs(activeIndex - prevActiveIndex) > 2;
      const useInstant = needsInstantSync || isLargeJump;

      console.log('[LyricsLines] Scrolling to line', activeIndex, { prevActiveIndex, useInstant, hasInitialSync });

      if (!hasInitialSync && activeIndex >= 0) {
        hasInitialSync = true;
      }

      prevActiveIndex = activeIndex;
      scrollActiveIntoView(activeIndex, useInstant);
    }
  });

  // Reset initial sync flag when lines change (new track)
  $effect(() => {
    if (lines.length > 0) {
      // New lyrics loaded - reset sync state
      hasInitialSync = false;
      prevActiveIndex = -1;
    }
  });
</script>

<div
  class="lyrics-lines"
  class:compact
  class:center
  class:immersive
  class:static={!isSynced}
  bind:this={container}
>
  {#if lines.length === 0}
    <div class="lyrics-empty">No lyrics available</div>
  {:else}
    <!-- Spacer at top to allow first lines to scroll to center (only for synced) -->
    {#if isSynced}
      <div class="lyrics-spacer"></div>
    {/if}

    {#each lines as line, index (index)}
      <div
        class="lyrics-line"
        class:active={isSynced && index === activeIndex}
        class:past={isSynced && index < activeIndex}
        style="--line-opacity: {isSynced ? getLineOpacity(index, activeIndex) : 1}; {isSynced && index === activeIndex ? `--line-progress: ${Math.max(0, Math.min(1, activeProgress))}` : ''}"
        data-line-index={index}
      >
        <span class="line-text">{line.text}</span>
      </div>
    {/each}

    <!-- Spacer at bottom to allow last lines to scroll to center (only for synced) -->
    {#if isSynced}
      <div class="lyrics-spacer"></div>
    {/if}
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

  /* Static mode - non-synced lyrics, start at top */
  .lyrics-lines.static {
    justify-content: flex-start;
  }

  .lyrics-lines.static .lyrics-line {
    opacity: 0.85;
    color: var(--text-primary);
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
    font-size: 24px;
    font-weight: 500;
  }

  .lyrics-lines.immersive .lyrics-line.active {
    font-size: 30px;
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
      opacity 200ms ease-out,
      transform 200ms ease-out,
      font-size 150ms ease-out,
      font-weight 150ms ease-out,
      color 250ms ease-out;
    transform-origin: left center;
    will-change: opacity, transform;
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
    text-shadow:
      0 0 40px rgba(99, 102, 241, 0.3),
      0 0 80px rgba(99, 102, 241, 0.15);
  }

  .lyrics-lines.center .lyrics-line.active {
    transform: scale(1.05);
  }

  /* Shimmer karaoke effect on active line */
  .lyrics-line.active .line-text {
    --progress-pos: calc(var(--line-progress, 0) * 100%);
    background: linear-gradient(
      90deg,
      #a78bfa 0%,
      #a78bfa var(--progress-pos),
      var(--text-primary) var(--progress-pos),
      var(--text-primary) 100%
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
