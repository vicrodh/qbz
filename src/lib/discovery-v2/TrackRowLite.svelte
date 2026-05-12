<script lang="ts">
  import { cachedSrc } from '$lib/actions/cachedImage';

  interface Props {
    trackId: number;
    title: string;
    artist: string;
    artwork?: string;
    isPlaying?: boolean;
    onClick?: () => void;
    onArtistClick?: () => void;
  }

  let {
    trackId,
    title,
    artist,
    artwork,
    isPlaying = false,
    onClick,
    onArtistClick,
  }: Props = $props();

  function handleRowClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('.artist-link')) return;
    onClick?.();
  }

  function handleArtist(e: MouseEvent) {
    e.stopPropagation();
    onArtistClick?.();
  }
</script>

<div
  class="track-row"
  class:is-playing={isPlaying}
  data-track-id={trackId}
  role="button"
  tabindex="0"
  onclick={handleRowClick}
  onkeydown={(e) => e.key === 'Enter' && onClick?.()}
>
  <div class="thumb">
    {#if artwork}
      <img use:cachedSrc={artwork} alt={title} loading="lazy" decoding="async" />
    {:else}
      <div class="thumb-placeholder"></div>
    {/if}
  </div>
  <div class="text">
    <div class="title">{title}</div>
    {#if onArtistClick}
      <button class="artist-link" type="button" onclick={handleArtist}>{artist}</button>
    {:else}
      <div class="artist">{artist}</div>
    {/if}
  </div>
</div>

<style>
  /* Compact track row used in the "Recently Played Tracks" grid (4×3).
     Tiny artwork + two text lines. Cero efectos. */
  .track-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 6px;
    cursor: pointer;
    background: transparent;
    border: none;
    text-align: left;
    min-width: 0;
  }

  .track-row:hover {
    background: var(--bg-tertiary);
  }

  .thumb {
    flex: 0 0 44px;
    width: 44px;
    height: 44px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    overflow: hidden;
  }

  .thumb img,
  .thumb-placeholder {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist,
  .artist-link {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
  }

  .track-row.is-playing .title {
    color: var(--accent-primary);
  }
</style>
