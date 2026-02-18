<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { ArrowLeft, Download, Check, Loader2, AlertTriangle, Library } from 'lucide-svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import Dropdown from '../Dropdown.svelte';
  import ViewTransition from '../ViewTransition.svelte';
  import { getAlbumDetail, getFormats, downloadAlbum, downloadTrack, markTrackDownloaded } from '$lib/services/purchases';
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

  function formatTotalDuration(seconds: number): string {
    const hrs = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    if (hrs > 0) return `${hrs}h ${mins}m`;
    return `${mins}m`;
  }

  async function promptForFolder(action: 'all' | number) {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const { audioDir } = await import('@tauri-apps/api/path');
      const defaultPath = await audioDir();
      const dest = await open({
        directory: true,
        multiple: false,
        defaultPath,
        title: get(t)('purchases.chooseFolder'),
      });
      if (dest && typeof dest === 'string') {
        await executeDownload(action, dest);
      }
    } catch (err) {
      console.error('Folder picker error:', err);
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
          await markTrackDownloaded(track.id, album.id, destination).catch(() => {});
        } catch {
          downloadStatuses.set(track.id, 'failed');
        }
        downloadStatuses = new Map(downloadStatuses);
      }
      isDownloadingAll = false;
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
        await markTrackDownloaded(action, album.id, destination).catch(() => {});
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
  const totalDurationSeconds = $derived(
    (album?.tracks?.items || []).reduce((sum, track) => sum + track.duration, 0)
  );

  onMount(() => {
    loadAlbum();
  });
</script>

<ViewTransition duration={200} distance={12} direction="up">
<div class="purchase-album-detail">
  <!-- Back Navigation -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>Back</span>
  </button>

  {#if loading}
    <div class="loading-state">
      <div class="spinner"></div>
    </div>
  {:else if error}
    <div class="error-state">
      <p>{error}</p>
    </div>
  {:else if album}
    <!-- Album Header -->
    <div class="album-header">
      <div class="artwork" class:unavailable={!album.downloadable}>
        <img src={getQobuzImage(album.image)} alt={album.title} />
        {#if !album.downloadable}
          <div class="artwork-unavailable-overlay">
            <AlertTriangle size={18} />
            <span>{$t('purchases.unavailable')}</span>
          </div>
        {/if}
      </div>
      <div class="metadata">
        <h1 class="album-title">{album.title}</h1>
        {#if onArtistClick}
          <button class="artist-link" onclick={() => onArtistClick?.(album!.artist.id)}>
            {album.artist.name}
          </button>
        {:else}
          <div class="artist-name">{album.artist.name}</div>
        {/if}
        <div class="album-info">
          {#if album.purchased_at}
            {$t('purchases.purchasedOn')} {formatPurchaseDate(album.purchased_at)}
          {/if}
          {#if album.label}
            {#if album.purchased_at} &middot; {/if}
            {album.label.name}
          {/if}
          {#if album.genre}
            &middot; {album.genre.name}
          {/if}
        </div>
        <div class="album-quality">
          <QualityBadge
            bitDepth={album.maximum_bit_depth}
            samplingRate={album.maximum_sampling_rate}
            compact={true}
          />
        </div>
        <div class="album-stats">{totalTracks} tracks &middot; {formatTotalDuration(totalDurationSeconds)}</div>

        {#if album.downloadable}
          <!-- Actions -->
          <div class="actions">
            <button
              class="action-btn-circle primary"
              onclick={() => promptForFolder('all')}
              disabled={isDownloadingAll || !selectedFormatId}
              title={$t('purchases.downloadAll')}
            >
              {#if isDownloadingAll}
                <Loader2 size={20} class="spin" />
              {:else}
                <Download size={20} />
              {/if}
            </button>

            {#if formats.length > 0}
              <Dropdown
                value={getSelectedFormatLabel()}
                options={getFormatLabels()}
                onchange={handleFormatChange}
              />
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- Download progress -->
    {#if isDownloadingAll || allComplete}
      <div class="progress-section">
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

    <!-- Add to Library -->
    {#if allComplete}
      <div class="add-to-library">
        <button class="add-to-library-btn">
          <Library size={14} />
          <span>{$t('purchases.addToLibrary')}</span>
        </button>
        <span class="add-to-library-desc">{$t('purchases.addToLibraryDesc')}</span>
      </div>
    {/if}

    <!-- Divider -->
    <div class="divider"></div>

    <!-- Track List -->
    <div class="track-list">
      <!-- Table Header -->
      <div class="table-header">
        <div class="col-number">#</div>
        <div class="col-title">Title</div>
        <div class="col-duration">Duration</div>
        <div class="col-quality">Quality</div>
        <div class="col-icon"><Download size={14} /></div>
      </div>

      <!-- Track Rows -->
      <div class="tracks">
        {#each [...discGroups.entries()] as [discNum, discTracks] (discNum)}
          {#if isMultiDisc}
            <div class="disc-header">
              <span>Disc {discNum}</span>
            </div>
          {/if}
          {#each discTracks as track (track.id)}
            {@const status = getTrackStatus(track.id)}
            <div
              class="track-row"
              class:downloading={status === 'downloading'}
              class:complete={status === 'complete'}
              class:failed={status === 'failed'}
            >
              <div class="col-number">
                {#if status === 'downloading'}
                  <Loader2 size={14} class="spin" />
                {:else if status === 'complete'}
                  <Check size={14} class="status-complete" />
                {:else}
                  <span>{track.track_number}</span>
                {/if}
              </div>
              <div class="col-title">
                <span class="track-title">{track.title}</span>
                {#if track.performer.name !== album.artist.name}
                  <span class="track-performer">{track.performer.name}</span>
                {/if}
              </div>
              <div class="col-duration">
                {formatDuration(track.duration)}
              </div>
              <div class="col-quality">
                {#if track.maximum_bit_depth && track.maximum_sampling_rate}
                  {track.maximum_bit_depth}/{track.maximum_sampling_rate}
                {/if}
              </div>
              <div class="col-download">
                {#if status === 'complete'}
                  <span class="download-done"><Check size={14} /></span>
                {:else if status === 'downloading'}
                  <span class="download-active"><Loader2 size={14} class="spin" /></span>
                {:else if status === 'failed'}
                  <button
                    class="download-track-btn failed"
                    onclick={() => promptForFolder(track.id)}
                    title={$t('purchases.failed')}
                  >
                    <AlertTriangle size={14} />
                  </button>
                {:else if album.downloadable}
                  <button
                    class="download-track-btn"
                    onclick={() => promptForFolder(track.id)}
                    title={$t('purchases.downloadTrack')}
                  >
                    <Download size={14} />
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        {/each}
      </div>
    </div>
  {/if}
</div>
</ViewTransition>

<style>
  .purchase-album-detail {
    width: 100%;
    height: 100%;
    padding: 24px;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
  }

  .purchase-album-detail::-webkit-scrollbar {
    width: 6px;
  }

  .purchase-album-detail::-webkit-scrollbar-track {
    background: transparent;
  }

  .purchase-album-detail::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .purchase-album-detail::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  /* Back button (AlbumDetailView) */
  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    margin-bottom: 24px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-secondary);
  }

  /* Loading / Error */
  .loading-state,
  .error-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 64px;
    color: var(--text-muted);
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--bg-tertiary);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  /* Album Header (AlbumDetailView) */
  .album-header {
    display: flex;
    gap: 32px;
    margin-bottom: 32px;
  }

  .artwork {
    position: relative;
    flex-shrink: 0;
    width: 224px;
    height: 224px;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .artwork.unavailable {
    background: var(--bg-tertiary);
  }

  .artwork.unavailable img {
    display: none;
  }

  .artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artwork-unavailable-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
    padding: 16px;
  }

  .metadata {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .album-title {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .artist-link {
    font-size: 18px;
    font-weight: 500;
    color: var(--accent-primary);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    width: fit-content;
    margin-bottom: 8px;
  }

  .artist-link:hover {
    text-decoration: underline;
  }

  .artist-name {
    font-size: 18px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 8px;
  }

  .album-info {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .album-quality {
    margin-bottom: 4px;
  }

  .album-stats {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 24px;
  }

  /* Actions (AlbumDetailView pattern) */
  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  /* Progress section */
  .progress-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 14px;
    background: var(--bg-secondary);
    border-radius: 8px;
    border: 1px solid var(--bg-tertiary);
    margin-bottom: 16px;
  }

  .progress-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-muted);
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
    margin-bottom: 16px;
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
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
  }

  .add-to-library-btn:hover {
    opacity: 0.9;
  }

  .add-to-library-desc {
    font-size: 13px;
    color: var(--text-muted);
  }

  /* Divider (AlbumDetailView) */
  .divider {
    height: 1px;
    background-color: var(--bg-tertiary);
    margin: 32px 0;
  }

  /* Track List */
  .track-list {
    display: flex;
    flex-direction: column;
    width: 100%;
  }

  /* Table Header (AlbumDetailView) */
  .table-header {
    width: 100%;
    height: 40px;
    padding: 0 16px;
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    font-size: 12px;
    text-transform: uppercase;
    color: var(--text-muted);
    font-weight: 400;
    box-sizing: border-box;
  }

  .table-header .col-number {
    width: 48px;
    text-align: center;
  }

  .table-header .col-title {
    flex: 1;
    min-width: 0;
  }

  .table-header .col-duration {
    width: 80px;
    text-align: center;
  }

  .table-header .col-quality {
    width: 80px;
    text-align: center;
  }

  .table-header .col-icon {
    width: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    opacity: 0.5;
  }

  /* Tracks */
  .tracks {
    display: flex;
    flex-direction: column;
    width: 100%;
  }

  .disc-header {
    padding: 16px 16px 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  /* Track Row (AlbumDetailView column dimensions) */
  .track-row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 16px;
    height: 44px;
    border-radius: 6px;
    transition: background 150ms ease;
  }

  .track-row:hover {
    background: var(--bg-hover);
  }

  .track-row.downloading {
    background: var(--bg-active, var(--bg-hover));
  }

  .track-row.complete {
    opacity: 0.6;
  }

  .track-row .col-number {
    width: 48px;
    text-align: center;
    font-size: 14px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  :global(.status-complete) {
    color: var(--success, #4caf50);
  }

  .track-row .col-title {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .track-title {
    font-size: 14px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-performer {
    font-size: 12px;
    color: var(--text-muted);
  }

  .track-row .col-duration {
    width: 80px;
    text-align: center;
    font-size: 14px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  .track-row .col-quality {
    width: 80px;
    text-align: center;
    font-size: 12px;
    color: #666666;
  }

  .track-row .col-download {
    width: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .download-done {
    color: var(--success, #4caf50);
    display: flex;
    align-items: center;
  }

  .download-active {
    color: var(--accent-primary);
    display: flex;
    align-items: center;
  }

  .download-track-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
    opacity: 0;
  }

  .track-row:hover .download-track-btn {
    opacity: 1;
  }

  .download-track-btn:hover {
    color: var(--accent-primary);
  }

  .download-track-btn.failed {
    color: var(--error, #f44336);
    opacity: 1;
  }
</style>
