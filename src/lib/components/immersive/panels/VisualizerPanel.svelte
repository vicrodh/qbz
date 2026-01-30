<script lang="ts">
  import { onMount } from 'svelte';
  import { Settings } from 'lucide-svelte';

  type VisualizerMode = 'bars' | 'wave' | 'circular';

  interface Props {
    isPlaying?: boolean;
    artwork?: string;
  }

  let { isPlaying = false, artwork }: Props = $props();

  let canvas: HTMLCanvasElement | null = $state(null);
  let animationId: number | null = null;
  let mode: VisualizerMode = $state('bars');
  let showSettings = $state(false);

  // Extracted colors from artwork
  let colors = $state(['#7c3aed', '#a78bfa', '#c4b5fd']);

  // Optimized: fewer bars, throttled updates
  const BAR_COUNT = 32;
  let bars: number[] = [];
  let lastUpdate = 0;
  const UPDATE_INTERVAL = 50; // ms between visual updates (20fps)

  // Extract colors from artwork
  async function extractColors(imageUrl: string) {
    try {
      const img = new Image();
      img.crossOrigin = 'anonymous';

      await new Promise<void>((resolve, reject) => {
        img.onload = () => resolve();
        img.onerror = reject;
        img.src = imageUrl;
      });

      const tempCanvas = document.createElement('canvas');
      tempCanvas.width = 10;
      tempCanvas.height = 10;
      const ctx = tempCanvas.getContext('2d');
      if (!ctx) return;

      ctx.drawImage(img, 0, 0, 10, 10);
      const data = ctx.getImageData(0, 0, 10, 10).data;

      // Sample a few pixels
      const samples = [[0, 0], [9, 0], [0, 9], [4, 4], [9, 9]];
      const extractedColors: string[] = [];

      for (const [x, y] of samples) {
        const idx = (y * 10 + x) * 4;
        const r = data[idx];
        const g = data[idx + 1];
        const b = data[idx + 2];
        // Slightly brighten for visibility
        const br = Math.min(255, Math.round(r * 1.2));
        const bg = Math.min(255, Math.round(g * 1.2));
        const bb = Math.min(255, Math.round(b * 1.2));
        extractedColors.push(`rgb(${br}, ${bg}, ${bb})`);
      }

      colors = [...new Set(extractedColors)].slice(0, 3);
      if (colors.length < 3) {
        colors = ['#7c3aed', '#a78bfa', '#c4b5fd'];
      }
    } catch {
      // Use default colors on error
      colors = ['#7c3aed', '#a78bfa', '#c4b5fd'];
    }
  }

  // Watch artwork changes
  $effect(() => {
    if (artwork) {
      extractColors(artwork);
    }
  });

  function initBars() {
    bars = Array(BAR_COUNT).fill(0).map(() => Math.random() * 0.2 + 0.1);
  }

  function animate(timestamp: number) {
    if (!canvas) return;

    // Throttle updates for performance
    if (timestamp - lastUpdate < UPDATE_INTERVAL) {
      animationId = requestAnimationFrame(animate);
      return;
    }
    lastUpdate = timestamp;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const width = canvas.width / window.devicePixelRatio;
    const height = canvas.height / window.devicePixelRatio;

    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.save();
    ctx.scale(window.devicePixelRatio, window.devicePixelRatio);

    // Update bars with smooth animation
    bars = bars.map((bar, i) => {
      if (isPlaying) {
        const target = Math.random() * 0.85 + 0.15;
        return bar + (target - bar) * 0.2;
      } else {
        const idle = Math.sin(Date.now() / 2000 + i * 0.3) * 0.08 + 0.12;
        return bar + (idle - bar) * 0.08;
      }
    });

    // Create gradient
    const gradient = ctx.createLinearGradient(0, height, 0, 0);
    gradient.addColorStop(0, colors[0]);
    gradient.addColorStop(0.5, colors[1] || colors[0]);
    gradient.addColorStop(1, colors[2] || colors[0]);

    if (mode === 'bars') {
      drawBars(ctx, width, height, gradient);
    } else if (mode === 'wave') {
      drawWave(ctx, width, height, gradient);
    } else if (mode === 'circular') {
      drawCircular(ctx, width, height, gradient);
    }

    ctx.restore();
    animationId = requestAnimationFrame(animate);
  }

  function drawBars(ctx: CanvasRenderingContext2D, width: number, height: number, gradient: CanvasGradient) {
    const barWidth = width / BAR_COUNT * 0.7;
    const gap = width / BAR_COUNT * 0.3;
    const maxHeight = height * 0.7;

    ctx.fillStyle = gradient;

    bars.forEach((bar, i) => {
      const x = i * (barWidth + gap) + gap / 2;
      const barHeight = bar * maxHeight;
      const y = (height - barHeight) / 2;
      const radius = barWidth / 2;

      // Rounded rectangle
      ctx.beginPath();
      ctx.moveTo(x + radius, y);
      ctx.lineTo(x + barWidth - radius, y);
      ctx.quadraticCurveTo(x + barWidth, y, x + barWidth, y + radius);
      ctx.lineTo(x + barWidth, y + barHeight - radius);
      ctx.quadraticCurveTo(x + barWidth, y + barHeight, x + barWidth - radius, y + barHeight);
      ctx.lineTo(x + radius, y + barHeight);
      ctx.quadraticCurveTo(x, y + barHeight, x, y + barHeight - radius);
      ctx.lineTo(x, y + radius);
      ctx.quadraticCurveTo(x, y, x + radius, y);
      ctx.fill();
    });
  }

  function drawWave(ctx: CanvasRenderingContext2D, width: number, height: number, gradient: CanvasGradient) {
    const centerY = height / 2;
    const amplitude = height * 0.35;

    ctx.strokeStyle = gradient;
    ctx.lineWidth = 3;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    ctx.beginPath();
    bars.forEach((bar, i) => {
      const x = (i / (bars.length - 1)) * width;
      const y = centerY + (bar - 0.5) * amplitude * 2;
      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        const prevX = ((i - 1) / (bars.length - 1)) * width;
        const cpX = (prevX + x) / 2;
        ctx.quadraticCurveTo(cpX, centerY + (bars[i - 1] - 0.5) * amplitude * 2, x, y);
      }
    });
    ctx.stroke();

    // Mirror wave
    ctx.globalAlpha = 0.3;
    ctx.beginPath();
    bars.forEach((bar, i) => {
      const x = (i / (bars.length - 1)) * width;
      const y = centerY - (bar - 0.5) * amplitude * 2;
      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        const prevX = ((i - 1) / (bars.length - 1)) * width;
        const cpX = (prevX + x) / 2;
        ctx.quadraticCurveTo(cpX, centerY - (bars[i - 1] - 0.5) * amplitude * 2, x, y);
      }
    });
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  function drawCircular(ctx: CanvasRenderingContext2D, width: number, height: number, gradient: CanvasGradient) {
    const centerX = width / 2;
    const centerY = height / 2;
    const baseRadius = Math.min(width, height) * 0.2;
    const maxRadius = Math.min(width, height) * 0.4;

    ctx.strokeStyle = gradient;
    ctx.lineWidth = 4;
    ctx.lineCap = 'round';

    bars.forEach((bar, i) => {
      const angle = (i / bars.length) * Math.PI * 2 - Math.PI / 2;
      const innerRadius = baseRadius;
      const outerRadius = baseRadius + bar * (maxRadius - baseRadius);

      const x1 = centerX + Math.cos(angle) * innerRadius;
      const y1 = centerY + Math.sin(angle) * innerRadius;
      const x2 = centerX + Math.cos(angle) * outerRadius;
      const y2 = centerY + Math.sin(angle) * outerRadius;

      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.lineTo(x2, y2);
      ctx.stroke();
    });

    // Center circle
    ctx.fillStyle = 'rgba(0, 0, 0, 0.3)';
    ctx.beginPath();
    ctx.arc(centerX, centerY, baseRadius * 0.8, 0, Math.PI * 2);
    ctx.fill();
  }

  function resizeCanvas() {
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width * window.devicePixelRatio;
    canvas.height = rect.height * window.devicePixelRatio;
  }

  function cycleMode() {
    const modes: VisualizerMode[] = ['bars', 'wave', 'circular'];
    const currentIndex = modes.indexOf(mode);
    mode = modes[(currentIndex + 1) % modes.length];
  }

  onMount(() => {
    initBars();
    resizeCanvas();
    window.addEventListener('resize', resizeCanvas);
    animationId = requestAnimationFrame(animate);

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

  <!-- Mode indicator -->
  <button class="mode-btn" onclick={cycleMode} title="Change visualizer style">
    <Settings size={18} />
    <span class="mode-label">{mode}</span>
  </button>
</div>

<style>
  .visualizer-panel {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    /* Account for header and controls */
    padding: 70px 20px 120px;
    z-index: 5;
  }

  .visualizer-canvas {
    width: 100%;
    height: 100%;
    max-width: 100%;
    max-height: 100%;
  }

  .mode-btn {
    position: absolute;
    bottom: 140px;
    right: 30px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    background: rgba(0, 0, 0, 0.4);
    backdrop-filter: blur(12px);
    border: 1px solid var(--alpha-15, rgba(255, 255, 255, 0.15));
    border-radius: 20px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    font-size: 12px;
    cursor: pointer;
    transition: all 150ms ease;
    text-transform: capitalize;
  }

  .mode-btn:hover {
    background: rgba(0, 0, 0, 0.5);
    color: var(--text-primary, white);
  }

  .mode-label {
    font-weight: 500;
  }
</style>
