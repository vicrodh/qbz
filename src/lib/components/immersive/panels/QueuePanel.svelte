<script lang="ts">
  import { List, Play } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface QueueTrack {
    id: string | number;
    title: string;
    artist: string;
    artwork: string;
    duration?: string | number;
  }

  interface Props {
    tracks: QueueTrack[];
    currentIndex: number;
    onPlayTrack: (index: number) => void;
    onClear?: () => void;
  }

  let {
    tracks = [],
    currentIndex = 0,
    onPlayTrack,
    onClear
  }: Props = $props();

  const upcomingTracks = $derived(tracks.slice(currentIndex + 1));
  const currentTrack = $derived(tracks[currentIndex]);
  const hasUpcoming = $derived(upcomingTracks.length > 0);

  function formatDuration(duration?: string | number): string {
    if (!duration) return '';
    // If already a string (e.g., "3:45"), return as-is
    if (typeof duration === 'string') return duration;
    // If number (seconds), format it
    const mins = Math.floor(duration / 60);
    const secs = Math.floor(duration % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }
</script>

<div class="queue-panel">
  <!-- Header -->
  <div class="panel-header">
    <div class="header-title">
      <List size={18} />
      <span>{$t('player.queue') || 'Queue'}</span>
    </div>
    {#if hasUpcoming && onClear}
      <button class="clear-btn" onclick={onClear}>
        {$t('actions.clearQueue') || 'Clear'}
      </button>
    {/if}
  </div>

  <!-- Now Playing -->
  {#if currentTrack}
    <div class="section">
      <div class="section-label">{$t('player.nowPlaying') || 'Now Playing'}</div>
      <div class="track-item current">
        <img src={currentTrack.artwork} alt="" class="track-artwork" />
        <div class="track-info">
          <div class="track-title">{currentTrack.title}</div>
          <div class="track-artist">{currentTrack.artist}</div>
        </div>
        <div class="track-duration">{formatDuration(currentTrack.duration)}</div>
      </div>
    </div>
  {/if}

  <!-- Up Next -->
  <div class="section upcoming-section">
    <div class="section-label">
      {$t('player.upNext') || 'Up Next'}
      {#if hasUpcoming}
        <span class="track-count">({upcomingTracks.length})</span>
      {/if}
    </div>

    {#if hasUpcoming}
      <div class="tracks-list">
        {#each upcomingTracks as track, i (track.id + '-' + i)}
          {@const actualIndex = currentIndex + 1 + i}
          <div class="track-item">
            <button
              class="play-btn"
              onclick={() => onPlayTrack(actualIndex)}
              title={$t('actions.playNow') || 'Play now'}
            >
              <Play size={14} />
            </button>
            <img src={track.artwork} alt="" class="track-artwork" />
            <div class="track-info">
              <div class="track-title">{track.title}</div>
              <div class="track-artist">{track.artist}</div>
            </div>
            <div class="track-duration">{formatDuration(track.duration)}</div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-state">
        <span>{$t('player.queueEmpty') || 'Queue is empty'}</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .queue-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 4px 16px;
    border-bottom: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    margin-bottom: 16px;
    flex-shrink: 0;
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary, white);
  }

  .clear-btn {
    padding: 6px 12px;
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
    border: none;
    border-radius: 6px;
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    font-size: 12px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .clear-btn:hover {
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
    color: var(--text-primary, white);
  }

  .section {
    margin-bottom: 20px;
    flex-shrink: 0;
  }

  .upcoming-section {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .section-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    margin-bottom: 12px;
    padding: 0 4px;
  }

  .track-count {
    font-weight: 400;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .tracks-list {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-right: 8px;
  }

  .tracks-list::-webkit-scrollbar {
    width: 4px;
  }

  .tracks-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .tracks-list::-webkit-scrollbar-thumb {
    background: var(--alpha-20, rgba(255, 255, 255, 0.2));
    border-radius: 2px;
  }

  .track-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px;
    border-radius: 8px;
    transition: background 150ms ease;
  }

  .track-item:hover {
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .track-item.current {
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
    border: 1px solid var(--alpha-15, rgba(255, 255, 255, 0.15));
  }

  .play-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
    border: none;
    border-radius: 50%;
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    cursor: pointer;
    opacity: 0;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .track-item:hover .play-btn {
    opacity: 1;
  }

  .play-btn:hover {
    background: var(--alpha-25, rgba(255, 255, 255, 0.25));
    color: var(--text-primary, white);
    transform: scale(1.1);
  }

  .track-artwork {
    width: 40px;
    height: 40px;
    border-radius: 4px;
    object-fit: cover;
    flex-shrink: 0;
  }

  .track-info {
    flex: 1;
    min-width: 0;
  }

  .track-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary, white);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-artist {
    font-size: 12px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 2px;
  }

  .track-duration {
    font-size: 12px;
    font-family: var(--font-mono);
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    flex-shrink: 0;
  }

  .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 120px;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
    font-size: 14px;
    font-style: italic;
  }
</style>
