<script lang="ts">
  import { ArrowLeft, Play, Shuffle, ListMusic, Search, X, ChevronDown, ChevronRight, ImagePlus, Edit3, BarChart2, WifiOff } from 'lucide-svelte';
  import AlbumMenu from '../AlbumMenu.svelte';
  import PlaylistCollage from '../PlaylistCollage.svelte';
  import PlaylistModal from '../PlaylistModal.svelte';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import TrackRow from '../TrackRow.svelte';
  import { type DownloadStatus } from '$lib/stores/downloadState';
  import {
    subscribe as subscribeOffline,
    getStatus as getOfflineStatus,
    type OfflineStatus
  } from '$lib/stores/offlineStore';
  import { t } from '$lib/i18n';
  import { onMount } from 'svelte';

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
    playlist_track_id?: number; // Qobuz playlist-specific ID for removal
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
    playlistTrackId?: number; // Qobuz playlist-specific ID for removal
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
    hidden?: boolean;
    position?: number;
  }

  interface PlaylistStats {
    qobuz_playlist_id: number;
    play_count: number;
    last_played_at?: number;
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
    onPlaylistUpdated?: () => void;
    onPlaylistDeleted?: (playlistId: number) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
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
    onLocalTrackPlayLater,
    onPlaylistUpdated,
    onPlaylistDeleted,
    activeTrackId = null,
    isPlaybackActive = false
  }: Props = $props();

  let playlist = $state<Playlist | null>(null);
  let tracks = $state<DisplayTrack[]>([]);
  let localTracks = $state<LocalLibraryTrack[]>([]);
  let localTracksMap = $state<Map<number, LocalLibraryTrack>>(new Map());
  let hasLocalTracks = $derived(localTracks.length > 0);

  // Total counts including local tracks
  let totalTrackCount = $derived((playlist?.tracks_count ?? 0) + localTracks.length);
  let localTracksDuration = $derived(localTracks.reduce((sum, t) => sum + t.duration_secs, 0));
  let totalDuration = $derived((playlist?.duration ?? 0) + localTracksDuration);

  let loading = $state(true);
  let error = $state<string | null>(null);
  let playBtnHovered = $state(false);

  // Offline mode state
  let offlineStatus = $state<OfflineStatus>(getOfflineStatus());
  let tracksWithLocalCopies = $state<Set<number>>(new Set());

  // Local settings state
  let searchQuery = $state('');
  let sortBy = $state<SortField>('default');
  let sortOrder = $state<SortOrder>('asc');
  let customArtworkPath = $state<string | null>(null);
  let showSortMenu = $state(false);
  let playlistSettings = $state<PlaylistSettings | null>(null);
  let playlistStats = $state<PlaylistStats | null>(null);
  let editModalOpen = $state(false);

  // Subscribe to offline status changes
  onMount(() => {
    const unsubscribe = subscribeOffline(() => {
      offlineStatus = getOfflineStatus();
      // Re-check local copies when offline status changes
      if (offlineStatus.isOffline && tracks.length > 0) {
        checkTracksLocalStatus();
      }
    });
    return unsubscribe;
  });

  // Check if a track is available (has local copy when offline, always available when online)
  function isTrackAvailable(track: DisplayTrack): boolean {
    if (!offlineStatus.isOffline) return true;
    if (track.isLocal) return true; // Local tracks are always available
    return tracksWithLocalCopies.has(track.id);
  }

  // Check which tracks have local copies (for offline mode)
  async function checkTracksLocalStatus() {
    if (!offlineStatus.isOffline || tracks.length === 0) {
      tracksWithLocalCopies = new Set();
      return;
    }

    try {
      const qobuzTrackIds = tracks.filter(t => !t.isLocal).map(t => t.id);
      if (qobuzTrackIds.length === 0) {
        tracksWithLocalCopies = new Set();
        return;
      }

      const localIds = await invoke<number[]>('playlist_get_tracks_with_local_copies', {
        trackIds: qobuzTrackIds
      });
      tracksWithLocalCopies = new Set(localIds);
    } catch (err) {
      console.error('Failed to check local track status:', err);
      tracksWithLocalCopies = new Set();
    }
  }

  // Reload playlist when playlistId changes
  $effect(() => {
    // Access playlistId to create dependency
    const id = playlistId;
    loadPlaylist();
    loadSettings();
    loadLocalTracks();
    loadStats();
  });

  // Check local track status after loading tracks and when offline
  $effect(() => {
    if (offlineStatus.isOffline && tracks.length > 0) {
      checkTracksLocalStatus();
    }
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
          albumArt: t.album?.image?.large || t.album?.image?.thumbnail || t.album?.image?.small,
          albumId: t.album?.id,
          artistId: t.performer?.id,
          duration: formatDuration(t.duration),
          durationSeconds: t.duration,
          hires: t.hires,
          bitDepth: t.maximum_bit_depth,
          samplingRate: t.maximum_sampling_rate,
          isrc: t.isrc,
          playlistTrackId: t.playlist_track_id,
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
    // Reset state before loading new playlist settings
    sortBy = 'default';
    sortOrder = 'asc';
    customArtworkPath = null;
    searchQuery = '';
    playlistSettings = null;

    try {
      const settings = await invoke<PlaylistSettings | null>('playlist_get_settings', { playlistId });
      playlistSettings = settings;
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

  async function loadStats() {
    try {
      const stats = await invoke<PlaylistStats | null>('playlist_get_stats', { playlistId });
      playlistStats = stats;
    } catch (err) {
      console.error('Failed to load playlist stats:', err);
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

  async function removeTrackFromPlaylist(track: DisplayTrack) {
    try {
      if (track.isLocal && track.localTrackId) {
        // Remove local track
        await invoke('playlist_remove_local_track', { playlistId, localTrackId: track.localTrackId });
        await loadLocalTracks();
      } else if (track.playlistTrackId) {
        // Remove Qobuz track using playlist_track_id
        await invoke('remove_tracks_from_playlist', {
          playlistId,
          playlistTrackIds: [track.playlistTrackId]
        });
        await loadPlaylist();
      }
      // Notify parent to refresh sidebar counts
      onPlaylistUpdated?.();
    } catch (err) {
      console.error('Failed to remove track from playlist:', err);
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
        hires: t.hires ?? false,
        bit_depth: t.bitDepth ?? null,
        sample_rate: t.samplingRate ?? null,
      }));

      try {
        await invoke('set_queue', { tracks: queueTracks, startIndex: 0 });
        onTrackPlay(tracks[0]);

        // Increment play count
        const stats = await invoke<PlaylistStats>('playlist_increment_play_count', { playlistId });
        playlistStats = stats;
      } catch (err) {
        console.error('Failed to set queue:', err);
      }
    }
  }

  function handleEditSuccess() {
    editModalOpen = false;
    loadPlaylist(); // Reload playlist data
    loadSettings(); // Reload settings (including hidden status)
    onPlaylistUpdated?.();
  }

  function handleDelete(deletedPlaylistId: number) {
    editModalOpen = false;
    onPlaylistDeleted?.(deletedPlaylistId);
    onBack();
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

  async function handlePlayAllNext() {
    if (tracks.length === 0) return;
    // Add in reverse order so first track ends up right after current
    for (let i = tracks.length - 1; i >= 0; i--) {
      const t = tracks[i];
      try {
        await invoke('add_to_queue_next', {
          track: {
            id: t.id,
            title: t.title,
            artist: t.artist || 'Unknown Artist',
            album: t.album || playlist?.name || 'Playlist',
            duration_secs: t.durationSeconds,
            artwork_url: t.albumArt || getPlaylistImage(),
            hires: t.hires ?? false,
            bit_depth: t.bitDepth ?? null,
            sample_rate: t.samplingRate ?? null,
          }
        });
      } catch (err) {
        console.error('Failed to add track next:', err);
      }
    }
  }

  async function handlePlayAllLater() {
    if (tracks.length === 0) return;
    const queueTracks = tracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.artist || 'Unknown Artist',
      album: t.album || playlist?.name || 'Playlist',
      duration_secs: t.durationSeconds,
      artwork_url: t.albumArt || getPlaylistImage(),
      hires: t.hires ?? false,
      bit_depth: t.bitDepth ?? null,
      sample_rate: t.samplingRate ?? null,
    }));

    try {
      await invoke('add_tracks_to_queue', { tracks: queueTracks });
    } catch (err) {
      console.error('Failed to add to queue:', err);
    }
  }

  function sharePlaylistQobuz() {
    if (!playlist?.id) return;
    const url = `https://play.qobuz.com/playlist/${playlist.id}`;
    writeText(url);
  }

  function sharePlaylistSonglink() {
    if (!playlist?.id) return;
    const qobuzUrl = `https://play.qobuz.com/playlist/${playlist.id}`;
    const songlinkUrl = `https://song.link/${encodeURIComponent(qobuzUrl)}`;
    writeText(songlinkUrl);
  }
</script>

<div class="playlist-detail">
  <!-- Navigation Row -->
  <div class="nav-row">
    <button class="back-btn" onclick={onBack}>
      <ArrowLeft size={16} />
      <span>Back</span>
    </button>
    {#if playlist}
      <button class="edit-btn" onclick={() => editModalOpen = true} title="Edit playlist">
        <Edit3 size={16} />
      </button>
    {/if}
  </div>

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
      <!-- Playlist Artwork - Collage or Custom -->
      <div class="artwork-container">
        {#if customArtworkPath}
          <div class="artwork custom-artwork">
            <img src={`asset://localhost/${encodeURIComponent(customArtworkPath)}`} alt={playlist.name} />
            <div class="artwork-overlay">
              <button class="artwork-btn artwork-clear" onclick={clearCustomArtwork} title="Remove custom artwork">
                <X size={20} />
              </button>
            </div>
          </div>
        {:else}
          <div class="collage-wrapper">
            <PlaylistCollage
              artworks={tracks.slice(0, 4).map(t => t.albumArt).filter((a): a is string => !!a)}
              size={200}
            />
            <div class="artwork-overlay">
              <button class="artwork-btn" onclick={selectCustomArtwork} title="Set custom artwork">
                <ImagePlus size={24} />
              </button>
            </div>
          </div>
        {/if}
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
          <span>{totalTrackCount} tracks{#if hasLocalTracks} <span class="local-count">({localTracks.length} local)</span>{/if}</span>
          <span class="separator">•</span>
          <span>{formatTotalDuration(totalDuration)}</span>
          {#if playlistStats && playlistStats.play_count > 0}
            <span class="separator">•</span>
            <span class="play-count" title="Times played">
              <BarChart2 size={12} />
              {playlistStats.play_count}
            </span>
          {/if}
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
          <AlbumMenu
            onPlayNext={handlePlayAllNext}
            onPlayLater={handlePlayAllLater}
            onShareQobuz={sharePlaylistQobuz}
            onShareSonglink={sharePlaylistSonglink}
          />
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

    </div>

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
        {@const isActiveTrack = isPlaybackActive && (
          track.isLocal
            ? (track.localTrackId !== undefined && activeTrackId === track.localTrackId)
            : activeTrackId === track.id
        )}
        {@const available = isTrackAvailable(track)}
        <div
          class="track-row-wrapper"
          class:unavailable={!available}
          title={!available ? $t('offline.trackNotAvailable') : undefined}
        >
          {#if !available && !track.isLocal}
            <div class="unavailable-indicator" title={$t('offline.trackNotAvailable')}>
              <WifiOff size={12} />
            </div>
          {/if}
          <TrackRow
            trackId={track.isLocal ? undefined : track.id}
            number={idx + 1}
            title={track.title}
            artist={track.artist}
            album={track.album}
            duration={track.duration}
            quality={track.bitDepth && track.samplingRate
              ? `${track.bitDepth}bit/${track.samplingRate}kHz`
              : track.hires
                ? 'Hi-Res'
                : '-'}
            isPlaying={isActiveTrack}
            isLocal={track.isLocal}
            hideFavorite={track.isLocal || !available}
            hideDownload={track.isLocal || !available}
            downloadStatus={downloadInfo.status}
            downloadProgress={downloadInfo.progress}
            onPlay={available ? () => handleTrackClick(track) : undefined}
            onDownload={available && !track.isLocal && onTrackDownload ? () => onTrackDownload(track) : undefined}
            onRemoveDownload={available && !track.isLocal && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
            menuActions={available ? {
              onPlayNow: () => handleTrackClick(track),
              onPlayNext: track.isLocal ? () => handleTrackPlayNext(track) : (onTrackPlayNext ? () => onTrackPlayNext(track) : undefined),
              onPlayLater: track.isLocal ? () => handleTrackPlayLater(track) : (onTrackPlayLater ? () => onTrackPlayLater(track) : undefined),
              onAddToPlaylist: !track.isLocal && onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined,
              onRemoveFromPlaylist: () => removeTrackFromPlaylist(track),
              onShareQobuz: !track.isLocal && onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
              onShareSonglink: !track.isLocal && onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined,
              onGoToAlbum: !track.isLocal && track.albumId && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.albumId!) : undefined,
              onGoToArtist: !track.isLocal && track.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(track.artistId!) : undefined
            } : {}}
          />
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

<!-- Edit Playlist Modal -->
{#if playlist}
  <PlaylistModal
    isOpen={editModalOpen}
    mode="edit"
    playlist={{ id: playlist.id, name: playlist.name, tracks_count: playlist.tracks_count }}
    isHidden={playlistSettings?.hidden ?? false}
    onClose={() => editModalOpen = false}
    onSuccess={handleEditSuccess}
    onDelete={handleDelete}
  />
{/if}

<style>
  .playlist-detail {
    padding: 24px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  /* Custom scrollbar */
  .playlist-detail::-webkit-scrollbar {
    width: 6px;
  }

  .playlist-detail::-webkit-scrollbar-track {
    background: transparent;
  }

  .playlist-detail::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .playlist-detail::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .nav-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
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
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-primary);
  }

  .edit-btn {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    transition: all 150ms ease;
  }

  .edit-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
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

  .collage-wrapper {
    position: relative;
  }

  .collage-wrapper .artwork-overlay {
    position: absolute;
    inset: 0;
    z-index: 10;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    opacity: 0;
    transition: opacity 150ms ease;
    border-radius: 6px;
  }

  .collage-wrapper:hover .artwork-overlay {
    opacity: 1;
  }

  .artwork {
    width: 186px;
    height: 186px;
    position: relative;
    border-radius: 8px;
    overflow: hidden;
    background-color: var(--bg-tertiary);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .custom-artwork {
    width: 200px;
    height: 200px;
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
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 8px 0;
    line-height: 1.2;
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

  .play-count {
    display: flex;
    align-items: center;
    gap: 4px;
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
    border-radius: 8px;
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
    border-radius: 8px;
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

  .no-results {
    padding: 48px;
    text-align: center;
    color: var(--text-muted);
  }

  .no-results p {
    margin: 0;
  }

  .local-count {
    color: var(--text-muted);
    font-size: 0.9em;
  }


  .track-row-wrapper {
    display: flex;
    align-items: center;
    position: relative;
  }

  .track-row-wrapper :global(.track-row) {
    flex: 1;
  }

  /* Unavailable track styles (offline mode) */
  .track-row-wrapper.unavailable {
    padding-left: 24px;
    opacity: 0.4;
    pointer-events: none;
    user-select: none;
  }

  .track-row-wrapper.unavailable :global(.track-row) {
    filter: grayscale(100%);
  }

  .unavailable-indicator {
    position: absolute;
    left: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    color: var(--text-muted);
  }

</style>
