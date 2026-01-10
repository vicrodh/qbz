<script lang="ts">
  import { onMount } from 'svelte';

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
  }

  let {
    lines,
    activeIndex = -1,
    activeProgress = 0,
    dimInactive = true,
    center = false,
    compact = false,
    scrollToActive = true
  }: Props = $props();

  let container: HTMLDivElement | null = null;

  function scrollActiveIntoView() {
    if (!container || activeIndex < 0) return;
    const target = container.querySelector<HTMLElement>(`[data-line-index=\"${activeIndex}\"]`);
    if (!target) return;
    target.scrollIntoView({ block: 'center', behavior: 'smooth' });
  }

  onMount(() => {
    if (scrollToActive) {
      scrollActiveIntoView();
    }
  });

  $effect(() => {
    if (scrollToActive) {
      scrollActiveIntoView();
    }
  });
</script>

<div
  class="lyrics-lines"
  class:compact={compact}
  class:center={center}
  bind:this={container}
>
  {#if lines.length === 0}
    <div class="lyrics-empty">No lyrics available</div>
  {:else}
    {#each lines as line, index}
      <div
        class="lyrics-line"
        class:active={index === activeIndex}
        class:dimmed={dimInactive && index !== activeIndex}
        style={index === activeIndex ? `--line-progress: ${Math.max(0, Math.min(1, activeProgress))}` : undefined}
        data-line-index={index}
      >
        <span class="line-text">{line.text}</span>
      </div>
    {/each}
  {/if}
</div>

<style>
  .lyrics-lines {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 12px 16px 24px 16px;
    overflow-y: auto;
    height: 100%;
    scroll-behavior: smooth;
  }

  .lyrics-lines.center {
    text-align: center;
  }

  .lyrics-lines.compact {
    gap: 10px;
  }

  .lyrics-line {
    color: var(--lyrics-color, var(--text-muted));
    font-family: var(--lyrics-font-family, var(--font-sans));
    font-size: var(--lyrics-font-size, 18px);
    line-height: var(--lyrics-line-height, 1.45);
    letter-spacing: 0.01em;
    transition: color 180ms ease, opacity 180ms ease, transform 180ms ease;
    position: relative;
  }

  .lyrics-line.dimmed {
    opacity: var(--lyrics-dimmed-opacity, 0.45);
  }

  .lyrics-line.active {
    color: var(--lyrics-active-color, var(--text-primary));
    font-size: var(--lyrics-active-size, 24px);
    font-weight: 600;
    opacity: 1;
    transform: translateX(0);
    text-shadow: var(--lyrics-shadow, 0 10px 30px rgba(0, 0, 0, 0.45));
  }

  .lyrics-line.active .line-text {
    background: linear-gradient(
      90deg,
      var(--lyrics-active-color, var(--text-primary)) calc(var(--line-progress, 0) * 100%),
      rgba(255, 255, 255, 0.35) calc(var(--line-progress, 0) * 100%)
    );
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }

  .lyrics-empty {
    color: var(--text-muted);
    font-size: 14px;
    text-align: center;
    padding: 24px 0;
  }
</style>
