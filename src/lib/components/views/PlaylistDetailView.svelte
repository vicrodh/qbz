<script lang="ts">
  import { ArrowLeft, Play, Shuffle, Plus, ListMusic, Search, X, ChevronDown, ImagePlus, HardDrive, Info } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import TrackRow from '../TrackRow.svelte';
  import { type DownloadStatus } from '$lib/stores/downloadState';

  interface PlaylistTrack {
    id: number;
    title: string;
    duration: number;
    track_number: number;
    performer?: { id?: number; name: string };
    album?: { id: string; title: string; image: { small?: string; thumbnail?: string; large?: string } };
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    isrc?: string;
  }

  interface Playlist {
    id: number;
    name: string;
    description?: string;
    owner: { id: number; name: string };
    images?: string[];
    tracks_count: number;
    duration: number;
    is_public: boolean;
    tracks?: { items: PlaylistTrack[]; total: number };
  }

  interface DisplayTrack {
    id: number;
    number: number;
    title: string;
    artist?: string;
    album?: string;
    albumArt?: string;
    albumId?: string;
    artistId?: number;
    duration: string;
    durationSeconds: number;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
    isrc?: string;
    isLocal?: boolean;
    localTrackId?: number;
    artworkPath?: string;
  }

  // Local library track from backend
  interface LocalLibraryTrack {
    id: number;
    file_path: string;
    title: string;
    artist: string;
    album: string;
    duration_secs: number;
    format: string;
    bit_depth?: number;
    sample_rate: number;
    artwork_path?: string;
  }

  interface PlaylistSettings {
    qobuz_playlist_id: number;
    custom_artwork_path?: string;
    sort_by: string;
    sort_order: string;
    last_search_query?: string;
    notes?: string;
  }

  type SortField = 'default' | 'title' | 'artist' | 'album' | 'duration';
  type SortOrder = 'asc' | 'desc';

  interface Props {
    playlistId: number;
    onBack: () => void;
    onTrackPlay?: (track: DisplayTrack) => void;
    onTrackPlayNext?: (track: DisplayTrack) => void;
    onTrackPlayLater?: (track: DisplayTrack) => void;
    onTrackAddFavorite?: (trackId: number) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onTrackShareQobuz?: (trackId: number) => void;
    onTrackShareSonglink?: (track: DisplayTrack) => void;
    onTrackGoToAlbum?: (albumId: string) => void;
    onTrackGoToArtist?: (artistId: number) => void;
    onTrackDownload?: (track: DisplayTrack) => void;
    onTrackRemoveDownload?: (trackId: number) => void;
    getTrackDownloadStatus?: (trackId: number) => { status: DownloadStatus; progress: number };
    downloadStateVersion?: number;
    onLocalTrackPlay?: (track: LocalLibraryTrack) => void;
    onLocalTrackPlayNext?: (track: LocalLibraryTrack) => void;
    onLocalTrackPlayLater?: (track: LocalLibraryTrack) => void;
  }

  let {
    playlistId,
    onBack,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddFavorite,
    onTrackAddToPlaylist,
    onTrackShareQobuz,
    onTrackShareSonglink,
    onTrackGoToAlbum,
    onTrackGoToArtist,
    onTrackDownload,
    onTrackRemoveDownload,
    getTrackDownloadStatus,
    downloadStateVersion,
    onLocalTrackPlay,
    onLocalTrackPlayNext,
    onLocalTrackPlayLater
  }: Props = $props();

  let playlist = $state<Playlist | null>(null);
  let tracks = $state<DisplayTrack[]>([]);
  let localTracks = $state<LocalLibraryTrack[]>([]);
  let localTracksMap = $state<Map<number, LocalLibraryTrack>>(new Map());
  let hasLocalTracks = $derived(localTracks.length > 0);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let playBtnHovered = $state(false);
  let favoriteTrackIds = $state<Set<number>>(new Set());

  // Local settings state
  let searchQuery = $state('');
  let sortBy = $state<SortField>('default');
  let sortOrder = $state<SortOrder>('asc');
  let customArtworkPath = $state<string | null>(null);
  let showSortMenu = $state(false);

  interface FavoritesResponse {
    tracks?: { items: Array<{ id: number }>; total: number };
  }

  onMount(() => {
    loadPlaylist();
    loadSettings();
    loadFavorites();
    loadLocalTracks();
  });

  async function loadLocalTracks() {
    try {
      const result = await invoke<LocalLibraryTrack[]>('playlist_get_local_tracks', { playlistId });
      localTracks = result;
      // Create a map for quick lookup
      localTracksMap = new Map(result.map(t => [t.id, t]));
    } catch (err) {
      console.error('Failed to load local tracks:', err);
    }
  }

  async function loadFavorites() {
    try {
      const response = await invoke<FavoritesResponse>('get_favorites', {
        favType: 'tracks',
        limit: 500,
        offset: 0
      });
      if (response.tracks?.items) {
        favoriteTrackIds = new Set(response.tracks.items.map(item => item.id));
      }
    } catch (err) {
      console.error('Failed to load favorite tracks:', err);
    }
  }

  async function loadPlaylist() {
    loading = true;
    error = null;
    try {
      const result = await invoke<Playlist>('get_playlist', { playlistId });
      playlist = result;

      if (result.tracks?.items) {
        tracks = result.tracks.items.map((t, idx) => ({
          id: t.id,
          number: idx + 1,
          title: t.title,
          artist: t.performer?.name,
          album: t.album?.title,
          albumArt: t.album?.image?.thumbnail || t.album?.image?.small,
          albumId: t.album?.id,
          artistId: t.performer?.id,
          duration: formatDuration(t.duration),
          durationSeconds: t.duration,
          hires: t.hires,
          bitDepth: t.maximum_bit_depth,
          samplingRate: t.maximum_sampling_rate,
          isrc: t.isrc,
        }));
      }
    } catch (err) {
      console.error('Failed to load playlist:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function loadSettings() {
    try {
      const settings = await invoke<PlaylistSettings | null>('playlist_get_settings', { playlistId });
      if (settings) {
        sortBy = (settings.sort_by as SortField) || 'default';
        sortOrder = (settings.sort_order as SortOrder) || 'asc';
        customArtworkPath = settings.custom_artwork_path || null;
        searchQuery = settings.last_search_query || '';
      }
    } catch (err) {
      console.error('Failed to load playlist settings:', err);
    }
  }

  async function saveSort(field: SortField, order: SortOrder) {
    sortBy = field;
    sortOrder = order;
    showSortMenu = false;
    try {
      await invoke('playlist_set_sort', { playlistId, sortBy: field, sortOrder: order });
    } catch (err) {
      console.error('Failed to save sort settings:', err);
    }
  }

  async function selectCustomArtwork() {
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }]
      });
      if (selected && typeof selected === 'string') {
        customArtworkPath = selected;
        await invoke('playlist_set_artwork', { playlistId, artworkPath: selected });
      }
    } catch (err) {
      console.error('Failed to select artwork:', err);
    }
  }

  async function clearCustomArtwork() {
    customArtworkPath = null;
    try {
      await invoke('playlist_set_artwork', { playlistId, artworkPath: null });
    } catch (err) {
      console.error('Failed to clear artwork:', err);
    }
  }

  // Convert local tracks to DisplayTrack format
  function localTrackToDisplay(track: LocalLibraryTrack, index: number): DisplayTrack {
    return {
      id: -track.id, // Negative ID to distinguish from Qobuz tracks
      number: index + 1,
      title: track.title,
      artist: track.artist,
      album: track.album,
      albumArt: track.artwork_path ? `asset://localhost/${encodeURIComponent(track.artwork_path)}` : undefined,
      duration: formatDuration(track.duration_secs),
      durationSeconds: track.duration_secs,
      hires: (track.bit_depth && track.bit_depth >= 24) || track.sample_rate > 48000,
      bitDepth: track.bit_depth,
      samplingRate: track.sample_rate,
      isLocal: true,
      localTrackId: track.id,
      artworkPath: track.artwork_path
    };
  }

  // Filtered and sorted tracks (merged Qobuz + local)
  let displayTracks = $derived.by(() => {
    // Combine Qobuz tracks with local tracks
    const localDisplayTracks = localTracks.map((t, idx) => localTrackToDisplay(t, tracks.length + idx));
    let result = [...tracks, ...localDisplayTracks];

    // Filter by search query
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      result = result.filter(t =>
        t.title.toLowerCase().includes(query) ||
        (t.artist?.toLowerCase().includes(query)) ||
        (t.album?.toLowerCase().includes(query))
      );
    }

    // Sort
    if (sortBy !== 'default') {
      result.sort((a, b) => {
        let cmp = 0;
        switch (sortBy) {
          case 'title':
            cmp = a.title.localeCompare(b.title);
            break;
          case 'artist':
            cmp = (a.artist || '').localeCompare(b.artist || '');
            break;
          case 'album':
            cmp = (a.album || '').localeCompare(b.album || '');
            break;
          case 'duration':
            cmp = a.durationSeconds - b.durationSeconds;
            break;
        }
        return sortOrder === 'desc' ? -cmp : cmp;
      });
    }

    return result;
  });

  const sortOptions: { field: SortField; label: string }[] = [
    { field: 'default', label: 'Default' },
    { field: 'title', label: 'Title' },
    { field: 'artist', label: 'Artist' },
    { field: 'album', label: 'Album' },
    { field: 'duration', label: 'Duration' },
  ];

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function formatTotalDuration(seconds: number): string {
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    if (hours > 0) {
      return `${hours} hr ${mins} min`;
    }
    return `${mins} min`;
  }

  function getPlaylistImage(): string {
    // Use custom artwork if set (convert to tauri asset URL)
    if (customArtworkPath) {
      return `asset://localhost/${encodeURIComponent(customArtworkPath)}`;
    }
    if (playlist?.images && playlist.images.length > 0) {
      return playlist.images[0];
    }
    // Return first track's album art if available
    if (tracks.length > 0 && tracks[0].albumArt) {
      return tracks[0].albumArt;
    }
    return '';
  }

  function handleTrackClick(track: DisplayTrack) {
    if (track.isLocal && track.localTrackId) {
      // Handle local track play
      const localTrack = localTracksMap.get(track.localTrackId);
      if (localTrack && onLocalTrackPlay) {
        onLocalTrackPlay(localTrack);
      }
    } else if (onTrackPlay) {
      onTrackPlay(track);
    }
  }

  function handleTrackPlayNext(track: DisplayTrack) {
    if (track.isLocal && track.localTrackId) {
      const localTrack = localTracksMap.get(track.localTrackId);
      if (localTrack && onLocalTrackPlayNext) {
        onLocalTrackPlayNext(localTrack);
      }
    } else if (onTrackPlayNext) {
      onTrackPlayNext(track);
    }
  }

  function handleTrackPlayLater(track: DisplayTrack) {
    if (track.isLocal && track.localTrackId) {
      const localTrack = localTracksMap.get(track.localTrackId);
      if (localTrack && onLocalTrackPlayLater) {
        onLocalTrackPlayLater(localTrack);
      }
    } else if (onTrackPlayLater) {
      onTrackPlayLater(track);
    }
  }

  async function removeLocalTrack(track: DisplayTrack) {
    if (!track.localTrackId) return;
    try {
      await invoke('playlist_remove_local_track', { playlistId, localTrackId: track.localTrackId });
      await loadLocalTracks(); // Refresh
    } catch (err) {
      console.error('Failed to remove local track:', err);
    }
  }

  async function handlePlayAll() {
    if (tracks.length > 0 && onTrackPlay) {
      // Set queue with all tracks and play first
      const queueTracks = tracks.map(t => ({
        id: t.id,
        title: t.title,
        artist: t.artist || 'Unknown Artist',
        album: t.album || playlist?.name || 'Playlist',
        duration_secs: t.durationSeconds,
        artwork_url: t.albumArt || getPlaylistImage(),
      }));

      try {
        await invoke('set_queue', { tracks: queueTracks, startIndex: 0 });
        onTrackPlay(tracks[0]);
      } catch (err) {
        console.error('Failed to set queue:', err);
      }
    }
  }

  async function handleShuffle() {
    if (tracks.length > 0 && onTrackPlay) {
      try {
        await invoke('set_shuffle', { enabled: true });
        await handlePlayAll();
      } catch (err) {
        console.error('Failed to shuffle:', err);
      }
    }
  }

  async function handleAddToQueue() {
    if (tracks.length > 0) {
      const queueTracks = tracks.map(t => ({
        id: t.id,
        title: t.title,
        artist: t.artist || 'Unknown Artist',
        album: t.album || playlist?.name || 'Playlist',
        duration_secs: t.durationSeconds,
        artwork_url: t.albumArt || getPlaylistImage(),
      }));

      try {
        await invoke('add_tracks_to_queue', { tracks: queueTracks });
      } catch (err) {
        console.error('Failed to add to queue:', err);
      }
    }
  }
</script>

<div class="playlist-detail">
  <!-- Back Navigation -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>Back</span>
  </button>

  {#if loading}
    <div class="loading">
      <div class="spinner"></div>
      <p>Loading playlist...</p>
    </div>
  {:else if error}
    <div class="error">
      <p>Failed to load playlist</p>
      <p class="error-detail">{error}</p>
      <button class="retry-btn" onclick={loadPlaylist}>Retry</button>
    </div>
  {:else if playlist}
    <!-- Playlist Header -->
    <div class="playlist-header">
      <!-- Playlist Artwork with customization -->
      <div class="artwork-container">
        <div class="artwork">
          {#if getPlaylistImage()}
            <img src={getPlaylistImage()} alt={playlist.name} />
          {:else}
            <div class="artwork-placeholder">
              <ListMusic size={64} />
            </div>
          {/if}
          <div class="artwork-overlay">
            <button class="artwork-btn" onclick={selectCustomArtwork} title="Set custom artwork">
              <ImagePlus size={24} />
            </button>
            {#if customArtworkPath}
              <button class="artwork-btn artwork-clear" onclick={clearCustomArtwork} title="Remove custom artwork">
                <X size={20} />
              </button>
            {/if}
          </div>
        </div>
      </div>

      <!-- Playlist Metadata -->
      <div class="metadata">
        <span class="playlist-label">Playlist</span>
        <h1 class="playlist-title">{playlist.name}</h1>
        {#if playlist.description}
          <p class="playlist-description">{playlist.description}</p>
        {/if}
        <div class="playlist-info">
          <span class="owner">{playlist.owner.name}</span>
          <span class="separator">•</span>
          <span>{playlist.tracks_count} tracks</span>
          <span class="separator">•</span>
          <span>{formatTotalDuration(playlist.duration)}</span>
        </div>

        <!-- Action Buttons -->
        <div class="actions">
          <button
            class="play-btn"
            style="background-color: {playBtnHovered ? 'var(--accent-hover)' : 'var(--accent-primary)'}"
            onmouseenter={() => (playBtnHovered = true)}
            onmouseleave={() => (playBtnHovered = false)}
            onclick={handlePlayAll}
          >
            <Play size={18} fill="white" color="white" />
            <span>Play</span>
          </button>
          <button class="secondary-btn" onclick={handleShuffle}>
            <Shuffle size={18} />
            <span>Shuffle</span>
          </button>
          <button class="icon-btn" onclick={handleAddToQueue} title="Add all to queue">
            <Plus size={20} color="white" />
          </button>
        </div>
      </div>
    </div>

    <!-- Track List Controls -->
    <div class="track-controls">
      <!-- Search -->
      <div class="search-container">
        <Search size={16} class="search-icon" />
        <input
          type="text"
          placeholder="Search in playlist..."
          bind:value={searchQuery}
          class="search-input"
        />
        {#if searchQuery}
          <button class="search-clear" onclick={() => searchQuery = ''}>
            <X size={14} />
          </button>
        {/if}
      </div>

      <!-- Sort dropdown -->
      <div class="sort-container">
        <button class="sort-btn" onclick={() => showSortMenu = !showSortMenu}>
          <span>Sort: {sortOptions.find(o => o.field === sortBy)?.label}</span>
          <span class="chevron" class:rotated={showSortMenu}><ChevronDown size={14} /></span>
        </button>
        {#if showSortMenu}
          <div class="sort-menu">
            {#each sortOptions as option}
              <button
                class="sort-option"
                class:active={sortBy === option.field}
                onclick={() => saveSort(option.field, sortOrder)}
              >
                {option.label}
              </button>
            {/each}
            <div class="sort-divider"></div>
            <button
              class="sort-option"
              class:active={sortOrder === 'asc'}
              onclick={() => saveSort(sortBy, 'asc')}
            >
              Ascending
            </button>
            <button
              class="sort-option"
              class:active={sortOrder === 'desc'}
              onclick={() => saveSort(sortBy, 'desc')}
            >
              Descending
            </button>
          </div>
        {/if}
      </div>

      <span class="track-count">
        {displayTracks.length}{searchQuery ? ` / ${tracks.length + localTracks.length}` : ''} tracks
        {#if hasLocalTracks}
          <span class="local-count">({localTracks.length} local)</span>
        {/if}
      </span>
    </div>

    <!-- Local tracks notice -->
    {#if hasLocalTracks}
      <div class="local-notice">
        <Info size={14} />
        <span>Local tracks are only available on this device</span>
      </div>
    {/if}

    <!-- Track List -->
    <div class="track-list">
      <div class="track-list-header">
        <span class="col-number">#</span>
        <span class="col-title">Title</span>
        <span class="col-album">Album</span>
        <span class="col-duration">Duration</span>
      </div>

      {#each displayTracks as track, idx (`${track.id}-${downloadStateVersion}`)}
        {@const downloadInfo = track.isLocal ? { status: 'none' as const, progress: 0 } : (getTrackDownloadStatus?.(track.id) ?? { status: 'none' as const, progress: 0 })}
        <div class="track-row-wrapper" class:is-local={track.isLocal}>
          {#if track.isLocal}
            <div class="local-indicator" title="Local track">
              <HardDrive size={12} />
            </div>
          {/if}
          <TrackRow
            number={idx + 1}
            title={track.title}
            artist={track.artist}
            duration={track.duration}
            quality={track.hires ? 'Hi-Res' : undefined}
            isFavorite={!track.isLocal && favoriteTrackIds.has(track.id)}
            hideDownload={track.isLocal}
            downloadStatus={downloadInfo.status}
            downloadProgress={downloadInfo.progress}
            onPlay={() => handleTrackClick(track)}
            onDownload={!track.isLocal && onTrackDownload ? () => onTrackDownload(track) : undefined}
            onRemoveDownload={!track.isLocal && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
            menuActions={{
              onPlayNow: () => handleTrackClick(track),
              onPlayNext: track.isLocal ? () => handleTrackPlayNext(track) : (onTrackPlayNext ? () => onTrackPlayNext(track) : undefined),
              onPlayLater: track.isLocal ? () => handleTrackPlayLater(track) : (onTrackPlayLater ? () => onTrackPlayLater(track) : undefined),
              onAddFavorite: !track.isLocal && onTrackAddFavorite ? () => onTrackAddFavorite(track.id) : undefined,
              onAddToPlaylist: !track.isLocal && onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined,
              onShareQobuz: !track.isLocal && onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
              onShareSonglink: !track.isLocal && onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined,
              onGoToAlbum: !track.isLocal && track.albumId && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.albumId) : undefined,
              onGoToArtist: !track.isLocal && track.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(track.artistId) : undefined
            }}
          />
          {#if track.isLocal}
            <button class="remove-local-btn" onclick={() => removeLocalTrack(track)} title="Remove from playlist">
              <X size={14} />
            </button>
          {/if}
        </div>
      {/each}

      {#if displayTracks.length === 0 && searchQuery}
        <div class="no-results">
          <p>No tracks match "{searchQuery}"</p>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .playlist-detail {
    padding: 24px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 14px;
    margin-bottom: 24px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-primary);
  }

  .loading,
  .error {
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

  @keyframes spin {
    to { transform: rotate(360deg); }
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

  .playlist-header {
    display: flex;
    gap: 32px;
    margin-bottom: 32px;
  }

  .artwork-container {
    flex-shrink: 0;
  }

  .artwork {
    width: 232px;
    height: 232px;
    position: relative;
    border-radius: 8px;
    overflow: hidden;
    background-color: var(--bg-tertiary);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .artwork img {
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
    color: var(--text-muted);
  }

  .artwork-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .artwork:hover .artwork-overlay {
    opacity: 1;
  }

  .artwork-btn {
    width: 48px;
    height: 48px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.3);
    border-radius: 50%;
    color: white;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .artwork-btn:hover {
    background: rgba(255, 255, 255, 0.2);
    border-color: rgba(255, 255, 255, 0.5);
  }

  .artwork-btn.artwork-clear {
    width: 36px;
    height: 36px;
    background: rgba(255, 0, 0, 0.3);
    border-color: rgba(255, 100, 100, 0.5);
  }

  .artwork-btn.artwork-clear:hover {
    background: rgba(255, 0, 0, 0.5);
  }

  .metadata {
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    min-width: 0;
  }

  .playlist-label {
    font-size: 12px;
    text-transform: uppercase;
    color: var(--text-muted);
    font-weight: 600;
    letter-spacing: 0.1em;
    margin-bottom: 8px;
  }

  .playlist-title {
    font-size: 48px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 8px 0;
    line-height: 1.1;
  }

  .playlist-description {
    font-size: 14px;
    color: var(--text-secondary);
    margin: 0 0 12px 0;
    line-height: 1.4;
  }

  .playlist-info {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-secondary);
    margin-bottom: 24px;
  }

  .owner {
    font-weight: 500;
    color: var(--text-primary);
  }

  .separator {
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .play-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 32px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 24px;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .secondary-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 24px;
    background-color: transparent;
    color: var(--text-primary);
    border: 1px solid var(--text-muted);
    border-radius: 24px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: border-color 150ms ease;
  }

  .secondary-btn:hover {
    border-color: var(--text-primary);
  }

  .icon-btn {
    width: 40px;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    cursor: pointer;
    opacity: 0.7;
    transition: opacity 150ms ease;
  }

  .icon-btn:hover {
    opacity: 1;
  }

  .track-list {
    margin-top: 24px;
  }

  .track-list-header {
    display: grid;
    grid-template-columns: 48px 1fr 1fr 80px;
    gap: 16px;
    padding: 8px 16px;
    font-size: 12px;
    text-transform: uppercase;
    color: var(--text-muted);
    font-weight: 600;
    letter-spacing: 0.05em;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 8px;
  }

  .col-number {
    text-align: center;
  }

  .col-duration {
    text-align: right;
  }

  /* Track Controls */
  .track-controls {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 24px;
    margin-bottom: 16px;
  }

  .search-container {
    display: flex;
    align-items: center;
    gap: 8px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 8px 12px;
    flex: 1;
    max-width: 300px;
  }

  .search-container :global(.search-icon) {
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

  .search-clear {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .search-clear:hover {
    color: var(--text-primary);
  }

  .sort-container {
    position: relative;
  }

  .sort-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background-color: var(--bg-tertiary);
    border: none;
    border-radius: 8px;
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
    transition: color 150ms ease;
  }

  .sort-btn:hover {
    color: var(--text-primary);
  }

  .sort-btn .chevron {
    display: flex;
    transition: transform 150ms ease;
  }

  .sort-btn .chevron.rotated {
    transform: rotate(180deg);
  }

  .sort-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    background-color: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px;
    min-width: 140px;
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .sort-option {
    display: block;
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 13px;
    text-align: left;
    cursor: pointer;
    border-radius: 4px;
    transition: all 150ms ease;
  }

  .sort-option:hover {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .sort-option.active {
    color: var(--accent-primary);
  }

  .sort-divider {
    height: 1px;
    background-color: var(--bg-tertiary);
    margin: 4px 0;
  }

  .track-count {
    margin-left: auto;
    font-size: 13px;
    color: var(--text-muted);
  }

  .no-results {
    padding: 48px;
    text-align: center;
    color: var(--text-muted);
  }

  .no-results p {
    margin: 0;
  }

  /* Local tracks styles */
  .local-count {
    color: var(--accent-primary);
    margin-left: 4px;
  }

  .local-notice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    margin-bottom: 16px;
    background-color: rgba(var(--accent-primary-rgb, 139, 92, 246), 0.1);
    border: 1px solid rgba(var(--accent-primary-rgb, 139, 92, 246), 0.2);
    border-radius: 8px;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .local-notice :global(svg) {
    color: var(--accent-primary);
    flex-shrink: 0;
  }

  .track-row-wrapper {
    display: flex;
    align-items: center;
    position: relative;
  }

  .track-row-wrapper.is-local {
    padding-left: 24px;
  }

  .track-row-wrapper :global(.track-row) {
    flex: 1;
  }

  .local-indicator {
    position: absolute;
    left: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    color: var(--accent-primary);
  }

  .remove-local-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
    opacity: 0;
    transition: all 150ms ease;
  }

  .track-row-wrapper:hover .remove-local-btn {
    opacity: 1;
  }

  .remove-local-btn:hover {
    color: #ff6b6b;
    background-color: rgba(255, 107, 107, 0.1);
  }
</style>
