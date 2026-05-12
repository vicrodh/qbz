<script lang="ts">
  import { tick } from 'svelte';
  import { t } from 'svelte-i18n';
  import type {
    LyricsFont,
    LyricsFontSize,
    LyricsDimming
  } from '$lib/stores/lyricsDisplayStore';

  interface LyricsLine {
    text: string;
    timeMs?: number; // Optional timing for synced lyrics
    endMs?: number; // Optional end-of-vocal marker (LRC gap markers)
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
    fontMode?: LyricsFont;
    fontSizeMode?: LyricsFontSize;
    dimmingMode?: LyricsDimming;
    activeColor?: string;
    uppercase?: boolean;
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
    isSynced = false,
    fontMode,
    fontSizeMode,
    dimmingMode,
    activeColor,
    uppercase = false
  }: Props = $props();

  // Sung duration for a line. Uses endMs (LRC gap marker → end of vocal)
  // when available, otherwise next line's start, otherwise a 5s default.
  function getLineDuration(index: number): number {
    if (!isSynced || index < 0 || index >= lines.length) return 3000;

    const currentLine = lines[index];
    const nextLine = lines[index + 1];

    if (!currentLine?.timeMs) return 3000;
    const bound = currentLine.endMs ?? nextLine?.timeMs;
    if (!bound) return 5000;

    const duration = bound - currentLine.timeMs;
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

    // Sidebar dimming override (only when dimmingMode is provided)
    if (dimmingMode === 'off') return 1;
    if (dimmingMode === 'soft') return 0.6;
    // 'strong' (or undefined) falls through to the existing ladder

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
  class:size-small={compact && fontSizeMode === 'small'}
  class:size-medium={compact && fontSizeMode === 'medium'}
  class:size-large={compact && fontSizeMode === 'large'}
  class:size-xl={compact && fontSizeMode === 'xl'}
  class:font-line-seed-jp={compact && fontMode === 'line-seed-jp'}
  class:font-montserrat={compact && fontMode === 'montserrat'}
  class:font-noto-sans={compact && fontMode === 'noto-sans'}
  class:font-source-sans-3={compact && fontMode === 'source-sans-3'}
  class:uppercase={compact && uppercase}
  style:--lyrics-active-color={compact && activeColor ? activeColor : null}
  bind:this={container}
>
  {#if lines.length === 0}
    <div class="lyrics-empty">{$t('player.noLyrics')}</div>
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

  /* Immersive mode: hide scrollbar but keep scrolling */
  .lyrics-lines.immersive {
    scrollbar-width: none;
  }

  .lyrics-lines.immersive::-webkit-scrollbar {
    display: none;
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
    gap: clamp(18px, 2.5vh, 30px);
    padding: 16px 24px;
    /* Containment: isolate layout/paint to this subtree */
    contain: layout style;
  }

  .lyrics-lines.immersive .lyrics-line {
    font-family: 'Montserrat', var(--font-sans), sans-serif;
    font-size: clamp(24px, 2.6vw, 34px);
    font-weight: 500;
    line-height: 1.35;
    letter-spacing: 0.01em;
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
    font-size: clamp(28px, 3.2vw, 42px);
    font-weight: 700;
    color: #ffffff !important;
    opacity: 1;
  }

  /* Immersive mode: simple bright white text, no animation */
  .lyrics-lines.immersive .lyrics-line.active .line-text {
    color: #ffffff !important;
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
    /* Use filter: drop-shadow on the parent — text-shadow inherits into the
       background-clipped span and tints the gradient on WebKit (macOS). */
    filter:
      drop-shadow(0 1px 3px rgba(0, 0, 0, 0.6))
      drop-shadow(0 0 12px color-mix(in srgb, var(--lyrics-active-color, var(--accent-primary)) 35%, transparent));
  }

  .lyrics-lines.center .lyrics-line.active {
    transform: scale(1.05);
  }

  /* Shimmer karaoke effect on active line */
  .lyrics-line.active .line-text {
    --progress-pos: calc(var(--line-progress, 0) * 100%);
    background: linear-gradient(
      90deg,
      var(--lyrics-active-color, var(--accent-primary)) 0%,
      var(--lyrics-active-color, var(--accent-primary)) var(--progress-pos),
      var(--text-primary) var(--progress-pos),
      var(--text-primary) 100%
    );
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    -webkit-text-fill-color: transparent;
    /* Suppress inherited text-shadow — on WebKit (macOS) shadows render in
       the text-glyph shape and tint the gradient-clipped fill, making the
       active line look dim/red instead of crisp. */
    text-shadow: none;
  }

  .lyrics-empty {
    color: var(--text-muted);
    font-size: 14px;
    text-align: center;
    padding: 48px 0;
  }

  /* Sidebar font size overrides (compact mode only) */
  .lyrics-lines.compact.size-small .lyrics-line {
    font-size: 13px;
  }
  .lyrics-lines.compact.size-small .lyrics-line.active {
    font-size: 15px;
  }
  .lyrics-lines.compact.size-medium .lyrics-line {
    font-size: 15px;
  }
  .lyrics-lines.compact.size-medium .lyrics-line.active {
    font-size: 17px;
  }
  .lyrics-lines.compact.size-large .lyrics-line {
    font-size: 18px;
  }
  .lyrics-lines.compact.size-large .lyrics-line.active {
    font-size: 21px;
  }
  .lyrics-lines.compact.size-xl .lyrics-line {
    font-size: 22px;
  }
  .lyrics-lines.compact.size-xl .lyrics-line.active {
    font-size: 26px;
  }

  /* Sidebar uppercase override (compact mode only) */
  .lyrics-lines.compact.uppercase .lyrics-line {
    text-transform: uppercase;
  }

  /* Sidebar font family overrides (compact mode only) */
  .lyrics-lines.compact.font-line-seed-jp .lyrics-line {
    font-family: 'LINE Seed JP', -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  }
  .lyrics-lines.compact.font-montserrat .lyrics-line {
    font-family: 'Montserrat', -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  }
  .lyrics-lines.compact.font-noto-sans .lyrics-line {
    font-family: 'Noto Sans', -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  }
  .lyrics-lines.compact.font-source-sans-3 .lyrics-line {
    font-family: 'Source Sans 3', -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
  }
</style>
