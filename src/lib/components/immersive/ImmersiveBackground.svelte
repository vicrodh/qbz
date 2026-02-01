<script lang="ts">
  interface Props {
    artwork: string;
  }

  let { artwork }: Props = $props();

  let canvasRef: HTMLCanvasElement | undefined = $state();
  let isLoading = $state(true);
  let currentArtwork = $state('');

  // Generate a small blurred version using Canvas
  // Key insight: blur(30px) on a 64x64 image is MUCH cheaper than blur(120px) on full image
  // The small source + small blur = smooth result with minimal CPU
  async function generateBlurredBackground(imageUrl: string): Promise<void> {
    if (!canvasRef || !imageUrl) return;

    const ctx = canvasRef.getContext('2d');
    if (!ctx) return;

    const img = new Image();
    img.crossOrigin = 'anonymous';

    img.onload = () => {
      // Use 64x64 - large enough to avoid blocky pixels, small enough to be efficient
      const size = 64;
      canvasRef!.width = size;
      canvasRef!.height = size;

      // Draw image scaled down
      ctx.drawImage(img, 0, 0, size, size);

      // Apply color adjustments
      const imageData = ctx.getImageData(0, 0, size, size);
      const data = imageData.data;

      for (let i = 0; i < data.length; i += 4) {
        const r = data[i];
        const g = data[i + 1];
        const b = data[i + 2];
        const avg = (r + g + b) / 3;

        // Saturation boost (1.3x)
        const satFactor = 1.3;
        let newR = avg + (r - avg) * satFactor;
        let newG = avg + (g - avg) * satFactor;
        let newB = avg + (b - avg) * satFactor;

        // Brightness reduction (0.55x) - slightly darker for better text contrast
        data[i] = Math.min(255, Math.max(0, newR * 0.55));
        data[i + 1] = Math.min(255, Math.max(0, newG * 0.55));
        data[i + 2] = Math.min(255, Math.max(0, newB * 0.55));
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
    /* Extend beyond viewport to hide blur edges */
    inset: -80px;
    width: calc(100% + 160px);
    height: calc(100% + 160px);
    /* Smooth interpolation when scaling up */
    image-rendering: auto;
    /* Small blur to smooth out the 64x64 canvas - MUCH cheaper than 120px on full image */
    filter: blur(40px);
    /* GPU layer for performance */
    transform: scale(1.15) translateZ(0);
    will-change: opacity, filter;
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
