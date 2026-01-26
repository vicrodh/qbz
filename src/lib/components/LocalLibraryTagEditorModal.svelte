<script lang="ts">
  import Modal from './Modal.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ask } from '@tauri-apps/plugin-dialog';
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

  interface Props {
    isOpen: boolean;
    album: LocalAlbum | null;
    tracks: LocalTrack[];
    onClose: () => void;
    onSaved: () => Promise<void> | void;
  }

  let { isOpen, album, tracks, onClose, onSaved }: Props = $props();

  type PersistenceMode = 'sidecar' | 'direct';

  let albumTitle = $state('');
  let albumArtist = $state('');
  let yearInput = $state('');
  let genre = $state('');
  let catalogNumber = $state('');
  let persistence: PersistenceMode = $state('sidecar');
  let saving = $state(false);

  type TrackEdit = {
    id: number;
    filePath: string;
    cueStartSecs?: number;
    title: string;
    discNumber?: number;
    trackNumber?: number;
  };

  let trackEdits = $state<TrackEdit[]>([]);

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
    }
  });

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
        <p class="subtitle">
          Changes apply to LocalLibrary indexing and search. Qobuz catalog is not modified.
        </p>

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

        <div class="section">
          <h3>Tracklist</h3>
          <div class="track-table">
            <div class="track-head">
              <div class="col-num">#</div>
              <div class="col-title">Track title</div>
              <div class="col-track">Track</div>
              <div class="col-disc">Disc</div>
            </div>
            {@const totalDiscs = Math.max(1, ...trackEdits.map(t => t.discNumber ?? 1))}
            <div class="track-body">
              {#each trackEdits as t, i (t.id)}
                <div class="track-row">
                  <div class="col-num">{i + 1}</div>
                  <div class="col-title">
                    <input class="text control-xs" type="text" bind:value={t.title} />
                  </div>
                  <div class="col-track">
                    <input class="text control-xs num" type="number" min="1" step="1" bind:value={t.trackNumber} />
                  </div>
                  <div class="col-disc">
                    <div class="disc-of">
                      <input class="text control-xs num" type="number" min="1" step="1" bind:value={t.discNumber} />
                      <span class="disc-sep">of</span>
                      <input class="text control-xs num" type="number" value={totalDiscs} readonly tabindex="-1" />
                    </div>
                  </div>
                </div>
              {/each}
            </div>
          </div>
        </div>

      <div class="section">
        <h3>Persistence</h3>
        <div class="radio-group">
          <label class="radio">
            <input type="radio" name="persistence" value="sidecar" bind:group={persistence} />
            <span>Save as QBZ sidecar (default, does not modify audio files)</span>
          </label>
          <label class="radio">
            <input type="radio" name="persistence" value="direct" bind:group={persistence} />
            <span>Write changes to audio files (embedded tags)</span>
          </label>
        </div>
        {#if persistence === 'direct'}
          <div class="warning">
            SECURITY NOTE: This will modify files on disk. Ensure the album path is mounted read-write and that you have permissions.
          </div>
        {/if}
      </div>

      <div class="section">
        <h3>Reference (read-only)</h3>
        <div class="ref-grid">
          <div class="ref-item">
            <span class="ref-label">Album path</span>
            <span class="ref-value mono">{album?.directory_path ?? ''}</span>
          </div>
          <div class="ref-item">
            <span class="ref-label">Format</span>
            <span class="ref-value">{album ? `${album.format.toUpperCase()} ${album.bit_depth ?? 16}-bit / ${(album.sample_rate / 1000).toFixed(1)} kHz` : ''}</span>
          </div>
        </div>
      </div>
    </div>
  {/snippet}

  {#snippet footer()}
    <button class="secondary-btn" onclick={onClose} disabled={saving}>Cancel</button>
    <button class="primary-btn" onclick={handleSave} disabled={saving}>
      {saving ? 'Saving...' : 'Save changes'}
    </button>
  {/snippet}
</Modal>

  <style>
    .tag-editor {
      display: flex;
      flex-direction: column;
      gap: 18px;
    }

  .subtitle {
    margin: 0;
    color: var(--text-muted);
    font-size: 13px;
  }

    .grid {
      display: grid;
      gap: 12px;
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

  .section h3 {
    margin: 0 0 10px 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .track-table {
    --track-row-height: 44px;
    border: 1px solid var(--bg-tertiary);
    border-radius: 10px;
    overflow: hidden;
  }

  .track-head, .track-row {
    display: grid;
    grid-template-columns: 44px 1fr 90px 160px;
    gap: 10px;
    align-items: center;
    min-height: var(--track-row-height);
    padding: 0 12px;
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
    max-height: calc(var(--track-row-height) * 5);
    overflow-y: auto;
    scroll-snap-type: y mandatory;
    scrollbar-gutter: stable;
  }

    .track-body .track-row {
      scroll-snap-align: start;
      scroll-snap-stop: always;
    }

    .track-body .track-row:nth-child(even) {
      background: var(--bg-secondary);
    }

    .col-num {
      color: var(--text-muted);
      text-align: right;
      padding-right: 6px;
    }

    .col-track {
      width: 100%;
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

    .radio-group {
      display: flex;
      flex-direction: column;
      gap: 8px;
    }

  .radio {
    display: flex;
    gap: 10px;
    align-items: center;
    color: var(--text-primary);
    font-size: 13px;
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
    font-size: 13px;
    color: var(--text-primary);
  }

  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace;
    word-break: break-all;
  }
</style>
