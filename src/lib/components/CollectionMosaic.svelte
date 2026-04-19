<script lang="ts">
  import { CassetteTape, LibraryBig, User } from 'lucide-svelte';
  import type { MixtapeCollectionItem, CollectionKind } from '$lib/stores/mixtapeCollectionsStore';

  interface Props {
    items: MixtapeCollectionItem[];
    size: number;
    kind: CollectionKind;
    artistAvatarUrl?: string;
  }

  let { items, size, kind, artistAvatarUrl }: Props = $props();

  /** 3×3 is reserved for large Collection shelves; everything else uses 2×2. */
  const cols = $derived(
    kind === 'collection' && items.length >= 9 ? 3 : 2
  );
  const cellCount = $derived(cols * cols);

  /** Fill with real items, pad with empty placeholders to cellCount. */
  const cells = $derived(
    Array.from({ length: cellCount }, (_, i) => items[i] ?? null),
  );

  const hasArtistOverlay = $derived(
    kind === 'artist_collection' && !!artistAvatarUrl,
  );

  const avatarSize = $derived(Math.max(40, Math.round(size * 0.22)));
  const iconSize = $derived(Math.round(size * 0.4));
</script>

<div
  class="mosaic"
  style:width="{size}px"
  style:height="{size}px"
  style:--cols={cols}
>
  {#if items.length === 0}
    <div class="empty-mosaic">
      {#if kind === 'mixtape'}
        <CassetteTape size={iconSize} />
      {:else if kind === 'artist_collection'}
        <User size={iconSize} />
      {:else}
        <LibraryBig size={iconSize} />
      {/if}
    </div>
  {:else}
    {#each cells as item, i (i)}
      {#if item?.artwork_url}
        <img
          class="cell"
          src={item.artwork_url}
          alt=""
          loading="lazy"
        />
      {:else}
        <div class="cell empty-cell"></div>
      {/if}
    {/each}
  {/if}

  {#if hasArtistOverlay}
    <div
      class="artist-overlay"
      style:width="{avatarSize}px"
      style:height="{avatarSize}px"
      style:background-image="url({artistAvatarUrl})"
    ></div>
  {/if}
</div>

<style>
  .mosaic {
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--cols), 1fr);
    grid-template-rows: repeat(var(--cols), 1fr);
    gap: 2px;
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-tertiary);
  }

  .cell {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .empty-cell {
    background-color: var(--bg-tertiary);
  }

  .empty-mosaic {
    grid-column: 1 / -1;
    grid-row: 1 / -1;
    display: grid;
    place-items: center;
    color: var(--text-muted);
    opacity: 0.6;
  }

  .artist-overlay {
    position: absolute;
    bottom: 8px;
    right: 8px;
    border-radius: 50%;
    background-color: var(--bg-tertiary);
    background-size: cover;
    background-position: center;
    border: 3px solid var(--bg-secondary);
  }
</style>
