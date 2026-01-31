<script lang="ts">
  interface Props {
    artwork: string;
  }

  let { artwork }: Props = $props();

  let canvasRef: HTMLCanvasElement | undefined = $state();
  let isLoading = $state(true);
  let currentArtwork = $state('');

  // Generate a tiny blurred version using Canvas (GPU-free approach)
  // Instead of CSS blur(120px) which kills WebKit performance,
  // we resize the image to 8x8 pixels and scale it up - natural blur effect
  async function generateBlurredBackground(imageUrl: string): Promise<void> {
    if (!canvasRef || !imageUrl) return;

    const ctx = canvasRef.getContext('2d');
    if (!ctx) return;

    const img = new Image();
    img.crossOrigin = 'anonymous';

    img.onload = () => {
      // Draw to tiny size (8x8) - this creates natural blur when scaled
      const tinySize = 8;
      canvasRef!.width = tinySize;
      canvasRef!.height = tinySize;

      // Draw image scaled down to 8x8
      ctx.drawImage(img, 0, 0, tinySize, tinySize);

      // Slightly boost saturation by adjusting colors
      const imageData = ctx.getImageData(0, 0, tinySize, tinySize);
      const data = imageData.data;

      for (let i = 0; i < data.length; i += 4) {
        // Simple saturation boost
        const r = data[i];
        const g = data[i + 1];
        const b = data[i + 2];
        const avg = (r + g + b) / 3;

        // Saturation factor (1.3x)
        const satFactor = 1.3;
        data[i] = Math.min(255, avg + (r - avg) * satFactor);
        data[i + 1] = Math.min(255, avg + (g - avg) * satFactor);
        data[i + 2] = Math.min(255, avg + (b - avg) * satFactor);

        // Brightness reduction (0.6x)
        data[i] = data[i] * 0.6;
        data[i + 1] = data[i + 1] * 0.6;
        data[i + 2] = data[i + 2] * 0.6;
      }

      ctx.putImageData(imageData, 0, 0);
      isLoading = false;
    };

    img.onerror = () => {
      isLoading = false;
    };

    img.src = imageUrl;
  }

  // Track artwork changes
  $effect(() => {
    if (!artwork || artwork === currentArtwork) return;
    currentArtwork = artwork;
    isLoading = true;
    generateBlurredBackground(artwork);
  });
</script>

<div class="immersive-background" class:loading={isLoading}>
  <!-- Canvas-generated tiny image, scaled up via CSS -->
  <canvas
    bind:this={canvasRef}
    class="background-canvas"
    aria-hidden="true"
  ></canvas>

  <!-- Dark overlay for better contrast -->
  <div class="dark-overlay"></div>
</div>

<style>
  .immersive-background {
    position: absolute;
    inset: 0;
    overflow: hidden;
    z-index: 0;
    background-color: #0a0a0b;
  }

  .background-canvas {
    position: absolute;
    /* Extend beyond viewport for seamless edges */
    inset: -50px;
    width: calc(100% + 100px);
    height: calc(100% + 100px);
    /* Scale up tiny canvas - creates natural blur effect */
    /* Using image-rendering: auto for smooth interpolation */
    image-rendering: auto;
    /* GPU layer for smooth transitions */
    transform: scale(1.1) translateZ(0);
    will-change: opacity;
    transition: opacity 500ms ease-out;
  }

  .loading .background-canvas {
    opacity: 0;
  }

  /* Subtle dark overlay */
  .dark-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.15);
    pointer-events: none;
  }
</style>
