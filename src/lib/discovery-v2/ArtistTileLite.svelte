<script lang="ts">
  import { User } from 'lucide-svelte';

  interface Props {
    artistId: number;
    name: string;
    image?: string;
    onClick?: () => void;
  }

  let { artistId, name, image, onClick }: Props = $props();
</script>

<div
  class="tile"
  data-artist-id={artistId}
  role="button"
  tabindex="0"
  onclick={onClick}
  onkeydown={(e) => e.key === 'Enter' && onClick?.()}
>
  <div class="avatar">
    {#if image}
      <img class="img" src={image} alt={name} loading="lazy" decoding="async" />
    {:else}
      <div class="placeholder"><User size={36} /></div>
    {/if}
  </div>
  <div class="name">{name}</div>
</div>

<style>
  /* Circular artist tile. No play count, no hover state. Cero efectos. */
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 140px;
    cursor: pointer;
    background: transparent;
    border: none;
    padding: 0;
    text-align: center;
  }

  .avatar {
    width: 140px;
    height: 140px;
    border-radius: 50%;
    overflow: hidden;
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .placeholder {
    color: var(--text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }
</style>
