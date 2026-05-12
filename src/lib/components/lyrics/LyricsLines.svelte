<script lang="ts">
  import { tick, untrack } from 'svelte';
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

  // Dynamic block measurement.
  //   measuredBlockMs    — wall-clock gap between two notifications. Drives
  //                        the CSS transition duration so each block's
  //                        animation matches its real audio duration (no
  //                        pauses, no accumulated lag).
  //   measuredBlockDelta — progress increment per notification. Used as a
  //                        lookahead so the visual leads the audio by one
  //                        block — by the time the transition completes,
  //                        audio has caught up to where we transitioned
  //                        to. Net: zero visible lag, while still
  //                        correcting against actual audio on every tick.
  let measuredBlockMs = $state(175);
  let measuredBlockDelta = $state(0);
  // Lookahead is suppressed on the first notification of a new line: at
  // that point we have no measurement of this line's cadence yet, and
  // adding a stale prior-line delta would push the visual into the line
  // before it should be. Flipped on as soon as we've observed one block.
  let lookaheadEnabled = $state(false);
  let prevBlockTime = 0;
  let prevBlockProgress = 0;
  let prevBlockIndex = -1;

  // $effect.pre — runs BEFORE the DOM update for the prop change that
  // triggered it. With a plain $effect, on a line transition the derived
  // below would first paint with stale (previous-line) lookahead state,
  // and only then the effect would reset it — causing a brief flash at the
  // stale position followed by a backward transition to 0. Running pre-DOM
  // means the first paint of the new line already sees lookaheadEnabled =
  // false and the snapshot is correct.
  $effect.pre(() => {
    const idx = activeIndex;
    const progress = activeProgress;

    if (!isSynced || idx < 0) {
      prevBlockTime = 0;
      prevBlockIndex = -1;
      lookaheadEnabled = false;
      return;
    }

    const now = performance.now();

    if (idx !== prevBlockIndex) {
      // Line transition: seed transition duration from line duration
      // (we don't have a real block-time measurement yet), and clear the
      // lookahead so the first paint of the new line is exactly at the
      // store's reported progress (no early/late drift across boundaries).
      const dur = untrack(() => getLineDuration(idx));
      measuredBlockMs = Math.max(80, Math.round(dur * 0.035));
      measuredBlockDelta = 0;
      lookaheadEnabled = false;
      prevBlockIndex = idx;
      prevBlockTime = now;
      prevBlockProgress = progress;
      return;
    }

    if (prevBlockTime > 0) {
      const dt = now - prevBlockTime;
      const dp = progress - prevBlockProgress;
      // Floor of 5ms: previously was 50ms (calibrated for setInterval ticks
      // at 80–200ms), which silently rejected every rAF-rate update — so
      // measuredBlockMs stayed pinned at the seed value (~89ms for a 2.5s
      // line) and the transition was always 89ms long even when ticks were
      // arriving every 16ms. With the floor lowered, the EMA settles to
      // the real notification interval and the playhead lag drops ~10x.
      if (dt >= 5 && dt <= 1500) {
        measuredBlockMs = Math.round(measuredBlockMs * 0.3 + dt * 0.7);
      }
      // Forward-only progress deltas (backward = seek, not a tick we'd predict)
      if (dp > 0 && dp < 0.2) {
        measuredBlockDelta = measuredBlockDelta * 0.3 + dp * 0.7;
        lookaheadEnabled = true;
      }
    }
    prevBlockTime = now;
    prevBlockProgress = progress;
  });

  // Inline style for the (single) active line. Computed reactively so any
  // change to lookahead / measurement state updates the karaoke position
  // without restarting any animations.
  //
  // Always uses activeProgress as the floor — lookahead just adds a small
  // measured offset on top. Earlier this forced 0 on first paint, but if
  // lookaheadEnabled ever fails to flip on (e.g. the line gets only a
  // single notification before audio stops or a seek lands directly at
  // p=1) the gradient would stay at 0 forever (= the line never goes
  // through). Tracking activeProgress directly makes that failure mode
  // graceful: even with no lookahead, the playhead still reflects audio.
  const activeLineStyle = $derived.by(() => {
    if (!isSynced || activeIndex < 0) return '';
    const base = Math.max(0, Math.min(1, activeProgress));
    const prog = lookaheadEnabled
      ? Math.max(0, Math.min(1, base + measuredBlockDelta))
      : base;
    return `--line-progress: ${prog}; --block-interval: ${measuredBlockMs}ms`;
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
      <!-- Single <div> per line with class toggles, so CSS transitions can
           animate state changes (active ↔ past, size/scale/color) instead
           of being killed by destroy-and-recreate when activeIndex moves. -->
      {@const isActive = isSynced && index === activeIndex}
      <div
        class="lyrics-line {immersive && isSynced ? getDistanceClass(index, activeIndex) : ''}"
        class:active={isActive}
        class:past={isSynced && index < activeIndex}
        style={isActive
          ? activeLineStyle
          : (immersive ? '' : `--line-opacity: ${isSynced ? getLineOpacity(index, activeIndex) : 1}`)}
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

  /* Immersive mode: simple bright white text.
     Also overrides the sidebar's karaoke gradient + transparent fill so the
     active line stays solid white instead of gradient-clipped. */
  .lyrics-lines.immersive .lyrics-line.active .line-text {
    background: none;
    color: #ffffff !important;
    -webkit-text-fill-color: #ffffff;
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
    /* Transitions on every property the active class swaps, so going
       active ↔ past animates smoothly in both directions (the same DOM
       element persists across state changes — see the each-block above). */
    transition:
      opacity 220ms ease-out,
      color 220ms ease-out,
      font-size 220ms cubic-bezier(0.4, 0, 0.2, 1),
      font-weight 220ms ease-out,
      transform 220ms cubic-bezier(0.4, 0, 0.2, 1);
    transform-origin: left center;
    /* Prevent horizontal overflow with long lyrics */
    word-wrap: break-word;
    overflow-wrap: break-word;
  }

  /* Register --line-progress as an animatable number so CSS can interpolate
     the gradient stop position between block notifications. */
  @property --line-progress {
    syntax: '<number>';
    inherits: true;
    initial-value: 0;
  }

  /* Active line: mirror the base transitions and add --line-progress
     (interpolated over --block-interval, set per-line in JS to match audio
     speed). Re-declaring the full list because `transition` is shorthand
     and would otherwise drop the size/scale/color animations on activation. */
  .lyrics-line.active {
    transition:
      opacity 220ms ease-out,
      color 220ms ease-out,
      font-size 220ms cubic-bezier(0.4, 0, 0.2, 1),
      font-weight 220ms ease-out,
      transform 220ms cubic-bezier(0.4, 0, 0.2, 1),
      --line-progress var(--block-interval, 175ms) linear;
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
    /* No text-shadow or filter on the active line — text-shadow inherits
       into the background-clipped span and tints the gradient on WebKit
       (macOS), while filter: drop-shadow rasterizes the line at its
       layout box and clips descenders (g, y, p). Bold + scale + colored
       gradient is enough emphasis without either. */
  }

  .lyrics-lines.center .lyrics-line.active {
    transform: scale(1.05);
  }

  /* Karaoke effect on active line — hard cut at the playhead.
     Visual smoothness comes from --line-progress transitioning over
     --block-interval; the gradient is a sharp two-color split moving
     with that transition. The +1% bias on the playhead stops pushes the
     cut just past 100% at p=1, guaranteeing the rightmost text pixels
     are in the active color (so the last line of a song reliably fills
     during the trailing instrumental coda). */
  .lyrics-line.active .line-text {
    --progress-pos: calc(var(--line-progress, 0) * 100%);
    background: linear-gradient(
      90deg,
      var(--lyrics-active-color, var(--accent-primary)) 0%,
      var(--lyrics-active-color, var(--accent-primary)) calc(var(--progress-pos) + 1%),
      var(--text-primary) calc(var(--progress-pos) + 1%),
      var(--text-primary) 100%
    );
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    -webkit-text-fill-color: transparent;
  }

  @media (prefers-reduced-motion: reduce) {
    .lyrics-line.active {
      transition: none;
    }
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
