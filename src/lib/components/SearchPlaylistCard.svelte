<script lang="ts">
  import { tick } from 'svelte';
  import { Play, Heart, MoreHorizontal, ListPlus, Library, Share2, ListMusic } from 'lucide-svelte';
  import PlaylistCollage from './PlaylistCollage.svelte';
  import type { Playlist } from '$lib/stores/searchState';

  interface Props {
    playlist: Playlist;
    onclick?: () => void;
    onPlay?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onCopyToLibrary?: () => void;
    onShareQobuz?: () => void;
  }

  let {
    playlist,
    onclick,
    onPlay,
    onPlayNext,
    onPlayLater,
    onCopyToLibrary,
    onShareQobuz
  }: Props = $props();

  let menuOpen = $state(false);
  let menuTriggerRef: HTMLButtonElement | null = null;
  let menuEl: HTMLDivElement | null = null;
  let menuStyle = $state('');

  // Ticker animation for long titles
  let titleRef: HTMLDivElement | null = $state(null);
  let titleTextRef: HTMLSpanElement | null = $state(null);
  let titleOverflow = $state(0);
  const tickerSpeed = 40;
  const titleOffset = $derived(titleOverflow > 0 ? `-${titleOverflow + 16}px` : '0px');
  const titleDuration = $derived(titleOverflow > 0 ? `${(titleOverflow + 16) / tickerSpeed}s` : '0s');
  let overflowMeasured = false;

  function measureOverflowOnce() {
    if (!overflowMeasured && titleRef && titleTextRef) {
      const overflow = titleTextRef.scrollWidth - titleRef.clientWidth;
      titleOverflow = overflow > 0 ? overflow : 0;
      overflowMeasured = true;
    }
  }

  function getPlaylistImage(): string {
    // Prefer images300 for better quality, fallback to images150, then images
    if (playlist.images300?.length) return playlist.images300[0];
    if (playlist.images150?.length) return playlist.images150[0];
    if (playlist.images?.length) return playlist.images[0];
    return '';
  }

  function getGenreLabel(): string {
    if (playlist.genres?.length) return playlist.genres[0].name;
    return '';
  }

  function handlePlay(event: MouseEvent) {
    event.stopPropagation();
    onPlay?.();
  }

  function handleCardClick(event: MouseEvent) {
    if ((event.target as HTMLElement).closest('.action-buttons')) return;
    onclick?.();
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

  async function positionMenu() {
    await tick();
    if (!menuTriggerRef || !menuEl) return;
    const triggerRect = menuTriggerRef.getBoundingClientRect();
    const menuRect = menuEl.getBoundingClientRect();
    let left = triggerRect.right - menuRect.width;
    let top = triggerRect.bottom + 8;
    if (left < 8) left = 8;
    if (left + menuRect.width > window.innerWidth - 8) {
      left = window.innerWidth - menuRect.width - 8;
    }
    if (top + menuRect.height > window.innerHeight - 8) {
      top = triggerRect.top - menuRect.height - 8;
    }
    menuStyle = `left: ${left}px; top: ${top}px;`;
  }

  function toggleMenu(event: MouseEvent) {
    event.stopPropagation();
    menuOpen = !menuOpen;
    if (menuOpen) positionMenu();
  }

  function closeMenu() {
    menuOpen = false;
  }

  // Close menu on click outside
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
</script>

<div
  class="playlist-card"
  class:menu-open={menuOpen}
  onclick={handleCardClick}
  onmouseenter={measureOverflowOnce}
  onfocus={measureOverflowOnce}
  role="button"
  tabindex="0"
  onkeydown={(e) => e.key === 'Enter' && onclick?.()}
>
  <div class="artwork-container">
    {#if getPlaylistImage()}
      <img src={getPlaylistImage()} alt={playlist.name} loading="lazy" />
    {:else}
      <PlaylistCollage artworks={playlist.images || []} size={162} />
    {/if}

    <div class="action-overlay">
      {#if getGenreLabel()}
        <div class="overlay-info">
          <span class="overlay-genre">{getGenreLabel()}</span>
        </div>
      {/if}
      <div class="action-buttons">
        <button class="overlay-btn overlay-btn--minor" type="button" title="Add to favorites">
          <Heart size={18} />
        </button>
        <button class="overlay-btn" type="button" onclick={handlePlay} title="Play">
          <Play size={18} fill="white" />
        </button>
        <button
          class="overlay-btn overlay-btn--minor"
          type="button"
          bind:this={menuTriggerRef}
          onclick={toggleMenu}
          title="More options"
        >
          <MoreHorizontal size={18} />
        </button>
      </div>
    </div>
  </div>

  <div class="card-info">
    <div
      class="card-title"
      class:scrollable={titleOverflow > 0}
      style="--ticker-offset: {titleOffset}; --ticker-duration: {titleDuration};"
      bind:this={titleRef}
    >
      <span class="title-text" bind:this={titleTextRef}>{playlist.name}</span>
    </div>
    <div class="card-subtitle">
      <span class="owner">{playlist.owner.name}</span>
      <span class="separator">-</span>
      <span class="tracks">{playlist.tracks_count} tracks</span>
    </div>
  </div>
</div>

{#if menuOpen}
  <div class="playlist-menu" bind:this={menuEl} style={menuStyle} use:portal>
    <button class="menu-item" onclick={() => { onPlayNext?.(); closeMenu(); }}>
      <ListPlus size={14} /> <span>Play next</span>
    </button>
    <button class="menu-item" onclick={() => { onPlayLater?.(); closeMenu(); }}>
      <ListMusic size={14} /> <span>Add to queue</span>
    </button>
    <div class="menu-separator"></div>
    <button class="menu-item" onclick={() => { onCopyToLibrary?.(); closeMenu(); }}>
      <Library size={14} /> <span>Copy to library</span>
    </button>
    <div class="menu-separator"></div>
    <button class="menu-item" onclick={() => { onShareQobuz?.(); closeMenu(); }}>
      <Share2 size={14} /> <span>Share (Qobuz)</span>
    </button>
  </div>
{/if}

<style>
  .playlist-card {
    width: 162px;
    cursor: pointer;
    user-select: none;
  }

  .artwork-container {
    width: 162px;
    height: 162px;
    border-radius: 6px;
    overflow: hidden;
    position: relative;
    background: var(--bg-tertiary);
  }

  .artwork-container img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .action-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, rgba(0,0,0,0.6) 0%, transparent 40%, transparent 60%, rgba(0,0,0,0.6) 100%);
    opacity: 0;
    transition: opacity 150ms ease;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    padding: 10px;
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
    font-size: 12px;
    font-weight: 600;
    color: white;
    text-shadow: 0 1px 3px rgba(0,0,0,0.8);
  }

  .action-buttons {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
  }

  .overlay-btn {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    box-shadow: inset 0 0 0 1px rgba(255,255,255,0.85);
    transition: all 150ms ease;
  }

  .overlay-btn:hover {
    background: rgba(0,0,0,0.3);
    box-shadow: inset 0 0 0 1px var(--accent-primary);
  }

  .overlay-btn--minor {
    width: 28px;
    height: 28px;
  }

  .card-info {
    padding: 10px 4px 4px;
  }

  .card-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    white-space: nowrap;
  }

  .card-title.scrollable {
    mask-image: linear-gradient(to right, black calc(100% - 24px), transparent);
  }

  .card-title.scrollable:hover .title-text {
    animation: ticker var(--ticker-duration) linear infinite;
  }

  @keyframes ticker {
    0%, 20% { transform: translateX(0); }
    80%, 100% { transform: translateX(var(--ticker-offset)); }
  }

  .card-subtitle {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .separator {
    opacity: 0.5;
  }

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
