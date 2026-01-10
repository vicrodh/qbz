<script lang="ts">
  import { X, GripVertical, Play } from 'lucide-svelte';
  import GlassSurface from './glass/GlassSurface.svelte';

  interface QueueTrack {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    duration: string;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    currentTrack?: QueueTrack;
    upcomingTracks: QueueTrack[];
    onPlayTrack?: (trackId: string) => void;
    onClearQueue?: () => void;
    onSaveAsPlaylist?: () => void;
  }

  let {
    isOpen,
    onClose,
    currentTrack,
    upcomingTracks,
    onPlayTrack,
    onClearQueue,
    onSaveAsPlaylist
  }: Props = $props();

  let hoveredTrack = $state<string | null>(null);
</script>

{#if isOpen}
  <!-- Backdrop -->
  <div class="backdrop" onclick={onClose} role="presentation"></div>

  <!-- Queue Panel -->
  <GlassSurface rootClassName="queue-panel" enableRipple={false} enableDistortion={false}>
    <!-- Header -->
    <div class="header">
      <h2>Queue</h2>
      <button class="close-btn" onclick={onClose}>
        <X size={24} />
      </button>
    </div>

    <!-- Content -->
    <div class="content">
      <!-- Now Playing -->
      {#if currentTrack}
        <div class="section">
          <div class="section-header">Now Playing</div>
          <div class="now-playing-card">
            <img src={currentTrack.artwork} alt={currentTrack.title} />
            <div class="track-info">
              <div class="track-title">{currentTrack.title}</div>
              <div class="track-artist">{currentTrack.artist}</div>
            </div>
          </div>
        </div>
      {/if}

      <!-- Next Up -->
      {#if upcomingTracks.length > 0}
        <div class="section">
          <div class="section-header">Next Up ({upcomingTracks.length})</div>
          <div class="tracks">
            {#each upcomingTracks as track, index}
              <div
                class="queue-track"
                class:hovered={hoveredTrack === track.id}
                onmouseenter={() => (hoveredTrack = track.id)}
                onmouseleave={() => (hoveredTrack = null)}
                onclick={() => onPlayTrack?.(track.id)}
                role="button"
                tabindex="0"
                onkeydown={(e) => e.key === 'Enter' && onPlayTrack?.(track.id)}
              >
                <!-- Drag Handle -->
                <div class="drag-handle" class:visible={hoveredTrack === track.id}>
                  <GripVertical size={16} />
                </div>

                <!-- Track Number / Play Icon -->
                <div class="track-number">
                  {#if hoveredTrack === track.id}
                    <Play size={14} fill="white" color="white" />
                  {:else}
                    <span>{index + 1}</span>
                  {/if}
                </div>

                <!-- Track Info -->
                <div class="track-info">
                  <div class="track-title">{track.title}</div>
                  <div class="track-artist">{track.artist}</div>
                </div>

                <!-- Duration -->
                <div class="track-duration">{track.duration}</div>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Empty State -->
      {#if upcomingTracks.length === 0 && !currentTrack}
        <div class="empty-state">
          <div class="emoji">🎵</div>
          <div class="empty-title">Queue is empty</div>
          <div class="empty-text">Search for music or browse your library to add tracks.</div>
        </div>
      {/if}
    </div>

    <!-- Footer Actions -->
    {#if upcomingTracks.length > 0 || currentTrack}
      <div class="footer">
        <button class="clear-btn" onclick={onClearQueue}>Clear Queue</button>
        <button class="save-btn" onclick={onSaveAsPlaylist}>Save as Playlist</button>
      </div>
    {/if}
  </GlassSurface>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background-color: rgba(0, 0, 0, 0.4);
    z-index: 40;
  }

  :global(.queue-panel) {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: 360px;
    z-index: 50;
    display: flex;
    flex-direction: column;
    animation: slideInRight 200ms ease-out;
    --glass-bg: rgba(30, 30, 35, 0.85);
    --glass-blur: 24px;
    --glass-radius: 0;
    --glass-border: rgba(255, 255, 255, 0.08);
    --glass-shadow: -4px 0 24px rgba(0, 0, 0, 0.5);
  }

  @keyframes slideInRight {
    from {
      transform: translateX(100%);
    }
    to {
      transform: translateX(0);
    }
  }

  .header {
    padding: 20px;
    border-bottom: 1px solid var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .header h2 {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: 16px 20px;
  }

  .section {
    margin-bottom: 24px;
  }

  .section-header {
    font-size: 12px;
    text-transform: uppercase;
    color: #666666;
    font-weight: 600;
    letter-spacing: 0.05em;
    margin-bottom: 12px;
  }

  .now-playing-card {
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 12px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .now-playing-card img {
    width: 56px;
    height: 56px;
    border-radius: 4px;
    object-fit: cover;
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

  .track-artist {
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tracks {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .queue-track {
    height: 48px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 8px;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .queue-track.hovered {
    background-color: var(--bg-tertiary);
  }

  .drag-handle {
    color: #666666;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .drag-handle.visible {
    opacity: 1;
  }

  .track-number {
    width: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .track-number span {
    font-size: 14px;
    color: #666666;
  }

  .track-duration {
    font-size: 13px;
    color: #666666;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 64px 0;
    text-align: center;
  }

  .emoji {
    font-size: 48px;
    margin-bottom: 16px;
  }

  .empty-title {
    font-size: 16px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 8px;
  }

  .empty-text {
    font-size: 14px;
    color: var(--text-muted);
    max-width: 240px;
  }

  .footer {
    padding: 16px;
    border-top: 1px solid var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .clear-btn {
    font-size: 14px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    transition: color 150ms ease;
  }

  .clear-btn:hover {
    color: var(--text-primary);
  }

  .save-btn {
    padding: 8px 16px;
    border-radius: 8px;
    background-color: var(--accent-primary);
    color: white;
    font-size: 14px;
    font-weight: 500;
    border: none;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .save-btn:hover {
    background-color: var(--accent-hover);
  }
</style>
