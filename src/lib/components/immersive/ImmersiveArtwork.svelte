<script lang="ts">
  interface Props {
    artwork: string;
    trackTitle: string;
    variant?: 'floating' | 'vinyl';
  }

  let { artwork, trackTitle, variant = 'floating' }: Props = $props();
</script>

<div class="artwork-container" class:vinyl={variant === 'vinyl'}>
  {#if variant === 'vinyl'}
    <div class="vinyl-disc">
      <div class="vinyl-grooves"></div>
      <div class="vinyl-label">
        <img src={artwork} alt={trackTitle} />
      </div>
    </div>
  {:else}
    <div class="artwork-wrapper">
      <img src={artwork} alt={trackTitle} />
    </div>
  {/if}
</div>

<style>
  .artwork-container {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  /* Floating variant (default) */
  .artwork-wrapper {
    width: 100%;
    max-width: 380px;
    aspect-ratio: 1;
    border-radius: 12px;
    overflow: hidden;
    box-shadow:
      0 32px 64px rgba(0, 0, 0, 0.5),
      0 0 0 1px rgba(255, 255, 255, 0.05);
  }

  .artwork-wrapper img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Vinyl variant */
  .vinyl-disc {
    position: relative;
    width: 100%;
    max-width: 380px;
    aspect-ratio: 1;
    border-radius: 50%;
    background: linear-gradient(
      135deg,
      #1a1a1a 0%,
      #0a0a0a 50%,
      #1a1a1a 100%
    );
    box-shadow:
      0 32px 64px rgba(0, 0, 0, 0.6),
      inset 0 2px 4px rgba(255, 255, 255, 0.05);
  }

  .vinyl-grooves {
    position: absolute;
    inset: 5%;
    border-radius: 50%;
    background: repeating-radial-gradient(
      circle at center,
      transparent 0px,
      transparent 2px,
      rgba(255, 255, 255, 0.03) 2px,
      rgba(255, 255, 255, 0.03) 3px
    );
  }

  .vinyl-label {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 45%;
    aspect-ratio: 1;
    border-radius: 50%;
    overflow: hidden;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .vinyl-label img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Responsive */
  @media (max-width: 1200px) {
    .artwork-wrapper,
    .vinyl-disc {
      max-width: 320px;
    }
  }

  @media (max-width: 900px) {
    .artwork-wrapper,
    .vinyl-disc {
      max-width: 280px;
    }
  }
</style>
