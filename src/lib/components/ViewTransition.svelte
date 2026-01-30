<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    children: import('svelte').Snippet;
    delay?: number;      // Delay before starting animation (ms)
    duration?: number;   // Animation duration (ms)
    direction?: 'up' | 'down' | 'left' | 'right' | 'fade';
    distance?: number;   // Distance to travel (px)
  }

  let {
    children,
    delay = 0,
    duration = 300,
    direction = 'up',
    distance = 20
  }: Props = $props();

  let visible = $state(false);
  let containerEl: HTMLDivElement;

  onMount(() => {
    const timer = setTimeout(() => {
      visible = true;
    }, delay);

    return () => clearTimeout(timer);
  });

  // Calculate initial transform based on direction
  const initialTransform = $derived.by(() => {
    switch (direction) {
      case 'up': return `translateY(${distance}px)`;
      case 'down': return `translateY(-${distance}px)`;
      case 'left': return `translateX(${distance}px)`;
      case 'right': return `translateX(-${distance}px)`;
      case 'fade': return 'none';
      default: return `translateY(${distance}px)`;
    }
  });
</script>

<div
  class="view-transition"
  class:visible
  bind:this={containerEl}
  style:--transition-duration="{duration}ms"
  style:--initial-transform={initialTransform}
>
  {@render children()}
</div>

<style>
  .view-transition {
    opacity: 0;
    transform: var(--initial-transform);
    transition:
      opacity var(--transition-duration) cubic-bezier(0.4, 0, 0.2, 1),
      transform var(--transition-duration) cubic-bezier(0.4, 0, 0.2, 1);
    width: 100%;
    height: 100%;
  }

  .view-transition.visible {
    opacity: 1;
    transform: none;
  }
</style>
