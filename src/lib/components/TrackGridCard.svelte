<script lang="ts">
  import { Play, Pause } from 'lucide-svelte';
  import TrackMenu from './TrackMenu.svelte';
  import { toggleTrackFavorite, isTrackFavorite, subscribe as subscribeFavorites } from '$lib/stores/favoritesStore';
  import { onMount } from 'svelte';

  interface TrackMenuActions {
    onPlayNow?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onAddToPlaylist?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onGoToAlbum?: () => void;
    onGoToArtist?: () => void;
    onShowInfo?: () => void;
    onDownload?: () => void;
    isTrackDownloaded?: boolean;
    onReDownload?: () => void;
    onRemoveDownload?: () => void;
  }

  interface Props {
    trackId?: number;
    title: string;
    album: string;
    artwork?: string | null;
    isPlaying?: boolean;
    isActiveTrack?: boolean;
    isBlacklisted?: boolean;
    onPlay?: () => void;
    onAlbumClick?: () => void;
    menuActions?: TrackMenuActions;
  }

  let {
    trackId,
    title,
    album,
    artwork,
    isPlaying = false,
    isActiveTrack = false,
    isBlacklisted = false,
    onPlay,
    onAlbumClick,
    menuActions
  }: Props = $props();

  let isHovered = $state(false);
  let contextMenuPos = $state<{ x: number; y: number } | null>(null);
  let favoriteFromStore = $state(false);

  onMount(() => {
    if (trackId !== undefined) {
      favoriteFromStore = isTrackFavorite(trackId);
      const unsub = subscribeFavorites(() => {
        favoriteFromStore = isTrackFavorite(trackId);
      });
      return unsub;
    }
  });

  const playNowAction = $derived(menuActions?.onPlayNow ?? onPlay);

  function handleCardClick(event: MouseEvent) {
    if ((event.target as HTMLElement).closest('button, .track-grid-card-menu')) return;
    if (isBlacklisted) return;
    onPlay?.();
  }

  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    contextMenuPos = { x: event.clientX, y: event.clientY };
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="track-grid-card"
  class:active={isActiveTrack}
  class:blacklisted={isBlacklisted}
  onmouseenter={() => (isHovered = true)}
  onmouseleave={() => (isHovered = false)}
  onclick={handleCardClick}
  oncontextmenu={handleContextMenu}
>
  <div class="track-grid-cover">
    {#if artwork}
      <img src={artwork} alt="" loading="lazy" decoding="async" />
    {:else}
      <div class="track-grid-cover-placeholder"></div>
    {/if}
    {#if !isBlacklisted}
      <button
        class="track-grid-play"
        class:visible={isHovered || isActiveTrack}
        onclick={(e) => { e.stopPropagation(); onPlay?.(); }}
        aria-label={isPlaying ? 'Pause' : 'Play'}
      >
        {#if isPlaying}
          <Pause size={14} fill="currentColor" />
        {:else}
          <Play size={14} fill="currentColor" />
        {/if}
      </button>
    {/if}
  </div>

  <div class="track-grid-info">
    <span class="track-grid-title" title={title}>{title}</span>
    <button
      class="track-grid-album"
      title={album}
      onclick={(e) => { e.stopPropagation(); onAlbumClick?.(); }}
      disabled={!onAlbumClick}
    >{album}</button>
  </div>

  <div class="track-grid-card-menu">
    <TrackMenu
      onPlayNow={playNowAction}
      onPlayNext={menuActions?.onPlayNext}
      onPlayLater={menuActions?.onPlayLater}
      onAddFavorite={trackId !== undefined ? () => toggleTrackFavorite(trackId) : undefined}
      onAddToPlaylist={menuActions?.onAddToPlaylist}
      onShareQobuz={menuActions?.onShareQobuz}
      onShareSonglink={menuActions?.onShareSonglink}
      onGoToAlbum={menuActions?.onGoToAlbum}
      onGoToArtist={menuActions?.onGoToArtist}
      onShowInfo={menuActions?.onShowInfo}
      onDownload={menuActions?.onDownload}
      isTrackDownloaded={menuActions?.isTrackDownloaded}
      onReDownload={menuActions?.onReDownload}
      onRemoveDownload={menuActions?.onRemoveDownload}
      contextMenuPosition={contextMenuPos}
      onContextMenuClosed={() => { contextMenuPos = null; }}
    />
  </div>
</div>

<style>
  .track-grid-card {
    display: grid;
    grid-template-columns: 48px minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    padding: 6px 8px;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
    min-width: 0;
  }

  .track-grid-card:hover,
  .track-grid-card.active {
    background-color: var(--bg-hover);
  }

  .track-grid-card.blacklisted {
    opacity: 0.4;
    cursor: default;
  }

  .track-grid-cover {
    position: relative;
    width: 48px;
    height: 48px;
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
    background: var(--bg-tertiary);
  }

  .track-grid-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .track-grid-cover-placeholder {
    width: 100%;
    height: 100%;
    background: var(--bg-tertiary);
  }

  .track-grid-play {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    border: none;
    color: var(--text-primary);
    cursor: pointer;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .track-grid-play.visible {
    opacity: 1;
  }

  .track-grid-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 2px;
  }

  .track-grid-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-grid-album {
    font-size: 11px;
    color: var(--text-muted);
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  .track-grid-album:hover:not(:disabled) {
    color: var(--text-secondary);
    text-decoration: underline;
  }

  .track-grid-album:disabled {
    cursor: default;
  }

  .track-grid-card-menu {
    display: flex;
    align-items: center;
    flex-shrink: 0;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .track-grid-card:hover .track-grid-card-menu,
  .track-grid-card.active .track-grid-card-menu {
    opacity: 1;
  }
</style>
