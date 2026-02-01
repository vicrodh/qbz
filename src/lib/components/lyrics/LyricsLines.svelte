<script lang="ts">
  import { tick } from 'svelte';

  interface LyricsLine {
    text: string;
    timeMs?: number; // Optional timing for synced lyrics
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

  // Calculate line duration for CSS animation (immersive mode only)
  function getLineDuration(index: number): number {
    if (!isSynced || index < 0 || index >= lines.length) return 3000;

    const currentLine = lines[index];
    const nextLine = lines[index + 1];

    if (!currentLine?.timeMs) return 3000; // Default 3 seconds
    if (!nextLine?.timeMs) return 5000; // Last line, assume 5 seconds

    const duration = nextLine.timeMs - currentLine.timeMs;
    // Clamp between 1-10 seconds
    return Math.max(1000, Math.min(10000, duration));
  }

  // Track active line changes for animation reset
  let animationKey = $state(0);
  let lastActiveIndex = $state(-1);

  $effect(() => {
    if (activeIndex !== lastActiveIndex && activeIndex >= 0) {
      lastActiveIndex = activeIndex;
      animationKey++; // Force animation restart
    }
  });

  let container: HTMLDivElement | null = null;
  let lastScrolledIndex = -1;
  let lastLyricsKey = '';

  // In immersive mode, use CSS-only opacity via data attributes (no inline styles)
  // This avoids per-line style recalculation on every render
  function getDistanceClass(index: number, active: number): string {
    if (!dimInactive || active < 0) return '';
    if (index === active) return '';
    const distance = Math.abs(index - active);
    if (distance === 1) return 'distance-1';
    if (distance === 2) return 'distance-2';
    if (distance === 3) return 'distance-3';
    return 'distance-far';
  }

  // Only calculate inline opacity for non-immersive mode (karaoke needs precise values)
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

  // React to activeIndex changes - scroll to keep active line visible
  $effect(() => {
    if (!scrollToActive || activeIndex < 0 || !isSynced) return;
    if (activeIndex === lastScrolledIndex) return;

    // Determine scroll behavior
    const isLargeJump = lastScrolledIndex >= 0 && Math.abs(activeIndex - lastScrolledIndex) > 2;
    const isInitialSync = lastScrolledIndex === -1 && activeIndex > 0;
    const useInstant = isLargeJump || isInitialSync;

    lastScrolledIndex = activeIndex;
    scrollActiveIntoView(activeIndex, useInstant);
  });

  // Reset scroll tracking when lyrics change (new track)
  // Use first line text as key to detect actual content change, not just array reference
  $effect(() => {
    const newKey = lines.length > 0 ? `${lines.length}-${lines[0].text}` : '';
    if (newKey !== lastLyricsKey) {
      lastLyricsKey = newKey;
      lastScrolledIndex = -1;
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
      {#if immersive && isSynced && index === activeIndex}
        <!-- Active line in immersive mode: CSS animation with duration -->
        {#key animationKey}
          <div
            class="lyrics-line active {getDistanceClass(index, activeIndex)}"
            style="--line-duration: {getLineDuration(index)}ms"
            data-line-index={index}
          >
            <span class="line-text">{line.text}</span>
          </div>
        {/key}
      {:else}
        <div
          class="lyrics-line {immersive && isSynced ? getDistanceClass(index, activeIndex) : ''}"
          class:active={isSynced && index === activeIndex}
          class:past={isSynced && index < activeIndex}
          style={immersive ? '' : `--line-opacity: ${isSynced ? getLineOpacity(index, activeIndex) : 1}; ${isSynced && index === activeIndex ? `--line-progress: ${Math.max(0, Math.min(1, activeProgress))}` : ''}`}
          data-line-index={index}
        >
          <span class="line-text">{line.text}</span>
        </div>
      {/if}
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
    overflow-x: hidden;
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

  /* Immersive mode - larger text with Oswald font */
  /* Performance: uses CSS classes for opacity instead of inline styles */
  .lyrics-lines.immersive {
    gap: 20px;
    padding: 24px;
    /* Containment: isolate layout/paint to this subtree */
    contain: layout style;
  }

  .lyrics-lines.immersive .lyrics-line {
    font-family: 'Oswald', var(--font-sans), sans-serif;
    font-size: 28px;
    font-weight: 400;
    letter-spacing: 0.02em;
    /* Text shadow for contrast against any background */
    text-shadow:
      0 1px 2px rgba(0, 0, 0, 0.5),
      0 2px 8px rgba(0, 0, 0, 0.3);
    /* Remove expensive transitions in immersive mode */
    transition: opacity 200ms ease-out, color 200ms ease-out;
    /* Containment per line */
    contain: layout style;
  }

  /* Distance-based opacity classes (CSS-only, no inline styles) */
  .lyrics-lines.immersive .lyrics-line.distance-1 {
    opacity: 0.5;
  }
  .lyrics-lines.immersive .lyrics-line.distance-2 {
    opacity: 0.35;
  }
  .lyrics-lines.immersive .lyrics-line.distance-3 {
    opacity: 0.25;
  }
  .lyrics-lines.immersive .lyrics-line.distance-far {
    opacity: 0.15;
  }

  .lyrics-lines.immersive .lyrics-line.active {
    font-size: 30px;
    font-weight: 600;
    color: #ffffff !important;
    opacity: 1;
  }

  /* CSS-only karaoke animation in immersive mode */
  /* Uses pseudo-element underline sweep - keeps text bright white */
  .lyrics-lines.immersive .lyrics-line.active .line-text {
    --duration: var(--line-duration, 3000ms);
    position: relative;
    display: inline-block;
    color: #ffffff;
  }

  /* Glowing underline that sweeps left to right */
  .lyrics-lines.immersive .lyrics-line.active .line-text::after {
    content: '';
    position: absolute;
    bottom: -2px;
    left: 0;
    height: 3px;
    width: 0%;
    background: linear-gradient(
      90deg,
      transparent 0%,
      #a78bfa 20%,
      #c4b5fd 50%,
      #a78bfa 80%,
      transparent 100%
    );
    border-radius: 2px;
    box-shadow:
      0 0 8px #a78bfa,
      0 0 16px rgba(167, 139, 250, 0.5);
    animation: karaoke-underline var(--duration) linear forwards;
  }

  @keyframes karaoke-underline {
    0% {
      width: 0%;
    }
    100% {
      width: 100%;
    }
  }

  /* Past lines in immersive should be clearly dimmer than active */
  .lyrics-lines.immersive .lyrics-line.past {
    color: rgba(255, 255, 255, 0.35);
    font-weight: 400;
  }

  .lyrics-line {
    color: var(--text-secondary);
    font-family: var(--font-sans);
    font-size: 16px;
    font-weight: 500;
    line-height: 1.5;
    letter-spacing: 0.01em;
    opacity: var(--line-opacity, 1);
    /* Minimal transitions for non-immersive mode */
    transition: opacity 200ms ease-out, color 200ms ease-out;
    transform-origin: left center;
    /* Prevent horizontal overflow with long lyrics */
    word-wrap: break-word;
    overflow-wrap: break-word;
  }

  /* Only active line gets full transitions */
  .lyrics-line.active {
    transition:
      opacity 200ms ease-out,
      transform 200ms ease-out,
      font-size 150ms ease-out,
      color 250ms ease-out;
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
      0 1px 3px rgba(0, 0, 0, 0.6),
      0 2px 10px rgba(0, 0, 0, 0.4),
      0 0 40px rgba(99, 102, 241, 0.4),
      0 0 80px rgba(99, 102, 241, 0.2);
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
