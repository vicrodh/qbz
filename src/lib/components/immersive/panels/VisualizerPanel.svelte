<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';

  interface Props {
    enabled?: boolean;
    artwork?: string;
    trackTitle?: string;
    artist?: string;
  }

  let { enabled = true, artwork = '', trackTitle = '', artist = '' }: Props = $props();

  let canvasRef: HTMLCanvasElement | null = $state(null);
  let ctx: CanvasRenderingContext2D | null = null;
  let animationFrame: number | null = null;
  let unlisten: UnlistenFn | null = null;
  let isInitialized = false;

  const NUM_BARS = 64;
  const frequencyData = new Float32Array(NUM_BARS);
  const smoothedData = new Float32Array(NUM_BARS);

  // Smoothing for visual continuity
  const SMOOTHING = 0.7;

  async function init() {
    if (!canvasRef || isInitialized) return;

    ctx = canvasRef.getContext('2d');
    if (!ctx) {
      console.warn('Canvas 2D not available');
      return;
    }

    isInitialized = true;

    // Enable backend
    try {
      await invoke('set_visualizer_enabled', { enabled: true });
      console.log('[Visualizer] Backend enabled');
    } catch (e) {
      console.error('[Visualizer] Failed to enable backend:', e);
    }

    // Listen for frequency data
    unlisten = await listen<number[]>('viz:data', (event) => {
      const payload = event.payload;
      if (Array.isArray(payload)) {
        const bytes = new Uint8Array(payload);
        const floats = new Float32Array(bytes.buffer);
        if (floats.length === NUM_BARS) {
          // Apply smoothing
          for (let i = 0; i < NUM_BARS; i++) {
            smoothedData[i] = smoothedData[i] * SMOOTHING + floats[i] * (1 - SMOOTHING);
          }
          frequencyData.set(smoothedData);
        }
      }
    });

    render();
  }

  function render() {
    if (!ctx || !canvasRef) return;

    const rect = canvasRef.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvasRef.width = rect.width * dpr;
    canvasRef.height = rect.height * dpr;
    ctx.scale(dpr, dpr);

    const width = rect.width;
    const height = rect.height;
    const centerY = height / 2;
    const maxAmplitude = height * 0.35;

    // Clear with black
    ctx.fillStyle = '#000000';
    ctx.fillRect(0, 0, width, height);

    // Draw wave - top half
    ctx.beginPath();
    ctx.moveTo(0, centerY);

    for (let i = 0; i < NUM_BARS; i++) {
      const x = (i / (NUM_BARS - 1)) * width;
      const amplitude = frequencyData[i] * maxAmplitude;
      const y = centerY - amplitude;

      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        // Smooth curve using quadratic bezier
        const prevX = ((i - 1) / (NUM_BARS - 1)) * width;
        const cpX = (prevX + x) / 2;
        ctx.quadraticCurveTo(prevX, centerY - frequencyData[i - 1] * maxAmplitude, cpX, (centerY - frequencyData[i - 1] * maxAmplitude + y) / 2);
      }
    }

    // Create gradient for top wave
    const gradientTop = ctx.createLinearGradient(0, centerY - maxAmplitude, 0, centerY);
    gradientTop.addColorStop(0, 'rgba(0, 255, 200, 0.9)');
    gradientTop.addColorStop(0.5, 'rgba(100, 100, 255, 0.7)');
    gradientTop.addColorStop(1, 'rgba(150, 50, 200, 0.3)');

    ctx.strokeStyle = gradientTop;
    ctx.lineWidth = 3;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.stroke();

    // Draw mirrored wave - bottom half
    ctx.beginPath();
    for (let i = 0; i < NUM_BARS; i++) {
      const x = (i / (NUM_BARS - 1)) * width;
      const amplitude = frequencyData[i] * maxAmplitude;
      const y = centerY + amplitude;

      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        const prevX = ((i - 1) / (NUM_BARS - 1)) * width;
        const cpX = (prevX + x) / 2;
        ctx.quadraticCurveTo(prevX, centerY + frequencyData[i - 1] * maxAmplitude, cpX, (centerY + frequencyData[i - 1] * maxAmplitude + y) / 2);
      }
    }

    // Gradient for bottom wave (mirrored colors)
    const gradientBottom = ctx.createLinearGradient(0, centerY, 0, centerY + maxAmplitude);
    gradientBottom.addColorStop(0, 'rgba(150, 50, 200, 0.3)');
    gradientBottom.addColorStop(0.5, 'rgba(100, 100, 255, 0.7)');
    gradientBottom.addColorStop(1, 'rgba(0, 255, 200, 0.9)');

    ctx.strokeStyle = gradientBottom;
    ctx.stroke();

    // Draw center line (subtle)
    ctx.beginPath();
    ctx.moveTo(0, centerY);
    ctx.lineTo(width, centerY);
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.1)';
    ctx.lineWidth = 1;
    ctx.stroke();

    animationFrame = requestAnimationFrame(render);
  }

  async function cleanup() {
    if (animationFrame) {
      cancelAnimationFrame(animationFrame);
      animationFrame = null;
    }

    if (unlisten) {
      unlisten();
      unlisten = null;
    }

    try {
      await invoke('set_visualizer_enabled', { enabled: false });
      console.log('[Visualizer] Backend disabled');
    } catch (e) {
      console.error('[Visualizer] Failed to disable backend:', e);
    }

    isInitialized = false;
  }

  onMount(() => {
    if (enabled) {
      init();
    }
    return cleanup;
  });

  $effect(() => {
    if (enabled && !isInitialized) {
      init();
    } else if (!enabled && isInitialized) {
      cleanup();
    }
  });
</script>

<div class="visualizer-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="visualizer-canvas"></canvas>

  <div class="track-info">
    {#if artwork}
      <img src={artwork} alt={trackTitle} class="artwork" />
    {/if}
    <div class="text">
      <h1 class="title">{trackTitle}</h1>
      <p class="artist">{artist}</p>
    </div>
  </div>
</div>

<style>
  .visualizer-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 300ms ease;
    z-index: 5;
    background: #000000;
  }

  .visualizer-panel.visible {
    opacity: 1;
  }

  .visualizer-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .track-info {
    position: relative;
    z-index: 10;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 16px;
  }

  .artwork {
    width: min(180px, 22vw);
    height: min(180px, 22vw);
    border-radius: 12px;
    box-shadow: 0 10px 40px rgba(0, 0, 0, 0.6);
    object-fit: cover;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .title {
    font-size: clamp(16px, 2.2vw, 24px);
    font-weight: 700;
    color: white;
    margin: 0;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.8);
  }

  .artist {
    font-size: clamp(12px, 1.5vw, 16px);
    color: rgba(255, 255, 255, 0.8);
    margin: 0;
    text-shadow: 0 2px 8px rgba(0, 0, 0, 0.8);
  }
</style>
