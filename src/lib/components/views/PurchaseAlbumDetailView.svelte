<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { ArrowLeft, Download, Check, Loader2, FolderOpen, Music, AlertTriangle, Library } from 'lucide-svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import Dropdown from '../Dropdown.svelte';
  import { getAlbumDetail, getFormats, downloadAlbum, downloadTrack } from '$lib/services/purchases';
  import { formatDuration, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import type { PurchasedAlbum, PurchasedTrack, PurchaseFormatOption, PurchaseDownloadProgress } from '$lib/types/purchases';

  interface Props {
    albumId: string;
    onBack?: () => void;
    onArtistClick?: (artistId: number) => void;
  }

  let { albumId, onBack, onArtistClick }: Props = $props();

  let album = $state<PurchasedAlbum | null>(null);
  let formats = $state<PurchaseFormatOption[]>([]);
  let selectedFormatId = $state<number | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // Download state
  let downloadStatuses = $state<Map<number, PurchaseDownloadProgress['status']>>(new Map());
  let isDownloadingAll = $state(false);
  let allComplete = $state(false);

  // Mock mode folder picker
  let mockFolderPath = $state('~/Music');
  let showMockFolderInput = $state(false);
  let pendingDownloadAction = $state<'all' | number | null>(null);

  function formatPurchaseDate(iso?: string): string {
    if (!iso) return '';
    try {
      return new Date(iso).toLocaleDateString(undefined, {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
      });
    } catch {
      return '';
    }
  }

  function formatQualityLabel(bitDepth?: number, samplingRate?: number): string {
    if (!bitDepth || !samplingRate) return '';
    return `${bitDepth}-bit / ${samplingRate} kHz`;
  }

  function getFormatLabels(): string[] {
    return formats.map((f) => f.label);
  }

  function getSelectedFormatLabel(): string {
    const fmt = formats.find((f) => f.id === selectedFormatId);
    return fmt?.label || '';
  }

  function handleFormatChange(label: string) {
    const fmt = formats.find((f) => f.label === label);
    if (fmt) selectedFormatId = fmt.id;
  }

  function getTrackStatus(trackId: number): PurchaseDownloadProgress['status'] | null {
    return downloadStatuses.get(trackId) || null;
  }

  function groupByDisc(trackList: PurchasedTrack[]): Map<number, PurchasedTrack[]> {
    const groups = new Map<number, PurchasedTrack[]>();
    for (const track of trackList) {
      const disc = track.media_number ?? 1;
      if (!groups.has(disc)) groups.set(disc, []);
      groups.get(disc)!.push(track);
    }
    return groups;
  }

  async function promptForFolder(action: 'all' | number) {
    const isMock = import.meta.env.DEV && import.meta.env.VITE_MOCK_PURCHASES === 'true';
    if (isMock) {
      pendingDownloadAction = action;
      showMockFolderInput = true;
      return;
    }

    // Real mode — use Tauri dialog
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const { audioDir } = await import('@tauri-apps/api/path');
      const defaultPath = await audioDir();
      const dest = await open({
        directory: true,
        defaultPath,
        title: get(t)('purchases.chooseFolder'),
      });
      if (dest) {
        await executeDownload(action, dest as string);
      }
    } catch (err) {
      console.error('Folder picker error:', err);
    }
  }

  async function confirmMockFolder() {
    showMockFolderInput = false;
    if (pendingDownloadAction !== null) {
      await executeDownload(pendingDownloadAction, mockFolderPath);
      pendingDownloadAction = null;
    }
  }

  async function executeDownload(action: 'all' | number, destination: string) {
    if (!album || selectedFormatId === null) return;

    if (action === 'all') {
      isDownloadingAll = true;
      const trackItems = album.tracks?.items || [];
      for (const track of trackItems) {
        downloadStatuses.set(track.id, 'downloading');
        downloadStatuses = new Map(downloadStatuses);
        try {
          await downloadTrack(track.id, selectedFormatId, destination);
          downloadStatuses.set(track.id, 'complete');
        } catch {
          downloadStatuses.set(track.id, 'failed');
        }
        downloadStatuses = new Map(downloadStatuses);
      }
      isDownloadingAll = false;
      // Check if all complete
      const allDone = trackItems.every(
        (track) => downloadStatuses.get(track.id) === 'complete'
      );
      if (allDone) allComplete = true;
    } else {
      downloadStatuses.set(action, 'downloading');
      downloadStatuses = new Map(downloadStatuses);
      try {
        await downloadTrack(action, selectedFormatId, destination);
        downloadStatuses.set(action, 'complete');
      } catch {
        downloadStatuses.set(action, 'failed');
      }
      downloadStatuses = new Map(downloadStatuses);
    }
  }

  async function loadAlbum() {
    loading = true;
    error = null;
    try {
      const [albumData, formatData] = await Promise.all([
        getAlbumDetail(albumId),
        getFormats(albumId),
      ]);
      album = albumData;
      formats = formatData;
      if (formatData.length > 0) {
        selectedFormatId = formatData[0].id;
      }
    } catch (err) {
      error = String(err);
    } finally {
      loading = false;
    }
  }

  const discGroups = $derived(
    album?.tracks?.items ? groupByDisc(album.tracks.items) : new Map()
  );
  const isMultiDisc = $derived(discGroups.size > 1);
  const completedCount = $derived(
    Array.from(downloadStatuses.values()).filter((s) => s === 'complete').length
  );
  const totalTracks = $derived(album?.tracks?.items?.length || 0);

  onMount(() => {
    loadAlbum();
  });
</script>

<div class="purchase-album-detail">
  <!-- Back button -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>{$t('nav.backTo')}</span>
  </button>

  {#if loading}
    <div class="loading-state">
      <Loader2 size={24} class="spin" />
      <span>{$t('actions.loading')}</span>
    </div>
  {:else if error}
    <div class="error-state">
      <p>{error}</p>
    </div>
  {:else if album}
    <!-- Album header -->
    <div class="album-header">
      <div class="album-cover">
        <img src={getQobuzImage(album.image)} alt={album.title} />
      </div>
      <div class="album-info">
        <h2 class="album-title">{album.title}</h2>
        <button class="artist-name" onclick={() => onArtistClick?.(album!.artist.id)}>
          {album.artist.name}
        </button>
        {#if album.purchased_at}
          <span class="purchase-date">
            {$t('purchases.purchasedOn')} {formatPurchaseDate(album.purchased_at)}
          </span>
        {/if}
        {#if album.label}
          <span class="label-name">{album.label.name}</span>
        {/if}
        <div class="quality-info">
          <QualityBadge
            bitDepth={album.maximum_bit_depth}
            samplingRate={album.maximum_sampling_rate}
          />
          <span class="quality-text">
            {formatQualityLabel(album.maximum_bit_depth, album.maximum_sampling_rate)}
          </span>
        </div>

        {#if !album.downloadable}
          <div class="unavailable-banner">
            <AlertTriangle size={14} />
            <span>{$t('purchases.unavailable')}</span>
          </div>
        {:else}
          <!-- Format selector + Download All -->
          <div class="download-controls">
            {#if formats.length > 0}
              <div class="format-picker">
                <span class="format-label">{$t('purchases.format')}:</span>
                <Dropdown
                  value={getSelectedFormatLabel()}
                  options={getFormatLabels()}
                  onchange={handleFormatChange}
                />
              </div>
            {/if}
            <button
              class="download-all-btn"
              onclick={() => promptForFolder('all')}
              disabled={isDownloadingAll || !selectedFormatId}
            >
              {#if isDownloadingAll}
                <Loader2 size={14} class="spin" />
                <span>{$t('purchases.downloading')}</span>
              {:else}
                <Download size={14} />
                <span>{$t('purchases.downloadAll')}</span>
              {/if}
            </button>
          </div>
        {/if}
      </div>
    </div>

    <!-- Mock folder picker -->
    {#if showMockFolderInput}
      <div class="mock-folder-picker">
        <FolderOpen size={14} />
        <span>{$t('purchases.chooseFolder')}:</span>
        <input type="text" bind:value={mockFolderPath} />
        <button class="confirm-btn" onclick={confirmMockFolder}>OK</button>
        <button class="cancel-btn" onclick={() => { showMockFolderInput = false; pendingDownloadAction = null; }}>
          {$t('actions.cancel')}
        </button>
      </div>
    {/if}

    <!-- Download progress bar -->
    {#if isDownloadingAll || allComplete}
      <div class="progress-bar-container">
        <div class="progress-label">
          {#if allComplete}
            <Check size={14} />
            <span>{$t('purchases.complete')}</span>
          {:else}
            <Loader2 size={14} class="spin" />
            <span>{$t('purchases.downloadProgress', { values: { current: completedCount, total: totalTracks } })}</span>
          {/if}
        </div>
        <div class="progress-bar">
          <div
            class="progress-fill"
            class:complete={allComplete}
            style="width: {totalTracks > 0 ? (completedCount / totalTracks) * 100 : 0}%"
          ></div>
        </div>
      </div>
    {/if}

    <!-- Add to Library button (after all downloads complete) -->
    {#if allComplete}
      <div class="add-to-library">
        <button class="add-to-library-btn">
          <Library size={14} />
          <span>{$t('purchases.addToLibrary')}</span>
        </button>
        <span class="add-to-library-desc">{$t('purchases.addToLibraryDesc')}</span>
      </div>
    {/if}

    <!-- Track list -->
    <div class="track-list">
      {#each [...discGroups.entries()] as [discNum, discTracks] (discNum)}
        {#if isMultiDisc}
          <div class="disc-header">
            <span>Disc {discNum}</span>
          </div>
        {/if}
        {#each discTracks as track (track.id)}
          {@const status = getTrackStatus(track.id)}
          <div class="track-row" class:downloading={status === 'downloading'} class:complete={status === 'complete'} class:failed={status === 'failed'}>
            <span class="track-number">{track.track_number}</span>
            <div class="track-info">
              <span class="track-title">{track.title}</span>
              {#if track.performer.name !== album.artist.name}
                <span class="track-performer">{track.performer.name}</span>
              {/if}
            </div>
            <span class="track-duration">{formatDuration(track.duration)}</span>
            <div class="track-quality">
              {#if track.hires}
                <QualityBadge bitDepth={track.maximum_bit_depth} samplingRate={track.maximum_sampling_rate} />
              {/if}
            </div>
            <div class="track-action">
              {#if status === 'complete'}
                <span class="status-icon complete"><Check size={14} /></span>
              {:else if status === 'downloading'}
                <span class="status-icon downloading"><Loader2 size={14} class="spin" /></span>
              {:else if status === 'failed'}
                <button class="download-track-btn failed" onclick={() => promptForFolder(track.id)} title={$t('purchases.failed')}>
                  <AlertTriangle size={14} />
                </button>
              {:else if album.downloadable}
                <button class="download-track-btn" onclick={() => promptForFolder(track.id)} title={$t('purchases.downloadTrack')}>
                  <Download size={14} />
                </button>
              {/if}
            </div>
          </div>
        {/each}
      {/each}
    </div>
  {/if}
</div>

<style>
  .purchase-album-detail {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 4px 0;
    font-size: 0.8125rem;
    width: fit-content;
  }

  .back-btn:hover {
    color: var(--accent-primary);
  }

  .loading-state,
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 60px 20px;
    color: var(--text-tertiary);
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  /* Album header */
  .album-header {
    display: flex;
    gap: 24px;
  }

  .album-cover {
    flex-shrink: 0;
    width: 200px;
    height: 200px;
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-secondary);
  }

  .album-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .album-info {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .album-title {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .artist-name {
    background: none;
    border: none;
    padding: 0;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 1rem;
    text-align: left;
  }

  .artist-name:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .purchase-date,
  .label-name {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
  }

  .quality-info {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
  }

  .quality-text {
    font-size: 0.8125rem;
    color: var(--text-secondary);
  }

  .unavailable-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    padding: 8px 12px;
    background: var(--bg-warning, rgba(255, 152, 0, 0.1));
    border: 1px solid var(--border-warning, rgba(255, 152, 0, 0.3));
    border-radius: 6px;
    color: var(--text-warning, #ff9800);
    font-size: 0.8125rem;
  }

  .download-controls {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 8px;
    flex-wrap: wrap;
  }

  .format-picker {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .format-label {
    font-size: 0.8125rem;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .download-all-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    border-radius: 6px;
    border: none;
    background: var(--accent-primary);
    color: var(--accent-on-primary, #fff);
    cursor: pointer;
    font-size: 0.8125rem;
    font-weight: 500;
    transition: opacity 0.15s ease;
  }

  .download-all-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .download-all-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Mock folder picker */
  .mock-folder-picker {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-primary);
    border-radius: 8px;
    font-size: 0.8125rem;
    color: var(--text-secondary);
  }

  .mock-folder-picker input {
    flex: 1;
    background: var(--bg-primary);
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    padding: 4px 8px;
    color: var(--text-primary);
    font-size: 0.8125rem;
    font-family: monospace;
  }

  .confirm-btn {
    padding: 4px 12px;
    border-radius: 4px;
    border: none;
    background: var(--accent-primary);
    color: var(--accent-on-primary, #fff);
    cursor: pointer;
    font-size: 0.8125rem;
  }

  .cancel-btn {
    padding: 4px 12px;
    border-radius: 4px;
    border: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 0.8125rem;
  }

  /* Progress bar */
  .progress-bar-container {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 14px;
    background: var(--bg-secondary);
    border-radius: 8px;
    border: 1px solid var(--border-primary);
  }

  .progress-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 0.8125rem;
    color: var(--text-secondary);
  }

  .progress-bar {
    height: 4px;
    background: var(--bg-tertiary);
    border-radius: 2px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent-primary);
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .progress-fill.complete {
    background: var(--success, #4caf50);
  }

  /* Add to Library */
  .add-to-library {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    background: var(--bg-secondary);
    border-radius: 8px;
    border: 1px solid var(--success, #4caf50);
  }

  .add-to-library-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    border-radius: 6px;
    border: none;
    background: var(--success, #4caf50);
    color: #fff;
    cursor: pointer;
    font-size: 0.8125rem;
    font-weight: 500;
    white-space: nowrap;
  }

  .add-to-library-desc {
    font-size: 0.8125rem;
    color: var(--text-tertiary);
  }

  /* Track list */
  .track-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding-bottom: 24px;
  }

  .disc-header {
    padding: 12px 12px 6px;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .track-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 6px;
    transition: background 0.15s ease;
  }

  .track-row:hover {
    background: var(--bg-hover);
  }

  .track-row.downloading {
    background: var(--bg-active);
  }

  .track-row.complete {
    opacity: 0.7;
  }

  .track-number {
    width: 28px;
    text-align: right;
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }

  .track-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .track-title {
    font-size: 0.875rem;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-performer {
    font-size: 0.75rem;
    color: var(--text-tertiary);
  }

  .track-duration {
    flex-shrink: 0;
    font-size: 0.8125rem;
    color: var(--text-tertiary);
    min-width: 45px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .track-quality {
    flex-shrink: 0;
  }

  .track-action {
    flex-shrink: 0;
    width: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .status-icon {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .status-icon.complete {
    color: var(--success, #4caf50);
  }

  .status-icon.downloading {
    color: var(--accent-primary);
  }

  .download-track-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    border: 1px solid var(--border-primary);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .download-track-btn:hover {
    background: var(--accent-primary);
    color: var(--accent-on-primary, #fff);
    border-color: var(--accent-primary);
  }

  .download-track-btn.failed {
    color: var(--error, #f44336);
    border-color: var(--error, #f44336);
  }
</style>
