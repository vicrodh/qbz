<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { X, Loader2 } from 'lucide-svelte';
  import type { TrackInfo, Performer } from '$lib/types';

  interface Props {
    isOpen: boolean;
    trackId: number | null;
    onClose: () => void;
    onArtistClick?: (artistId: number) => void;
    onPerformerSearch?: (name: string) => void;
    onLabelClick?: (labelId: number, labelName: string) => void;
  }

  let { isOpen, trackId, onClose, onArtistClick, onPerformerSearch, onLabelClick }: Props = $props();

  let loading = $state(false);
  let error = $state<string | null>(null);
  let trackInfo = $state<TrackInfo | null>(null);

  // Format duration from seconds to M:SS
  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  // Format quality string
  function formatQuality(track: TrackInfo['track']): string {
    const parts: string[] = [];
    if (track.maximum_bit_depth) {
      parts.push(`${track.maximum_bit_depth}-bit`);
    }
    if (track.maximum_sampling_rate) {
      parts.push(`${track.maximum_sampling_rate}kHz`);
    }
    if (parts.length === 0) {
      return track.hires_streamable ? 'Hi-Res' : 'Lossless';
    }
    return parts.join(' / ');
  }

  // Convert CamelCase role to display format
  function formatRole(role: string): string {
    return role.replace(/([A-Z])/g, ' $1').trim().toUpperCase();
  }

  // Group performers by role and return ordered entries
  function getGroupedCredits(performers: Performer[]): { role: string; names: string[] }[] {
    const grouped: Record<string, string[]> = {};

    performers.forEach(performer => {
      performer.roles.forEach(role => {
        if (!grouped[role]) {
          grouped[role] = [];
        }
        if (!grouped[role].includes(performer.name)) {
          grouped[role].push(performer.name);
        }
      });
    });

    // Define priority roles
    const priorityFirst = ['Composer', 'Lyricist'];
    const priorityLast = ['MainArtist', 'Main Artist'];

    const entries = Object.entries(grouped);

    // Sort: Composer/Lyricist first, MainArtist last, others alphabetically
    entries.sort(([a], [b]) => {
      const aIsFirst = priorityFirst.some(p => a.toLowerCase() === p.toLowerCase());
      const bIsFirst = priorityFirst.some(p => b.toLowerCase() === p.toLowerCase());
      const aIsLast = priorityLast.some(p => a.toLowerCase() === p.toLowerCase());
      const bIsLast = priorityLast.some(p => b.toLowerCase() === p.toLowerCase());

      if (aIsFirst && !bIsFirst) return -1;
      if (!aIsFirst && bIsFirst) return 1;
      if (aIsLast && !bIsLast) return 1;
      if (!aIsLast && bIsLast) return -1;

      // Within priority first, Composer before Lyricist
      if (aIsFirst && bIsFirst) {
        if (a.toLowerCase() === 'composer') return -1;
        if (b.toLowerCase() === 'composer') return 1;
      }

      return a.localeCompare(b);
    });

    return entries.map(([role, names]) => ({ role, names }));
  }

  // Load track info when modal opens
  $effect(() => {
    if (isOpen && trackId) {
      loadTrackInfo(trackId);
    } else {
      trackInfo = null;
      error = null;
    }
  });

  async function loadTrackInfo(id: number) {
    loading = true;
    error = null;
    try {
      trackInfo = await invoke<TrackInfo>('get_track_info', { trackId: id });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      trackInfo = null;
    } finally {
      loading = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    }
  }

  // Derived: grouped credits
  const groupedCredits = $derived(
    trackInfo?.performers ? getGroupedCredits(trackInfo.performers) : []
  );
</script>

<svelte:window on:keydown={handleKeydown} />

{#if isOpen}
  <div class="modal-overlay" onclick={onClose} role="dialog" aria-modal="true">
    <div class="track-modal" onclick={(e) => e.stopPropagation()}>
      {#if loading}
        <div class="loading-state">
          <Loader2 size={32} class="spinner" />
          <span>Loading track info...</span>
        </div>
      {:else if error}
        <div class="error-state">
          <p>Failed to load track info</p>
          <span class="error-message">{error}</span>
        </div>
      {:else if trackInfo}
        <!-- Header -->
        <div class="modal-header">
          <div class="header-titles">
            <h2 class="track-title">{trackInfo.track.title}</h2>
            {#if trackInfo.track.album?.title}
              <span class="album-title">{trackInfo.track.album.title}</span>
            {/if}
            {#if trackInfo.track.performer?.name}
              {@const artistId = trackInfo.track.performer.id}
              {#if artistId && onArtistClick}
                <button
                  class="artist-link"
                  onclick={() => {
                    onArtistClick(artistId);
                    onClose();
                  }}
                >
                  {trackInfo.track.performer.name}
                </button>
              {:else}
                <span class="artist-name">{trackInfo.track.performer.name}</span>
              {/if}
            {/if}
          </div>
          <button class="close-btn" onclick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>

        <!-- Content -->
        <div class="modal-content">
          <!-- Metadata Grid (3 columns) -->
          <div class="metadata-grid">
            <div class="metadata-item">
              <span class="metadata-label">DURATION</span>
              <span class="metadata-value">{formatDuration(trackInfo.track.duration)}</span>
            </div>
            <div class="metadata-item">
              <span class="metadata-label">QUALITY</span>
              <span class="metadata-value">{formatQuality(trackInfo.track)}</span>
            </div>
            {#if trackInfo.track.isrc}
              <div class="metadata-item">
                <span class="metadata-label">ISRC</span>
                <span class="metadata-value mono">{trackInfo.track.isrc}</span>
              </div>
            {/if}
            {#if trackInfo.track.album?.label}
              <div class="metadata-item">
                <span class="metadata-label">LABEL</span>
                {#if onLabelClick}
                  <button
                    class="label-link"
                    onclick={() => {
                      onLabelClick!(trackInfo!.track.album!.label!.id, trackInfo!.track.album!.label!.name);
                      onClose();
                    }}
                  >
                    {trackInfo.track.album.label.name}
                  </button>
                {:else}
                  <span class="metadata-value">{trackInfo.track.album.label.name}</span>
                {/if}
              </div>
            {/if}
          </div>

          <!-- Credits Grid (2 columns) -->
          {#if groupedCredits.length > 0}
            <div class="divider"></div>
            <div class="credits-grid">
              {#each groupedCredits as { role, names }}
                <div class="credit-item">
                  <span class="credit-label">{formatRole(role)}</span>
                  <span class="credit-value">
                    {#each names as name, i}
                      {#if onPerformerSearch}
                        <button
                          class="performer-link"
                          onclick={() => { onPerformerSearch(name); onClose(); }}
                        >{name}</button>{#if i < names.length - 1}, {/if}
                      {:else}
                        {name}{#if i < names.length - 1}, {/if}
                      {/if}
                    {/each}
                  </span>
                </div>
              {/each}
            </div>
          {/if}

          <!-- Copyright -->
          {#if trackInfo.track.copyright}
            <div class="divider"></div>
            <div class="copyright">{trackInfo.track.copyright}</div>
          {/if}
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

  .track-modal {
    background: rgba(26, 26, 26, 0.95);
    backdrop-filter: blur(20px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px;
    width: 100%;
    max-width: 560px;
    max-height: 80vh;
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
    min-height: 72px;
    box-sizing: border-box;
  }

  .header-titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    flex: 1;
    padding-right: 16px;
  }

  .track-title {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.3;
  }

  .album-title {
    font-size: 13px;
    color: var(--text-tertiary, #888888);
  }

  .artist-link {
    font-size: 16px;
    font-weight: 600;
    color: var(--accent-primary);
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    transition: opacity 200ms ease;
  }

  .artist-link:hover {
    opacity: 0.8;
    text-decoration: underline;
  }

  .artist-name {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
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

  /* Content */
  .modal-content {
    padding: 0 24px 24px;
    overflow-y: auto;
  }

  /* Metadata Grid */
  .metadata-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 24px;
  }

  .metadata-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .metadata-label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .metadata-value {
    font-size: 14px;
    color: var(--text-primary);
  }

  .metadata-value.mono {
    font-family: monospace;
    font-size: 13px;
    color: var(--text-tertiary, #888888);
  }

  /* Divider */
  .divider {
    height: 1px;
    background: var(--bg-tertiary);
    margin: 20px 0;
  }

  /* Credits Grid */
  .credits-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 16px 24px;
  }

  .credit-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .credit-label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .credit-value {
    font-size: 14px;
    color: var(--text-primary);
  }

  .performer-link {
    background: none;
    border: none;
    padding: 0;
    font-size: inherit;
    color: var(--text-primary);
    cursor: pointer;
    transition: color 150ms ease;
  }

  .performer-link:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  /* Copyright */
  .copyright {
    font-size: 12px;
    color: var(--text-muted);
  }

  /* Label link */
  .label-link {
    background: none;
    border: none;
    padding: 0;
    font-size: 14px;
    color: var(--accent-primary);
    cursor: pointer;
    text-align: left;
    transition: opacity 150ms ease;
  }

  .label-link:hover {
    opacity: 0.8;
    text-decoration: underline;
  }
</style>
