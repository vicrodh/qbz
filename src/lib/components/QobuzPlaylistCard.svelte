<script lang="ts">
  import { tick, onMount } from 'svelte';
  import { Play, ListMusic, Plus, MoreHorizontal, ListPlus, Library, Share2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    openMenu as openGlobalMenu,
    closeMenu as closeGlobalMenu,
    subscribe as subscribeGlobal,
    getActiveMenuId
  } from '$lib/stores/floatingMenuStore';

  interface Props {
    playlistId: number;
    name: string;
    owner: string;
    image?: string;
    trackCount?: number;
    duration?: number;
    genre?: string;
    onclick?: () => void;
    onPlay?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onCopyToLibrary?: () => void;
    onShareQobuz?: () => void;
  }

  let {
    playlistId,
    name,
    owner,
    image,
    trackCount,
    duration,
    genre,
    onclick,
    onPlay,
    onPlayNext,
    onPlayLater,
    onCopyToLibrary,
    onShareQobuz
  }: Props = $props();

  let imageError = $state(false);
  let imageLoaded = $state(false);
  let menuOpen = $state(false);
  let menuTriggerRef: HTMLButtonElement | null = $state(null);
  let menuEl: HTMLDivElement | null = $state(null);
  let menuStyle = $state('');

  const cardSize = 180;

  // Global floating menu coordination
  const menuId = `qobuz-playlist-${playlistId}-${Date.now()}`;

  onMount(() => {
    const unsubscribe = subscribeGlobal(() => {
      const activeId = getActiveMenuId();
      if (activeId !== null && activeId !== menuId && menuOpen) {
        menuOpen = false;
      }
    });
    return unsubscribe;
  });

  function handleImageError() {
    imageError = true;
  }

  function handleImageLoad() {
    imageLoaded = true;
  }

  function handlePlay(event: MouseEvent) {
    event.stopPropagation();
    onPlay?.();
  }

  function handleCopyToLibrary(event: MouseEvent) {
    event.stopPropagation();
    onCopyToLibrary?.();
  }

  function handleCardClick(event: MouseEvent) {
    const target = event.target;
    if (target instanceof HTMLElement && target.closest('.action-buttons')) return;
    onclick?.();
  }

  function formatDuration(seconds: number): string {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    if (hours > 0) {
      return `${hours}h ${minutes}m`;
    }
    return `${minutes}m`;
  }

  // Portal for menu
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        if (node.parentNode) node.parentNode.removeChild(node);
      }
    };
  }

  async function positionMenu(coords?: { x: number; y: number }) {
    await tick();
    if (!menuEl) return;
    const menuRect = menuEl.getBoundingClientRect();
    const padding = 8;
    let left: number;
    let top: number;

    if (coords) {
      left = coords.x;
      top = coords.y;
    } else if (menuTriggerRef) {
      const triggerRect = menuTriggerRef.getBoundingClientRect();
      left = triggerRect.right - menuRect.width;
      top = triggerRect.bottom + 8;
    } else {
      return;
    }

    if (left < padding) left = padding;
    if (left + menuRect.width > window.innerWidth - padding) {
      left = window.innerWidth - menuRect.width - padding;
    }
    if (top + menuRect.height > window.innerHeight - padding) {
      top = coords ? coords.y - menuRect.height : (menuTriggerRef ? menuTriggerRef.getBoundingClientRect().top - menuRect.height - 8 : padding);
      if (top < padding) top = padding;
    }
    menuStyle = `left: ${left}px; top: ${top}px;`;
  }

  function toggleMenu(event: MouseEvent) {
    event.stopPropagation();
    menuOpen = !menuOpen;
    if (menuOpen) {
      openGlobalMenu(menuId);
      positionMenu();
    } else {
      closeGlobalMenu(menuId);
    }
  }

  function closeMenu() {
    menuOpen = false;
    closeGlobalMenu(menuId);
  }

  function handleClickOutside(event: MouseEvent) {
    if (menuOpen && menuEl && !menuEl.contains(event.target as Node) &&
        menuTriggerRef && !menuTriggerRef.contains(event.target as Node)) {
      menuOpen = false;
    }
  }

  $effect(() => {
    if (menuOpen) {
      document.addEventListener('click', handleClickOutside);
      return () => document.removeEventListener('click', handleClickOutside);
    }
  });

  const hasOverlay = $derived(!!(onPlay || onCopyToLibrary || genre || trackCount || duration));
</script>

<div
  class="playlist-card"
  class:menu-open={menuOpen}
  style="width: {cardSize}px"
  onclick={handleCardClick}
  oncontextmenu={(e) => { e.preventDefault(); e.stopPropagation(); menuOpen = true; openGlobalMenu(menuId); positionMenu({ x: e.clientX, y: e.clientY }); }}
  role="button"
  tabindex="0"
  onkeydown={(e) => e.key === 'Enter' && onclick?.()}
>
  <!-- Artwork Container (Square) -->
  <div
    class="artwork-container"
    style="width: {cardSize}px; height: {cardSize}px;"
  >
    <!-- Blurred background image for color effect -->
    {#if !imageError && image}
      <div 
        class="artwork-bg" 
        style="background-image: url({image});"
      ></div>
    {/if}

    <!-- Placeholder -->
    <div class="artwork-placeholder">
      <ListMusic size={48} />
    </div>

    <!-- Main Image (contain) -->
    {#if !imageError && image}
      <img
        src={image}
        alt={name}
        class="artwork-main"
        loading="lazy"
        decoding="async"
        onerror={handleImageError}
        onload={handleImageLoad}
      />
    {/if}

    <!-- Action Overlay -->
    {#if hasOverlay}
      <div class="action-overlay">
        <!-- Overlay Info (top) -->
        <div class="overlay-info">
          {#if genre}
            <span class="overlay-genre">{genre}</span>
          {/if}
          <div class="overlay-meta">
            {#if trackCount}
              <span>{trackCount} {$t('playlist.tracks')}</span>
            {/if}
            {#if duration}
              <span>{formatDuration(duration)}</span>
            {/if}
          </div>
        </div>

        <!-- Action Buttons (bottom) -->
        <div class="action-buttons">
          {#if onCopyToLibrary}
            <button
              class="overlay-btn overlay-btn--minor"
              type="button"
              onclick={handleCopyToLibrary}
              title={$t('playlist.copyToLibrary')}
            >
              <Plus size={18} />
            </button>
          {:else}
            <div class="overlay-btn--spacer"></div>
          {/if}

          {#if onPlay}
            <button class="overlay-btn" type="button" onclick={handlePlay} title={$t('actions.play')}>
              <Play size={18} fill="white" color="white" />
            </button>
          {/if}

          <button
            class="overlay-btn overlay-btn--minor"
            type="button"
            bind:this={menuTriggerRef}
            onclick={toggleMenu}
            title={$t('actions.moreOptions')}
          >
            <MoreHorizontal size={18} />
          </button>
        </div>
      </div>
    {/if}
  </div>

  <!-- Text Info -->
  <div class="info">
    <div class="title" title={name}>{name}</div>
    <div class="owner">{owner}</div>
  </div>
</div>

<!-- Context Menu Portal -->
{#if menuOpen}
  <div class="playlist-menu" bind:this={menuEl} style={menuStyle} use:portal>
    {#if onPlayNext}
      <button class="menu-item" onclick={() => { onPlayNext(); closeMenu(); }}>
        <ListPlus size={14} /> <span>{$t('player.playNext')}</span>
      </button>
    {/if}
    {#if onPlayLater}
      <button class="menu-item" onclick={() => { onPlayLater(); closeMenu(); }}>
        <ListMusic size={14} /> <span>{$t('player.addToQueue')}</span>
      </button>
    {/if}
    {#if (onPlayNext || onPlayLater) && (onCopyToLibrary || onShareQobuz)}
      <div class="menu-separator"></div>
    {/if}
    {#if onCopyToLibrary}
      <button class="menu-item" onclick={() => { onCopyToLibrary(); closeMenu(); }}>
        <Library size={14} /> <span>{$t('playlist.copyToLibrary')}</span>
      </button>
    {/if}
    {#if onCopyToLibrary && onShareQobuz}
      <div class="menu-separator"></div>
    {/if}
    {#if onShareQobuz}
      <button class="menu-item" onclick={() => { onShareQobuz(); closeMenu(); }}>
        <Share2 size={14} /> <span>{$t('actions.shareQobuz')}</span>
      </button>
    {/if}
  </div>
{/if}

<style>
  .playlist-card {
    flex-shrink: 0;
    cursor: pointer;
    transition: transform 150ms ease;
  }

  .artwork-container {
    position: relative;
    margin-bottom: 8px;
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-tertiary);
  }

  /* Blurred background - simulates dominant color */
  .artwork-bg {
    position: absolute;
    inset: -20px;
    background-size: cover;
    background-position: center;
    filter: blur(30px) saturate(1.5);
    transform: scale(1.2);
    z-index: 0;
  }

  /* Darken overlay on blurred bg */
  .artwork-bg::after {
    content: '';
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.25);
  }

  .artwork-main {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: inherit;
    z-index: 1;
  }

  .artwork-placeholder {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    z-index: 0;
  }

  /* Hide placeholder when image exists */
  .artwork-container:has(.artwork-bg) .artwork-placeholder {
    display: none;
  }

  .action-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, rgba(0,0,0,0.7) 0%, transparent 40%, transparent 60%, rgba(0,0,0,0.7) 100%);
    opacity: 0;
    transition: opacity 150ms ease;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: 8px 12px 12px;
    border-radius: inherit;
    z-index: 2;
  }

  .playlist-card:hover .action-overlay,
  .playlist-card.menu-open .action-overlay {
    opacity: 1;
  }

  .overlay-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .overlay-genre {
    font-size: 14px;
    font-weight: 600;
    color: white;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.8);
    word-wrap: break-word;
    overflow-wrap: break-word;
  }

  .overlay-meta {
    display: flex;
    gap: 8px;
    font-size: 12px;
    font-weight: 400;
    color: rgba(255, 255, 255, 0.85);
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.8);
  }

  .overlay-meta span:not(:last-child)::after {
    content: '\2022';
    margin-left: 8px;
    opacity: 0.6;
  }

  .action-buttons {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
  }

  .overlay-btn {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.85), 0 0 1px rgba(0, 0, 0, 0.3);
    transition: transform 150ms ease, background-color 150ms ease, box-shadow 150ms ease;
  }

  .overlay-btn:hover {
    background-color: rgba(0, 0, 0, 0.3);
    box-shadow: inset 0 0 0 1px var(--accent-primary), 0 0 4px rgba(0, 0, 0, 0.5);
  }

  .overlay-btn--minor {
    width: 30px;
    height: 30px;
  }

  .overlay-btn--spacer {
    width: 30px;
    height: 30px;
    visibility: hidden;
  }

  .info {
    width: 100%;
  }

  .title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.4;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-bottom: 2px;
  }

  .owner {
    font-size: 12px;
    font-weight: 400;
    color: var(--text-muted);
    line-height: 1.4;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Context Menu */
  .playlist-menu {
    position: fixed;
    z-index: 30000;
    min-width: 160px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 2px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .menu-item {
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-secondary);
    text-align: left;
    font-size: 12px;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 8px;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .menu-item:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .menu-separator {
    height: 1px;
    background-color: var(--bg-hover);
    margin: 4px 0;
  }
</style>
