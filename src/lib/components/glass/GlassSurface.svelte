<script lang="ts">
  import { onDestroy, type Snippet } from 'svelte';
  import GlassFilterDefs from './GlassFilterDefs.svelte';

  interface Props {
    as?: string;
    rootClassName?: string;
    contentClassName?: string;
    enableRipple?: boolean;
    triggerRipple?: boolean;
    rippleDurationMs?: number;
    enableDistortion?: boolean;
    includeDefs?: boolean;
    onclick?: (event: MouseEvent) => void;
    children?: Snippet;
  }

  let {
    as = 'div',
    rootClassName = '',
    contentClassName = '',
    enableRipple = true,
    triggerRipple = false,
    rippleDurationMs = 800,
    enableDistortion = true,
    includeDefs = true,
    onclick,
    children
  }: Props = $props();

  const filterId = `glass-dist-${Math.random().toString(36).slice(2, 9)}`;

  let isAnimating = $state(false);
  let rippleX = $state(50);
  let rippleY = $state(50);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let container: HTMLElement | null = null;
  let lastTrigger = false;

  function startRipple(xPercent: number, yPercent: number) {
    rippleX = xPercent;
    rippleY = yPercent;
    isAnimating = true;

    if (timer) {
      clearTimeout(timer);
    }
    timer = setTimeout(() => {
      isAnimating = false;
      timer = null;
    }, rippleDurationMs);
  }

  function handleClick(event: MouseEvent) {
    if (enableRipple && container && event.button === 0) {
      const rect = container.getBoundingClientRect();
      const x = ((event.clientX - rect.left) / rect.width) * 100;
      const y = ((event.clientY - rect.top) / rect.height) * 100;
      startRipple(x, y);
    }
    onclick?.(event);
  }

  $effect(() => {
    if (triggerRipple && !lastTrigger) {
      startRipple(50, 50);
    }
    lastTrigger = triggerRipple;
  });

  onDestroy(() => {
    if (timer) {
      clearTimeout(timer);
    }
  });
</script>

{#if includeDefs}
  <GlassFilterDefs id={filterId} />
{/if}

<svelte:element
  this={as}
  class={`glass-surface ${rootClassName}`.trim()}
  style={`--glass-ripple-duration: ${rippleDurationMs}ms;`}
  bind:this={container}
  onclick={handleClick}
  role="presentation"
>
  <div class="glass-backdrop" style={enableDistortion ? `filter: url(#${filterId})` : undefined}></div>
  <div class="glass-sheen"></div>
  <div class="glass-border"></div>
  {#if isAnimating}
    <div class="glass-ripple" style={`--ripple-x: ${rippleX}%; --ripple-y: ${rippleY}%`}></div>
  {/if}
  <div class={`glass-content ${contentClassName}`.trim()}>
    {#if children}
      {@render children()}
    {/if}
  </div>
</svelte:element>

<style>
  .glass-surface {
    overflow: hidden;
    border-radius: var(--glass-radius, 14px);
    background: var(--glass-fallback, rgba(255, 255, 255, 0.06));
    color: var(--glass-text, inherit);
    box-shadow: var(--glass-shadow, 0 18px 40px rgba(0, 0, 0, 0.35));
    isolation: isolate;
  }

  .glass-backdrop {
    position: absolute;
    inset: 0;
    background: var(--glass-bg, rgba(255, 255, 255, 0.08));
    border-radius: inherit;
    z-index: 0;
    transition: filter 0.3s ease;
  }

  @supports ((-webkit-backdrop-filter: blur(1px)) or (backdrop-filter: blur(1px))) {
    .glass-backdrop {
      -webkit-backdrop-filter: blur(var(--glass-blur, 16px)) saturate(140%);
      backdrop-filter: blur(var(--glass-blur, 16px)) saturate(140%);
    }
  }

  .glass-sheen {
    position: absolute;
    inset: 0;
    z-index: 1;
    background: linear-gradient(140deg, rgba(255, 255, 255, 0.18), rgba(255, 255, 255, 0));
    opacity: 0.8;
    pointer-events: none;
  }

  .glass-border {
    position: absolute;
    inset: 0;
    z-index: 2;
    border-radius: inherit;
    border: 1px solid var(--glass-border, rgba(255, 255, 255, 0.15));
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.04);
    pointer-events: none;
  }

  .glass-ripple {
    position: absolute;
    left: var(--ripple-x, 50%);
    top: var(--ripple-y, 50%);
    width: 160px;
    height: 160px;
    border-radius: 50%;
    background: radial-gradient(circle, rgba(255, 255, 255, 0.35) 0%, rgba(255, 255, 255, 0.15) 35%, transparent 70%);
    transform: translate(-50%, -50%) scale(0);
    animation: glassRipple var(--glass-ripple-duration, 800ms) cubic-bezier(0.34, 1.56, 0.64, 1);
    z-index: 2;
    pointer-events: none;
  }

  .glass-content {
    position: relative;
    z-index: 3;
  }

  @keyframes glassRipple {
    0% {
      transform: translate(-50%, -50%) scale(0);
      opacity: 0.8;
    }
    60% {
      opacity: 0.35;
    }
    100% {
      transform: translate(-50%, -50%) scale(3.4);
      opacity: 0;
    }
  }
</style>
