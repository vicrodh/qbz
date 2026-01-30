<script lang="ts">
  interface Props {
    artwork: string;
  }

  let { artwork }: Props = $props();

  let isLoading = $state(true);
  let currentArtwork = $state('');

  // Track artwork changes for fade transition
  $effect(() => {
    if (!artwork || artwork === currentArtwork) return;
    currentArtwork = artwork;
    isLoading = true;
  });

  function handleImageLoad() {
    isLoading = false;
  }
</script>

<div class="immersive-background" class:loading={isLoading}>
  <!-- Heavily blurred artwork - just color blobs -->
  {#if artwork}
    <img
      src={artwork}
      alt=""
      class="background-image"
      aria-hidden="true"
      onload={handleImageLoad}
    />
  {/if}

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

  .background-image {
    position: absolute;
    /* Extend beyond viewport to hide blur edges */
    inset: -100px;
    width: calc(100% + 200px);
    height: calc(100% + 200px);
    object-fit: cover;
    object-position: center;
    /* Heavy blur - should be unrecognizable, just color blobs */
    filter: blur(120px) saturate(1.3) brightness(0.6);
    transform: scale(1.2);
    transition: opacity 500ms ease-out;
  }

  .loading .background-image {
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
