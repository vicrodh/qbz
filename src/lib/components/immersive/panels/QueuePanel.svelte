<script lang="ts">
  import { List, Play, History } from 'lucide-svelte';
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
    // History props
    historyTracks?: QueueTrack[];
    onPlayHistoryTrack?: (trackId: string) => void;
  }

  let {
    tracks = [],
    currentIndex = 0,
    onPlayTrack,
    onClear,
    historyTracks = [],
    onPlayHistoryTrack
  }: Props = $props();

  // Sub-tab state: 'queue' or 'history'
  let activeSubTab = $state<'queue' | 'history'>('queue');

  const upcomingTracks = $derived(tracks.slice(currentIndex + 1));
  const currentTrack = $derived(tracks[currentIndex]);
  const hasUpcoming = $derived(upcomingTracks.length > 0);
  const hasHistory = $derived(historyTracks.length > 0);

  function formatDuration(duration?: string | number): string {
    if (!duration) return '';
    if (typeof duration === 'string') return duration;
    const mins = Math.floor(duration / 60);
    const secs = Math.floor(duration % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }
</script>

<div class="queue-panel">
  <!-- Header with sub-tabs -->
  <div class="panel-header">
    <div class="sub-tabs">
      <button
        class="sub-tab"
        class:active={activeSubTab === 'queue'}
        onclick={() => activeSubTab = 'queue'}
      >
        <List size={16} />
        <span>{$t('player.queue') || 'Queue'}</span>
        {#if hasUpcoming}
          <span class="count">({upcomingTracks.length})</span>
        {/if}
      </button>
      <button
        class="sub-tab"
        class:active={activeSubTab === 'history'}
        onclick={() => activeSubTab = 'history'}
      >
        <History size={16} />
        <span>{$t('player.history') || 'History'}</span>
        {#if hasHistory}
          <span class="count">({historyTracks.length})</span>
        {/if}
      </button>
    </div>
    {#if activeSubTab === 'queue' && hasUpcoming && onClear}
      <button class="clear-btn" onclick={onClear}>
        {$t('player.clearQueue') || 'Clear'}
      </button>
    {/if}
  </div>

  <!-- Queue Content -->
  {#if activeSubTab === 'queue'}
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
    <div class="section list-section">
      <div class="section-label">{$t('player.upNext') || 'Up Next'}</div>

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
  {:else}
    <!-- History Content -->
    <div class="section list-section">
      {#if hasHistory}
        <div class="tracks-list">
          {#each historyTracks as track, i (track.id + '-history-' + i)}
            <div class="track-item">
              <button
                class="play-btn"
                onclick={() => onPlayHistoryTrack?.(String(track.id))}
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
          <span>{$t('player.noHistoryYet') || 'No history yet'}</span>
        </div>
      {/if}
    </div>
  {/if}
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
    padding: 0 4px 12px;
    border-bottom: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    margin-bottom: 16px;
    flex-shrink: 0;
  }

  .sub-tabs {
    display: flex;
    gap: 4px;
  }

  .sub-tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: none;
    border: none;
    border-radius: 6px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .sub-tab:hover {
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
    color: var(--alpha-80, rgba(255, 255, 255, 0.8));
  }

  .sub-tab.active {
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
    color: var(--text-primary, white);
  }

  .sub-tab .count {
    font-size: 11px;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .sub-tab.active .count {
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
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
    margin-bottom: 16px;
    flex-shrink: 0;
  }

  .list-section {
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
