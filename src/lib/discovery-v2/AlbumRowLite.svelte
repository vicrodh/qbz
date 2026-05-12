<script lang="ts">
  import { Play, MoreHorizontal } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import AlbumQuickMenu from './AlbumQuickMenu.svelte';

  interface Props {
    albumId: string;
    title: string;
    artist: string;
    artwork?: string;
    quality?: string;
    /** 1-based rank shown on the left at rest; swaps to a play button on hover. */
    rank?: number;
    onClick?: () => void;
    onArtistClick?: () => void;
    onPlay?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onAddToPlaylist?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onDownload?: () => void;
  }

  let {
    albumId,
    title,
    artist,
    artwork,
    quality,
    rank,
    onClick,
    onArtistClick,
    onPlay,
    onPlayNext,
    onPlayLater,
    onAddToPlaylist,
    onShareQobuz,
    onShareSonglink,
    onDownload,
  }: Props = $props();

  let menuOpen = $state(false);
  let menuAnchor = $state<{ x: number; y: number } | null>(null);

  function handleRowClick(e: MouseEvent) {
    if (
      (e.target as HTMLElement).closest(
        '.rank-cell, .artist-link, .menu-btn'
      )
    )
      return;
    onClick?.();
  }

  function handlePlay(e: MouseEvent) {
    e.stopPropagation();
    onPlay?.();
  }

  function handleArtist(e: MouseEvent) {
    e.stopPropagation();
    onArtistClick?.();
  }

  function handleMenu(e: MouseEvent) {
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuAnchor = { x: rect.left, y: rect.bottom + 4 };
    menuOpen = true;
  }
</script>

<div
  class="album-row"
  data-album-id={albumId}
  role="button"
  tabindex="0"
  onclick={handleRowClick}
  onkeydown={(e) => e.key === 'Enter' && onClick?.()}
>
  <div class="rank-cell">
    {#if rank !== undefined}
      <span class="rank">{rank}</span>
    {/if}
    {#if onPlay}
      <button
        class="rank-play"
        type="button"
        aria-label={$t('actions.play')}
        onclick={handlePlay}
      >
        <Play size={14} fill="currentColor" />
      </button>
    {/if}
  </div>
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
  {#if quality}
    <div class="quality">{quality}</div>
  {/if}
  <button
    class="menu-btn"
    type="button"
    aria-label={$t('actions.moreActions')}
    onclick={handleMenu}
  >
    <MoreHorizontal size={16} />
  </button>
</div>

<AlbumQuickMenu
  isOpen={menuOpen}
  anchor={menuAnchor}
  onClose={() => (menuOpen = false)}
  onPlayNext={onPlayNext ? () => onPlayNext?.() : undefined}
  onPlayLater={onPlayLater ? () => onPlayLater?.() : undefined}
  onAddToPlaylist={onAddToPlaylist ? () => onAddToPlaylist?.() : undefined}
  onGoToAlbum={onClick ? () => onClick?.() : undefined}
  onGoToArtist={onArtistClick ? () => onArtistClick?.() : undefined}
  onShareQobuz={onShareQobuz ? () => onShareQobuz?.() : undefined}
  onShareSonglink={onShareSonglink ? () => onShareSonglink?.() : undefined}
  onDownload={onDownload ? () => onDownload?.() : undefined}
/>

<style>
  /* Compact album row used in the "Popular albums" 4×3 grid. Layout:
       [rank/play 32px] [44px thumb] [title/artist flex] [quality] [kebab 28px]
     At rest the rank number shows on the left; on row hover the rank
     hides and the play button takes its place. Opacity-only swap, no
     transform on parent. */
  .album-row {
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

  .album-row:hover {
    background: var(--bg-tertiary);
  }

  .rank-cell {
    position: relative;
    flex: 0 0 32px;
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .rank {
    font-size: 13px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    transition: opacity 120ms ease;
  }

  .album-row:hover .rank {
    opacity: 0;
  }

  .rank-play {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    border-radius: 50%;
    border: none;
    background: var(--accent-primary);
    color: var(--btn-primary-text, #000);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition: opacity 120ms ease;
  }

  .album-row:hover .rank-play {
    opacity: 1;
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

  .quality {
    flex: 0 0 auto;
    font-family: 'LINE Seed JP', var(--font-sans);
    font-size: 10px;
    color: var(--alpha-85);
    background: var(--alpha-10);
    border: 1px solid var(--alpha-15);
    border-radius: 3px;
    padding: 2px 5px;
    white-space: nowrap;
  }

  .menu-btn {
    flex: 0 0 28px;
    width: 28px;
    height: 28px;
    border-radius: 4px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition: opacity 120ms ease;
  }

  .album-row:hover .menu-btn {
    opacity: 1;
  }

  .menu-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
</style>
