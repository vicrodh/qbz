<script lang="ts">
  import { Music } from 'lucide-svelte';

  interface Props {
    artworks: string[];  // Album art URLs from tracks
    size?: number;       // Total width in px
    class?: string;      // Additional CSS classes
  }

  let {
    artworks = [],
    size = 120,
    class: className = ''
  }: Props = $props();

  // Get unique artworks (dedupe same album covers)
  const uniqueArtworks = $derived(() => {
    const seen = new Set<string>();
    return artworks.filter(art => {
      if (!art || seen.has(art)) return false;
      seen.add(art);
      return true;
    }).slice(0, 4);
  });

  const count = $derived(uniqueArtworks().length);
</script>

<div
  class="collage {className}"
  class:single={count === 1}
  class:empty={count === 0}
  style="--size: {size}px; --count: {count}"
>
  {#if count === 0}
    <!-- Empty placeholder -->
    <div class="placeholder">
      <Music size={size * 0.3} />
    </div>
  {:else if count === 1}
    <!-- Single cover -->
    <img src={uniqueArtworks()[0]} alt="" class="single-cover" />
  {:else}
    <!-- Collage with overlap -->
    {#each uniqueArtworks() as art, i}
      <img
        src={art}
        alt=""
        class="cover"
        style="--index: {i}"
      />
    {/each}
  {/if}
</div>

<style>
  .collage {
    position: relative;
    width: var(--size);
    height: calc(var(--size) * 0.65);
    overflow: hidden;
    border-radius: 6px;
    background: var(--bg-tertiary);
    flex-shrink: 0;
  }

  .collage.single,
  .collage.empty {
    height: var(--size);
  }

  .placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    background: var(--bg-tertiary);
  }

  .single-cover {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .cover {
    position: absolute;
    top: 0;
    height: 100%;
    width: auto;
    aspect-ratio: 1;
    object-fit: cover;
    /* Overlap calculation: each cover shifts right by percentage of container width */
    left: calc(var(--index) * (var(--size) * 0.22));
    /* First cover on top (highest z-index), last on bottom */
    z-index: calc(4 - var(--index));
    /* Shadow on right edge for depth */
    box-shadow: 4px 0 12px rgba(0, 0, 0, 0.4);
    border-radius: 2px;
  }

  /* Adjust for 2 covers - more spacing */
  .collage[style*="--count: 2"] .cover {
    left: calc(var(--index) * (var(--size) * 0.35));
  }

  /* Adjust for 3 covers */
  .collage[style*="--count: 3"] .cover {
    left: calc(var(--index) * (var(--size) * 0.28));
  }
</style>
