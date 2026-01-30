<script lang="ts">
  import { generateBlurredBackground, extractDominantColors, createGradientFromColors } from '$lib/utils/imageBlur';

  interface Props {
    artwork: string;
    mode?: 'blur' | 'gradient' | 'solid';
  }

  let { artwork, mode = 'blur' }: Props = $props();

  let backgroundStyle = $state('');
  let isLoading = $state(true);
  let currentArtwork = $state('');

  // Generate background when artwork changes
  $effect(() => {
    if (!artwork || artwork === currentArtwork) return;

    currentArtwork = artwork;
    isLoading = true;

    if (mode === 'blur') {
      generateBlurredBackground(artwork)
        .then((dataUrl) => {
          backgroundStyle = `url(${dataUrl})`;
          isLoading = false;
        })
        .catch((err) => {
          console.error('[ImmersiveBackground] Blur generation failed:', err);
          // Fallback to gradient mode
          fallbackToGradient();
        });
    } else if (mode === 'gradient') {
      fallbackToGradient();
    } else {
      // Solid dark background
      backgroundStyle = 'var(--bg-primary)';
      isLoading = false;
    }
  });

  async function fallbackToGradient() {
    try {
      const colors = await extractDominantColors(artwork);
      backgroundStyle = createGradientFromColors(colors);
    } catch {
      backgroundStyle = 'linear-gradient(135deg, #1a1a2e 0%, #16213e 100%)';
    }
    isLoading = false;
  }
</script>

<div class="immersive-background" class:loading={isLoading}>
  <!-- Pre-computed blur image or gradient -->
  <div
    class="background-image"
    style="background: {backgroundStyle};"
  ></div>

  <!-- Overlay for consistent darkness and vignette -->
  <div class="background-overlay"></div>
</div>

<style>
  .immersive-background {
    position: absolute;
    inset: 0;
    overflow: hidden;
    z-index: 0;
  }

  .background-image {
    position: absolute;
    inset: -20px; /* Slight overflow to avoid edge artifacts */
    background-size: cover;
    background-position: center;
    background-repeat: no-repeat;
    transition: background 300ms ease-out;

    /* Scale up the tiny blurred image */
    transform: scale(1.1);
  }

  .loading .background-image {
    opacity: 0;
  }

  .background-overlay {
    position: absolute;
    inset: 0;

    /* Vignette effect + ensure readability */
    background:
      radial-gradient(ellipse at center, transparent 0%, rgba(0, 0, 0, 0.3) 100%),
      linear-gradient(to bottom, rgba(0, 0, 0, 0.2) 0%, transparent 30%, transparent 70%, rgba(0, 0, 0, 0.4) 100%);
  }

  /* No blur filter here - the image is pre-blurred! */
  /* This is the key performance optimization */
</style>
