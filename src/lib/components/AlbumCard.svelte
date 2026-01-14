<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { Play, Disc3, Heart } from 'lucide-svelte';
  import AlbumMenu from './AlbumMenu.svelte';
  import {
    subscribe as subscribeAlbumFavorites,
    isAlbumFavorite,
    loadAlbumFavorites,
    toggleAlbumFavorite
  } from '$lib/stores/albumFavoritesStore';

  interface Props {
    albumId?: string;
    artwork: string;
    title: string;
    artist: string;
    quality?: string;
    size?: 'standard' | 'large';
    onclick?: () => void;
    onPlay?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onDownload?: () => void;
    showFavorite?: boolean;
    favoriteEnabled?: boolean;
  }

  let {
    albumId,
    artwork,
    title,
    artist,
    quality,
    size = 'standard',
    onclick,
    onPlay,
    onPlayNext,
    onPlayLater,
    onShareQobuz,
    onShareSonglink,
    onDownload,
    showFavorite,
    favoriteEnabled
  }: Props = $props();

  let imageError = $state(false);
  const cardSize = $derived(size === 'large' ? 180 : 162);
  let titleRef: HTMLDivElement | null = $state(null);
  let titleTextRef: HTMLSpanElement | null = $state(null);
  let titleOverflow = $state(0);
  let artistRef: HTMLDivElement | null = $state(null);
  let artistTextRef: HTMLSpanElement | null = $state(null);
  let artistOverflow = $state(0);
  const titleOffset = $derived(titleOverflow > 0 ? `-${titleOverflow + 16}px` : '0px');
  const artistOffset = $derived(artistOverflow > 0 ? `-${artistOverflow + 16}px` : '0px');
  const tickerSpeed = 80;
  const titleDuration = $derived(titleOverflow > 0 ? `${(titleOverflow + 16) / tickerSpeed}s` : '0s');
  const artistDuration = $derived(artistOverflow > 0 ? `${(artistOverflow + 16) / tickerSpeed}s` : '0s');

  let favoriteFromStore = $state(false);
  const isFavorite = $derived(albumId ? favoriteFromStore : false);
  const hasMenu = $derived(!!(onPlayNext || onPlayLater || onShareQobuz || onShareSonglink || onDownload));
  const showFavoriteButton = $derived(showFavorite ?? !!albumId);
  const favoriteAvailable = $derived(favoriteEnabled ?? !!albumId);
  const hasOverlay = $derived(!!(showFavoriteButton || onPlay || hasMenu));

  function handleImageError() {
    imageError = true;
  }

  async function handleToggleFavorite(event: MouseEvent) {
    event.stopPropagation();
    if (!albumId || !favoriteAvailable) return;
    await toggleAlbumFavorite(albumId);
  }

  function handlePlay(event: MouseEvent) {
    event.stopPropagation();
    onPlay?.();
  }

  function updateOverflow() {
    if (titleRef && titleTextRef) {
      const overflow = titleTextRef.scrollWidth - titleRef.clientWidth;
      titleOverflow = overflow > 0 ? overflow : 0;
    }
    if (artistRef && artistTextRef) {
      const overflow = artistTextRef.scrollWidth - artistRef.clientWidth;
      artistOverflow = overflow > 0 ? overflow : 0;
    }
  }

  onMount(() => {
    updateOverflow();
    const observer = new ResizeObserver(() => updateOverflow());
    if (titleRef) {
      observer.observe(titleRef);
    }
    if (artistRef) {
      observer.observe(artistRef);
    }

    if (albumId) {
      void loadAlbumFavorites();
      favoriteFromStore = isAlbumFavorite(albumId);
      const unsubscribe = subscribeAlbumFavorites(() => {
        favoriteFromStore = isAlbumFavorite(albumId);
      });
      return () => {
        observer.disconnect();
        unsubscribe();
      };
    }
    return () => observer.disconnect();
  });

  $effect(() => {
    title;
    tick().then(updateOverflow);
  });

  $effect(() => {
    artist;
    tick().then(updateOverflow);
  });
</script>

<div
  class="album-card"
  style="width: {cardSize}px"
  onclick={onclick}
  role="button"
  tabindex="0"
  onkeydown={(e) => e.key === 'Enter' && onclick?.()}
>
  <!-- Artwork Container -->
  <div
    class="artwork-container"
    style="width: {cardSize}px; height: {cardSize}px"
  >
    {#if imageError || !artwork}
      <div class="artwork-placeholder">
        <Disc3 size={48} />
      </div>
    {:else}
      <img src={artwork} alt={title} loading="lazy" decoding="async" onerror={handleImageError} />
    {/if}

    <!-- Action Overlay -->
    {#if hasOverlay}
      <div class="action-overlay">
        <div class="action-buttons">
          {#if showFavoriteButton}
            <button
              class="overlay-btn"
              class:is-active={isFavorite}
              class:disabled={!favoriteAvailable}
              type="button"
              aria-disabled={!favoriteAvailable}
              disabled={!favoriteAvailable}
              onclick={handleToggleFavorite}
              title={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
            >
              <Heart size={18} fill={isFavorite ? 'white' : 'none'} color="white" />
            </button>
          {/if}
          {#if onPlay}
            <button class="overlay-btn" type="button" onclick={handlePlay} title="Play">
              <Play size={18} fill="white" color="white" />
            </button>
          {/if}
          {#if hasMenu}
            <div class="overlay-menu">
              <AlbumMenu
                onPlayNext={onPlayNext}
                onPlayLater={onPlayLater}
                onShareQobuz={onShareQobuz}
                onShareSonglink={onShareSonglink}
                onDownload={onDownload}
              />
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>

  <!-- Text Info -->
  <div class="info">
    <div
      class="title"
      class:scrollable={titleOverflow > 0}
      style="--ticker-offset: {titleOffset}; --ticker-duration: {titleDuration};"
      bind:this={titleRef}
    >
      <span class="title-text" bind:this={titleTextRef}>{title}</span>
    </div>
    <div
      class="artist"
      class:scrollable={artistOverflow > 0}
      style="--ticker-offset: {artistOffset}; --ticker-duration: {artistDuration};"
      bind:this={artistRef}
    >
      <span class="artist-text" bind:this={artistTextRef}>{artist}</span>
    </div>
    {#if quality}
      <div class="quality-badge">{quality}</div>
    {/if}
  </div>
</div>

<style>
  .album-card {
    flex-shrink: 0;
    cursor: pointer;
    transition: transform 150ms ease;
  }

  .artwork-container {
    position: relative;
    margin-bottom: 8px;
    border-radius: 8px;
    overflow: hidden;
    transition: transform 150ms ease-out;
  }

  .album-card:hover .artwork-container {
    transform: scale(1.02);
  }

  .artwork-container img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
  }

  .quality-badge {
    display: inline-block;
    margin-top: 4px;
    font-size: 10px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.85);
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 4px;
    padding: 2px 6px;
  }

  .play-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .action-overlay {
    position: absolute;
    inset: -1px;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    opacity: 0;
    transition: opacity 150ms ease;
    background: rgba(10, 10, 10, 0.25);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    pointer-events: auto;
    border-radius: inherit;
  }

  .album-card:hover .action-overlay {
    opacity: 1;
  }

  .action-buttons {
    display: flex;
    align-items: center;
    gap: 12px;
    pointer-events: auto;
    position: absolute;
    left: 50%;
    top: 75%;
    transform: translate(-50%, -50%);
  }

  .overlay-btn {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.85);
    background: transparent;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: transform 150ms ease, background-color 150ms ease;
  }

  .overlay-btn:hover {
    transform: scale(1.05);
    background-color: rgba(255, 255, 255, 0.1);
  }

  .overlay-btn.is-active {
    background-color: rgba(255, 255, 255, 0.2);
  }

  .overlay-btn.disabled,
  .overlay-btn:disabled {
    opacity: 0.5;
    cursor: default;
    transform: none;
  }

  .overlay-menu {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  :global(.album-card .album-menu) {
    display: flex;
    align-items: center;
  }

  :global(.album-card .album-menu .menu-trigger) {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.85);
    background: transparent;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
  }

  :global(.album-card .album-menu .menu-trigger:hover) {
    background-color: rgba(255, 255, 255, 0.1);
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

  .title.scrollable {
    text-overflow: clip;
  }

  .title-text {
    display: inline-block;
    white-space: nowrap;
  }

  .album-card:hover .title.scrollable .title-text {
    animation: title-ticker var(--ticker-duration) linear infinite;
    will-change: transform;
  }

  @keyframes title-ticker {
    from { transform: translateX(0); }
    to { transform: translateX(var(--ticker-offset)); }
  }

  .artist {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-muted);
    line-height: 1.4;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist.scrollable {
    text-overflow: clip;
  }

  .artist-text {
    display: inline-block;
    white-space: nowrap;
  }

  .album-card:hover .artist.scrollable .artist-text {
    animation: title-ticker var(--ticker-duration) linear infinite;
    will-change: transform;
  }
</style>
