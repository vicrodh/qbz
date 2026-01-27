<script lang="ts">
  import Modal from './Modal.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ask } from '@tauri-apps/plugin-dialog';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { showToast } from '$lib/stores/toastStore';

  interface LocalTrack {
    id: number;
    file_path: string;
    cue_start_secs?: number;
    cue_file_path?: string;
    title: string;
    track_number?: number;
    disc_number?: number;
    year?: number;
    genre?: string;
    catalog_number?: string;
  }

  interface LocalAlbum {
    id: string;
    title: string;
    artist: string;
    year?: number;
    genre?: string;
    catalog_number?: string;
    directory_path: string;
    format: string;
    bit_depth?: number;
    sample_rate: number;
  }

  // Remote metadata types
  interface RemoteAlbumSearchResult {
    provider: 'musicbrainz' | 'discogs';
    provider_id: string;
    title: string;
    artist: string;
    year?: number;
    track_count?: number;
    country?: string;
    label?: string;
    catalog_number?: string;
    confidence?: number;
    format?: string;
  }

  interface RemoteTrackMetadata {
    disc_number?: number;
    track_number?: number;
    title: string;
    duration_ms?: number;
  }

  interface RemoteAlbumMetadata {
    provider: 'musicbrainz' | 'discogs';
    provider_id: string;
    title: string;
    artist: string;
    year?: number;
    genres?: string[];
    label?: string;
    catalog_number?: string;
    country?: string;
    barcode?: string;
    tracks?: RemoteTrackMetadata[];
    disc_count?: number;
    source_url?: string;
  }

  interface Props {
    isOpen: boolean;
    album: LocalAlbum | null;
    tracks: LocalTrack[];
    onClose: () => void;
    onSaved: () => Promise<void> | void;
  }

  let { isOpen, album, tracks, onClose, onSaved }: Props = $props();

  type PersistenceMode = 'sidecar' | 'direct';
  type RemoteProvider = 'musicbrainz' | 'discogs';

  let albumTitle = $state('');
  let albumArtist = $state('');
  let yearInput = $state('');
  let genre = $state('');
  let catalogNumber = $state('');
  let albumTotalDiscs = $state(1);
  let persistence: PersistenceMode = $state('sidecar');
  let saving = $state(false);

  // Remote metadata search state
  let remoteProvider: RemoteProvider = $state('musicbrainz');
  let remoteSearching = $state(false);
  let remoteLoading = $state(false);
  let remoteResults = $state<RemoteAlbumSearchResult[]>([]);
  let selectedResult = $state<RemoteAlbumSearchResult | null>(null);
  let showRemotePanel = $state(false);
  let hasSearched = $state(false);

  type TrackEdit = {
    id: number;
    filePath: string;
    cueStartSecs?: number;
    title: string;
    discNumber?: number;
    trackNumber?: number;
  };

  let trackEdits = $state<TrackEdit[]>([]);
  const totalDiscs = $derived(Math.max(1, ...trackEdits.map(t => t.discNumber ?? 1)));

  function resetFromAlbum() {
    if (!album) return;
    albumTitle = album.title ?? '';
    albumArtist = album.artist ?? '';
    const firstWithYear = tracks.find(t => typeof t.year === 'number')?.year;
    yearInput = (album.year ?? firstWithYear) ? String(album.year ?? firstWithYear) : '';

    const firstGenre = tracks.find(t => (t.genre ?? '').trim())?.genre;
    genre = (album.genre ?? firstGenre ?? '').toString();

    const firstCatalog = tracks.find(t => (t.catalog_number ?? '').trim())?.catalog_number;
    catalogNumber = (album.catalog_number ?? firstCatalog ?? '').toString();
    albumTotalDiscs = totalDiscs;
    persistence = 'sidecar';

    trackEdits = tracks.map(t => ({
      id: t.id,
      filePath: t.file_path,
      cueStartSecs: t.cue_start_secs,
      title: t.title ?? '',
      discNumber: t.disc_number,
      trackNumber: t.track_number
    }));
  }

  $effect(() => {
    if (isOpen) {
      resetFromAlbum();
      // Reset remote state when modal opens
      remoteResults = [];
      selectedResult = null;
      showRemotePanel = false;
      hasSearched = false;
    }
  });

  // Remote metadata functions
  async function searchRemoteMetadata() {
    if (!albumTitle.trim() && !albumArtist.trim()) {
      showToast('Enter album title or artist to search', 'error');
      return;
    }

    remoteSearching = true;
    remoteResults = [];
    selectedResult = null;

    try {
      const results = await invoke<RemoteAlbumSearchResult[]>('remote_metadata_search', {
        provider: remoteProvider,
        query: albumTitle.trim(),
        artist: albumArtist.trim() || null,
        limit: 10
      });
      remoteResults = results;
      hasSearched = true;
      showRemotePanel = results.length > 0;
    } catch (err) {
      console.error('Remote search failed:', err);
      showToast(`Search failed: ${err}`, 'error');
      hasSearched = true;
    } finally {
      remoteSearching = false;
    }
  }

  async function applyRemoteMetadata() {
    if (!selectedResult) return;

    remoteLoading = true;
    try {
      const metadata = await invoke<RemoteAlbumMetadata>('remote_metadata_get_album', {
        provider: selectedResult.provider,
        providerId: selectedResult.provider_id
      });

      // Apply album-level metadata
      if (metadata.title) albumTitle = metadata.title;
      if (metadata.artist) albumArtist = metadata.artist;
      if (metadata.year) yearInput = String(metadata.year);
      if (metadata.genres && metadata.genres.length > 0) {
        genre = metadata.genres.slice(0, 3).join(', ');
      }
      if (metadata.catalog_number) catalogNumber = metadata.catalog_number;
      if (metadata.disc_count) albumTotalDiscs = metadata.disc_count;

      // Check for track count mismatch
      const remoteTrackCount = metadata.tracks?.length ?? 0;
      const localTrackCount = trackEdits.length;
      const hasMismatch = remoteTrackCount > 0 && remoteTrackCount !== localTrackCount;

      // Apply track-level metadata if available
      if (metadata.tracks && metadata.tracks.length > 0) {
        // Match tracks by position
        const remoteTracks = metadata.tracks;
        for (let i = 0; i < trackEdits.length && i < remoteTracks.length; i++) {
          const remote = remoteTracks[i];
          if (remote.title) trackEdits[i].title = remote.title;
          if (remote.track_number !== undefined) trackEdits[i].trackNumber = remote.track_number;
          if (remote.disc_number !== undefined) trackEdits[i].discNumber = remote.disc_number;
        }
      }

      const providerName = selectedResult?.provider === 'musicbrainz' ? 'MusicBrainz' : 'Discogs';
      if (hasMismatch) {
        showToast(
          `Applied from ${providerName}. Track count differs: local=${localTrackCount}, remote=${remoteTrackCount}`,
          'warning'
        );
      } else {
        showToast(`Applied metadata from ${providerName}`, 'success');
      }
      showRemotePanel = false;
    } catch (err) {
      console.error('Failed to fetch metadata:', err);
      // Check for rate limiting
      const errStr = String(err);
      if (errStr.includes('429') || errStr.toLowerCase().includes('rate')) {
        showToast('Rate limited. Please wait a moment and try again.', 'warning');
      } else {
        showToast(`Failed to fetch metadata: ${err}`, 'error');
      }
    } finally {
      remoteLoading = false;
    }
  }

  function getSourceUrl(result: RemoteAlbumSearchResult): string {
    if (result.provider === 'musicbrainz') {
      return `https://musicbrainz.org/release/${result.provider_id}`;
    } else {
      return `https://www.discogs.com/release/${result.provider_id}`;
    }
  }

  async function openInBrowser() {
    if (!selectedResult) return;
    const url = getSourceUrl(selectedResult);
    try {
      await openUrl(url);
    } catch (err) {
      console.error('Failed to open URL:', err);
      showToast('Failed to open browser', 'error');
    }
  }

    function parseYear(): number | null {
      const trimmed = yearInput.trim();
      if (!trimmed) return null;
      const num = Number(trimmed);
      if (!Number.isFinite(num)) return null;
      if (!Number.isInteger(num)) return null;
      const year = num;
      if (year < 0 || year > 3000) return null;
      return year;
    }

  function buildPayload() {
    if (!album) return null;
    const year = parseYear();
    if (yearInput.trim() && year === null) {
      throw new Error('Year must be a number (e.g. 1999).');
    }

    return {
      albumGroupKey: album.id,
      albumTitle: albumTitle.trim(),
      albumArtist: albumArtist.trim(),
      year,
      genre: genre.trim() ? genre.trim() : null,
      catalogNumber: catalogNumber.trim() ? catalogNumber.trim() : null,
      tracks: trackEdits.map(t => ({
        id: t.id,
        filePath: t.filePath,
        cueStartSecs: t.cueStartSecs ?? null,
        title: t.title.trim(),
        discNumber: t.discNumber ?? null,
        trackNumber: t.trackNumber ?? null
      }))
    };
  }

  async function confirmDirectWriteOnce(): Promise<boolean> {
    const key = 'qbz.localLibraryTagEditor.directWriteAcknowledged';
    const already = localStorage.getItem(key) === '1';
    if (already) return true;

    const confirmed = await ask(
      'This will modify audio files on disk. QBZ cannot undo changes once written. Ensure the album path is mounted read-write and you have permissions.',
      {
        title: 'Write tags to audio files?',
        kind: 'warning',
        okLabel: 'Write',
        cancelLabel: 'Cancel'
      }
    );
    if (!confirmed) return false;
    localStorage.setItem(key, '1');
    return true;
  }

  async function handleSave() {
    if (!album) return;
    if (!albumTitle.trim()) {
      alert('Album title is required.');
      return;
    }
    if (trackEdits.some(t => !t.title.trim())) {
      alert('Track titles cannot be empty.');
      return;
    }

    if (persistence === 'direct') {
      const anyCue = tracks.some(t => !!t.cue_file_path || typeof t.cue_start_secs === 'number');
      if (anyCue) {
        alert('Writing tags to files is not supported for CUE-based albums. Use sidecar mode.');
        return;
      }

      const ok = await confirmDirectWriteOnce();
      if (!ok) return;
    }

    let payload;
    try {
      payload = buildPayload();
    } catch (err) {
      alert(String(err));
      return;
    }
    if (!payload) return;

    saving = true;
    try {
      if (persistence === 'sidecar') {
        await invoke('library_update_album_metadata', { request: payload });
      } else {
        await invoke('library_write_album_metadata_to_files', { request: payload });
      }
      showToast('Album metadata saved', 'success');
      await onSaved();
      onClose();
    } catch (err) {
      alert(`Failed to save metadata: ${err}`);
    } finally {
      saving = false;
    }
  }
</script>

  <Modal
    isOpen={isOpen}
    onClose={onClose}
    title="Edit metadata"
    maxWidth="820px"
  >
    {#snippet children()}
      <div class="tag-editor">
        <div class="grid grid-2">
          <div class="field">
            <label>Album name</label>
            <input class="text control-sm" type="text" bind:value={albumTitle} />
          </div>
          <div class="field">
            <label>Album artist</label>
            <input class="text control-sm" type="text" bind:value={albumArtist} />
          </div>
        </div>

        <div class="grid grid-3">
          <div class="field">
            <label>Year</label>
            <input
              class="text control-sm"
              type="number"
              step="1"
              inputmode="numeric"
              bind:value={yearInput}
              placeholder="e.g. 1999"
            />
          </div>
          <div class="field">
            <label>Genre</label>
            <input class="text control-sm" type="text" bind:value={genre} placeholder="e.g. ROCK, POP, etc" />
          </div>
          <div class="field">
            <label>Catalog / Release ID</label>
            <input class="text control-sm" type="text" bind:value={catalogNumber} />
          </div>
        </div>

        <!-- Remote Metadata Lookup -->
        <div class="remote-section">
          <div class="remote-header">
            <select
              class="select-inline control-xs"
              bind:value={remoteProvider}
            >
              <option value="musicbrainz">MusicBrainz</option>
              <option value="discogs">Discogs</option>
            </select>
            <button
              class="btn btn-secondary btn-sm"
              onclick={searchRemoteMetadata}
              disabled={remoteSearching}
              type="button"
            >
              {#if remoteSearching}
                <span class="spinner-inline"></span>
                Searching...
              {:else}
                <svg class="icon-inline" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                  <circle cx="11" cy="11" r="8"/>
                  <path d="m21 21-4.3-4.3"/>
                </svg>
                Search
              {/if}
            </button>
            {#if hasSearched}
              {#if remoteResults.length > 0}
                <button
                  class="btn btn-ghost btn-sm result-status"
                  onclick={() => showRemotePanel = !showRemotePanel}
                  type="button"
                >
                  <span class="result-text">{remoteResults.length} results</span>
                  <svg class="icon-inline chevron" class:rotated={showRemotePanel} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <polyline points="6 9 12 15 18 9"/>
                  </svg>
                </button>
              {:else}
                <span class="no-results">No results, try different search</span>
              {/if}
            {/if}
          </div>

          {#if showRemotePanel && remoteResults.length > 0}
            <div class="remote-panel">

              <div class="remote-results">
                {#each remoteResults as result (result.provider_id)}
                  <button
                    class="remote-result"
                    class:selected={selectedResult?.provider_id === result.provider_id}
                    onclick={() => selectedResult = result}
                    type="button"
                  >
                    <span class="result-title">{result.title}</span>
                    <span class="result-artist">{result.artist}</span>
                    <div class="result-details">
                      {#if result.year}<span class="detail">{result.year}</span>{/if}
                      {#if result.track_count}<span class="detail">{result.track_count} tracks</span>{/if}
                      {#if result.country}<span class="detail">{result.country}</span>{/if}
                      {#if result.format}<span class="detail">{result.format}</span>{/if}
                    </div>
                    {#if result.label || result.catalog_number}
                      <div class="result-label">
                        {#if result.label}<span>{result.label}</span>{/if}
                        {#if result.catalog_number}<span class="mono">{result.catalog_number}</span>{/if}
                      </div>
                    {/if}
                  </button>
                {/each}
              </div>

              <div class="remote-actions">
                <button
                  class="btn btn-ghost btn-xs"
                  onclick={openInBrowser}
                  disabled={!selectedResult}
                  type="button"
                  title="Open in browser"
                >
                  <svg class="icon-inline" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>
                    <polyline points="15 3 21 3 21 9"/>
                    <line x1="10" y1="14" x2="21" y2="3"/>
                  </svg>
                </button>
                <button
                  class="btn btn-primary btn-sm"
                  onclick={applyRemoteMetadata}
                  disabled={!selectedResult || remoteLoading}
                  type="button"
                >
                  {#if remoteLoading}
                    <span class="spinner-inline"></span>
                  {:else}
                    Apply
                  {/if}
                </button>
              </div>
            </div>
          {/if}
        </div>

        <div class="section">
          <div class="track-table">
            <div class="track-head">
              <div class="cell cell-head">Track</div>
              <div class="cell cell-head">Track title</div>
              <div class="cell cell-head">Disc</div>
            </div>
            <div class="track-body">
              {#each trackEdits as t, i (t.id)}
                <div class="track-row">
                  <div class="cell">
                    <input class="table-input control-xs num" type="number" min="1" step="1" bind:value={t.trackNumber} />
                  </div>
                  <div class="cell">
                    <input class="table-input control-xs" type="text" bind:value={t.title} />
                  </div>
                  <div class="cell">
                    <div class="disc-of">
                      <input class="table-input control-xs num" type="number" min="1" step="1" bind:value={t.discNumber} />
                      <span class="disc-sep">of</span>
                      <input
                        class="table-input control-xs num"
                        type="number"
                        min="1"
                        step="1"
                        bind:value={albumTotalDiscs}
                      />
                    </div>
                  </div>
                </div>
              {/each}
            </div>
          </div>
        </div>

        <div class="ref-inline">
          <span class="ref-label">Album path</span>
          <span class="ref-value mono">{album?.directory_path ?? ''}</span>
        </div>
      </div>
    {/snippet}

  {#snippet footer()}
    <div class="footer-row">
      <div class="footer-left">
        <label class="footer-label" for="persistence-select">Persistence</label>
        <select
          id="persistence-select"
          class="select-inline control-xs"
          bind:value={persistence}
        >
          <option value="sidecar">QBZ sidecar (does not modify files)</option>
          <option value="direct">Write to audio files (embedded tags)</option>
        </select>
        {#if persistence === 'direct'}
          <span class="warning-inline">Writes to files on disk.</span>
        {/if}
      </div>
      <div class="footer-actions">
        <button class="btn btn-secondary" onclick={onClose} disabled={saving}>Cancel</button>
        <button class="btn btn-primary" onclick={handleSave} disabled={saving}>
          {#if saving}
            <span class="spinner-inline"></span>
            {persistence === 'direct' ? 'Writing tags...' : 'Saving...'}
          {:else}
            Save
          {/if}
        </button>
      </div>
    </div>
  {/snippet}
</Modal>

<style>
  .tag-editor {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  :global(.btn-xs) {
    padding: 4px 8px !important;
    font-size: 11px !important;
  }

  .grid {
    display: grid;
    gap: 10px;
  }

    .grid-2 {
      grid-template-columns: 1fr 1fr;
    }

  .grid-3 {
    grid-template-columns: 1fr 1fr 1fr;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .text {
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 10px 12px;
    color: var(--text-primary);
    font-size: 14px;
  }

    /* Bootstrap-ish control sizing (relative step down). */
    .control-sm {
      padding: 6px 10px;
      font-size: 13px;
      border-radius: 6px;
    }

    .control-xs {
      padding: 4px 8px;
      font-size: 12px;
      border-radius: 6px;
    }

    .num {
      text-align: center;
    }

.text:focus {
  outline: none;
  border-color: var(--accent-primary);
}

.ref-inline {
  display: flex;
  gap: 8px;
  align-items: baseline;
  color: var(--text-primary);
  padding: 0 2px;
}

.track-table {
  --track-row-height: 36px;
  border: 1px solid var(--bg-tertiary);
  border-radius: 10px;
  overflow: hidden;
  display: grid;
  grid-template-rows: auto 1fr;
}

.track-head,
.track-row {
  display: grid;
  grid-template-columns: 90px 1fr 180px;
  align-items: stretch;
  min-height: var(--track-row-height);
}

  .track-head {
    background: var(--bg-tertiary);
    color: var(--text-muted);
    font-size: 12px;
  }

  .track-row {
    background: var(--bg-primary);
    border-top: 1px solid var(--bg-tertiary);
  }

  .track-body {
    max-height: calc(var(--track-row-height) * 6);
    overflow-y: auto;
    scroll-snap-type: y mandatory;
    scrollbar-gutter: stable;
    overscroll-behavior: contain;
    padding: 0;
  }

  .track-body .track-row {
    scroll-snap-align: start;
    scroll-snap-stop: always;
  }

  .track-body .track-row:nth-child(even) {
    background: var(--bg-secondary);
  }

  .cell {
    border-right: 1px solid var(--bg-tertiary);
    padding: 6px 10px;
    display: flex;
    align-items: center;
  }

  .cell:last-child {
    border-right: none;
  }

  .cell-head {
    font-weight: 600;
    color: var(--text-muted);
  }

  .disc-of {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    gap: 8px;
    align-items: center;
  }

  .disc-sep {
    font-size: 12px;
    color: var(--text-muted);
  }

  .table-input {
    width: 100%;
    background: transparent;
    border: none;
    border-radius: 0;
    color: var(--text-primary);
    padding: 2px 0;
    height: auto;
    border-bottom: 1px solid transparent;
  }

  .table-input:focus {
    outline: none;
    border-bottom: 2px solid var(--accent-primary);
    margin-bottom: -1px;
  }

.track-row:focus-within,
.cell:focus-within {
  background: var(--bg-hover);
}

/* Hide number spinners, keep keyboard support */
input[type="number"]::-webkit-outer-spin-button,
input[type="number"]::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

input[type="number"] {
  -moz-appearance: textfield;
}

.ref-inline {
  display: flex;
  align-items: baseline;
  gap: 8px;
  color: var(--text-primary);
  padding: 4px 2px 0;
}

.ref-inline {
  display: flex;
  align-items: baseline;
  gap: 8px;
  color: var(--text-primary);
  padding: 0 2px;
}

.footer-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  width: 100%;
}

.footer-left {
  display: flex;
  align-items: center;
  gap: 10px;
}

.footer-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-left: auto;
}

.footer-actions :global(.btn) {
  min-width: 96px;
}

.select-inline {
  appearance: none;
  background: var(--bg-secondary);
  border: 1px solid var(--bg-tertiary);
  border-radius: 6px;
  padding: 6px 28px 6px 10px;
  font-size: 12px;
  color: var(--text-primary);
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23888888' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
  cursor: pointer;
}

.select-inline:focus {
  outline: none;
  border-color: var(--accent-primary);
}

  .warning {
    margin-top: 10px;
    background: rgba(245, 158, 11, 0.12);
    border: 1px solid rgba(245, 158, 11, 0.25);
    color: var(--text-primary);
    padding: 10px 12px;
    border-radius: 10px;
    font-size: 12px;
    line-height: 1.4;
  }

  .ref-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }

  .ref-item {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
    border: 1px solid var(--bg-tertiary);
    border-radius: 10px;
    background: var(--bg-secondary);
  }

  .ref-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .ref-value {
    font-size: 11px;
    color: var(--text-muted);
  }

  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;
    word-break: break-all;
  }

  /* Remote metadata panel styles */
  .remote-section {
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    overflow: hidden;
  }

  .remote-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    background: var(--bg-secondary);
  }

  .result-status {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px !important;
  }

  .result-status .chevron {
    transition: transform 0.2s ease;
  }

  .result-status .chevron.rotated {
    transform: rotate(180deg);
  }

  .result-text {
    font-size: 12px;
    color: var(--accent-primary);
  }

  .no-results {
    margin-left: auto;
    font-size: 11px;
    color: var(--text-muted);
    font-style: italic;
  }

  .remote-panel {
    border-top: 1px solid var(--bg-tertiary);
    padding: 8px;
    background: var(--bg-primary);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .remote-results {
    display: flex;
    gap: 6px;
    overflow-x: auto;
    overflow-y: hidden;
    scroll-snap-type: x mandatory;
    padding: 4px;
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
  }

  .remote-result {
    flex: 0 0 calc(50% - 3px);
    min-width: 200px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 10px;
    background: var(--bg-secondary);
    border: 1px solid transparent;
    border-radius: 6px;
    text-align: left;
    cursor: pointer;
    transition: background 0.15s ease, border-color 0.15s ease;
    scroll-snap-align: start;
  }

  .remote-result:hover {
    background: var(--bg-hover);
  }

  .remote-result.selected {
    background: rgba(var(--accent-primary-rgb), 0.1);
    border-color: var(--accent-primary);
  }

  .result-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.3;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .result-artist {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .result-details {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 2px;
  }

  .result-details .detail {
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: 3px;
  }

  .result-label {
    display: flex;
    gap: 6px;
    font-size: 10px;
    color: var(--text-muted);
    opacity: 0.8;
    margin-top: auto;
    padding-top: 4px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .result-label .mono {
    opacity: 0.7;
  }

  .remote-actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 6px;
  }

  .spinner-inline {
    width: 14px;
    height: 14px;
    border: 2px solid transparent;
    border-top-color: currentColor;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    display: inline-block;
    margin-right: 6px;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .icon-inline {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
  }
</style>
