<script lang="ts">
  import { History, Play } from 'lucide-svelte';
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
    onPlayTrack: (trackId: string) => void;
  }

  let {
    tracks = [],
    onPlayTrack
  }: Props = $props();

  const hasHistory = $derived(tracks.length > 0);

  function formatDuration(duration?: string | number): string {
    if (!duration) return '';
    if (typeof duration === 'string') return duration;
    const mins = Math.floor(duration / 60);
    const secs = Math.floor(duration % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }
</script>

<div class="history-panel">
  <!-- Header -->
  <div class="panel-header">
    <div class="header-title">
      <History size={18} />
      <span>{$t('player.history') || 'History'}</span>
    </div>
    {#if hasHistory}
      <span class="track-count">{tracks.length} {$t('player.tracks') || 'tracks'}</span>
    {/if}
  </div>

  <!-- History List -->
  <div class="history-section">
    {#if hasHistory}
      <div class="tracks-list">
        {#each tracks as track, i (track.id + '-' + i)}
          <div class="track-item">
            <button
              class="play-btn"
              onclick={() => onPlayTrack(String(track.id))}
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
        <span>{$t('player.historyEmpty') || 'No history yet'}</span>
      </div>
    {/if}
  </div>
</div>

<style>
  .history-panel {
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

  .track-count {
    font-size: 12px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
  }

  .history-section {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
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
