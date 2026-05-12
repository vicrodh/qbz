<script lang="ts">
  import { Play, ListMusic } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    playlistId: number;
    name: string;
    image?: string;
    onClick?: () => void;
    onPlay?: () => void;
  }

  let { playlistId, name, image, onClick, onPlay }: Props = $props();

  function handleCardClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('.play-btn')) return;
    onClick?.();
  }

  function handlePlay(e: MouseEvent) {
    e.stopPropagation();
    onPlay?.();
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
  <div class="cover-wrap">
    {#if image}
      <img class="cover" src={image} alt={name} loading="lazy" decoding="async" />
    {:else}
      <div class="cover cover-placeholder">
        <ListMusic size={48} />
      </div>
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
  <div class="title">{name}</div>
</div>

<style>
  /* Cero efectos. Same dimensions as AlbumCardLite for grid alignment. */
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
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
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
</style>
