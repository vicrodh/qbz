<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import {
    HardDrive, Music, Disc3, Mic2, FolderPlus, Trash2, RefreshCw,
    Settings, X, Play, Plus, AlertCircle, Clock, ImageDown
  } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';

  // Backend types matching Rust models
  interface LocalTrack {
    id: number;
    file_path: string;
    title: string;
    artist: string;
    album: string;
    album_artist?: string;
    track_number?: number;
    disc_number?: number;
    year?: number;
    genre?: string;
    duration_secs: number;
    format: string;
    bit_depth?: number;
    sample_rate: number;
    channels: number;
    file_size_bytes: number;
    cue_file_path?: string;
    cue_start_secs?: number;
    cue_end_secs?: number;
    artwork_path?: string;
    last_modified: number;
    indexed_at: number;
  }

  interface LocalAlbum {
    id: string;
    title: string;
    artist: string;
    year?: number;
    artwork_path?: string;
    track_count: number;
    total_duration_secs: number;
    format: string;
    bit_depth?: number;
    sample_rate: number;
    directory_path: string;
  }

  interface LocalArtist {
    name: string;
    album_count: number;
    track_count: number;
  }

  interface LibraryStats {
    track_count: number;
    album_count: number;
    artist_count: number;
    total_duration_secs: number;
    total_size_bytes: number;
  }

  interface ScanProgress {
    status: 'Idle' | 'Scanning' | 'Complete' | 'Error';
    total_files: number;
    processed_files: number;
    current_file?: string;
    errors: { file_path: string; error: string }[];
  }

  interface Props {
    onAlbumClick?: (album: LocalAlbum) => void;
    onTrackPlay?: (track: LocalTrack) => void;
    onTrackPlayNext?: (track: LocalTrack) => void;
    onTrackPlayLater?: (track: LocalTrack) => void;
    onSetLocalQueue?: (trackIds: number[]) => void;
  }

  let {
    onAlbumClick,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onSetLocalQueue
  }: Props = $props();

  // View state
  type TabType = 'albums' | 'artists' | 'tracks';
  let activeTab = $state<TabType>('albums');
  let showSettings = $state(false);

  // Data state
  let albums = $state<LocalAlbum[]>([]);
  let artists = $state<LocalArtist[]>([]);
  let tracks = $state<LocalTrack[]>([]);
  let stats = $state<LibraryStats | null>(null);
  let folders = $state<string[]>([]);
  let scanProgress = $state<ScanProgress | null>(null);

  // Loading state
  let loading = $state(false);
  let scanning = $state(false);
  let error = $state<string | null>(null);
  let fetchingArtwork = $state(false);
  let hasDiscogsCredentials = $state(false);

  // Album detail state (for viewing album tracks)
  let selectedAlbum = $state<LocalAlbum | null>(null);
  let albumTracks = $state<LocalTrack[]>([]);

  onMount(() => {
    loadLibraryData();
    loadFolders();
    checkDiscogsCredentials();
  });

  async function checkDiscogsCredentials() {
    try {
      hasDiscogsCredentials = await invoke<boolean>('discogs_has_credentials');
    } catch {
      hasDiscogsCredentials = false;
    }
  }

  async function loadLibraryData() {
    loading = true;
    error = null;
    try {
      const [albumsResult, statsResult] = await Promise.all([
        invoke<LocalAlbum[]>('library_get_albums', { limit: 100, offset: 0 }),
        invoke<LibraryStats>('library_get_stats')
      ]);
      albums = albumsResult;
      stats = statsResult;
    } catch (err) {
      console.error('Failed to load library:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function loadFolders() {
    try {
      folders = await invoke<string[]>('library_get_folders');
    } catch (err) {
      console.error('Failed to load folders:', err);
    }
  }

  async function loadArtists() {
    loading = true;
    try {
      artists = await invoke<LocalArtist[]>('library_get_artists');
    } catch (err) {
      console.error('Failed to load artists:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function loadTracks() {
    loading = true;
    try {
      tracks = await invoke<LocalTrack[]>('library_search', { query: '', limit: 500 });
    } catch (err) {
      console.error('Failed to load tracks:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  function handleTabChange(tab: TabType) {
    activeTab = tab;
    selectedAlbum = null;

    if (tab === 'artists' && artists.length === 0) {
      loadArtists();
    } else if (tab === 'tracks' && tracks.length === 0) {
      loadTracks();
    }
  }

  async function handleAddFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select Music Folder'
      });

      if (!selected || typeof selected !== 'string') return;

      await invoke('library_add_folder', { path: selected });
      await loadFolders();
    } catch (err) {
      console.error('Failed to add folder:', err);
    }
  }

  async function handleRemoveFolder(path: string) {
    if (!confirm(`Remove "${path}" from library? This will remove all indexed tracks from this folder.`)) {
      return;
    }

    try {
      await invoke('library_remove_folder', { path });
      await loadFolders();
      await loadLibraryData();
    } catch (err) {
      console.error('Failed to remove folder:', err);
      alert(`Failed to remove folder: ${err}`);
    }
  }

  async function handleScan() {
    if (folders.length === 0) {
      alert('Please add at least one folder to scan.');
      return;
    }

    scanning = true;
    scanProgress = {
      status: 'Scanning',
      total_files: 0,
      processed_files: 0,
      current_file: undefined,
      errors: []
    };

    // Start polling for progress
    const pollInterval = setInterval(async () => {
      try {
        scanProgress = await invoke<ScanProgress>('library_get_scan_progress');
        if (scanProgress.status === 'Complete' || scanProgress.status === 'Error') {
          clearInterval(pollInterval);
          scanning = false;
          await loadLibraryData();
          if (activeTab === 'artists') await loadArtists();
          if (activeTab === 'tracks') await loadTracks();
        }
      } catch (err) {
        console.error('Failed to get scan progress:', err);
      }
    }, 500);

    try {
      await invoke('library_scan');
    } catch (err) {
      console.error('Scan failed:', err);
      scanning = false;
      clearInterval(pollInterval);
    }
  }

  async function handleClearLibrary() {
    if (!confirm('Clear entire library? This will remove all indexed tracks. Your files will not be deleted.')) {
      return;
    }

    try {
      await invoke('library_clear');
      await loadLibraryData();
      albums = [];
      artists = [];
      tracks = [];
    } catch (err) {
      console.error('Failed to clear library:', err);
    }
  }

  async function handleFetchMissingArtwork() {
    if (!hasDiscogsCredentials) {
      alert('Discogs credentials not configured. Please set up DISCOGS_API_CLIENT_KEY and DISCOGS_API_CLIENT_SECRET.');
      return;
    }

    fetchingArtwork = true;
    try {
      const count = await invoke<number>('library_fetch_missing_artwork');
      if (count > 0) {
        alert(`Fetched artwork for ${count} albums from Discogs.`);
        await loadLibraryData();
      } else {
        alert('No albums needed artwork updates.');
      }
    } catch (err) {
      console.error('Failed to fetch artwork:', err);
      alert(`Failed to fetch artwork: ${err}`);
    } finally {
      fetchingArtwork = false;
    }
  }

  async function handleAlbumClick(album: LocalAlbum) {
    selectedAlbum = album;
    try {
      albumTracks = await invoke<LocalTrack[]>('library_get_album_tracks', {
        album: album.title,
        artist: album.artist
      });
    } catch (err) {
      console.error('Failed to load album tracks:', err);
    }
  }

  async function handleTrackPlay(track: LocalTrack) {
    try {
      await invoke('library_play_track', { trackId: track.id });
      onTrackPlay?.(track);
    } catch (err) {
      console.error('Failed to play track:', err);
      alert(`Failed to play: ${err}`);
    }
  }

  async function handlePlayAllAlbum() {
    if (!selectedAlbum || albumTracks.length === 0) return;

    // Build queue from album tracks
    const queueTracks = albumTracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.artist,
      album: t.album,
      duration_secs: t.duration_secs,
      artwork_url: t.artwork_path ? `file://${t.artwork_path}` : null
    }));

    try {
      await invoke('set_queue', { tracks: queueTracks, startIndex: 0 });
      // Register these as local tracks for proper queue playback
      onSetLocalQueue?.(albumTracks.map(t => t.id));
      await handleTrackPlay(albumTracks[0]);
    } catch (err) {
      console.error('Failed to play album:', err);
    }
  }

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function formatTotalDuration(seconds: number): string {
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    if (hours > 0) {
      return `${hours}h ${mins}m`;
    }
    return `${mins} min`;
  }

  function formatFileSize(bytes: number): string {
    if (bytes >= 1073741824) {
      return `${(bytes / 1073741824).toFixed(1)} GB`;
    }
    if (bytes >= 1048576) {
      return `${(bytes / 1048576).toFixed(1)} MB`;
    }
    return `${(bytes / 1024).toFixed(1)} KB`;
  }

  function getQualityBadge(track: LocalTrack): string {
    const format = track.format.toUpperCase();
    const bitDepth = track.bit_depth ?? 16;
    const sampleRate = track.sample_rate / 1000; // Convert to kHz

    // Format: "FLAC 24/96" style that audiophiles love
    return `${format} ${bitDepth}/${sampleRate}`;
  }

  function isHiRes(track: LocalTrack): boolean {
    return (track.bit_depth ?? 16) >= 24 || track.sample_rate > 48000;
  }

  function formatSampleRate(hz: number): string {
    return `${(hz / 1000).toFixed(1)} kHz`;
  }

  function formatBitDepth(bits?: number): string {
    return bits ? `${bits}-bit` : '16-bit';
  }

  function getAlbumQualityBadge(album: LocalAlbum): string {
    const format = album.format.toUpperCase();
    const bitDepth = album.bit_depth ?? 16;
    const sampleRate = album.sample_rate / 1000;
    return `${format} ${bitDepth}/${sampleRate}`;
  }

  function isAlbumHiRes(album: LocalAlbum): boolean {
    return (album.bit_depth ?? 16) >= 24 || album.sample_rate > 48000;
  }

  function getArtworkUrl(path?: string): string {
    if (!path) return '';
    // Use asset:// protocol for Tauri 2.0 to load local files in WebView
    return `asset://localhost/${encodeURIComponent(path)}`;
  }
</script>

<div class="library-view">
  {#if selectedAlbum}
    <!-- Album Detail View -->
    <div class="album-detail">
      <button class="back-btn" onclick={() => (selectedAlbum = null)}>
        <X size={20} />
        <span>Back to Library</span>
      </button>

      <div class="album-header">
        <div class="album-artwork">
          {#if selectedAlbum.artwork_path}
            <img src={getArtworkUrl(selectedAlbum.artwork_path)} alt={selectedAlbum.title} />
          {:else}
            <div class="artwork-placeholder">
              <Disc3 size={64} />
            </div>
          {/if}
        </div>
        <div class="album-info">
          <h1>{selectedAlbum.title}</h1>
          <p class="artist">{selectedAlbum.artist}</p>
          <p class="meta">
            {#if selectedAlbum.year}{selectedAlbum.year} &bull; {/if}
            {selectedAlbum.track_count} tracks &bull;
            {formatTotalDuration(selectedAlbum.total_duration_secs)}
          </p>
          {#if albumTracks.length > 0}
            {@const firstTrack = albumTracks[0]}
            <div class="audio-specs">
              <span class="spec-badge" class:hires={isHiRes(firstTrack)}>
                {firstTrack.format.toUpperCase()}
              </span>
              <span class="spec-item">{formatBitDepth(firstTrack.bit_depth)}</span>
              <span class="spec-item">{formatSampleRate(firstTrack.sample_rate)}</span>
              <span class="spec-item">{firstTrack.channels === 2 ? 'Stereo' : firstTrack.channels === 1 ? 'Mono' : `${firstTrack.channels}ch`}</span>
            </div>
          {/if}
          <div class="album-actions">
            <button class="play-btn" onclick={handlePlayAllAlbum}>
              <Play size={16} fill="white" />
              <span>Play All</span>
            </button>
          </div>
        </div>
      </div>

      <div class="track-list">
        {#each albumTracks as track, index (track.id)}
          <TrackRow
            number={track.track_number ?? index + 1}
            title={track.title}
            artist={track.artist !== selectedAlbum?.artist ? track.artist : undefined}
            duration={formatDuration(track.duration_secs)}
            quality={getQualityBadge(track)}
            hideDownload={true}
            onPlay={() => handleTrackPlay(track)}
            menuActions={{
              onPlayNow: () => handleTrackPlay(track),
              onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
              onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined
            }}
          />
        {/each}
      </div>
    </div>
  {:else}
    <!-- Main Library View -->
    <div class="header">
      <div class="header-icon">
        <HardDrive size={32} />
      </div>
      <div class="header-content">
        <h1>Local Library</h1>
        {#if stats}
          <p class="subtitle">
            {stats.album_count} albums &bull; {stats.track_count} tracks &bull;
            {formatTotalDuration(stats.total_duration_secs)} &bull;
            {formatFileSize(stats.total_size_bytes)}
          </p>
        {:else}
          <p class="subtitle">Your local music collection</p>
        {/if}
      </div>
      <div class="header-actions">
        <button class="icon-btn" onclick={handleScan} disabled={scanning} title="Scan library">
          <RefreshCw size={20} class={scanning ? 'spinning' : ''} />
        </button>
        <button class="icon-btn" onclick={() => (showSettings = !showSettings)} title="Library settings">
          <Settings size={20} />
        </button>
      </div>
    </div>

    <!-- Scan Progress -->
    {#if scanning && scanProgress}
      <div class="scan-progress">
        <div class="progress-bar">
          <div
            class="progress-fill"
            style="width: {scanProgress.total_files > 0
              ? (scanProgress.processed_files / scanProgress.total_files) * 100
              : 0}%"
          ></div>
        </div>
        <div class="progress-text">
          <span>Scanning: {scanProgress.processed_files} / {scanProgress.total_files} files</span>
          {#if scanProgress.current_file}
            <span class="current-file">{scanProgress.current_file.split('/').pop()}</span>
          {/if}
        </div>
      </div>
    {/if}

    <!-- Settings Panel -->
    {#if showSettings}
      <div class="settings-panel">
        <div class="settings-header">
          <h3>Library Folders</h3>
          <button class="add-folder-btn" onclick={handleAddFolder}>
            <FolderPlus size={16} />
            <span>Add Folder</span>
          </button>
        </div>

        {#if folders.length === 0}
          <div class="no-folders">
            <p>No folders added yet. Add a folder to start building your library.</p>
          </div>
        {:else}
          <div class="folder-list">
            {#each folders as folder}
              <div class="folder-item">
                <span class="folder-path">{folder}</span>
                <button class="remove-btn" onclick={() => handleRemoveFolder(folder)} title="Remove folder">
                  <Trash2 size={14} />
                </button>
              </div>
            {/each}
          </div>
        {/if}

        <div class="settings-actions">
          {#if hasDiscogsCredentials}
            <button
              class="secondary-btn"
              onclick={handleFetchMissingArtwork}
              disabled={fetchingArtwork}
            >
              <ImageDown size={14} class={fetchingArtwork ? 'spinning' : ''} />
              <span>{fetchingArtwork ? 'Fetching...' : 'Fetch Missing Artwork'}</span>
            </button>
          {:else}
            <div class="discogs-hint">
              <span>Configure Discogs API for automatic artwork fetching</span>
            </div>
          {/if}
          <button class="danger-btn" onclick={handleClearLibrary}>
            <Trash2 size={14} />
            <span>Clear Library</span>
          </button>
        </div>
      </div>
    {/if}

    <!-- Tabs -->
    <div class="tabs">
      <button
        class="tab"
        class:active={activeTab === 'albums'}
        onclick={() => handleTabChange('albums')}
      >
        <Disc3 size={16} />
        <span>Albums</span>
        {#if stats}<span class="count">({stats.album_count})</span>{/if}
      </button>
      <button
        class="tab"
        class:active={activeTab === 'artists'}
        onclick={() => handleTabChange('artists')}
      >
        <Mic2 size={16} />
        <span>Artists</span>
        {#if stats}<span class="count">({stats.artist_count})</span>{/if}
      </button>
      <button
        class="tab"
        class:active={activeTab === 'tracks'}
        onclick={() => handleTabChange('tracks')}
      >
        <Music size={16} />
        <span>Tracks</span>
        {#if stats}<span class="count">({stats.track_count})</span>{/if}
      </button>
    </div>

    <!-- Content -->
    <div class="content">
      {#if loading}
        <div class="loading">
          <div class="spinner"></div>
          <p>Loading library...</p>
        </div>
      {:else if error}
        <div class="error">
          <AlertCircle size={48} />
          <p>Failed to load library</p>
          <p class="error-detail">{error}</p>
          <button class="retry-btn" onclick={loadLibraryData}>Retry</button>
        </div>
      {:else if activeTab === 'albums'}
        {#if albums.length === 0}
          <div class="empty">
            <Disc3 size={48} />
            <p>No albums in library</p>
            <p class="empty-hint">Add folders and scan to build your library</p>
          </div>
        {:else}
          <div class="album-grid">
            {#each albums as album (album.id)}
              <AlbumCard
                artwork={getArtworkUrl(album.artwork_path)}
                title={album.title}
                artist={album.artist}
                quality={getAlbumQualityBadge(album)}
                onclick={() => handleAlbumClick(album)}
              />
            {/each}
          </div>
        {/if}
      {:else if activeTab === 'artists'}
        {#if artists.length === 0}
          <div class="empty">
            <Mic2 size={48} />
            <p>No artists in library</p>
          </div>
        {:else}
          <div class="artist-grid">
            {#each artists as artist (artist.name)}
              <div class="artist-card">
                <div class="artist-icon">
                  <Mic2 size={32} />
                </div>
                <div class="artist-name">{artist.name}</div>
                <div class="artist-stats">
                  {artist.album_count} albums &bull; {artist.track_count} tracks
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {:else if activeTab === 'tracks'}
        {#if tracks.length === 0}
          <div class="empty">
            <Music size={48} />
            <p>No tracks in library</p>
          </div>
        {:else}
          <div class="track-list">
            {#each tracks as track, index (track.id)}
              <TrackRow
                number={index + 1}
                title={track.title}
                artist={track.artist}
                duration={formatDuration(track.duration_secs)}
                quality={getQualityBadge(track)}
                hideDownload={true}
                onPlay={() => handleTrackPlay(track)}
                menuActions={{
                  onPlayNow: () => handleTrackPlay(track),
                  onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
                  onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined
                }}
              />
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .library-view {
    padding: 24px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  /* Header */
  .header {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 24px;
  }

  .header-icon {
    width: 80px;
    height: 80px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--accent-primary) 0%, #64b5f6 100%);
    border-radius: 16px;
    color: white;
  }

  .header-content {
    flex: 1;
  }

  .header-content h1 {
    font-size: 32px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 4px 0;
  }

  .subtitle {
    font-size: 14px;
    color: var(--text-muted);
    margin: 0;
  }

  .header-actions {
    display: flex;
    gap: 8px;
  }

  .icon-btn {
    width: 40px;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 8px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .icon-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .icon-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(.spinning) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Scan Progress */
  .scan-progress {
    background: var(--bg-secondary);
    border-radius: 8px;
    padding: 16px;
    margin-bottom: 24px;
  }

  .progress-bar {
    height: 4px;
    background: var(--bg-tertiary);
    border-radius: 2px;
    overflow: hidden;
    margin-bottom: 8px;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent-primary);
    transition: width 300ms ease;
  }

  .progress-text {
    display: flex;
    justify-content: space-between;
    font-size: 13px;
    color: var(--text-muted);
  }

  .current-file {
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Settings Panel */
  .settings-panel {
    background: var(--bg-secondary);
    border-radius: 12px;
    padding: 20px;
    margin-bottom: 24px;
  }

  .settings-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  .settings-header h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .add-folder-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .add-folder-btn:hover {
    background: var(--accent-hover);
  }

  .no-folders {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 14px;
  }

  .folder-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .folder-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    background: var(--bg-tertiary);
    border-radius: 8px;
  }

  .folder-path {
    font-size: 13px;
    color: var(--text-primary);
    font-family: var(--font-mono);
  }

  .remove-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    transition: color 150ms ease;
  }

  .remove-btn:hover {
    color: #ef4444;
  }

  .settings-actions {
    margin-top: 16px;
    padding-top: 16px;
    border-top: 1px solid var(--bg-tertiary);
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
  }

  .secondary-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .secondary-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    border-color: var(--text-muted);
  }

  .secondary-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .discogs-hint {
    font-size: 12px;
    color: var(--text-muted);
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    flex: 1;
  }

  .danger-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: transparent;
    color: #ef4444;
    border: 1px solid #ef4444;
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .danger-btn:hover {
    background: #ef4444;
    color: white;
  }

  /* Tabs */
  .tabs {
    display: flex;
    gap: 8px;
    margin-bottom: 24px;
    border-bottom: 1px solid var(--bg-tertiary);
    padding-bottom: 16px;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 20px;
    background: none;
    border: none;
    border-radius: 20px;
    color: var(--text-muted);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .tab:hover {
    color: var(--text-primary);
    background-color: var(--bg-tertiary);
  }

  .tab.active {
    color: var(--text-primary);
    background-color: var(--bg-tertiary);
  }

  .tab .count {
    font-size: 12px;
    opacity: 0.7;
  }

  /* Content */
  .content {
    min-height: 200px;
  }

  .loading,
  .error,
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
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

  .error-detail {
    font-size: 12px;
    margin-top: 8px;
  }

  .retry-btn {
    margin-top: 16px;
    padding: 8px 24px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    cursor: pointer;
  }

  .empty-hint {
    font-size: 13px;
    margin-top: 8px;
  }

  /* Album Grid */
  .album-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 24px;
  }

  /* Artist Grid */
  .artist-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 24px;
  }

  .artist-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 20px;
    background: var(--bg-secondary);
    border-radius: 12px;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .artist-card:hover {
    background: var(--bg-tertiary);
  }

  .artist-icon {
    width: 100px;
    height: 100px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border-radius: 50%;
    margin-bottom: 12px;
    color: var(--text-muted);
  }

  .artist-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
    margin-bottom: 4px;
  }

  .artist-stats {
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }

  /* Track List */
  .track-list {
    display: flex;
    flex-direction: column;
  }

  /* Album Detail */
  .album-detail {
    padding-bottom: 100px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 14px;
    cursor: pointer;
    margin-bottom: 24px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-primary);
  }

  .album-header {
    display: flex;
    gap: 24px;
    margin-bottom: 32px;
  }

  .album-artwork {
    width: 200px;
    height: 200px;
    border-radius: 8px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .album-artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
  }

  .album-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
  }

  .album-info h1 {
    font-size: 32px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 8px 0;
  }

  .album-info .artist {
    font-size: 16px;
    color: var(--text-primary);
    margin: 0 0 8px 0;
  }

  .album-info .meta {
    font-size: 14px;
    color: var(--text-muted);
    margin: 0 0 12px 0;
  }

  .audio-specs {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }

  .spec-badge {
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .spec-badge.hires {
    background: linear-gradient(135deg, #f59e0b 0%, #d97706 100%);
    color: white;
  }

  .spec-item {
    font-size: 13px;
    color: var(--text-secondary);
    padding: 4px 8px;
    background: var(--bg-secondary);
    border-radius: 4px;
  }

  .album-actions {
    display: flex;
    gap: 12px;
  }

  .play-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 28px;
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 24px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .play-btn:hover {
    background: var(--accent-hover);
  }
</style>
