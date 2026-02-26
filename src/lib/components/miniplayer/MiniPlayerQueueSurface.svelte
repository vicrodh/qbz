<script lang="ts">
  import { t } from '$lib/i18n';
  import { Play } from 'lucide-svelte';
  import type { MiniPlayerQueueTrack } from './types';

  interface Props {
    tracks: MiniPlayerQueueTrack[];
    currentTrackId?: string;
    isPlaying?: boolean;
    onTrackPlay?: (trackId: string) => void;
  }

  let { tracks, currentTrackId, isPlaying = false, onTrackPlay }: Props = $props();
</script>

<div class="queue-surface">
  {#if tracks.length === 0}
    <div class="queue-empty">{$t('player.queueEmpty')}</div>
  {:else}
    <div class="queue-list mini-scrollbar">
      {#each tracks as queueTrack (queueTrack.id)}
        {@const activeTrack = queueTrack.id === currentTrackId}
        <div class="queue-row" class:active={activeTrack}>
          {#if activeTrack}
            <div class="active-bar" aria-hidden="true"></div>
          {/if}

          <div class="row-artwork-wrap">
            {#if queueTrack.artwork}
              <img src={queueTrack.artwork} alt={queueTrack.title} class="row-artwork" />
            {:else}
              <div class="row-artwork placeholder" aria-hidden="true"></div>
            {/if}

            {#if activeTrack && isPlaying}
              <div class="artwork-playing-indicator" aria-hidden="true">
                <div class="bar"></div>
                <div class="bar"></div>
                <div class="bar"></div>
              </div>
            {/if}

            <button
              class="artwork-play-btn"
              type="button"
              title={$t('player.play')}
              aria-label={$t('player.play')}
              onclick={(event) => {
                event.stopPropagation();
                onTrackPlay?.(queueTrack.id);
              }}
            >
              <Play size={14} />
            </button>
          </div>

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

  .row-artwork-wrap {
    position: relative;
    width: 36px;
    height: 36px;
    border-radius: 5px;
    overflow: hidden;
    flex-shrink: 0;
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
    margin-left: auto;
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-muted);
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;
  }

  .artwork-play-btn {
    position: absolute;
    inset: 0;
    border: none;
    border-radius: 5px;
    padding: 0;
    cursor: pointer;
    background: color-mix(in srgb, black 52%, transparent);
    color: var(--text-primary);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    visibility: hidden;
    pointer-events: none;
    transition: opacity 120ms ease;
  }

  .row-artwork-wrap:hover .artwork-play-btn,
  .row-artwork-wrap:focus-within .artwork-play-btn {
    opacity: 1;
    visibility: visible;
    pointer-events: auto;
  }

  .artwork-playing-indicator {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px 4px;
    border-radius: 999px;
    background: color-mix(in srgb, black 48%, transparent);
    transition: opacity 120ms ease;
  }

  .row-artwork-wrap:hover .artwork-playing-indicator {
    opacity: 0;
  }

  .artwork-playing-indicator .bar {
    width: 2px;
    background-color: var(--accent-primary);
    border-radius: 9999px;
    transform-origin: bottom;
    animation: equalize 1s ease-in-out infinite;
  }

  .artwork-playing-indicator .bar:nth-child(1) {
    height: 10px;
  }

  .artwork-playing-indicator .bar:nth-child(2) {
    height: 13px;
    animation-delay: 0.15s;
  }

  .artwork-playing-indicator .bar:nth-child(3) {
    height: 8px;
    animation-delay: 0.3s;
  }

  @keyframes equalize {
    0%, 100% {
      transform: scaleY(0.5);
      opacity: 0.7;
    }
    50% {
      transform: scaleY(1);
      opacity: 1;
    }
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
