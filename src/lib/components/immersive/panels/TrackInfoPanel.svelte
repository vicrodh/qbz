<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Loader2, Disc3, Clock, Music2, Tag, Building2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { TrackInfo, Performer } from '$lib/types';

  interface Props {
    trackId?: number;
    onArtistClick?: (artistId: number) => void;
    onLabelClick?: (labelId: number, labelName: string) => void;
  }

  let { trackId, onArtistClick, onLabelClick }: Props = $props();

  let loading = $state(false);
  let error = $state<string | null>(null);
  let trackInfo = $state<TrackInfo | null>(null);
  let loadedTrackId = $state<number | null>(null);

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
    return role.replace(/([A-Z])/g, ' $1').trim();
  }

  // Group performers by role
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

    // Priority ordering
    const priorityFirst = ['Composer', 'Lyricist', 'Producer'];
    const priorityLast = ['MainArtist', 'Main Artist'];

    const entries = Object.entries(grouped);

    entries.sort(([a], [b]) => {
      const aIsFirst = priorityFirst.some(p => a.toLowerCase() === p.toLowerCase());
      const bIsFirst = priorityFirst.some(p => b.toLowerCase() === p.toLowerCase());
      const aIsLast = priorityLast.some(p => a.toLowerCase() === p.toLowerCase());
      const bIsLast = priorityLast.some(p => b.toLowerCase() === p.toLowerCase());

      if (aIsFirst && !bIsFirst) return -1;
      if (!aIsFirst && bIsFirst) return 1;
      if (aIsLast && !bIsLast) return 1;
      if (!aIsLast && bIsLast) return -1;

      return a.localeCompare(b);
    });

    return entries.map(([role, names]) => ({ role, names }));
  }

  // Load track info only when trackId actually changes
  $effect(() => {
    if (trackId && trackId !== loadedTrackId) {
      loadTrackInfo(trackId);
    } else if (!trackId && loadedTrackId) {
      // Track ended/cleared
      trackInfo = null;
      error = null;
      loadedTrackId = null;
    }
  });

  async function loadTrackInfo(id: number) {
    loading = true;
    error = null;
    try {
      trackInfo = await invoke<TrackInfo>('get_track_info', { trackId: id });
      loadedTrackId = id;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      trackInfo = null;
      loadedTrackId = null;
    } finally {
      loading = false;
    }
  }

  const groupedCredits = $derived(
    trackInfo?.performers ? getGroupedCredits(trackInfo.performers) : []
  );
</script>

<div class="track-info-panel">
  {#if loading}
    <div class="loading-state">
      <Loader2 size={28} class="spinner" />
    </div>
  {:else if error}
    <div class="error-state">
      <span>{$t('errors.failedToLoad') || 'Failed to load'}</span>
    </div>
  {:else if !trackId}
    <div class="empty-state">
      <Music2 size={32} strokeWidth={1.5} />
      <span>{$t('player.noTrackSelected') || 'No track selected'}</span>
    </div>
  {:else if trackInfo}
    <div class="info-content">
      <!-- Track Header -->
      <div class="track-header">
        <h2 class="track-title">{trackInfo.track.title}</h2>
        {#if trackInfo.track.album?.title}
          <p class="album-title">{trackInfo.track.album.title}</p>
        {/if}
      </div>

      <!-- Metadata Section -->
      <div class="metadata-section">
        <div class="metadata-item">
          <Clock size={14} />
          <span class="metadata-label">{$t('track.duration') || 'Duration'}</span>
          <span class="metadata-value">{formatDuration(trackInfo.track.duration)}</span>
        </div>

        <div class="metadata-item">
          <Music2 size={14} />
          <span class="metadata-label">{$t('track.quality') || 'Quality'}</span>
          <span class="metadata-value">{formatQuality(trackInfo.track)}</span>
        </div>

        {#if trackInfo.track.isrc}
          <div class="metadata-item">
            <Tag size={14} />
            <span class="metadata-label">ISRC</span>
            <span class="metadata-value mono">{trackInfo.track.isrc}</span>
          </div>
        {/if}

        {#if trackInfo.track.album?.label}
          <div class="metadata-item">
            <Building2 size={14} />
            <span class="metadata-label">{$t('album.label') || 'Label'}</span>
            {#if onLabelClick}
              <button
                class="metadata-link"
                onclick={() => onLabelClick!(trackInfo!.track.album!.label!.id, trackInfo!.track.album!.label!.name)}
              >
                {trackInfo.track.album.label.name}
              </button>
            {:else}
              <span class="metadata-value">{trackInfo.track.album.label.name}</span>
            {/if}
          </div>
        {/if}
      </div>

      <!-- Credits Section -->
      {#if groupedCredits.length > 0}
        <div class="credits-section">
          <h3 class="section-title">{$t('player.credits') || 'Credits'}</h3>
          <div class="credits-list">
            {#each groupedCredits as { role, names }}
              <div class="credit-item">
                <span class="credit-role">{formatRole(role)}</span>
                <span class="credit-names">{names.join(', ')}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Copyright -->
      {#if trackInfo.track.copyright}
        <div class="copyright-section">
          <p class="copyright">{trackInfo.track.copyright}</p>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .track-info-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .loading-state,
  .error-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 100%;
    min-height: 200px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    font-size: 14px;
  }

  .loading-state :global(.spinner) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .info-content {
    flex: 1;
    overflow-y: auto;
    padding-right: 8px;
  }

  .info-content::-webkit-scrollbar {
    width: 4px;
  }

  .info-content::-webkit-scrollbar-track {
    background: transparent;
  }

  .info-content::-webkit-scrollbar-thumb {
    background: var(--alpha-20, rgba(255, 255, 255, 0.2));
    border-radius: 2px;
  }

  /* Track Header */
  .track-header {
    margin-bottom: 24px;
  }

  .track-title {
    font-size: 20px;
    font-weight: 700;
    color: var(--text-primary, white);
    margin: 0 0 6px 0;
    line-height: 1.3;
  }

  .album-title {
    font-size: 14px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    margin: 0;
  }

  /* Metadata Section */
  .metadata-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-bottom: 20px;
    border-bottom: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    margin-bottom: 20px;
  }

  .metadata-item {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
  }

  .metadata-label {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    min-width: 70px;
  }

  .metadata-value {
    font-size: 14px;
    color: var(--text-primary, white);
  }

  .metadata-value.mono {
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
  }

  .metadata-link {
    font-size: 14px;
    color: var(--accent-primary, #7c3aed);
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    transition: opacity 150ms ease;
  }

  .metadata-link:hover {
    opacity: 0.8;
    text-decoration: underline;
  }

  /* Credits Section */
  .credits-section {
    padding-bottom: 20px;
    border-bottom: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    margin-bottom: 20px;
  }

  .section-title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    margin: 0 0 16px 0;
  }

  .credits-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .credit-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .credit-role {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
  }

  .credit-names {
    font-size: 14px;
    color: var(--text-primary, white);
    line-height: 1.4;
  }

  /* Copyright */
  .copyright-section {
    padding-top: 4px;
  }

  .copyright {
    font-size: 11px;
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
    margin: 0;
    line-height: 1.5;
  }
</style>
