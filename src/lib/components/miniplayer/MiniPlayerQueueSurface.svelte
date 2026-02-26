<script lang="ts">
  import { t } from '$lib/i18n';
  import type { MiniPlayerQueueTrack } from './types';

  interface Props {
    tracks: MiniPlayerQueueTrack[];
    currentTrackId?: string;
  }

  let { tracks, currentTrackId }: Props = $props();
</script>

<div class="queue-surface">
  {#if tracks.length === 0}
    <div class="queue-empty">{$t('player.queueEmpty')}</div>
  {:else}
    <div class="queue-list mini-scrollbar">
      {#each tracks as queueTrack (queueTrack.id)}
        <div class="queue-row" class:active={queueTrack.id === currentTrackId}>
          {#if queueTrack.id === currentTrackId}
            <div class="active-bar" aria-hidden="true"></div>
          {/if}

          {#if queueTrack.artwork}
            <img src={queueTrack.artwork} alt={queueTrack.title} class="row-artwork" />
          {:else}
            <div class="row-artwork placeholder" aria-hidden="true"></div>
          {/if}

          <div class="row-meta">
            <div class="row-title">{queueTrack.title}</div>
            <div class="row-artist">{queueTrack.artist}</div>
          </div>

          {#if queueTrack.quality}
            <span class="quality">{queueTrack.quality}</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .queue-surface {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .queue-empty {
    flex: 1;
    display: grid;
    place-items: center;
    color: var(--text-muted);
    font-size: 13px;
    padding: 16px;
  }

  .queue-list {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: 8px 0 10px;
  }

  .queue-row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    transition: background 120ms ease;
  }

  .queue-row:hover {
    background: var(--alpha-8);
  }

  .queue-row.active {
    background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
  }

  .active-bar {
    position: absolute;
    left: 0;
    top: 4px;
    bottom: 4px;
    width: 3px;
    border-radius: 3px;
    background: var(--accent-primary);
  }

  .row-artwork {
    width: 36px;
    height: 36px;
    border-radius: 5px;
    object-fit: cover;
    flex-shrink: 0;
  }

  .row-artwork.placeholder {
    background: var(--alpha-10);
  }

  .row-meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
  }

  .row-title,
  .row-artist {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .queue-row.active .row-title {
    color: var(--text-primary);
  }

  .row-artist {
    font-size: 12px;
    color: var(--text-muted);
  }

  .quality {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-muted);
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;
  }

  .mini-scrollbar {
    scrollbar-width: thin;
    scrollbar-color: var(--alpha-18) transparent;
  }

  .mini-scrollbar::-webkit-scrollbar {
    width: 5px;
  }

  .mini-scrollbar::-webkit-scrollbar-thumb {
    background: var(--alpha-18);
    border-radius: 4px;
  }
</style>
