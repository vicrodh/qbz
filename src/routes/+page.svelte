<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';

  // Download state management
  import {
    initDownloadStates,
    startDownloadEventListeners,
    stopDownloadEventListeners,
    downloadTrack,
    removeDownload,
    getDownloadState,
    subscribe as subscribeDownloads,
    type DownloadStatus
  } from '$lib/stores/downloadState';

  // Toast state management
  import {
    showToast,
    hideToast,
    subscribe as subscribeToast,
    type Toast as ToastData
  } from '$lib/stores/toastStore';

  // UI state management
  import {
    subscribe as subscribeUI,
    openQueue,
    closeQueue,
    toggleQueue,
    openFullScreen,
    closeFullScreen,
    toggleFullScreen,
    openFocusMode,
    closeFocusMode,
    toggleFocusMode,
    openCastPicker,
    closeCastPicker,
    openPlaylistModal,
    closePlaylistModal,
    openPlaylistImport,
    closePlaylistImport,
    handleEscapeKey as handleUIEscape,
    getUIState,
    type UIState
  } from '$lib/stores/uiStore';

  // Auth state management
  import {
    subscribe as subscribeAuth,
    setLoggedIn,
    setLoggedOut,
    getAuthState,
    type UserInfo
  } from '$lib/stores/authStore';

  // Favorites state management
  import { loadFavorites } from '$lib/stores/favoritesStore';

  // Navigation state management
  import {
    subscribe as subscribeNav,
    navigateTo as navTo,
    goBack as navGoBack,
    goForward as navGoForward,
    selectPlaylist,
    getNavigationState,
    type ViewType,
    type NavigationState
  } from '$lib/stores/navigationStore';

  // Player state management
  import {
    subscribe as subscribePlayer,
    setCurrentTrack,
    setIsPlaying,
    setIsFavorite,
    setIsSkipping,
    setQueueEnded,
    setOnTrackEnded,
    togglePlay,
    seek as playerSeek,
    setVolume as playerSetVolume,
    setPendingSessionRestore,
    startPolling,
    stopPolling,
    reset as resetPlayer,
    getPlayerState,
    type PlayingTrack,
    type PlayerState
  } from '$lib/stores/playerStore';

  // Queue state management
  import {
    subscribe as subscribeQueue,
    syncQueueState,
    toggleShuffle as queueToggleShuffle,
    toggleRepeat as queueToggleRepeat,
    addToQueueNext,
    addToQueue,
    addTracksToQueue,
    setQueue,
    clearQueue,
    playQueueIndex,
    nextTrack,
    previousTrack,
    moveQueueTrack,
    setLocalTrackIds,
    clearLocalTrackIds,
    isLocalTrack,
    getBackendQueueState,
    getQueueState,
    type QueueTrack,
    type BackendQueueTrack,
    type RepeatMode
  } from '$lib/stores/queueStore';

  // Types
  import type {
    QobuzTrack,
    QobuzAlbum,
    QobuzArtist,
    Track,
    AlbumDetail,
    ArtistDetail,
    PlaylistTrack,
    DisplayTrack,
    LocalLibraryTrack,
    SongLinkResponse
  } from '$lib/types';

  // Adapters
  import {
    convertQobuzAlbum,
    convertQobuzArtist,
    formatDuration
  } from '$lib/adapters/qobuzAdapters';

  // Services
  import {
    playTrack,
    setToastCallback as setPlaybackToastCallback,
    checkTrackFavorite,
    toggleTrackFavorite,
    showTrackNotification,
    updateLastfmNowPlaying,
    cleanup as cleanupPlayback
  } from '$lib/services/playbackService';

  import {
    setToastCallback as setTrackActionsToastCallback,
    queueTrackNext,
    queueTrackLater,
    buildQueueTrackFromQobuz,
    buildQueueTrackFromAlbumTrack,
    buildQueueTrackFromPlaylistTrack,
    buildQueueTrackFromLocalTrack,
    queueQobuzTrackNext,
    queueQobuzTrackLater,
    queuePlaylistTrackNext,
    queuePlaylistTrackLater,
    queueLocalTrackNext,
    queueLocalTrackLater,
    handleAddToFavorites,
    addToPlaylist,
    shareQobuzTrackLink,
    shareSonglinkTrack
  } from '$lib/services/trackActions';

  // App bootstrap
  import { bootstrapApp } from '$lib/app/bootstrap';

  // Recommendation scoring
  import { trainScores } from '$lib/services/recoService';

  // Session persistence
  import {
    loadSessionState,
    saveSessionState,
    saveSessionVolume,
    saveSessionPlaybackMode,
    debouncedSavePosition,
    flushPositionSave,
    clearSession,
    type PersistedQueueTrack
  } from '$lib/services/sessionService';

  // Lyrics state management
  import {
    subscribe as subscribeLyrics,
    toggleSidebar as toggleLyricsSidebar,
    startWatching as startLyricsWatching,
    stopWatching as stopLyricsWatching,
    startActiveLineUpdates,
    stopActiveLineUpdates,
    getLyricsState,
    type LyricsLine
  } from '$lib/stores/lyricsStore';

  // Components
  import TitleBar from '$lib/components/TitleBar.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import AboutModal from '$lib/components/AboutModal.svelte';
  import NowPlayingBar from '$lib/components/NowPlayingBar.svelte';
  import Toast from '$lib/components/Toast.svelte';

  // Views
  import LoginView from '$lib/components/views/LoginView.svelte';
  import HomeView from '$lib/components/views/HomeView.svelte';
  import SearchView from '$lib/components/views/SearchView.svelte';
  import SettingsView from '$lib/components/views/SettingsView.svelte';
  import AlbumDetailView from '$lib/components/views/AlbumDetailView.svelte';
  import ArtistDetailView from '$lib/components/views/ArtistDetailView.svelte';
  import PlaylistDetailView from '$lib/components/views/PlaylistDetailView.svelte';
  import FavoritesView from '$lib/components/views/FavoritesView.svelte';
  import LocalLibraryView from '$lib/components/views/LocalLibraryView.svelte';
  import PlaylistManagerView from '$lib/components/views/PlaylistManagerView.svelte';

  // Overlays
  import QueuePanel from '$lib/components/QueuePanel.svelte';
  import ExpandedPlayer from '$lib/components/ExpandedPlayer.svelte';
  import FocusMode from '$lib/components/FocusMode.svelte';
  import PlaylistModal from '$lib/components/PlaylistModal.svelte';
  import PlaylistImportModal from '$lib/components/PlaylistImportModal.svelte';
  import CastPicker from '$lib/components/CastPicker.svelte';
  import LyricsSidebar from '$lib/components/lyrics/LyricsSidebar.svelte';

  // Auth State (from authStore subscription)
  let isLoggedIn = $state(false);
  let userInfo = $state<UserInfo | null>(null);

  // View State (from navigationStore subscription)
  let activeView = $state<ViewType>('home');
  let selectedPlaylistId = $state<number | null>(null);
  // Album and Artist data are fetched, so kept local
  let selectedAlbum = $state<AlbumDetail | null>(null);
  let selectedArtist = $state<ArtistDetail | null>(null);

  // Overlay States (from uiStore subscription)
  let isQueueOpen = $state(false);
  let isFullScreenOpen = $state(false);
  let isFocusModeOpen = $state(false);
  let isCastPickerOpen = $state(false);

  // Playlist Modal State (from uiStore subscription)
  let isPlaylistModalOpen = $state(false);
  let playlistModalMode = $state<'create' | 'edit' | 'addTrack'>('create');
  let playlistModalTrackIds = $state<number[]>([]);
  let isPlaylistImportOpen = $state(false);
  let isAboutModalOpen = $state(false);
  let userPlaylists = $state<{ id: number; name: string; tracks_count: number }[]>([]);

  // Sidebar reference for refreshing playlists
  let sidebarRef: { getPlaylists: () => { id: number; name: string; tracks_count: number }[], refreshPlaylists: () => void } | undefined;

  // Playback State (from playerStore subscription)
  let currentTrack = $state<PlayingTrack | null>(null);
  let isPlaying = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let volume = $state(75);
  let isFavorite = $state(false);

  // Queue/Shuffle State (from queueStore subscription)
  let isShuffle = $state(false);
  let repeatMode = $state<RepeatMode>('off');
  let queue = $state<QueueTrack[]>([]);
  let queueTotalTracks = $state(0);

  // Toast State (from store subscription)
  let toast = $state<ToastData | null>(null);

  // Lyrics State (from lyricsStore subscription)
  let lyricsStatus = $state<'idle' | 'loading' | 'loaded' | 'error' | 'not_found'>('idle');
  let lyricsError = $state<string | null>(null);
  let lyricsLines = $state<LyricsLine[]>([]);
  let lyricsIsSynced = $state(false);
  let lyricsActiveIndex = $state(-1);
  let lyricsActiveProgress = $state(0);
  let lyricsSidebarVisible = $state(false);

  // Navigation wrapper (keeps debug logging)
  function navigateTo(view: string) {
    console.log('navigateTo called with:', view, 'current activeView:', activeView);
    navTo(view as ViewType);
  }

  async function handleAlbumClick(albumId: string) {
    try {
      showToast('Loading album...', 'info');
      const album = await invoke<QobuzAlbum>('get_album', { albumId });
      console.log('Album details:', album);

      selectedAlbum = convertQobuzAlbum(album);
      navigateTo('album');
      hideToast();
    } catch (err) {
      console.error('Failed to load album:', err);
      showToast('Failed to load album', 'error');
    }
  }


  async function handleArtistClick(artistId: number) {
    try {
      showToast('Loading artist...', 'info');
      const artist = await invoke<QobuzArtist>('get_artist', { artistId });
      console.log('Artist details:', artist);

      selectedArtist = convertQobuzArtist(artist);
      navigateTo('artist');
      hideToast();
    } catch (err) {
      console.error('Failed to load artist:', err);
      showToast('Failed to load artist', 'error');
    }
  }


  // Album-specific queue track builder (needs selectedAlbum context)
  function buildAlbumQueueTrack(track: Track): BackendQueueTrack {
    return buildQueueTrackFromAlbumTrack(
      track,
      selectedAlbum?.artwork || '',
      selectedAlbum?.artist || 'Unknown Artist',
      selectedAlbum?.title || ''
    );
  }

  // Playback Functions - QobuzTrack from search results
  async function handleTrackPlay(track: QobuzTrack) {
    console.log('Playing track:', track);

    const artwork = track.album?.image?.large || track.album?.image?.thumbnail || track.album?.image?.small || '';
    const quality = track.hires_streamable && track.maximum_bit_depth && track.maximum_sampling_rate
      ? `${track.maximum_bit_depth}bit/${track.maximum_sampling_rate}kHz`
      : track.hires_streamable
        ? 'Hi-Res'
        : '-';

    await playTrack({
      id: track.id,
      title: track.title,
      artist: track.performer?.name || 'Unknown Artist',
      album: track.album?.title || '',
      artwork,
      duration: track.duration,
      quality,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate,
      albumId: track.album?.id,
      artistId: track.performer?.id
    });
  }

  // Handle track play from album detail view
  async function handleAlbumTrackPlay(track: Track) {
    console.log('Playing album track:', track);

    const artwork = selectedAlbum?.artwork || '';
    const quality = track.hires && track.bitDepth && track.samplingRate
      ? `${track.bitDepth}bit/${track.samplingRate}kHz`
      : track.hires
        ? 'Hi-Res'
        : '-';

    // Build queue from album tracks before playing
    if (selectedAlbum?.tracks) {
      const trackIndex = selectedAlbum.tracks.findIndex(t => t.id === track.id);
      const queueTracks: BackendQueueTrack[] = selectedAlbum.tracks.map(t => ({
        id: t.id,
        title: t.title,
        artist: t.artist || selectedAlbum?.artist || 'Unknown Artist',
        album: selectedAlbum?.title || '',
        duration_secs: t.durationSeconds,
        artwork_url: artwork || null,
        hires: t.hires ?? false,
        bit_depth: t.bitDepth ?? null,
        sample_rate: t.samplingRate ?? null
      }));

      // Set the queue starting at the clicked track
      await setQueue(queueTracks, trackIndex >= 0 ? trackIndex : 0, true);
    }

    // Play track using unified service
    await playTrack({
      id: track.id,
      title: track.title,
      artist: track.artist || selectedAlbum?.artist || 'Unknown Artist',
      album: selectedAlbum?.title || '',
      artwork,
      duration: track.durationSeconds,
      quality,
      bitDepth: track.bitDepth,
      samplingRate: track.samplingRate,
      albumId: selectedAlbum?.id,
      artistId: track.artistId
    });
  }

  // Playback controls (delegating to playerStore)
  function handleSeek(time: number) {
    playerSeek(time);
  }

  function handleVolumeChange(newVolume: number) {
    playerSetVolume(newVolume);
  }

  async function toggleShuffle() {
    const result = await queueToggleShuffle();
    if (result.success) {
      showToast(result.enabled ? 'Shuffle enabled' : 'Shuffle disabled', 'info');
    }
  }

  async function toggleRepeat() {
    const result = await queueToggleRepeat();
    if (result.success) {
      const messages: Record<RepeatMode, string> = { off: 'Repeat off', all: 'Repeat all', one: 'Repeat one' };
      showToast(messages[result.mode], 'info');
    }
  }

  async function toggleFavorite() {
    if (!currentTrack) return;

    const result = await toggleTrackFavorite(currentTrack.id, isFavorite);
    if (result.success) {
      setIsFavorite(result.isFavorite);
      showToast(result.isFavorite ? 'Added to favorites' : 'Removed from favorites', 'success');
    } else {
      showToast('Failed to update favorites', 'error');
    }
  }

  // Skip track handlers - wired to backend queue via queueStore
  async function handleSkipBack() {
    const playerState = getPlayerState();
    if (!playerState.currentTrack || playerState.isSkipping) return;
    // If more than 3 seconds in, restart track; otherwise go to previous
    if (playerState.currentTime > 3) {
      handleSeek(0);
      return;
    }

    setIsSkipping(true);
    try {
      const prevTrack = await previousTrack();
      if (prevTrack) {
        await playQueueTrack(prevTrack);
      } else {
        // No previous track, just restart
        handleSeek(0);
      }
    } catch (err) {
      console.error('Failed to go to previous track:', err);
      showToast('Failed to go to previous track', 'error');
    } finally {
      setIsSkipping(false);
    }
  }

  async function handleSkipForward() {
    const playerState = getPlayerState();
    if (!playerState.currentTrack || playerState.isSkipping) return;

    setIsSkipping(true);
    try {
      const nextTrackResult = await nextTrack();
      if (nextTrackResult) {
        await playQueueTrack(nextTrackResult);
      } else {
        // No next track - stop playback
        await invoke('stop_playback');
        setIsPlaying(false);
        showToast('Queue ended', 'info');
      }
    } catch (err) {
      console.error('Failed to go to next track:', err);
      showToast('Failed to go to next track', 'error');
    } finally {
      setIsSkipping(false);
    }
  }

  // Helper to play a track from the queue
  async function playQueueTrack(track: BackendQueueTrack) {
    const isLocal = isLocalTrack(track.id);

    // Reset queue ended flag when playing a new track
    setQueueEnded(false);

    // Determine quality string from track data
    const quality = isLocal
      ? 'Local'
      : track.bit_depth && track.sample_rate
        ? `${track.bit_depth}bit/${track.sample_rate}kHz`
        : track.hires
          ? 'Hi-Res'
          : '-';

    // Play track using unified service
    await playTrack({
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork: track.artwork_url || '',
      duration: track.duration_secs,
      quality,
      bitDepth: track.bit_depth ?? undefined,
      samplingRate: track.sample_rate ?? undefined,
      isLocal
    }, { isLocal, showLoadingToast: false });
  }

  // Play a specific track from the queue panel
  async function handleQueueTrackPlay(trackId: string) {
    try {
      // Find the index in the queue
      const queueState = await getBackendQueueState();
      if (!queueState) {
        showToast('Failed to play track', 'error');
        return;
      }

      const allTracks = [queueState.current_track, ...queueState.upcoming].filter(Boolean) as BackendQueueTrack[];
      const trackIndex = allTracks.findIndex(t => String(t.id) === trackId);

      if (trackIndex >= 0) {
        // If it's the current track (index 0), just ensure it's playing
        if (trackIndex === 0 && queueState.current_index !== null) {
          // Already current, nothing to do
          return;
        }

        // Play by index (accounting for current track offset)
        const actualIndex = queueState.current_index !== null
          ? queueState.current_index + trackIndex
          : trackIndex;

        const track = await playQueueIndex(actualIndex);
        if (track) {
          await playQueueTrack(track);
        }
      }
    } catch (err) {
      console.error('Failed to play queue track:', err);
      showToast('Failed to play track', 'error');
    }
  }

  // Clear the queue
  async function handleClearQueue() {
    const success = await clearQueue();
    if (success) {
      showToast('Queue cleared', 'info');
    } else {
      showToast('Failed to clear queue', 'error');
    }
  }

  // Reorder tracks in the queue
  async function handleQueueReorder(fromIndex: number, toIndex: number) {
    const success = await moveQueueTrack(fromIndex, toIndex);
    if (!success) {
      showToast('Failed to reorder queue', 'error');
    }
  }

  // Play all tracks from album (starting from first track)
  async function handlePlayAllAlbum() {
    if (!selectedAlbum?.tracks?.length) return;
    const firstTrack = selectedAlbum.tracks[0];
    await handleAlbumTrackPlay(firstTrack);
  }

  // Shuffle play all tracks from album
  async function handleShuffleAlbum() {
    if (!selectedAlbum?.tracks?.length) return;

    // Set shuffle mode first
    try {
      await invoke('set_shuffle', { enabled: true });
      isShuffle = true;
    } catch (err) {
      console.error('Failed to enable shuffle:', err);
    }

    // Then play from first track (queue will be shuffled)
    await handlePlayAllAlbum();
    showToast('Shuffle play enabled', 'info');
  }

  // Add all album tracks next in queue (after current track)
  async function handleAddAlbumToQueueNext() {
    if (!selectedAlbum?.tracks?.length) return;

    const artwork = selectedAlbum.artwork || '';
    // Add in reverse order so first track ends up right after current
    for (let i = selectedAlbum.tracks.length - 1; i >= 0; i--) {
      const t = selectedAlbum.tracks[i];
      queueTrackNext({
        id: t.id,
        title: t.title,
        artist: t.artist || selectedAlbum?.artist || 'Unknown Artist',
        album: selectedAlbum?.title || '',
        duration_secs: t.durationSeconds,
        artwork_url: artwork || null,
        hires: t.hires ?? false,
        bit_depth: t.bitDepth ?? null,
        sample_rate: t.samplingRate ?? null
      });
    }
    showToast(`Playing ${selectedAlbum.tracks.length} tracks next`, 'success');
  }

  // Add all album tracks to end of queue
  async function handleAddAlbumToQueueLater() {
    if (!selectedAlbum?.tracks?.length) return;

    const artwork = selectedAlbum.artwork || '';
    const queueTracks: BackendQueueTrack[] = selectedAlbum.tracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.artist || selectedAlbum?.artist || 'Unknown Artist',
      album: selectedAlbum?.title || '',
      duration_secs: t.durationSeconds,
      artwork_url: artwork || null,
      hires: t.hires ?? false,
      bit_depth: t.bitDepth ?? null,
      sample_rate: t.samplingRate ?? null
    }));

    const success = await addTracksToQueue(queueTracks);
    if (success) {
      showToast(`Added ${queueTracks.length} tracks to queue`, 'success');
    } else {
      showToast('Failed to add to queue', 'error');
    }
  }

  // Share album Qobuz link
  function shareAlbumQobuzLink() {
    if (!selectedAlbum?.id) return;
    const url = `https://play.qobuz.com/album/${selectedAlbum.id}`;
    writeText(url);
    showToast('Album link copied to clipboard', 'success');
  }

  // Share album via song.link
  async function shareAlbumSonglink() {
    if (!selectedAlbum?.id) return;
    const qobuzUrl = `https://play.qobuz.com/album/${selectedAlbum.id}`;
    const songlinkUrl = `https://song.link/${encodeURIComponent(qobuzUrl)}`;
    writeText(songlinkUrl);
    showToast('Song.link copied to clipboard', 'success');
  }

  function handleAlbumTrackPlayNext(track: Track) {
    queueTrackNext(buildAlbumQueueTrack(track));
  }

  function handleAlbumTrackPlayLater(track: Track) {
    queueTrackLater(buildAlbumQueueTrack(track));
  }

  // Download handlers
  async function handleTrackDownload(track: Track) {
    try {
      await downloadTrack({
        id: track.id,
        title: track.title,
        artist: track.artist || selectedAlbum?.artist || 'Unknown',
        album: selectedAlbum?.title,
        albumId: selectedAlbum?.id,
        durationSecs: track.durationSeconds,
        quality: track.quality || '-',
        bitDepth: track.bitDepth,
        sampleRate: track.samplingRate,
      });
      showToast(`Downloading "${track.title}"`, 'info');
    } catch (err) {
      console.error('Failed to start download:', err);
      showToast('Failed to start download', 'error');
    }
  }

  async function handleTrackRemoveDownload(trackId: number) {
    try {
      await removeDownload(trackId);
      showToast('Removed from downloads', 'info');
    } catch (err) {
      console.error('Failed to remove download:', err);
      showToast('Failed to remove download', 'error');
    }
  }

  async function handleDownloadAlbum() {
    if (!selectedAlbum) return;

    const tracksToDownload = selectedAlbum.tracks.filter(track => {
      const status = getDownloadState(track.id).status;
      return status === 'none' || status === 'failed';
    });

    if (tracksToDownload.length === 0) {
      showToast('All tracks already downloaded', 'info');
      return;
    }

    showToast(`Downloading ${tracksToDownload.length} tracks from "${selectedAlbum.title}"`, 'info');

    for (const track of tracksToDownload) {
      try {
        await downloadTrack({
          id: track.id,
          title: track.title,
          artist: track.artist || selectedAlbum.artist || 'Unknown',
          album: selectedAlbum.title,
          albumId: selectedAlbum.id,
          durationSecs: track.durationSeconds,
          quality: track.quality || '-',
          bitDepth: track.bitDepth,
          sampleRate: track.samplingRate,
        });
      } catch (err) {
        console.error(`Failed to queue download for "${track.title}":`, err);
      }
    }
  }

  function getTrackDownloadStatus(trackId: number) {
    // Access downloadStateVersion to trigger reactivity
    void downloadStateVersion;
    return getDownloadState(trackId);
  }

  async function handleDisplayTrackDownload(track: PlaylistTrack) {
    try {
      const quality = track.bitDepth && track.samplingRate
        ? `${track.bitDepth}bit/${track.samplingRate}kHz`
        : track.hires
          ? 'Hi-Res'
          : '-';
      await downloadTrack({
        id: track.id,
        title: track.title,
        artist: track.artist || 'Unknown',
        album: track.album,
        albumId: track.albumId,
        durationSecs: track.durationSeconds,
        quality,
        bitDepth: track.bitDepth,
        sampleRate: track.samplingRate,
      });
      showToast(`Downloading "${track.title}"`, 'info');
    } catch (err) {
      console.error('Failed to start download:', err);
      showToast('Failed to start download', 'error');
    }
  }

  /**
   * Handle playback of DisplayTrack (used by ArtistDetailView, PlaylistDetailView, FavoritesView)
   * This is fire-and-forget to match view callback signatures
   */
  function handleDisplayTrackPlay(track: DisplayTrack): void {
    console.log('Playing display track:', track);

    // Determine quality string:
    // - If we have exact bitDepth/samplingRate, show them
    // - If hires flag is true but no exact values, show "Hi-Res"
    // - Otherwise show "-" (unknown - will be updated when streaming returns quality info)
    // TODO: Update quality when backend returns actual streaming quality
    const quality = track.bitDepth && track.samplingRate
      ? `${track.bitDepth}bit/${track.samplingRate}kHz`
      : track.hires
        ? 'Hi-Res'
        : '-';

    // Fire-and-forget async call
    playTrack({
      id: track.id,
      title: track.title,
      artist: track.artist || 'Unknown Artist',
      album: track.album || 'Playlist',
      artwork: track.albumArt || '',
      duration: track.durationSeconds,
      quality,
      bitDepth: track.bitDepth,
      samplingRate: track.samplingRate,
      albumId: track.albumId,
      artistId: track.artistId
    });
  }

  async function handleLocalTrackPlay(track: LocalLibraryTrack) {
    console.log('Playing local track:', track);

    const artwork = track.artwork_path ? `asset://localhost/${encodeURIComponent(track.artwork_path)}` : '';
    const quality = track.bit_depth && track.sample_rate
      ? (track.bit_depth >= 24 || track.sample_rate > 48000
        ? `${track.bit_depth}bit/${track.sample_rate / 1000}kHz`
        : track.format)
      : track.format;

    await playTrack({
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork,
      duration: track.duration_secs,
      quality,
      bitDepth: track.bit_depth,
      samplingRate: track.sample_rate ? Math.round(track.sample_rate / 1000) : undefined,
      isLocal: true
    }, { isLocal: true });
  }

  // Handle setting queue from local library (tracks need different playback command)
  function handleSetLocalQueue(trackIds: number[]) {
    // Set local track IDs via queueStore
    setLocalTrackIds(trackIds);
  }

  // Playlist Modal Functions
  function openCreatePlaylist() {
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    openPlaylistModal('create', []);
  }

  function openAddToPlaylist(trackIds: number[]) {
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    openPlaylistModal('addTrack', trackIds);
  }

  function handlePlaylistCreated() {
    if (playlistModalMode === 'addTrack') {
      showToast('Track added to playlist', 'success');
    } else {
      showToast('Playlist created', 'success');
    }
    sidebarRef?.refreshPlaylists();
  }

  function openImportPlaylist() {
    openPlaylistImport();
  }

  function handlePlaylistImported(summary: { qobuz_playlist_id?: number | null }) {
    sidebarRef?.refreshPlaylists();
    if (summary.qobuz_playlist_id) {
      selectPlaylist(summary.qobuz_playlist_id);
    }
  }

  // Auth Handlers
  async function handleLoginSuccess(info: UserInfo) {
    setLoggedIn(info);
    showToast(`Welcome, ${info.userName}!`, 'success');

    // Train recommendation scores in background (fire-and-forget)
    trainScores().then(() => {
      console.log('[Reco] Scores trained after login');
    }).catch(err => {
      console.debug('[Reco] Score training failed:', err);
    });

    // Restore previous session if available
    try {
      const session = await loadSessionState();
      if (session && session.queue_tracks.length > 0) {
        console.log('[Session] Restoring previous session...');

        // Restore queue
        const tracks: BackendQueueTrack[] = session.queue_tracks.map(t => ({
          id: t.id,
          title: t.title,
          artist: t.artist,
          album: t.album,
          duration_secs: t.duration_secs,
          artwork_url: t.artwork_url,
          hires: t.hires ?? false,
          bit_depth: t.bit_depth ?? null,
          sample_rate: t.sample_rate ?? null
        }));

        await setQueue(tracks, session.current_index ?? 0, true);

        // Restore shuffle/repeat mode
        if (session.shuffle_enabled) {
          await invoke('set_shuffle', { enabled: true });
        }
        if (session.repeat_mode !== 'off') {
          await invoke('set_repeat', { mode: session.repeat_mode });
        }

        // Restore volume
        playerSetVolume(Math.round(session.volume * 100));

        // If there was a track playing, restore it (paused)
        if (session.current_index !== null && tracks[session.current_index]) {
          const track = tracks[session.current_index];
          showToast(`Restored: ${track.title}`, 'info');

          // Set the current track info without auto-playing
          setCurrentTrack({
            id: track.id,
            title: track.title,
            artist: track.artist,
            album: track.album,
            artwork: track.artwork_url || '',
            duration: track.duration_secs,
            quality: 'Restored',
          });

          // Mark that this track needs to be loaded when user presses play
          setPendingSessionRestore(track.id, session.current_position_secs);
          console.log(`[Session] Track ${track.id} pending load, will resume at ${session.current_position_secs}s`);
        }

        console.log('[Session] Session restored successfully');
      }
    } catch (err) {
      console.error('[Session] Failed to restore session:', err);
    }
  }

  async function handleLogout() {
    try {
      await invoke('logout');
      // Clear saved credentials from keyring
      try {
        await invoke('clear_saved_credentials');
        console.log('Credentials cleared from keyring');
      } catch (clearErr) {
        console.error('Failed to clear credentials:', clearErr);
        // Don't block logout if clearing fails
      }
      // Clear session state
      await clearSession();
      setLoggedOut();
      currentTrack = null;
      isPlaying = false;
      showToast('Logged out successfully', 'info');
    } catch (err) {
      console.error('Logout error:', err);
      showToast('Failed to logout', 'error');
    }
  }

  // Save session state before window closes
  async function saveSessionBeforeClose() {
    if (!isLoggedIn || !currentTrack) return;

    try {
      // Get current queue state from backend
      const queueState = await getBackendQueueState();
      if (!queueState) return;

      // Build persisted queue tracks
      const allTracks: PersistedQueueTrack[] = [];
      if (queueState.current_track) {
        allTracks.push({
          id: queueState.current_track.id,
          title: queueState.current_track.title,
          artist: queueState.current_track.artist,
          album: queueState.current_track.album,
          duration_secs: queueState.current_track.duration_secs,
          artwork_url: queueState.current_track.artwork_url
        });
      }
      for (const track of queueState.upcoming) {
        allTracks.push({
          id: track.id,
          title: track.title,
          artist: track.artist,
          album: track.album,
          duration_secs: track.duration_secs,
          artwork_url: track.artwork_url
        });
      }

      await saveSessionState(
        allTracks,
        queueState.current_index,
        Math.floor(currentTime),
        volume / 100,
        isShuffle,
        repeatMode,
        isPlaying
      );
      console.log('[Session] Session saved on close');
    } catch (err) {
      console.error('[Session] Failed to save session on close:', err);
    }
  }

  // Keyboard Shortcuts
  function handleKeydown(e: KeyboardEvent) {
    if (!isLoggedIn) return;
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) {
      return;
    }

    switch (e.key) {
      case ' ':
        e.preventDefault();
        togglePlay();
        break;
      case 'ArrowLeft':
        if (e.altKey) {
          e.preventDefault();
          navGoBack();
        } else if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          handleSkipBack();
        }
        break;
      case 'ArrowRight':
        if (e.altKey) {
          e.preventDefault();
          navGoForward();
        } else if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          handleSkipForward();
        }
        break;
      case 'f':
        if (!e.ctrlKey && !e.metaKey) {
          toggleFocusMode();
        }
        break;
      case 'q':
        toggleQueue();
        break;
      case 'Escape':
        handleUIEscape();
        break;
    }
  }

  // Playback state polling - managed by playerStore
  // Start/stop polling based on whether there's a current track
  $effect(() => {
    if (currentTrack) {
      startPolling();
    } else {
      stopPolling();
    }

    return () => {
      stopPolling();
    };
  });

  // Download state update trigger
  let downloadStateVersion = $state(0);

  onMount(() => {
    // Bootstrap app (theme, mouse nav, Last.fm restore)
    const { cleanup: cleanupBootstrap } = bootstrapApp();

    // Keyboard navigation
    document.addEventListener('keydown', handleKeydown);

    // Session save on window close/hide
    const handleBeforeUnload = () => {
      saveSessionBeforeClose();
    };
    window.addEventListener('beforeunload', handleBeforeUnload);

    // Also save when visibility changes (app goes to background)
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden') {
        saveSessionBeforeClose();
      }
    };
    document.addEventListener('visibilitychange', handleVisibilityChange);

    // Initialize download states
    initDownloadStates();
    startDownloadEventListeners();

    // Load favorites for global state
    loadFavorites();

    // Set up service toast callbacks
    setPlaybackToastCallback(showToast);
    setTrackActionsToastCallback(showToast);

    // Subscribe to download state changes to trigger reactivity
    const unsubscribeDownloads = subscribeDownloads(() => {
      downloadStateVersion++;
    });

    // Subscribe to toast state changes
    const unsubscribeToast = subscribeToast((newToast) => {
      toast = newToast;
    });

    // Subscribe to UI state changes
    const unsubscribeUI = subscribeUI(() => {
      const uiState = getUIState();
      isQueueOpen = uiState.isQueueOpen;
      isFullScreenOpen = uiState.isFullScreenOpen;
      isFocusModeOpen = uiState.isFocusModeOpen;
      isCastPickerOpen = uiState.isCastPickerOpen;
      isPlaylistModalOpen = uiState.isPlaylistModalOpen;
      playlistModalMode = uiState.playlistModalMode;
      playlistModalTrackIds = uiState.playlistModalTrackIds;
      isPlaylistImportOpen = uiState.isPlaylistImportOpen;
    });

    // Subscribe to auth state changes
    const unsubscribeAuth = subscribeAuth(() => {
      const authState = getAuthState();
      isLoggedIn = authState.isLoggedIn;
      userInfo = authState.userInfo;
    });

    // Subscribe to navigation state changes
    const unsubscribeNav = subscribeNav(() => {
      const navState = getNavigationState();
      activeView = navState.activeView;
      selectedPlaylistId = navState.selectedPlaylistId;
    });

    // Subscribe to player state changes
    const unsubscribePlayer = subscribePlayer(() => {
      const playerState = getPlayerState();
      currentTrack = playerState.currentTrack;
      isPlaying = playerState.isPlaying;
      currentTime = playerState.currentTime;
      duration = playerState.duration;
      volume = playerState.volume;
      isFavorite = playerState.isFavorite;
    });

    // Subscribe to queue state changes
    const unsubscribeQueue = subscribeQueue(() => {
      const queueState = getQueueState();
      queue = queueState.queue;
      queueTotalTracks = queueState.queueTotalTracks;
      isShuffle = queueState.isShuffle;
      repeatMode = queueState.repeatMode;
    });

    // Subscribe to lyrics state changes
    const unsubscribeLyrics = subscribeLyrics(() => {
      const state = getLyricsState();
      lyricsStatus = state.status;
      lyricsError = state.error;
      lyricsLines = state.lines;
      lyricsIsSynced = state.isSynced;
      lyricsActiveIndex = state.activeIndex;
      lyricsActiveProgress = state.activeProgress;
      lyricsSidebarVisible = state.sidebarVisible;
    });

    // Start lyrics watcher for track changes
    startLyricsWatching();

    // Set up track ended callback for auto-advance
    setOnTrackEnded(async () => {
      const nextTrackResult = await nextTrack();
      if (nextTrackResult) {
        await playQueueTrack(nextTrackResult);
      } else {
        setQueueEnded(true);
      }
    });

    return () => {
      // Save session before cleanup
      saveSessionBeforeClose();
      cleanupBootstrap();
      document.removeEventListener('keydown', handleKeydown);
      window.removeEventListener('beforeunload', handleBeforeUnload);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      stopDownloadEventListeners();
      unsubscribeDownloads();
      unsubscribeToast();
      unsubscribeUI();
      unsubscribeAuth();
      unsubscribeNav();
      unsubscribePlayer();
      unsubscribeQueue();
      unsubscribeLyrics();
      stopLyricsWatching();
      stopActiveLineUpdates();
      stopPolling();
      cleanupPlayback();
    };
  });

  // Sync queue state when opening queue panel
  $effect(() => {
    if (isQueueOpen) {
      syncQueueState();
    }
  });

  // Start/stop lyrics active line updates based on playback state and visibility
  $effect(() => {
    const lyricsVisible = lyricsSidebarVisible || isFocusModeOpen || isFullScreenOpen;
    if (isPlaying && lyricsIsSynced && lyricsVisible) {
      startActiveLineUpdates();
    } else {
      stopActiveLineUpdates();
    }
  });

  // Derived values for NowPlayingBar
  const currentQueueTrack = $derived<QueueTrack | null>(currentTrack ? {
    id: String(currentTrack.id),
    artwork: currentTrack.artwork,
    title: currentTrack.title,
    artist: currentTrack.artist,
    duration: formatDuration(currentTrack.duration)
  } : null);
</script>

{#if !isLoggedIn}
  <LoginView onLoginSuccess={handleLoginSuccess} />
{:else}
  <div class="app">
    <!-- Custom Title Bar (CSD) -->
    <TitleBar />

    <div class="app-body">
    <!-- Sidebar -->
    <Sidebar
      bind:this={sidebarRef}
      {activeView}
      {selectedPlaylistId}
      onNavigate={navigateTo}
      onPlaylistSelect={selectPlaylist}
      onCreatePlaylist={openCreatePlaylist}
      onImportPlaylist={openImportPlaylist}
      onPlaylistManagerClick={() => navigateTo('playlist-manager')}
      onSettingsClick={() => navigateTo('settings')}
      onAboutClick={() => isAboutModalOpen = true}
      onLogout={handleLogout}
      userName={userInfo?.userName || 'User'}
      subscription={userInfo?.subscription || 'Qobuz'}
    />

    <!-- Content Area (main + lyrics sidebar) -->
    <div class="content-area">
    <!-- Main Content -->
    <main class="main-content">
      {#if activeView === 'home'}
        <HomeView
          userName={userInfo?.userName}
          onAlbumClick={handleAlbumClick}
          onArtistClick={handleArtistClick}
          onTrackPlay={handleDisplayTrackPlay}
        />
      {:else if activeView === 'search'}
        <SearchView
          onAlbumClick={handleAlbumClick}
          onTrackPlay={handleTrackPlay}
          onTrackPlayNext={queueQobuzTrackNext}
          onTrackPlayLater={queueQobuzTrackLater}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'settings'}
        <SettingsView
          onBack={navGoBack}
          onLogout={handleLogout}
          userName={userInfo?.userName}
          subscription={userInfo?.subscription}
        />
      {:else if activeView === 'album' && selectedAlbum}
        <AlbumDetailView
          album={selectedAlbum}
          onBack={navGoBack}
          onArtistClick={() => selectedAlbum?.artistId && handleArtistClick(selectedAlbum.artistId)}
          onTrackPlay={handleAlbumTrackPlay}
          onTrackPlayNext={handleAlbumTrackPlayNext}
          onTrackPlayLater={handleAlbumTrackPlayLater}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onPlayAll={handlePlayAllAlbum}
          onShuffleAll={handleShuffleAlbum}
          onPlayAllNext={handleAddAlbumToQueueNext}
          onPlayAllLater={handleAddAlbumToQueueLater}
          onAddTrackToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackDownload={handleTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          getTrackDownloadStatus={getTrackDownloadStatus}
          onDownloadAlbum={handleDownloadAlbum}
          onShareAlbumQobuz={shareAlbumQobuzLink}
          onShareAlbumSonglink={shareAlbumSonglink}
          {downloadStateVersion}
        />
      {:else if activeView === 'artist' && selectedArtist}
        <ArtistDetailView
          artist={selectedArtist}
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queueQobuzTrackNext}
          onTrackPlayLater={queueQobuzTrackLater}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
        />
      {:else if activeView === 'library'}
        <LocalLibraryView
          onTrackPlay={handleLocalTrackPlay}
          onTrackPlayNext={queueLocalTrackNext}
          onTrackPlayLater={queueLocalTrackLater}
          onSetLocalQueue={handleSetLocalQueue}
        />
      {:else if activeView === 'playlist' && selectedPlaylistId}
        <PlaylistDetailView
          playlistId={selectedPlaylistId}
          onBack={navGoBack}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queuePlaylistTrackNext}
          onTrackPlayLater={queuePlaylistTrackLater}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onTrackDownload={handleDisplayTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          getTrackDownloadStatus={getTrackDownloadStatus}
          {downloadStateVersion}
          onLocalTrackPlay={handleLocalTrackPlay}
          onLocalTrackPlayNext={queueLocalTrackNext}
          onLocalTrackPlayLater={queueLocalTrackLater}
          onPlaylistUpdated={() => sidebarRef?.refreshPlaylists()}
          onPlaylistDeleted={() => { sidebarRef?.refreshPlaylists(); navGoBack(); }}
        />
      {:else if activeView === 'favorites'}
        <FavoritesView
          onAlbumClick={handleAlbumClick}
          onTrackPlay={handleDisplayTrackPlay}
          onArtistClick={handleArtistClick}
          onTrackPlayNext={queuePlaylistTrackNext}
          onTrackPlayLater={queuePlaylistTrackLater}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onTrackDownload={handleDisplayTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          getTrackDownloadStatus={getTrackDownloadStatus}
          {downloadStateVersion}
        />
      {:else if activeView === 'playlist-manager'}
        <PlaylistManagerView
          onBack={navGoBack}
          onPlaylistSelect={selectPlaylist}
        />
      {/if}
    </main>

    <!-- Lyrics Sidebar -->
    {#if lyricsSidebarVisible}
      <LyricsSidebar
        title={currentTrack?.title}
        artist={currentTrack?.artist}
        lines={lyricsLines.map(l => ({ text: l.text }))}
        activeIndex={lyricsActiveIndex}
        activeProgress={lyricsActiveProgress}
        isSynced={lyricsIsSynced}
        isLoading={lyricsStatus === 'loading'}
        error={lyricsStatus === 'error' ? lyricsError : (lyricsStatus === 'not_found' ? 'No lyrics found' : null)}
      />
    {/if}
    </div>

    <!-- Now Playing Bar -->
    {#if currentTrack}
      <NowPlayingBar
        artwork={currentTrack.artwork}
        trackTitle={currentTrack.title}
        artist={currentTrack.artist}
        album={currentTrack.album}
        quality={currentTrack.quality}
        bitDepth={currentTrack.bitDepth}
        samplingRate={currentTrack.samplingRate}
        {isPlaying}
        onTogglePlay={togglePlay}
        onSkipBack={handleSkipBack}
        onSkipForward={handleSkipForward}
        {currentTime}
        {duration}
        onSeek={handleSeek}
        {volume}
        onVolumeChange={handleVolumeChange}
        {isShuffle}
        onToggleShuffle={toggleShuffle}
        {repeatMode}
        onToggleRepeat={toggleRepeat}
        {isFavorite}
        onToggleFavorite={toggleFavorite}
        onOpenQueue={openQueue}
        onOpenFullScreen={openFullScreen}
        onCast={openCastPicker}
        onToggleLyrics={toggleLyricsSidebar}
        lyricsActive={lyricsSidebarVisible}
        onArtistClick={() => {
          if (currentTrack?.isLocal) {
            showToast('Local track - search for artist in Search', 'info');
          } else if (currentTrack?.artistId) {
            handleArtistClick(currentTrack.artistId);
          }
        }}
        onAlbumClick={() => {
          if (currentTrack?.isLocal) {
            navigateTo('library');
          } else if (currentTrack?.albumId) {
            handleAlbumClick(currentTrack.albumId);
          }
        }}
      />
    {:else}
      <NowPlayingBar
        onOpenQueue={openQueue}
        onOpenFullScreen={openFullScreen}
        onCast={openCastPicker}
      />
    {/if}
    </div><!-- end app-body -->

    <!-- Queue Panel -->
    <QueuePanel
      isOpen={isQueueOpen}
      onClose={closeQueue}
      currentTrack={currentQueueTrack ?? undefined}
      upcomingTracks={queue}
      onPlayTrack={handleQueueTrackPlay}
      onClearQueue={handleClearQueue}
      onSaveAsPlaylist={() => showToast('Save as playlist coming soon', 'info')}
      onReorderTrack={handleQueueReorder}
    />

    <!-- Expanded Player -->
    {#if currentTrack}
      <ExpandedPlayer
        isOpen={isFullScreenOpen}
        onClose={closeFullScreen}
        artwork={currentTrack.artwork}
        trackTitle={currentTrack.title}
        artist={currentTrack.artist}
        album={currentTrack.album}
        quality={currentTrack.quality}
        qualityLevel={currentTrack.quality.includes('24') ? 5 : 3}
        bitDepth={currentTrack.bitDepth}
        samplingRate={currentTrack.samplingRate}
        {isPlaying}
        onTogglePlay={togglePlay}
        onSkipBack={handleSkipBack}
        onSkipForward={handleSkipForward}
        {currentTime}
        {duration}
        onSeek={handleSeek}
        {volume}
        onVolumeChange={handleVolumeChange}
        {isShuffle}
        onToggleShuffle={toggleShuffle}
        {repeatMode}
        onToggleRepeat={toggleRepeat}
        {isFavorite}
        onToggleFavorite={toggleFavorite}
        onOpenQueue={() => {
          closeFullScreen();
          openQueue();
        }}
        onOpenFocusMode={() => {
          closeFullScreen();
          openFocusMode();
        }}
        onCast={openCastPicker}
        lyricsLines={lyricsLines.map(l => ({ text: l.text }))}
        lyricsActiveIndex={lyricsActiveIndex}
        lyricsActiveProgress={lyricsActiveProgress}
        lyricsSynced={lyricsIsSynced}
        lyricsLoading={lyricsStatus === 'loading'}
        lyricsError={lyricsStatus === 'error' ? lyricsError : (lyricsStatus === 'not_found' ? 'No lyrics found' : null)}
      />
    {/if}

    <!-- Focus Mode -->
    {#if currentTrack}
      <FocusMode
        isOpen={isFocusModeOpen}
        onClose={closeFocusMode}
        artwork={currentTrack.artwork}
        trackTitle={currentTrack.title}
        artist={currentTrack.artist}
        {isPlaying}
        onTogglePlay={togglePlay}
        onSkipBack={handleSkipBack}
        onSkipForward={handleSkipForward}
        {currentTime}
        {duration}
        onSeek={handleSeek}
        lyricsLines={lyricsLines.map(l => ({ text: l.text }))}
        lyricsActiveIndex={lyricsActiveIndex}
        lyricsActiveProgress={lyricsActiveProgress}
        lyricsSynced={lyricsIsSynced}
        lyricsLoading={lyricsStatus === 'loading'}
        lyricsError={lyricsStatus === 'error' ? lyricsError : (lyricsStatus === 'not_found' ? 'No lyrics found' : null)}
      />
    {/if}

    <!-- Toast -->
    {#if toast}
      <Toast
        message={toast.message}
        type={toast.type}
        onClose={hideToast}
      />
    {/if}

    <!-- Playlist Modal -->
    <PlaylistModal
      isOpen={isPlaylistModalOpen}
      mode={playlistModalMode}
      trackIds={playlistModalTrackIds}
      {userPlaylists}
      onClose={closePlaylistModal}
      onSuccess={handlePlaylistCreated}
    />

    <!-- Playlist Import Modal -->
    <PlaylistImportModal
      isOpen={isPlaylistImportOpen}
      onClose={closePlaylistImport}
      onSuccess={handlePlaylistImported}
    />

    <!-- About Modal -->
    <AboutModal
      isOpen={isAboutModalOpen}
      onClose={() => isAboutModalOpen = false}
    />

    <!-- Cast Picker -->
    <CastPicker
      isOpen={isCastPickerOpen}
      onClose={closeCastPicker}
      onConnect={(deviceId) => {
        showToast(`Connected to device`, 'success');
      }}
    />
  </div>
{/if}

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    background-color: var(--bg-primary);
  }

  .app-body {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .content-area {
    display: flex;
    flex: 1;
    min-width: 0;
    height: calc(100vh - 136px); /* 104px NowPlayingBar + 32px TitleBar */
    overflow: hidden;
  }

  .main-content {
    flex: 1;
    min-width: 0;
    height: calc(100vh - 136px); /* 104px NowPlayingBar + 32px TitleBar */
    overflow: hidden; /* Views handle their own scrolling */
    padding-right: 8px; /* Gap between scrollbar and window edge */
  }

</style>
