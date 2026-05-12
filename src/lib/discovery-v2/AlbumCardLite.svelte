<script lang="ts">
  import { Play, Heart, MoreHorizontal } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import type { AlbumRibbon } from './data';
  import AlbumQuickMenu from './AlbumQuickMenu.svelte';

  interface Props {
    albumId: string;
    title: string;
    artist: string;
    artwork?: string;
    quality?: string;
    ribbon?: AlbumRibbon;
    genre?: string;
    releaseYear?: number;
    isPlaying?: boolean;
    isFavorite?: boolean;
    onClick?: () => void;
    onArtistClick?: () => void;
    onPlay?: () => void;
    onFavorite?: () => void;
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
    ribbon,
    genre,
    releaseYear,
    isPlaying = false,
    isFavorite = false,
    onClick,
    onArtistClick,
    onPlay,
    onFavorite,
    onPlayNext,
    onPlayLater,
    onAddToPlaylist,
    onShareQobuz,
    onShareSonglink,
    onDownload,
  }: Props = $props();

  // Quick-menu state. Position carries the click coordinates so the
  // portaled popover anchors near the kebab button.
  let menuOpen = $state(false);
  let menuAnchor = $state<{ x: number; y: number } | null>(null);

  function handleCardClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('.overlay-btn, .artist-link')) return;
    onClick?.();
  }

  function handlePlay(e: MouseEvent) {
    e.stopPropagation();
    onPlay?.();
  }

  function handleFavorite(e: MouseEvent) {
    e.stopPropagation();
    onFavorite?.();
  }

  function handleMenu(e: MouseEvent) {
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuAnchor = { x: rect.left, y: rect.bottom + 4 };
    menuOpen = true;
  }

  function handleArtist(e: MouseEvent) {
    e.stopPropagation();
    onArtistClick?.();
  }
</script>

<div
  class="card"
  class:is-playing={isPlaying}
  data-album-id={albumId}
  role="button"
  tabindex="0"
  onclick={handleCardClick}
  onkeydown={(e) => e.key === 'Enter' && onClick?.()}
>
  <div class="cover-wrap">
    {#if artwork}
      <img class="cover" use:cachedSrc={artwork} alt={title} loading="lazy" decoding="async" />
    {:else}
      <div class="cover cover-placeholder"></div>
    {/if}
    {#if ribbon}
      <div class="ribbon ribbon-{ribbon.kind}" title={ribbon.label}>{ribbon.label}</div>
    {/if}
    {#if genre || releaseYear}
      <div class="meta">
        {#if genre}<span class="meta-genre">{genre}</span>{/if}
        {#if releaseYear}<span class="meta-year">{releaseYear}</span>{/if}
      </div>
    {/if}
    <div class="actions">
      <button
        class="overlay-btn overlay-btn-primary"
        type="button"
        aria-label={$t('actions.play')}
        onclick={handlePlay}
      >
        <Play size={18} fill="currentColor" />
      </button>
      <button
        class="overlay-btn"
        class:is-favorite={isFavorite}
        type="button"
        aria-label={$t('actions.toggleFavorite')}
        onclick={handleFavorite}
      >
        <Heart size={16} fill={isFavorite ? 'currentColor' : 'none'} />
      </button>
      <button
        class="overlay-btn"
        type="button"
        aria-label={$t('actions.moreActions')}
        onclick={handleMenu}
      >
        <MoreHorizontal size={16} />
      </button>
    </div>
  </div>
  <div class="title">{title}</div>
  {#if onArtistClick}
    <button class="artist-link" type="button" onclick={handleArtist}>{artist}</button>
  {:else}
    <div class="artist">{artist}</div>
  {/if}
  {#if quality}
    <div class="quality">{quality}</div>
  {/if}
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
  /* Discovery V2 — zero effects.
     No transitions, no hover paint, no will-change, no backdrop-filter,
     no animation, no absolute decoration. Five elements per card. */
  .card {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 220px;
    cursor: pointer;
    background: transparent;
    border: none;
    padding: 0;
    text-align: left;
  }

  .cover-wrap {
    position: relative;
    width: 220px;
    height: 220px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    overflow: hidden;
  }

  /* Qobuz-style hover overlay: solid dark scrim covers the whole cover,
     opacity 0 → 1 on card hover. Single `::after`, no blur, no filter,
     no transform on parent. Pure opacity transition. The action buttons
     ride on top via a separate `.actions` container with its own opacity. */
  .cover-wrap::after {
    content: '';
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    opacity: 0;
    transition: opacity 150ms ease;
    pointer-events: none;
  }

  .card:hover .cover-wrap::after {
    opacity: 1;
  }

  .cover {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .cover-placeholder {
    width: 100%;
    height: 100%;
  }

  /* Hover meta (genre + year), top-left of the cover. Shows opposite-corner
     from the ribbon (top-left ribbon doesn't conflict since press/award
     ribbons are visually distinct enough; if both are present they stack
     vertically with the ribbon taking precedence at the very top). */
  .meta {
    position: absolute;
    top: 12px;
    right: 12px;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
    opacity: 0;
    transition: opacity 150ms ease;
    pointer-events: none;
    z-index: 1;
    color: rgba(255, 255, 255, 0.92);
    font-size: 12px;
    text-align: right;
    max-width: 60%;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.6);
  }

  .meta-genre {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .meta-year {
    font-size: 11px;
    opacity: 0.8;
  }

  .card:hover .meta {
    opacity: 1;
  }

  /* Action buttons — bottom-center of the cover, visible only on hover.
     Slide-up entrance (translateY 10px → 0) layered with opacity transition.
     Transform-only animation on a single element; no layout reflow, no
     paint propagation to siblings. */
  .actions {
    position: absolute;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%) translateY(10px);
    display: flex;
    align-items: center;
    gap: 12px;
    opacity: 0;
    transition: opacity 150ms ease, transform 150ms ease;
    pointer-events: none;
    z-index: 1;
  }

  .card:hover .actions {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
    pointer-events: auto;
  }

  /* Per-button styling. Outline-only circles like the Qobuz player, with
     accent fill for the primary play. Subtle hover scale on each — no
     layout reflow, transform-only. */
  .overlay-btn {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    border: 1.5px solid rgba(255, 255, 255, 0.9);
    background: transparent;
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
    transition: background-color 120ms ease, transform 120ms ease,
      color 120ms ease, border-color 120ms ease;
  }

  .overlay-btn:hover {
    background-color: rgba(255, 255, 255, 0.15);
    transform: scale(1.08);
  }

  .overlay-btn-primary {
    width: 44px;
    height: 44px;
    background: #fff;
    color: #000;
    border-color: #fff;
  }

  .overlay-btn-primary:hover {
    background: #fff;
    color: #000;
    transform: scale(1.08);
  }

  .overlay-btn.is-favorite {
    color: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist,
  .artist-link {
    font-size: 13px;
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

  .card.is-playing .title {
    color: var(--accent-primary);
  }

  /* Press / award ribbon. Three variants from the original AlbumCard:
     - press: solid gold gradient with dark readable text (most common)
     - qobuzissime: dark scrim with purple accent border
     - albumOfTheWeek: dark scrim with yellow accent border. */
  .ribbon {
    position: absolute;
    top: 8px;
    left: 0;
    padding: 4px 10px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: #fff;
    background: rgba(0, 0, 0, 0.88);
    border-top-right-radius: 3px;
    border-bottom-right-radius: 3px;
    border-left: 3px solid var(--accent-primary);
    pointer-events: none;
    max-width: calc(100% - 12px);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ribbon.ribbon-albumOfTheWeek {
    border-left-color: #eab308;
  }

  .ribbon.ribbon-qobuzissime {
    border-left-color: #8b5cf6;
  }

  .ribbon.ribbon-press {
    background: linear-gradient(135deg, #f5c042 0%, #d49511 100%);
    color: #1f1407;
    border-left: none;
    padding-left: 10px;
    text-shadow: 0 1px 0 rgba(255, 255, 255, 0.15);
  }

  /* Hi-Res / CD Quality badge. Sits at the bottom of the card, below the
     artist line. Small, static, no animation. */
  .quality {
    margin-top: 4px;
    font-family: 'LINE Seed JP', var(--font-sans);
    font-size: 10px;
    font-weight: 400;
    color: var(--alpha-85);
    background: var(--alpha-10);
    border: 1px solid var(--alpha-15);
    border-radius: 4px;
    padding: 3px 6px;
    align-self: flex-start;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
