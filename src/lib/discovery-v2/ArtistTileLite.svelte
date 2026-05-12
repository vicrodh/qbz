<script lang="ts">
  import { User, UserPlus, LoaderCircle } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';

  interface Props {
    artistId: number;
    name: string;
    image?: string;
    /** When provided, renders a Follow button below the name (the
     *  "Artists to Follow" use case). Top Artists pass no callback so
     *  the button doesn't render. */
    onFollow?: () => void;
    isFollowing?: boolean;
    onClick?: () => void;
  }

  let {
    artistId,
    name,
    image,
    onFollow,
    isFollowing = false,
    onClick,
  }: Props = $props();

  function handleTileClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('.follow-btn')) return;
    onClick?.();
  }

  function handleFollow(e: MouseEvent) {
    e.stopPropagation();
    onFollow?.();
  }
</script>

<div
  class="tile"
  role="button"
  tabindex="0"
  onclick={handleTileClick}
  onkeydown={(e) => e.key === 'Enter' && onClick?.()}
>
  <div class="avatar">
    {#if image}
      <img class="img" src={image} use:cachedSrc={image} alt={name} loading="lazy" decoding="async" />
    {:else}
      <div class="placeholder"><User size={36} /></div>
    {/if}
  </div>
  <div class="name">{name}</div>
  {#if onFollow}
    <span class="role">{$t('home.spotlightArtist')}</span>
    <button
      class="follow-btn"
      type="button"
      onclick={handleFollow}
      disabled={isFollowing}
    >
      {#if isFollowing}
        <LoaderCircle size={14} class="spinner" />
      {:else}
        <UserPlus size={14} />
      {/if}
      <span>{$t('home.followArtist')}</span>
    </button>
  {/if}
</div>

<style>
  /* Circular artist tile. Cero efectos beyond a low-cost background tint
     on hover so the cursor reads as interactive. When `onFollow` is
     supplied, a small Follow button mounts below the name (this is the
     Artists to Follow card variant). */
  .tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 170px;
    cursor: pointer;
    background: transparent;
    border: none;
    padding: 12px;
    text-align: center;
    border-radius: 8px;
    transition: background-color 120ms ease;
  }

  .tile:hover {
    background: var(--bg-tertiary);
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

  .role {
    font-size: 11px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  /* Follow button — small chip, only on the Artists-to-Follow variant.
     The disabled state shows a spinner inline. */
  .follow-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
    padding: 6px 14px;
    background: var(--bg-tertiary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 16px;
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 500;
    font-family: inherit;
    cursor: pointer;
  }

  .tile:hover .follow-btn {
    background: var(--bg-hover, var(--bg-secondary));
  }

  .follow-btn:disabled {
    opacity: 0.7;
    cursor: default;
  }
</style>
