<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import Modal from './Modal.svelte';
  import { Clock, Disc3, Music2, User, Users, Copyright, Loader2 } from 'lucide-svelte';
  import type { TrackInfo, Performer } from '$lib/types';

  interface Props {
    isOpen: boolean;
    trackId: number | null;
    onClose: () => void;
  }

  let { isOpen, trackId, onClose }: Props = $props();

  let loading = $state(false);
  let error = $state<string | null>(null);
  let trackInfo = $state<TrackInfo | null>(null);

  // Format duration from seconds to MM:SS
  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  // Format sample rate for display
  function formatSampleRate(rate?: number): string {
    if (!rate) return '';
    if (rate >= 1000) {
      return `${(rate / 1000).toFixed(1)}kHz`.replace('.0kHz', 'kHz');
    }
    return `${rate}Hz`;
  }

  // Format quality string
  function formatQuality(track: TrackInfo['track']): string {
    const parts: string[] = [];
    if (track.maximum_bit_depth) {
      parts.push(`${track.maximum_bit_depth}-bit`);
    }
    if (track.maximum_sampling_rate) {
      parts.push(formatSampleRate(track.maximum_sampling_rate * 1000));
    }
    if (parts.length === 0) {
      return track.hires_streamable ? 'Hi-Res' : 'Lossless';
    }
    return parts.join(' / ');
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
</script>

<Modal {isOpen} {onClose} title="Track Info" maxWidth="520px">
  {#snippet children()}
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
      <div class="track-info">
        <!-- Track Details Section -->
        <section class="info-section">
          <div class="info-row">
            <Music2 size={16} class="info-icon" />
            <span class="info-label">Title</span>
            <span class="info-value">{trackInfo.track.title}</span>
          </div>

          {#if trackInfo.track.performer?.name}
            <div class="info-row">
              <User size={16} class="info-icon" />
              <span class="info-label">Artist</span>
              <span class="info-value">{trackInfo.track.performer.name}</span>
            </div>
          {/if}

          {#if trackInfo.track.album?.title}
            <div class="info-row">
              <Disc3 size={16} class="info-icon" />
              <span class="info-label">Album</span>
              <span class="info-value">{trackInfo.track.album.title}</span>
            </div>
          {/if}

          <div class="info-row">
            <Clock size={16} class="info-icon" />
            <span class="info-label">Duration</span>
            <span class="info-value">{formatDuration(trackInfo.track.duration)}</span>
          </div>

          <div class="info-row">
            <span class="info-label quality-label">Quality</span>
            <span class="info-value quality-value">{formatQuality(trackInfo.track)}</span>
          </div>

          {#if trackInfo.track.isrc}
            <div class="info-row">
              <span class="info-label">ISRC</span>
              <span class="info-value mono">{trackInfo.track.isrc}</span>
            </div>
          {/if}

          {#if trackInfo.track.composer?.name}
            <div class="info-row">
              <span class="info-label">Composer</span>
              <span class="info-value">{trackInfo.track.composer.name}</span>
            </div>
          {/if}
        </section>

        <!-- Credits Section -->
        {#if trackInfo.performers.length > 0}
          <section class="credits-section">
            <h3 class="section-title">
              <Users size={16} />
              Credits
            </h3>
            <div class="performers-list">
              {#each trackInfo.performers as performer}
                <div class="performer">
                  <span class="performer-name">{performer.name}</span>
                  {#if performer.roles.length > 0}
                    <span class="performer-roles">{performer.roles.join(', ')}</span>
                  {/if}
                </div>
              {/each}
            </div>
          </section>
        {/if}

        <!-- Copyright Section -->
        {#if trackInfo.track.copyright}
          <section class="copyright-section">
            <div class="copyright">
              <Copyright size={14} />
              <span>{trackInfo.track.copyright}</span>
            </div>
          </section>
        {/if}
      </div>
    {/if}
  {/snippet}
</Modal>

<style>
  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 40px 20px;
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
    padding: 40px 20px;
    color: var(--text-muted);
  }

  .error-message {
    display: block;
    margin-top: 8px;
    font-size: 13px;
    color: var(--danger);
  }

  .track-info {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .info-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .info-row {
    display: grid;
    grid-template-columns: 20px 80px 1fr;
    gap: 8px;
    align-items: center;
  }

  .info-row :global(.info-icon) {
    color: var(--text-muted);
  }

  .info-label {
    font-size: 13px;
    color: var(--text-muted);
  }

  .quality-label {
    grid-column: 1 / 3;
  }

  .info-value {
    font-size: 14px;
    color: var(--text-primary);
  }

  .info-value.mono {
    font-family: monospace;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .quality-value {
    background: var(--bg-tertiary);
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 12px;
    width: fit-content;
  }

  .credits-section {
    border-top: 1px solid var(--bg-tertiary);
    padding-top: 16px;
  }

  .section-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 12px 0;
  }

  .performers-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .performer {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border-radius: 6px;
  }

  .performer-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .performer-roles {
    font-size: 12px;
    color: var(--text-muted);
  }

  .copyright-section {
    border-top: 1px solid var(--bg-tertiary);
    padding-top: 16px;
  }

  .copyright {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-muted);
  }
</style>
