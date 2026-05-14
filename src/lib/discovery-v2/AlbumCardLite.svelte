<script lang="ts">
  import { Play, Heart, MoreHorizontal } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import type { AlbumRibbon } from './data';
  import AlbumQuickMenu from './AlbumQuickMenu.svelte';
  import QualityBadgeStatic from '$lib/components/QualityBadgeStatic.svelte';

  interface Props {
    albumId: string;
    title: string;
    artist: string;
    artwork?: string;
    quality?: string;
    isHiRes?: boolean;
    /** Exact stored values so the quality-badge tooltip is accurate
     *  ("Hi-Res: 24-bit / 96 kHz" instead of falling back to the
     *  generic defaults derived from the quality string). */
    bitDepth?: number;
    samplingRate?: number;
    ribbon?: AlbumRibbon;
    genre?: string;
    releaseYear?: number;
    isPlaying?: boolean;
    isFavorite?: boolean;
    /** Album is already cached offline. When true, the kebab menu
     *  shows "Refresh offline copy" (`onReDownloadAlbum`) instead of
     *  "Make available offline" (`onDownload`). The flag drives only
     *  the menu copy/action; visual state of the card is unaffected. */
    isAlbumFullyDownloaded?: boolean;
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
    onReDownloadAlbum?: () => void;
  }

  let {
    albumId,
    title,
    artist,
    artwork,
    quality,
    isHiRes = false,
    bitDepth,
    samplingRate,
    ribbon,
    genre,
    releaseYear,
    isPlaying = false,
    isFavorite = false,
    isAlbumFullyDownloaded = false,
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
    onReDownloadAlbum,
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
  <div class="meta-row">
    <div class="text-stack">
      <div class="title">{title}</div>
      {#if onArtistClick}
        <button class="artist-link" type="button" onclick={handleArtist}>{artist}</button>
      {:else}
        <div class="artist">{artist}</div>
      {/if}
    </div>
    <QualityBadgeStatic iconOnly {quality} {bitDepth} {samplingRate} />
  </div>
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
  onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum?.() : undefined}
  {isAlbumFullyDownloaded}
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
    /* Absolute-positioned + inset:0 strictly bounds the image to
       cover-wrap's 220x220 box even when `cachedSrc` applies a
       `transform: translateZ(0)` to it for the WebKit 2.50+ texture-
       eviction workaround. Without absolute positioning, certain
       portrait-aspect artwork was visually escaping the `overflow:
       hidden` on cover-wrap and bleeding into the row above (user
       report 2026-05-12, SearchView Albums grid). */
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .cover-placeholder {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  /* Hover meta (genre + year). Anchored top-LEFT now that the award
     ribbon moved to the bottom-left edge — they no longer compete for
     the same corner. Left-aligned so genre + year read naturally as a
     top-of-cover annotation. */
  .meta {
    position: absolute;
    top: 12px;
    left: 12px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    opacity: 0;
    transition: opacity 150ms ease;
    pointer-events: none;
    z-index: 1;
    color: rgba(255, 255, 255, 0.92);
    font-size: 14px;
    text-align: left;
    max-width: 70%;
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
    font-size: 13px;
    opacity: 0.8;
  }

  .card:hover .meta {
    opacity: 1;
  }

  /* Action buttons — bottom-center of the cover, visible only on hover.
     Slide-up entrance (translateY 10px → 0) layered with opacity transition.
     Transform-only animation on a single element; no layout reflow, no
     paint propagation to siblings. */
  /* Action buttons raised so they sit above the bottom-anchored ribbon
     (ribbon height ~24px including padding). 20px base + 24px ribbon
     clearance = 44px. */
  .actions {
    position: absolute;
    bottom: 44px;
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

  /* Meta row hosts the two-line text stack (title + artist) on the
     left and the quality badge on the right. The badge is centered
     vertically across both lines, matching the height of the combined
     text stack. The card's parent gap (4px) applies once between the
     cover and this row, so the badge inherits the same separation
     from the album art that the text does. */
  .meta-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .text-stack {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
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
     - albumOfTheWeek: dark scrim with yellow accent border.
     Anchored bottom-left so the top of the artwork (often the most
     important part of cover art) stays uncovered. */
  .ribbon {
    position: absolute;
    bottom: 8px;
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

</style>
