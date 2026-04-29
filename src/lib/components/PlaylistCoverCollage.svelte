<script lang="ts">
  import { cachedSrc } from '$lib/actions/cachedImage';

  interface Props {
    images?: string[] | null | undefined;
    size?: number;
  }

  let { images = [], size = 22 }: Props = $props();

  // At 22x22 total (each tile ~11x11), the _150 variant that Qobuz
  // returns is 13× oversized. Swap to the _50 variant when the URL
  // matches Qobuz's `_<size>.jpg` pattern so each tile is ~2 KB
  // instead of ~8 KB. Non-Qobuz URLs (local library covers, data:,
  // etc.) pass through untouched.
  function downscaleQobuzCover(url: string): string {
    return url.replace(/_(150|300|600)\.jpg(\?.*)?$/i, '_50.jpg$2');
  }

  const tiles = $derived(
    (images ?? [])
      .filter((u) => !!u)
      .map(downscaleQobuzCover),
  );
</script>

{#if tiles.length === 0}
  <!-- Parent renders its fallback icon -->
{:else if tiles.length < 4}
  <img
    class="single"
    use:cachedSrc={tiles[0]}
    alt=""
    loading="lazy"
    decoding="async"
    style="width: {size}px; height: {size}px;"
  />
{:else}
  <div class="collage" style="width: {size}px; height: {size}px;">
    {#each tiles.slice(0, 4) as url}
      <img use:cachedSrc={url} alt="" loading="lazy" decoding="async" />
    {/each}
  </div>
{/if}

<style>
  .collage {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
    gap: 1px;
    border-radius: 3px;
    overflow: hidden;
    background: var(--bg-tertiary);
    flex-shrink: 0;
  }

  .collage img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .single {
    border-radius: 3px;
    object-fit: cover;
    display: block;
    flex-shrink: 0;
  }
</style>
