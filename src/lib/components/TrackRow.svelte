<script lang="ts">
  import { Play, Plus, ListPlus, MoreHorizontal } from 'lucide-svelte';

  interface Props {
    number: number;
    title: string;
    artist?: string;
    duration: string;
    quality?: string;
    isPlaying?: boolean;
    onPlay?: () => void;
    onAddToPlaylist?: () => void;
    onAddToQueue?: () => void;
  }

  let {
    number,
    title,
    artist,
    duration,
    quality,
    isPlaying = false,
    onPlay,
    onAddToPlaylist,
    onAddToQueue
  }: Props = $props();

  let isHovered = $state(false);
</script>

<div
  class="track-row"
  class:playing={isPlaying}
  class:hovered={isHovered && !isPlaying}
  onmouseenter={() => (isHovered = true)}
  onmouseleave={() => (isHovered = false)}
  onclick={onPlay}
  role="button"
  tabindex="0"
  onkeydown={(e) => e.key === 'Enter' && onPlay?.()}
>
  <!-- Track Number / Play Button -->
  <div class="track-number">
    {#if isHovered && !isPlaying}
      <Play size={16} class="play-icon" fill="white" />
    {:else if isPlaying}
      <div class="playing-indicator">
        <div class="bar"></div>
        <div class="bar"></div>
        <div class="bar"></div>
      </div>
    {:else}
      <span>{number}</span>
    {/if}
  </div>

  <!-- Track Info -->
  <div class="track-info">
    <div class="track-title" class:active={isPlaying}>{title}</div>
    {#if artist}
      <div class="track-artist">{artist}</div>
    {/if}
  </div>

  <!-- Duration -->
  <div class="track-duration">{duration}</div>

  <!-- Quality -->
  {#if quality}
    <div class="track-quality">{quality}</div>
  {/if}

  <!-- Hover Actions -->
  {#if isHovered && !isPlaying}
    <div class="track-actions">
      {#if onAddToQueue}
        <button
          class="action-btn"
          onclick={(e) => { e.stopPropagation(); onAddToQueue(); }}
          title="Add to queue"
        >
          <Plus size={18} />
        </button>
      {/if}
      {#if onAddToPlaylist}
        <button
          class="action-btn"
          onclick={(e) => { e.stopPropagation(); onAddToPlaylist(); }}
          title="Add to playlist"
        >
          <ListPlus size={18} />
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .track-row {
    width: 100%;
    height: 56px;
    padding: 0 16px;
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
    box-sizing: border-box;
  }

  .track-row.hovered {
    background-color: var(--bg-hover);
  }

  .track-row.playing {
    background-color: var(--bg-secondary);
  }

  .track-number {
    width: 48px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .track-number span {
    font-size: 14px;
    color: #666666;
  }

  .track-number :global(.play-icon) {
    color: white;
  }

  .playing-indicator {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .playing-indicator .bar {
    width: 3px;
    background-color: var(--accent-primary);
    border-radius: 9999px;
    animation: pulse 1s ease-in-out infinite;
  }

  .playing-indicator .bar:nth-child(1) {
    height: 12px;
  }

  .playing-indicator .bar:nth-child(2) {
    height: 16px;
    animation-delay: 0.2s;
  }

  .playing-indicator .bar:nth-child(3) {
    height: 10px;
    animation-delay: 0.4s;
  }

  @keyframes pulse {
    0%, 100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }

  .track-info {
    flex: 1;
    min-width: 0;
  }

  .track-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-title.active {
    color: var(--accent-primary);
  }

  .track-artist {
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-duration {
    font-size: 14px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    width: 80px;
    text-align: right;
  }

  .track-quality {
    font-size: 12px;
    color: #666666;
    width: 80px;
    text-align: right;
  }

  .track-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .action-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 150ms ease;
  }

  .action-btn:hover {
    color: var(--text-primary);
  }
</style>
