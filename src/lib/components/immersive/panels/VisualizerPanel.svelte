<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    isPlaying?: boolean;
  }

  let { isPlaying = false }: Props = $props();

  let canvas: HTMLCanvasElement | null = $state(null);
  let animationId: number | null = null;
  let bars: number[] = [];
  const BAR_COUNT = 64;
  const BAR_WIDTH = 4;
  const BAR_GAP = 2;

  // Initialize bars with random heights
  function initBars() {
    bars = Array(BAR_COUNT).fill(0).map(() => Math.random() * 0.3 + 0.1);
  }

  // Animate bars
  function animate() {
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const width = canvas.width;
    const height = canvas.height;
    const centerX = width / 2;
    const totalWidth = BAR_COUNT * (BAR_WIDTH + BAR_GAP) - BAR_GAP;
    const startX = centerX - totalWidth / 2;

    // Clear canvas
    ctx.clearRect(0, 0, width, height);

    // Update bar heights with smooth animation
    bars = bars.map((bar, i) => {
      if (isPlaying) {
        // Random movement when playing
        const target = Math.random() * 0.8 + 0.2;
        return bar + (target - bar) * 0.15;
      } else {
        // Slowly decay to idle state
        const idle = Math.sin(Date.now() / 1000 + i * 0.2) * 0.1 + 0.15;
        return bar + (idle - bar) * 0.05;
      }
    });

    // Draw bars
    const gradient = ctx.createLinearGradient(0, height, 0, 0);
    gradient.addColorStop(0, 'rgba(124, 58, 237, 0.8)'); // Purple
    gradient.addColorStop(0.5, 'rgba(139, 92, 246, 0.6)');
    gradient.addColorStop(1, 'rgba(167, 139, 250, 0.4)');

    ctx.fillStyle = gradient;

    bars.forEach((bar, i) => {
      const x = startX + i * (BAR_WIDTH + BAR_GAP);
      const barHeight = bar * height * 0.7;
      const y = (height - barHeight) / 2;

      // Rounded rectangle
      const radius = BAR_WIDTH / 2;
      ctx.beginPath();
      ctx.moveTo(x + radius, y);
      ctx.lineTo(x + BAR_WIDTH - radius, y);
      ctx.quadraticCurveTo(x + BAR_WIDTH, y, x + BAR_WIDTH, y + radius);
      ctx.lineTo(x + BAR_WIDTH, y + barHeight - radius);
      ctx.quadraticCurveTo(x + BAR_WIDTH, y + barHeight, x + BAR_WIDTH - radius, y + barHeight);
      ctx.lineTo(x + radius, y + barHeight);
      ctx.quadraticCurveTo(x, y + barHeight, x, y + barHeight - radius);
      ctx.lineTo(x, y + radius);
      ctx.quadraticCurveTo(x, y, x + radius, y);
      ctx.closePath();
      ctx.fill();
    });

    animationId = requestAnimationFrame(animate);
  }

  // Handle canvas resize
  function resizeCanvas() {
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * window.devicePixelRatio;
    canvas.height = rect.height * window.devicePixelRatio;
    const ctx = canvas.getContext('2d');
    if (ctx) {
      ctx.scale(window.devicePixelRatio, window.devicePixelRatio);
    }
  }

  onMount(() => {
    initBars();
    resizeCanvas();
    window.addEventListener('resize', resizeCanvas);
    animate();

    return () => {
      window.removeEventListener('resize', resizeCanvas);
      if (animationId) {
        cancelAnimationFrame(animationId);
      }
    };
  });
</script>

<div class="visualizer-panel">
  <canvas bind:this={canvas} class="visualizer-canvas"></canvas>
  <div class="visualizer-label">
    <span class="label-text">Audio Visualizer</span>
    {#if !isPlaying}
      <span class="label-hint">Play a track to see the visualization</span>
    {/if}
  </div>
</div>

<style>
  .visualizer-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    position: relative;
  }

  .visualizer-canvas {
    width: 100%;
    height: 200px;
    max-width: 500px;
  }

  .visualizer-label {
    position: absolute;
    bottom: 20px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    opacity: 0.5;
    transition: opacity 200ms ease;
  }

  .visualizer-panel:hover .visualizer-label {
    opacity: 0.3;
  }

  .label-text {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .label-hint {
    font-size: 11px;
    color: var(--alpha-30, rgba(255, 255, 255, 0.3));
  }
</style>
