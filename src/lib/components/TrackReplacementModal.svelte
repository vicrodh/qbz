<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { X, Search, Play, Check, AlertCircle, Loader2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface SearchTrack {
    id: number;
    title: string;
    duration: number;
    performer?: { id?: number; name: string };
    album?: {
      id: string;
      title: string;
      image?: { small?: string; thumbnail?: string; large?: string };
    };
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    streamable?: boolean;
  }

  interface SearchResultsPage {
    items: SearchTrack[];
    total: number;
  }

  interface Props {
    isOpen: boolean;
    trackTitle: string;
    trackArtist?: string;
    onClose: () => void;
    onSelect: (track: SearchTrack) => void;
    onPreview?: (track: SearchTrack) => void;
  }

  let {
    isOpen,
    trackTitle,
    trackArtist,
    onClose,
    onSelect,
    onPreview
  }: Props = $props();

  let searchQuery = $state('');
  let results = $state<SearchTrack[]>([]);
  let loading = $state(false);
  let searched = $state(false);
  let selectedTrackId = $state<number | null>(null);

  // Initialize search query when modal opens
  $effect(() => {
    if (isOpen) {
      // Build initial search query from track info
      const query = trackArtist
        ? `${trackTitle} ${trackArtist}`
        : trackTitle;
      searchQuery = query;
      selectedTrackId = null;
      searched = false;
      results = [];
      // Auto-search on open
      doSearch();
    }
  });

  async function doSearch() {
    if (!searchQuery.trim()) return;

    loading = true;
    searched = true;
    try {
      const response = await invoke<SearchResultsPage>('search_tracks', {
        query: searchQuery.trim(),
        limit: 20
      });
      // Filter out non-streamable tracks
      results = response.items.filter(t => t.streamable !== false);
    } catch (err) {
      console.error('Search failed:', err);
      results = [];
    } finally {
      loading = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      doSearch();
    } else if (e.key === 'Escape') {
      onClose();
    }
  }

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function formatQuality(track: SearchTrack): string {
    if (track.maximum_bit_depth && track.maximum_sampling_rate) {
      return `${track.maximum_bit_depth}bit/${track.maximum_sampling_rate}kHz`;
    }
    if (track.hires) return 'Hi-Res';
    return 'CD';
  }

  function handleSelect(track: SearchTrack) {
    selectedTrackId = track.id;
  }

  function handleConfirm() {
    const selected = results.find(t => t.id === selectedTrackId);
    if (selected) {
      onSelect(selected);
    }
  }

  function handlePreview(e: MouseEvent, track: SearchTrack) {
    e.stopPropagation();
    onPreview?.(track);
  }
</script>

{#if isOpen}
  <div class="modal-overlay" onclick={onClose} onkeydown={handleKeydown} role="dialog" aria-modal="true">
    <div class="modal-content" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>{$t('playlist.findReplacement')}</h2>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      <div class="search-section">
        <div class="search-info">
          <AlertCircle size={16} />
          <span>{$t('playlist.searchingFor')}: <strong>{trackTitle}</strong>{trackArtist ? ` - ${trackArtist}` : ''}</span>
        </div>
        <div class="search-input-container">
          <Search size={16} class="search-icon" />
          <input
            type="text"
            bind:value={searchQuery}
            placeholder={$t('search.placeholder')}
            class="search-input"
            onkeydown={handleKeydown}
          />
          <button class="search-btn" onclick={doSearch} disabled={loading}>
            {#if loading}
              <Loader2 size={16} class="spinner" />
            {:else}
              {$t('search.search')}
            {/if}
          </button>
        </div>
      </div>

      <div class="results-section">
        {#if loading}
          <div class="loading-state">
            <Loader2 size={24} class="spinner" />
            <span>{$t('common.loading')}</span>
          </div>
        {:else if searched && results.length === 0}
          <div class="empty-state">
            <AlertCircle size={32} />
            <p>{$t('playlist.noReplacementsFound')}</p>
            <p class="hint">{$t('playlist.tryDifferentSearch')}</p>
          </div>
        {:else if results.length > 0}
          <div class="results-list">
            {#each results as track (track.id)}
              <button
                class="result-item"
                class:selected={selectedTrackId === track.id}
                onclick={() => handleSelect(track)}
                ondblclick={() => { handleSelect(track); handleConfirm(); }}
              >
                <div class="track-artwork">
                  {#if track.album?.image?.thumbnail}
                    <img src={track.album.image.thumbnail} alt="" />
                  {:else}
                    <div class="artwork-placeholder"></div>
                  {/if}
                </div>
                <div class="track-info">
                  <div class="track-title">{track.title}</div>
                  <div class="track-meta">
                    <span class="artist">{track.performer?.name ?? 'Unknown'}</span>
                    <span class="separator">•</span>
                    <span class="album">{track.album?.title ?? 'Unknown Album'}</span>
                  </div>
                </div>
                <div class="track-quality" class:hires={track.hires}>
                  {formatQuality(track)}
                </div>
                <div class="track-duration">{formatDuration(track.duration)}</div>
                {#if onPreview}
                  <button
                    class="preview-btn"
                    onclick={(e) => handlePreview(e, track)}
                    title={$t('common.preview')}
                  >
                    <Play size={14} fill="currentColor" />
                  </button>
                {/if}
                {#if selectedTrackId === track.id}
                  <div class="selected-indicator">
                    <Check size={16} />
                  </div>
                {/if}
              </button>
            {/each}
          </div>
        {:else}
          <div class="empty-state initial">
            <Search size={32} />
            <p>{$t('playlist.searchToFindReplacement')}</p>
          </div>
        {/if}
      </div>

      <div class="modal-footer">
        <button class="cancel-btn" onclick={onClose}>
          {$t('common.cancel')}
        </button>
        <button
          class="confirm-btn"
          onclick={handleConfirm}
          disabled={selectedTrackId === null}
        >
          {$t('playlist.replaceTrack')}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    backdrop-filter: blur(4px);
  }

  .modal-content {
    background: var(--bg-primary);
    border-radius: 12px;
    width: 90%;
    max-width: 700px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
    border: 1px solid var(--bg-tertiary);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .modal-header h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    transition: all 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .search-section {
    padding: 16px 24px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .search-info {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .search-info strong {
    color: var(--text-primary);
  }

  .search-input-container {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-tertiary);
    border-radius: 8px;
    padding: 8px 12px;
  }

  .search-input-container :global(.search-icon) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 14px;
    outline: none;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-btn {
    padding: 6px 16px;
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .search-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .search-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .results-section {
    flex: 1;
    overflow-y: auto;
    min-height: 200px;
    max-height: 400px;
  }

  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px;
    color: var(--text-muted);
    gap: 12px;
  }

  .empty-state p {
    margin: 0;
  }

  .empty-state .hint {
    font-size: 13px;
    opacity: 0.7;
  }

  .empty-state.initial {
    color: var(--text-secondary);
  }

  .results-list {
    padding: 8px;
  }

  .result-item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 10px 12px;
    background: none;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: all 150ms ease;
    text-align: left;
  }

  .result-item:hover {
    background: var(--bg-hover);
  }

  .result-item.selected {
    background: var(--accent-primary-alpha, rgba(99, 102, 241, 0.15));
    outline: 1px solid var(--accent-primary);
  }

  .track-artwork {
    width: 44px;
    height: 44px;
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .track-artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    background: var(--bg-tertiary);
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

  .track-meta {
    font-size: 12px;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
  }

  .track-meta .separator {
    opacity: 0.5;
  }

  .track-meta .artist,
  .track-meta .album {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-meta .album {
    flex-shrink: 1;
  }

  .track-quality {
    font-size: 11px;
    color: var(--text-muted);
    padding: 2px 8px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    white-space: nowrap;
  }

  .track-quality.hires {
    color: var(--accent-primary);
    background: var(--accent-primary-alpha, rgba(99, 102, 241, 0.15));
  }

  .track-duration {
    font-size: 13px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    width: 45px;
    text-align: right;
  }

  .preview-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 50%;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
    opacity: 0;
  }

  .result-item:hover .preview-btn {
    opacity: 1;
  }

  .preview-btn:hover {
    background: var(--accent-primary);
    color: white;
  }

  .selected-indicator {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--accent-primary);
    color: white;
    border-radius: 50%;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    padding: 16px 24px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .cancel-btn,
  .confirm-btn {
    padding: 10px 20px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .cancel-btn {
    background: none;
    border: 1px solid var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .cancel-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .confirm-btn {
    background: var(--accent-primary);
    border: none;
    color: white;
  }

  .confirm-btn:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .confirm-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .spinner {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
