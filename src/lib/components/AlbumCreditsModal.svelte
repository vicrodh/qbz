<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Play, ChevronDown, ChevronUp, Loader2, X } from 'lucide-svelte';
  import type { AlbumCredits, TrackCredits } from '$lib/types';

  interface Props {
    isOpen: boolean;
    albumId: string | null;
    onClose: () => void;
    onTrackPlay?: (track: TrackCredits) => void;
    onPerformerSearch?: (name: string) => void;
    onLabelClick?: (labelId: number, labelName: string) => void;
  }

  let { isOpen, albumId, onClose, onTrackPlay, onPerformerSearch, onLabelClick }: Props = $props();

  function handlePerformerClick(name: string) {
    if (onPerformerSearch) {
      onPerformerSearch(name);
      onClose();
    }
  }

  type TabType = 'credits' | 'review';

  let loading = $state(false);
  let error = $state<string | null>(null);
  let credits = $state<AlbumCredits | null>(null);
  let expandedTracks = $state<Set<number>>(new Set());
  let hoveredTrack = $state<number | null>(null);
  let activeTab = $state<TabType>('credits');

  // Check if review tab should be available
  const hasReview = $derived(credits?.album?.description ? true : false);

  // Load album credits when modal opens
  $effect(() => {
    if (isOpen && albumId) {
      loadAlbumCredits(albumId);
    } else {
      credits = null;
      error = null;
      expandedTracks = new Set();
      activeTab = 'credits';
    }
  });

  async function loadAlbumCredits(id: string) {
    loading = true;
    error = null;
    try {
      credits = await invoke<AlbumCredits>('get_album_credits', { albumId: id });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      credits = null;
    } finally {
      loading = false;
    }
  }

  function toggleTrack(trackId: number) {
    const newSet = new Set(expandedTracks);
    if (newSet.has(trackId)) {
      newSet.delete(trackId);
    } else {
      newSet.add(trackId);
    }
    expandedTracks = newSet;
  }

  function handleTrackPlay(track: TrackCredits, e: MouseEvent) {
    e.stopPropagation();
    onTrackPlay?.(track);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

{#if isOpen}
  <div class="modal-overlay" onclick={onClose} role="dialog" aria-modal="true">
    <div class="credits-modal" onclick={(e) => e.stopPropagation()}>
      {#if loading}
        <div class="loading-state">
          <Loader2 size={32} class="spinner" />
          <span>Loading credits...</span>
        </div>
      {:else if error}
        <div class="error-state">
          <p>Failed to load album credits</p>
          <span class="error-message">{error}</span>
        </div>
      {:else if credits}
        <!-- Header with title and artist -->
        <div class="modal-header">
          <div class="header-titles">
            <h2 class="album-title">{credits.album.title}</h2>
            <span class="album-artist">{credits.album.artist}</span>
          </div>
          <button class="close-btn" onclick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>

        <!-- Two-column layout -->
        <div class="modal-body">
          <!-- Left column: Album info (fixed, no scroll) -->
          <div class="album-column">
            <div class="album-info-scroll">
              <img
                src={credits.album.artwork}
                alt={credits.album.title}
                class="album-artwork"
              />

              <div class="album-meta">
                {#if credits.album.label}
                  <p class="meta-row">
                    <span class="meta-label">Released by</span>
                    {#if credits.album.label_id && onLabelClick}
                      <button
                        class="label-link"
                        onclick={() => {
                          onLabelClick!(credits!.album.label_id!, credits!.album.label);
                          onClose();
                        }}
                      >
                        {credits.album.label}
                      </button>
                    {:else}
                      <span class="meta-value label-name">{credits.album.label}</span>
                    {/if}
                    {#if credits.album.release_date}
                      <span class="meta-date">on {new Date(credits.album.release_date).toLocaleDateString('en-US', { month: 'long', day: 'numeric', year: 'numeric' })}</span>
                    {/if}
                  </p>
                {/if}

                {#if credits.album.genre}
                  <p class="meta-row">
                    <span class="meta-value">{credits.album.genre}</span>
                    <span class="meta-separator">-</span>
                    <span class="meta-value">{credits.album.track_count} Tracks</span>
                    <span class="meta-separator">-</span>
                    <span class="meta-value">{credits.album.duration}</span>
                  </p>
                {/if}

                {#if credits.album.bit_depth || credits.album.sampling_rate}
                  <div class="quality-info">
                    {#if credits.album.bit_depth && credits.album.sampling_rate}
                      {credits.album.bit_depth}-Bit / {credits.album.sampling_rate} kHz
                    {:else if credits.album.bit_depth}
                      {credits.album.bit_depth}-Bit
                    {:else if credits.album.sampling_rate}
                      {credits.album.sampling_rate} kHz
                    {/if}
                  </div>
                {/if}
              </div>
            </div>
          </div>

          <!-- Right column: Track list or Review (scrollable) -->
          <div class="content-column">
            <!-- Tab switcher (only shown if review exists) -->
            {#if hasReview}
              <div class="tab-switcher">
                <button
                  class="tab-btn"
                  class:active={activeTab === 'credits'}
                  onclick={() => activeTab = 'credits'}
                >
                  Credits
                </button>
                <button
                  class="tab-btn"
                  class:active={activeTab === 'review'}
                  onclick={() => activeTab = 'review'}
                >
                  Review
                </button>
              </div>
            {/if}

            <div class="scrollable-content">
              {#if activeTab === 'credits'}
              <div class="tracks-list">
                {#each credits.tracks as track, index (track.id)}
                  {@const isExpanded = expandedTracks.has(track.id)}
                  {@const isHovered = hoveredTrack === track.id}
                  {@const hasCredits = track.performers.length > 0 || track.copyright}
                  {@const isLast = index === credits.tracks.length - 1}

                  <div
                    class="track-item"
                    class:expanded={isExpanded}
                    class:has-credits={hasCredits}
                  >
                    <button
                      class="track-header"
                      onclick={() => hasCredits && toggleTrack(track.id)}
                      onmouseenter={() => hoveredTrack = track.id}
                      onmouseleave={() => hoveredTrack = null}
                      disabled={!hasCredits}
                    >
                      <div class="track-number">
                        {#if isHovered && onTrackPlay}
                          <button
                            class="play-btn"
                            onclick={(e) => handleTrackPlay(track, e)}
                            aria-label="Play track"
                          >
                            <Play size={14} fill="currentColor" />
                          </button>
                        {:else}
                          <span>{track.number}</span>
                        {/if}
                      </div>
                      <div class="track-info">
                        <span class="track-title">{track.title}</span>
                        <span class="track-artist">{track.artist}</span>
                      </div>
                      {#if hasCredits}
                        <div class="track-chevron">
                          {#if isExpanded}
                            <ChevronUp size={18} />
                          {:else}
                            <ChevronDown size={18} />
                          {/if}
                        </div>
                      {/if}
                    </button>

                    {#if isExpanded && hasCredits}
                      <div class="track-credits">
                        {#each track.performers as performer}
                          <div class="performer-row">
                            {#if onPerformerSearch}
                              <button class="performer-link" onclick={() => handlePerformerClick(performer.name)}>{performer.name}</button>
                            {:else}
                              <span class="performer-name">{performer.name}</span>
                            {/if}
                            {#if performer.roles.length > 0}
                              <span class="performer-roles">, {performer.roles.join(', ')}</span>
                            {/if}
                          </div>
                        {/each}
                        {#if track.copyright}
                          <div class="track-copyright">{track.copyright}</div>
                        {/if}
                      </div>
                    {/if}
                  </div>

                  {#if !isLast}
                    <div class="track-divider"></div>
                  {/if}
                {/each}
              </div>
              {:else if activeTab === 'review' && credits.album.description}
                <div class="review-content">
                  <div class="review-text">
                    {@html credits.album.description}
                  </div>
                </div>
              {/if}
            </div>
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.75);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
    animation: fade-in 200ms ease-out;
  }

  @keyframes fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .credits-modal {
    background: rgba(26, 26, 26, 0.95);
    backdrop-filter: blur(20px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px;
    width: 100%;
    max-width: 850px;
    height: calc(80vh - 5px);
    display: flex;
    flex-direction: column;
    box-shadow: 0 24px 48px rgba(0, 0, 0, 0.6);
    animation: slide-up 200ms ease-out;
    margin: 20px;
  }

  @keyframes slide-up {
    from {
      opacity: 0;
      transform: scale(0.95);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 60px 20px;
    color: var(--text-muted);
    flex: 1;
  }

  .loading-state :global(.spinner) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .error-state {
    text-align: center;
    padding: 60px 20px;
    color: var(--text-muted);
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }

  .error-message {
    display: block;
    margin-top: 8px;
    font-size: 13px;
    color: var(--danger);
  }

  /* Header */
  .modal-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 16px 24px;
    flex-shrink: 0;
  }

  .header-titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    flex: 1;
    padding-right: 16px;
  }

  .album-title {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.3;
  }

  .album-artist {
    font-size: 13px;
    color: var(--text-tertiary, #888888);
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    border-radius: 50%;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    transition: background 200ms ease, color 200ms ease;
  }

  .close-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  /* Two-column body */
  .modal-body {
    display: flex;
    flex: 1;
    min-height: 0;
    padding: 0 24px 24px;
    gap: 24px;
  }

  /* Left column: Album info */
  .album-column {
    width: 260px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
  }

  .album-info-scroll {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .album-artwork {
    width: 200px;
    height: 200px;
    border-radius: 8px;
    object-fit: cover;
  }

  .album-meta {
    margin-top: 16px;
    font-size: 13px;
    color: var(--text-muted);
  }

  .meta-row {
    margin: 0 0 8px;
    line-height: 1.4;
  }

  .meta-label {
    display: block;
    color: var(--text-muted);
  }

  .label-name {
    font-weight: 600;
    color: var(--text-primary);
  }

  .label-link {
    background: none;
    border: none;
    padding: 0;
    font-size: inherit;
    font-weight: 600;
    color: var(--text-primary);
    cursor: pointer;
    transition: color 150ms ease;
  }

  .label-link:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .meta-date {
    display: block;
    color: var(--text-muted);
  }

  .meta-separator {
    margin: 0 4px;
  }

  .quality-info {
    margin-top: 12px;
    font-size: 13px;
    color: var(--text-secondary);
  }

  /* Tab switcher above content */
  .tab-switcher {
    flex-shrink: 0;
    display: flex;
    gap: 16px;
    margin-bottom: 12px;
    padding-bottom: 12px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
  }

  .tab-btn {
    background: none;
    border: none;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-muted);
    cursor: pointer;
    padding: 0;
    transition: color 150ms ease;
  }

  .tab-btn:hover {
    color: var(--text-secondary);
  }

  .tab-btn.active {
    color: var(--accent-primary);
  }

  /* Right column: Content */
  .content-column {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .scrollable-content {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }

  /* Track list */
  .tracks-list {
    display: flex;
    flex-direction: column;
    padding-right: 8px;
  }

  .track-item {
    /* No background */
  }

  .track-divider {
    height: 1px;
    background: rgba(255, 255, 255, 0.06);
  }

  .track-header {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 0;
    background: none;
    border: none;
    text-align: left;
    cursor: pointer;
    transition: opacity 150ms ease;
  }

  .track-header:hover:not(:disabled) {
    opacity: 0.8;
  }

  .track-header:disabled {
    cursor: default;
  }

  .track-number {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 14px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .play-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-primary);
    border: none;
    border-radius: 50%;
    color: white;
    cursor: pointer;
    transition: transform 150ms ease, background 150ms ease;
  }

  .play-btn:hover {
    transform: scale(1.05);
    background: var(--accent-hover);
  }

  .track-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
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
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-chevron {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .track-item.expanded .track-chevron {
    color: var(--text-primary);
  }

  .track-credits {
    padding: 0 0 12px 40px;
    font-size: 13px;
  }

  .performer-row {
    padding: 4px 0;
    color: var(--text-secondary);
  }

  .performer-name {
    font-weight: 500;
    color: var(--text-primary);
  }

  .performer-link {
    background: none;
    border: none;
    padding: 0;
    font-size: inherit;
    font-weight: 500;
    color: var(--text-primary);
    cursor: pointer;
    transition: color 150ms ease;
  }

  .performer-link:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .performer-roles {
    color: var(--text-muted);
  }

  .track-copyright {
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    font-size: 12px;
    color: var(--text-muted);
  }

  /* Review Content */
  .review-content {
    padding-right: 8px;
  }

  .review-text {
    font-size: 14px;
    line-height: 1.7;
    color: var(--text-secondary);
  }

  .review-text :global(p) {
    margin: 0 0 16px;
  }

  .review-text :global(p:last-child) {
    margin-bottom: 0;
  }

  .review-text :global(a) {
    color: var(--accent-primary);
    text-decoration: none;
  }

  .review-text :global(a:hover) {
    text-decoration: underline;
  }

  .review-text :global(strong),
  .review-text :global(b) {
    font-weight: 600;
    color: var(--text-primary);
  }

  .review-text :global(em),
  .review-text :global(i) {
    font-style: italic;
  }

  /* Scrollbar styling for scrollable content */
  .scrollable-content::-webkit-scrollbar {
    width: 8px;
  }

  .scrollable-content::-webkit-scrollbar-track {
    background: rgba(255, 255, 255, 0.03);
    border-radius: 4px;
  }

  .scrollable-content::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 4px;
  }

  .scrollable-content::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  /* Album info scroll (hidden scrollbar) */
  .album-info-scroll::-webkit-scrollbar {
    width: 0;
  }
</style>
