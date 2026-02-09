<script lang="ts">
  import { tick } from 'svelte';
  import { Play, Heart, MoreHorizontal, ListPlus, Share2, ListMusic, Trash2 } from 'lucide-svelte';
  import PlaylistCollage from './PlaylistCollage.svelte';

  interface FavoritePlaylist {
    id: number;
    name: string;
    tracks_count: number;
    images?: string[];
    duration: number;
    owner: { id: number; name: string };
  }

  interface Props {
    playlist: FavoritePlaylist;
    onclick?: () => void;
    onPlay?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onRemoveFavorite?: () => void;
    onShareQobuz?: () => void;
  }

  let {
    playlist,
    onclick,
    onPlay,
    onPlayNext,
    onPlayLater,
    onRemoveFavorite,
    onShareQobuz
  }: Props = $props();

  let menuOpen = $state(false);
  let menuTriggerRef: HTMLButtonElement | null = null;
  let menuEl = $state<HTMLDivElement | null>(null);
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

  function handlePlay(event: MouseEvent) {
    event.stopPropagation();
    onPlay?.();
  }

  function handleRemoveFavorite(event: MouseEvent) {
    event.stopPropagation();
    onRemoveFavorite?.();
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
    <PlaylistCollage artworks={playlist.images ?? []} size={140} />

    <div class="action-overlay">
      <div class="action-buttons">
        <button
          class="overlay-btn overlay-btn--minor"
          class:active={true}
          type="button"
          onclick={handleRemoveFavorite}
          title="Remove from favorites"
        >
          <Heart size={18} fill="currentColor" />
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
    <button class="menu-item" onclick={() => { onShareQobuz?.(); closeMenu(); }}>
      <Share2 size={14} /> <span>Share (Qobuz)</span>
    </button>
    <div class="menu-separator"></div>
    <button class="menu-item menu-item--danger" onclick={() => { onRemoveFavorite?.(); closeMenu(); }}>
      <Trash2 size={14} /> <span>Remove from favorites</span>
    </button>
  </div>
{/if}

<style>
  .playlist-card {
    width: 140px;
    cursor: pointer;
    user-select: none;
  }

  .artwork-container {
    width: 140px;
    height: 140px;
    border-radius: 6px;
    overflow: hidden;
    position: relative;
    background: var(--bg-tertiary);
  }

  .action-overlay {
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, rgba(0,0,0,0.3) 0%, transparent 30%, transparent 70%, rgba(0,0,0,0.6) 100%);
    opacity: 0;
    transition: opacity 150ms ease;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 10px;
  }

  .playlist-card:hover .action-overlay,
  .playlist-card.menu-open .action-overlay {
    opacity: 1;
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

  .overlay-btn--minor.active {
    color: var(--accent-primary);
    box-shadow: inset 0 0 0 1px var(--accent-primary);
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
    min-width: 180px;
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

  .menu-item--danger:hover {
    color: #ef4444;
  }

  .menu-separator {
    height: 1px;
    background-color: var(--bg-hover);
    margin: 4px 0;
  }
</style>
