<script lang="ts">
  import { Play, ListMusic, MoreHorizontal } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { extractPalette } from '$lib/utils/artworkPalette';
  import AlbumQuickMenu from './AlbumQuickMenu.svelte';

  interface Props {
    playlistId: number;
    name: string;
    image?: string;
    onClick?: () => void;
    onPlay?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onCopyToLibrary?: () => void;
    onShareQobuz?: () => void;
  }

  let {
    playlistId,
    name,
    image,
    onClick,
    onPlay,
    onPlayNext,
    onPlayLater,
    onCopyToLibrary,
    onShareQobuz,
  }: Props = $props();

  // Playlist covers are often non-square (rectangle). object-fit:contain
  // keeps the full image visible; we letterbox the gaps with the cover's
  // dominant color so the card doesn't look like an island floating in
  // theme-grey. Palette extraction is cached in artworkPalette.ts so
  // repeated playlists across sections re-use the result.
  let dominantBg = $state<string | undefined>(undefined);

  $effect(() => {
    if (!image) {
      dominantBg = undefined;
      return;
    }
    let cancelled = false;
    void extractPalette(image).then((palette) => {
      if (cancelled) return;
      dominantBg = palette.dominant?.hex ?? undefined;
    });
    return () => {
      cancelled = true;
    };
  });

  let menuOpen = $state(false);
  let menuAnchor = $state<{ x: number; y: number } | null>(null);

  function handleCardClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('.overlay-btn')) return;
    onClick?.();
  }

  function handlePlay(e: MouseEvent) {
    e.stopPropagation();
    onPlay?.();
  }

  function handleMenu(e: MouseEvent) {
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuAnchor = { x: rect.left, y: rect.bottom + 4 };
    menuOpen = true;
  }
</script>

<div
  class="card"
  data-playlist-id={playlistId}
  role="button"
  tabindex="0"
  onclick={handleCardClick}
  onkeydown={(e) => e.key === 'Enter' && onClick?.()}
>
  <div
    class="cover-wrap"
    style:background-color={dominantBg ?? 'var(--bg-tertiary)'}
  >
    {#if image}
      <img class="cover" use:cachedSrc={image} alt={name} loading="lazy" decoding="async" />
    {:else}
      <div class="cover cover-placeholder">
        <ListMusic size={48} />
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
        type="button"
        aria-label={$t('actions.moreActions')}
        onclick={handleMenu}
      >
        <MoreHorizontal size={16} />
      </button>
    </div>
  </div>
  <div class="title">{name}</div>
</div>

<AlbumQuickMenu
  isOpen={menuOpen}
  anchor={menuAnchor}
  onClose={() => (menuOpen = false)}
  onPlayNext={onPlayNext ? () => onPlayNext?.() : undefined}
  onPlayLater={onPlayLater ? () => onPlayLater?.() : undefined}
  onCopyToLibrary={onCopyToLibrary ? () => onCopyToLibrary?.() : undefined}
  onShareQobuz={onShareQobuz ? () => onShareQobuz?.() : undefined}
/>

<style>
  /* PlaylistCardLite — mirrors AlbumCardLite's hover overlay pattern:
     dark scrim + slide-up centered actions. Playlist-specific shape
     differences:
       - object-fit: contain on the cover (rectangles + portraits common
         for Qobuz/AppleMusic-styled playlists; cropping butchers them).
       - Dominant-color background fills letterbox space (extractPalette
         via the shared artworkPalette utility).
       - Two actions instead of three (no heart for V1; no playlist
         favorites store on the frontend yet — copy-to-library lives in
         the kebab menu instead).
  */
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
    border-radius: 6px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .cover {
    max-width: 100%;
    max-height: 100%;
    width: auto;
    height: auto;
    object-fit: contain;
    display: block;
  }

  .cover-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  /* Solid scrim covering the cover on hover. Single ::after with opacity
     transition — same cheap pattern as AlbumCardLite. */
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

  /* Centered action group with slide-up entrance. No ribbon on playlists,
     so we can keep the actions vertically centered (no 44px clearance
     needed like on album cards). */
  .actions {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%) translateY(10px);
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
    transform: translate(-50%, -50%) translateY(0);
    pointer-events: auto;
  }

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
    transition: background-color 120ms ease, transform 120ms ease;
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

  .title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
