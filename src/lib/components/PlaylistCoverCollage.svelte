<script lang="ts">
  interface Props {
    images?: string[] | null | undefined;
    size?: number;
  }

  let { images = [], size = 22 }: Props = $props();

  const tiles = $derived((images ?? []).filter((u) => !!u));
</script>

{#if tiles.length === 0}
  <!-- Parent renders its fallback icon -->
{:else if tiles.length < 4}
  <img
    class="single"
    src={tiles[0]}
    alt=""
    loading="lazy"
    decoding="async"
    style="width: {size}px; height: {size}px;"
  />
{:else}
  <div class="collage" style="width: {size}px; height: {size}px;">
    {#each tiles.slice(0, 4) as url}
      <img src={url} alt="" loading="lazy" decoding="async" />
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
