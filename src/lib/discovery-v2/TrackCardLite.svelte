<script lang="ts">
  import { Play } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    trackId: number;
    title: string;
    artist: string;
    artwork?: string;
    isPlaying?: boolean;
    onPlay?: () => void;
    onAlbumClick?: () => void;
    onArtistClick?: () => void;
  }

  let {
    trackId,
    title,
    artist,
    artwork,
    isPlaying = false,
    onPlay,
    onAlbumClick,
    onArtistClick,
  }: Props = $props();

  function handleCardClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('.play-btn, .artist-link')) return;
    onAlbumClick?.();
  }

  function handlePlay(e: MouseEvent) {
    e.stopPropagation();
    onPlay?.();
  }

  function handleArtist(e: MouseEvent) {
    e.stopPropagation();
    onArtistClick?.();
  }
</script>

<div
  class="card"
  class:is-playing={isPlaying}
  data-track-id={trackId}
  role="button"
  tabindex="0"
  onclick={handleCardClick}
  onkeydown={(e) => e.key === 'Enter' && onAlbumClick?.()}
>
  <div class="cover-wrap">
    {#if artwork}
      <img class="cover" src={artwork} alt={title} loading="lazy" decoding="async" />
    {:else}
      <div class="cover cover-placeholder"></div>
    {/if}
    <button
      class="play-btn"
      type="button"
      aria-label={$t('actions.play')}
      onclick={handlePlay}
    >
      <Play size={16} fill="currentColor" />
    </button>
  </div>
  <div class="title">{title}</div>
  {#if onArtistClick}
    <button class="artist-link" type="button" onclick={handleArtist}>{artist}</button>
  {:else}
    <div class="artist">{artist}</div>
  {/if}
</div>

<style>
  /* Same structure as AlbumCardLite — kept separate so future track-only
     metadata (duration badge, isrc context menu, etc.) can land here
     without bleeding into album cards. Cero efectos. */
  .card {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 180px;
    cursor: pointer;
    background: transparent;
    border: none;
    padding: 0;
    text-align: left;
  }

  .cover-wrap {
    position: relative;
    width: 180px;
    height: 180px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    overflow: hidden;
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

  .play-btn {
    position: absolute;
    bottom: 8px;
    right: 8px;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    border: none;
    background: var(--accent-primary);
    color: var(--btn-primary-text, #000);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
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
</style>
