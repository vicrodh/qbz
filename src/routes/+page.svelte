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
    LocalLibraryTrack,
    SongLinkResponse
  } from '$lib/types';

  // Components
  import Sidebar from '$lib/components/Sidebar.svelte';
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

  // Overlays
  import QueuePanel from '$lib/components/QueuePanel.svelte';
  import FullScreenNowPlaying from '$lib/components/FullScreenNowPlaying.svelte';
  import FocusMode from '$lib/components/FocusMode.svelte';
  import PlaylistModal from '$lib/components/PlaylistModal.svelte';
  import CastPicker from '$lib/components/CastPicker.svelte';

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

  // Navigation Functions (using store)
  function navigateTo(view: string) {
    console.log('navigateTo called with:', view, 'current activeView:', activeView);
    navTo(view as ViewType);
  }

  function goBack() {
    navGoBack();
  }

  function goForward() {
    navGoForward();
  }

  function handlePlaylistSelect(playlistId: number) {
    selectPlaylist(playlistId);
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

  function convertQobuzAlbum(album: QobuzAlbum): AlbumDetail {
    const artwork = album.image?.large || album.image?.thumbnail || album.image?.small || '';
    const quality = album.hires_streamable && album.maximum_bit_depth && album.maximum_sampling_rate
      ? `${album.maximum_bit_depth}-Bit / ${album.maximum_sampling_rate} kHz`
      : 'CD Quality';

    return {
      id: album.id,
      artwork,
      title: album.title,
      artist: album.artist?.name || 'Unknown Artist',
      artistId: album.artist?.id,
      year: album.release_date_original?.split('-')[0] || '',
      label: album.label?.name || '',
      genre: album.genre?.name || '',
      quality,
      trackCount: album.tracks_count || album.tracks?.items?.length || 0,
      duration: formatDurationMinutes(album.duration || 0),
      tracks: album.tracks?.items?.map((track, index) => ({
        id: track.id,
        number: index + 1,
        title: track.title,
        artist: track.performer?.name,
        duration: formatDuration(track.duration),
        durationSeconds: track.duration,
        quality: track.hires_streamable ? 'Hi-Res' : 'CD',
        hires: track.hires_streamable,
        bitDepth: track.maximum_bit_depth,
        samplingRate: track.maximum_sampling_rate,
        albumId: album.id,
        artistId: track.performer?.id ?? album.artist?.id,
        isrc: track.isrc
      })) || []
    };
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

  function convertQobuzArtist(artist: QobuzArtist): ArtistDetail {
    const image = artist.image?.large || artist.image?.thumbnail || artist.image?.small;

    return {
      id: artist.id,
      name: artist.name,
      image,
      albumsCount: artist.albums_count,
      biography: artist.biography,
      albums: artist.albums?.items?.map(album => {
        const artwork = album.image?.large || album.image?.thumbnail || album.image?.small || '';
        const quality = album.hires_streamable && album.maximum_bit_depth && album.maximum_sampling_rate
          ? `${album.maximum_bit_depth}bit/${album.maximum_sampling_rate}kHz`
          : 'CD Quality';
        return {
          id: album.id,
          title: album.title,
          artwork,
          year: album.release_date_original?.split('-')[0],
          quality
        };
      }) || [],
      totalAlbums: artist.albums?.total || artist.albums_count || 0
    };
  }

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function formatDurationMinutes(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  async function copyToClipboard(text: string, successMessage: string) {
    try {
      await writeText(text);
      showToast(successMessage, 'success');
    } catch (err) {
      console.error('Failed to copy to clipboard:', err);
      showToast('Failed to copy link', 'error');
    }
  }

  function buildQueueTrackFromQobuz(track: QobuzTrack): BackendQueueTrack {
    const artwork = track.album?.image?.large || track.album?.image?.thumbnail || track.album?.image?.small || '';
    return {
      id: track.id,
      title: track.title,
      artist: track.performer?.name || 'Unknown Artist',
      album: track.album?.title || '',
      duration_secs: track.duration,
      artwork_url: artwork || null
    };
  }

  function buildQueueTrackFromAlbumTrack(track: Track): BackendQueueTrack {
    const artwork = selectedAlbum?.artwork || '';
    return {
      id: track.id,
      title: track.title,
      artist: track.artist || selectedAlbum?.artist || 'Unknown Artist',
      album: selectedAlbum?.title || '',
      duration_secs: track.durationSeconds,
      artwork_url: artwork || null
    };
  }

  function buildQueueTrackFromDisplayTrack(track: PlaylistTrack): BackendQueueTrack {
    return {
      id: track.id,
      title: track.title,
      artist: track.artist || 'Unknown Artist',
      album: track.album || 'Playlist',
      duration_secs: track.durationSeconds,
      artwork_url: track.albumArt || null
    };
  }

  function buildQueueTrackFromLocalTrack(track: LocalLibraryTrack): BackendQueueTrack {
    const artwork = track.artwork_path ? `asset://localhost/${encodeURIComponent(track.artwork_path)}` : null;
    return {
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      duration_secs: track.duration_secs,
      artwork_url: artwork
    };
  }

  async function queueTrackNext(queueTrack: BackendQueueTrack, isLocal = false) {
    const success = await addToQueueNext(queueTrack, isLocal);
    if (success) {
      showToast('Queued to play next', 'success');
    } else {
      showToast('Failed to queue track', 'error');
    }
  }

  async function queueTrackLater(queueTrack: BackendQueueTrack, isLocal = false) {
    const success = await addToQueue(queueTrack, isLocal);
    if (success) {
      showToast('Added to queue', 'success');
    } else {
      showToast('Failed to add to queue', 'error');
    }
  }

  async function addTrackToFavorites(trackId: number) {
    try {
      await invoke('add_favorite', { favType: 'track', itemId: String(trackId) });
      showToast('Added to favorites', 'success');
    } catch (err) {
      console.error('Failed to add to favorites:', err);
      showToast('Failed to add to favorites', 'error');
    }
  }

  async function shareQobuzTrackLink(trackId: number) {
    try {
      const url = await invoke<string>('get_qobuz_track_url', { trackId });
      await copyToClipboard(url, 'Qobuz link copied');
    } catch (err) {
      console.error('Failed to get Qobuz link:', err);
      showToast(`Failed to share Qobuz link: ${err}`, 'error');
    }
  }

  async function shareSonglinkTrack(trackId: number, isrc?: string) {
    const qobuzUrl = `https://www.qobuz.com/track/${trackId}`;
    const resolvedIsrc = isrc?.trim();
    try {
      showToast('Fetching Song.link...', 'info');
      const response = await invoke<SongLinkResponse>('share_track_songlink', {
        isrc: resolvedIsrc?.length ? resolvedIsrc : null,
        url: qobuzUrl,
        trackId
      });
      await copyToClipboard(response.pageUrl, 'Song.link copied');
    } catch (err) {
      console.error('Failed to get Song.link:', err);
      showToast(`Song.link error: ${err}`, 'error');
    }
  }

  // Playback Functions
  async function handleTrackPlay(track: QobuzTrack) {
    console.log('Playing track:', track);

    const artwork = track.album?.image?.large || track.album?.image?.thumbnail || track.album?.image?.small || '';
    const quality = track.hires_streamable && track.maximum_bit_depth && track.maximum_sampling_rate
      ? `${track.maximum_bit_depth}bit/${track.maximum_sampling_rate}kHz`
      : 'CD Quality';

    const newTrack: PlayingTrack = {
      id: track.id,
      title: track.title,
      artist: track.performer?.name || 'Unknown Artist',
      album: track.album?.title || '',
      artwork,
      duration: track.duration,
      quality,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate
    };
    setCurrentTrack(newTrack);

    // Try to play the track
    try {
      console.log('Invoking play_track with trackId:', track.id);
      showToast(`Loading: ${track.title}`, 'info');
      await invoke('play_track', { trackId: track.id });
      console.log('play_track invoke succeeded');
      setIsPlaying(true);
      showToast(`Playing: ${track.title}`, 'success');

      // Update MPRIS metadata for system media controls
      await invoke('set_media_metadata', {
        title: track.title,
        artist: track.performer?.name || 'Unknown Artist',
        album: track.album?.title || '',
        durationSecs: track.duration,
        coverUrl: artwork || null
      });

      // Show system notification
      showTrackNotification(
        track.title,
        track.performer?.name || 'Unknown Artist',
        track.album?.title || '',
        artwork || undefined
      );

      // Check if track is favorite
      setIsFavorite(await checkTrackFavorite(track.id));
    } catch (err) {
      console.error('Failed to play track:', err);
      showToast(`Playback error: ${err}`, 'error');
      setIsPlaying(false);
    }
  }

  // Handle track play from album detail view
  async function handleAlbumTrackPlay(track: Track) {
    console.log('Playing album track:', track);

    // Use album artwork from selectedAlbum
    const artwork = selectedAlbum?.artwork || '';
    const quality = track.hires && track.bitDepth && track.samplingRate
      ? `${track.bitDepth}bit/${track.samplingRate}kHz`
      : 'CD Quality';

    const newTrack: PlayingTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist || selectedAlbum?.artist || 'Unknown Artist',
      album: selectedAlbum?.title || '',
      artwork,
      duration: track.durationSeconds,
      quality
    };
    setCurrentTrack(newTrack);

    // Build queue from album tracks
    if (selectedAlbum?.tracks) {
      const trackIndex = selectedAlbum.tracks.findIndex(t => t.id === track.id);
      const queueTracks: BackendQueueTrack[] = selectedAlbum.tracks.map(t => ({
        id: t.id,
        title: t.title,
        artist: t.artist || selectedAlbum?.artist || 'Unknown Artist',
        album: selectedAlbum?.title || '',
        duration_secs: t.durationSeconds,
        artwork_url: artwork || null
      }));

      // Set the queue starting at the clicked track (clearLocal=true for Qobuz tracks)
      const success = await setQueue(queueTracks, trackIndex >= 0 ? trackIndex : 0, true);
      if (success) {
        console.log(`Queue set with ${queueTracks.length} tracks, starting at index ${trackIndex}`);
      } else {
        console.error('Failed to set queue');
      }
    }

    // Try to play the track
    try {
      console.log('Invoking play_track with trackId:', track.id);
      showToast(`Loading: ${track.title}`, 'info');
      await invoke('play_track', { trackId: track.id });
      console.log('play_track invoke succeeded');
      setIsPlaying(true);
      showToast(`Playing: ${track.title}`, 'success');

      // Update MPRIS metadata for system media controls
      await invoke('set_media_metadata', {
        title: track.title,
        artist: track.artist || selectedAlbum?.artist || 'Unknown Artist',
        album: selectedAlbum?.title || '',
        durationSecs: track.durationSeconds,
        coverUrl: artwork || null
      });

      // Show system notification
      showTrackNotification(
        track.title,
        track.artist || selectedAlbum?.artist || 'Unknown Artist',
        selectedAlbum?.title || '',
        artwork || undefined
      );

      // Update Last.fm
      updateLastfmNowPlaying(
        track.title,
        track.artist || selectedAlbum?.artist || 'Unknown Artist',
        selectedAlbum?.title || '',
        track.durationSeconds,
        track.id
      );
    } catch (err) {
      console.error('Failed to play track:', err);
      showToast(`Playback error: ${err}`, 'error');
      setIsPlaying(false);
    }
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

  // Check if a track is in favorites
  async function checkTrackFavorite(trackId: number): Promise<boolean> {
    try {
      const response = await invoke<{ tracks?: { items: Array<{ id: number }> } }>('get_favorites', {
        favType: 'tracks',
        limit: 500
      });
      if (response.tracks?.items) {
        return response.tracks.items.some(item => item.id === trackId);
      }
      return false;
    } catch (err) {
      console.error('Failed to check favorite status:', err);
      return false;
    }
  }

  async function toggleFavorite() {
    if (!currentTrack) return;

    const trackId = String(currentTrack.id);
    const newFavoriteState = !isFavorite;

    try {
      if (newFavoriteState) {
        await invoke('add_favorite', { favType: 'track', itemId: trackId });
        showToast('Added to favorites', 'success');
      } else {
        await invoke('remove_favorite', { favType: 'track', itemId: trackId });
        showToast('Removed from favorites', 'success');
      }
      setIsFavorite(newFavoriteState);
    } catch (err) {
      console.error('Failed to toggle favorite:', err);
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

    const newTrack: PlayingTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork: track.artwork_url || '',
      duration: track.duration_secs,
      quality: isLocal ? 'Local' : 'Hi-Res',
      isLocal
    };
    setCurrentTrack(newTrack);

    try {
      // Use appropriate playback command based on track source
      if (isLocal) {
        await invoke('library_play_track', { trackId: track.id });
      } else {
        await invoke('play_track', { trackId: track.id });
      }
      setIsPlaying(true);

      // Update MPRIS
      await invoke('set_media_metadata', {
        title: track.title,
        artist: track.artist,
        album: track.album,
        durationSecs: track.duration_secs,
        coverUrl: track.artwork_url
      });

      // Show system notification
      showTrackNotification(track.title, track.artist, track.album, track.artwork_url || undefined);

      // Update Last.fm
      updateLastfmNowPlaying(track.title, track.artist, track.album, track.duration_secs, track.id);

      // Check if track is favorite (for Qobuz tracks only)
      if (!isLocal) {
        setIsFavorite(await checkTrackFavorite(track.id));
      } else {
        setIsFavorite(false);
      }

      // Refresh queue state
      await syncQueueState();
    } catch (err) {
      console.error('Failed to play queue track:', err);
      showToast(`Playback error: ${err}`, 'error');
      setIsPlaying(false);
    }
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

  // Add all album tracks to existing queue
  async function handleAddAlbumToQueue() {
    if (!selectedAlbum?.tracks?.length) return;

    const artwork = selectedAlbum.artwork || '';
    const queueTracks: BackendQueueTrack[] = selectedAlbum.tracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.artist || selectedAlbum?.artist || 'Unknown Artist',
      album: selectedAlbum?.title || '',
      duration_secs: t.durationSeconds,
      artwork_url: artwork || null
    }));

    const success = await addTracksToQueue(queueTracks);
    if (success) {
      showToast(`Added ${queueTracks.length} tracks to queue`, 'success');
    } else {
      showToast('Failed to add to queue', 'error');
    }
  }

  function handleQobuzTrackPlayNext(track: QobuzTrack) {
    queueTrackNext(buildQueueTrackFromQobuz(track));
  }

  function handleQobuzTrackPlayLater(track: QobuzTrack) {
    queueTrackLater(buildQueueTrackFromQobuz(track));
  }

  function handleAlbumTrackPlayNext(track: Track) {
    queueTrackNext(buildQueueTrackFromAlbumTrack(track));
  }

  function handleAlbumTrackPlayLater(track: Track) {
    queueTrackLater(buildQueueTrackFromAlbumTrack(track));
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
        quality: track.quality || 'CD Quality',
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
          quality: track.quality || 'CD Quality',
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
      const quality = track.hires && track.bitDepth && track.samplingRate
        ? `${track.bitDepth}bit/${track.samplingRate}kHz`
        : 'CD Quality';
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

  function handleDisplayTrackPlayNext(track: PlaylistTrack) {
    queueTrackNext(buildQueueTrackFromDisplayTrack(track));
  }

  function handleDisplayTrackPlayLater(track: PlaylistTrack) {
    queueTrackLater(buildQueueTrackFromDisplayTrack(track));
  }

  function handleLocalTrackPlayNext(track: LocalLibraryTrack) {
    queueTrackNext(buildQueueTrackFromLocalTrack(track), true);
  }

  function handleLocalTrackPlayLater(track: LocalLibraryTrack) {
    queueTrackLater(buildQueueTrackFromLocalTrack(track), true);
  }

  async function handlePlaylistTrackPlay(track: PlaylistTrack) {
    console.log('Playing playlist track:', track);

    const quality = track.hires && track.bitDepth && track.samplingRate
      ? `${track.bitDepth}bit/${track.samplingRate}kHz`
      : 'CD Quality';

    const newTrack: PlayingTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist || 'Unknown Artist',
      album: track.album || 'Playlist',
      artwork: track.albumArt || '',
      duration: track.durationSeconds,
      quality
    };
    setCurrentTrack(newTrack);

    try {
      await invoke('play_track', { trackId: track.id });
      setIsPlaying(true);

      await invoke('set_media_metadata', {
        title: track.title,
        artist: track.artist || 'Unknown Artist',
        album: track.album || 'Playlist',
        durationSecs: track.durationSeconds,
        coverUrl: track.albumArt
      });

      // Show system notification
      showTrackNotification(
        track.title,
        track.artist || 'Unknown Artist',
        track.album || 'Playlist',
        track.albumArt
      );

      // Update Last.fm
      updateLastfmNowPlaying(
        track.title,
        track.artist || 'Unknown Artist',
        track.album || '',
        track.durationSeconds,
        track.id
      );

      // Check if track is favorite
      setIsFavorite(await checkTrackFavorite(track.id));

      await syncQueueState();
    } catch (err) {
      console.error('Failed to play track:', err);
      showToast(`Playback error: ${err}`, 'error');
      setIsPlaying(false);
    }
  }

  async function handleLocalTrackPlay(track: LocalLibraryTrack) {
    console.log('Playing local track:', track);

    const artwork = track.artwork_path ? `asset://localhost/${encodeURIComponent(track.artwork_path)}` : '';
    const quality = track.bit_depth && track.sample_rate
      ? (track.bit_depth >= 24 || track.sample_rate > 48000
        ? `${track.bit_depth}bit/${track.sample_rate / 1000}kHz`
        : track.format)
      : track.format;

    const newTrack: PlayingTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork,
      duration: track.duration_secs,
      quality,
      isLocal: true
    };
    setCurrentTrack(newTrack);

    try {
      await invoke('library_play_track', { trackId: track.id });
      setIsPlaying(true);
      showToast(`Playing: ${track.title}`, 'success');

      // Update MPRIS metadata
      await invoke('set_media_metadata', {
        title: track.title,
        artist: track.artist,
        album: track.album,
        durationSecs: track.duration_secs,
        coverUrl: artwork || null
      });

      // Show system notification
      showTrackNotification(track.title, track.artist, track.album, artwork || undefined);

      // Update Last.fm
      updateLastfmNowPlaying(
        track.title,
        track.artist,
        track.album,
        track.duration_secs,
        track.id
      );

      // Local tracks don't have Qobuz favorites
      setIsFavorite(false);
    } catch (err) {
      console.error('Failed to play local track:', err);
      showToast(`Playback error: ${err}`, 'error');
      setIsPlaying(false);
    }
  }

  // Handle setting queue from local library (tracks need different playback command)
  function handleSetLocalQueue(trackIds: number[]) {
    // Set local track IDs via queueStore
    setLocalTrackIds(trackIds);
  }

  // System Notification for track changes
  async function showTrackNotification(title: string, artist: string, album: string, artworkUrl?: string) {
    try {
      await invoke('show_track_notification', {
        title,
        artist,
        album,
        artworkUrl: artworkUrl || null
      });
    } catch (err) {
      console.error('Failed to show track notification:', err);
    }
  }

  // Last.fm scrobbling state
  let lastScrobbledTrackId: number | null = null;
  let scrobbleTimeout: ReturnType<typeof setTimeout> | null = null;

  // Update Last.fm "now playing" and schedule scrobble
  async function updateLastfmNowPlaying(title: string, artist: string, album: string, durationSecs: number, trackId: number) {
    // Check if scrobbling is enabled
    const scrobblingEnabled = localStorage.getItem('qbz-lastfm-scrobbling') !== 'false';
    const sessionKey = localStorage.getItem('qbz-lastfm-session-key');

    if (!scrobblingEnabled || !sessionKey) return;

    try {
      // Update "now playing"
      await invoke('lastfm_now_playing', {
        artist,
        track: title,
        album: album || null
      });
      console.log('Last.fm: Updated now playing');

      // Schedule scrobble after 50% of track or 4 minutes (whichever is shorter)
      if (scrobbleTimeout) {
        clearTimeout(scrobbleTimeout);
      }

      const scrobbleDelay = Math.min(durationSecs * 0.5, 240) * 1000; // in ms

      scrobbleTimeout = setTimeout(async () => {
        if (lastScrobbledTrackId !== trackId) {
          try {
            const timestamp = Math.floor(Date.now() / 1000);
            await invoke('lastfm_scrobble', {
              artist,
              track: title,
              album: album || null,
              timestamp
            });
            lastScrobbledTrackId = trackId;
            console.log('Last.fm: Scrobbled track');
          } catch (err) {
            console.error('Last.fm scrobble failed:', err);
          }
        }
      }, scrobbleDelay);
    } catch (err) {
      console.error('Last.fm now playing failed:', err);
    }
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

  // Auth Handlers
  function handleLoginSuccess(info: UserInfo) {
    setLoggedIn(info);
    showToast(`Welcome, ${info.userName}!`, 'success');
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
      setLoggedOut();
      currentTrack = null;
      isPlaying = false;
      showToast('Logged out successfully', 'info');
    } catch (err) {
      console.error('Logout error:', err);
      showToast('Failed to logout', 'error');
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
          goBack();
        } else if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          handleSkipBack();
        }
        break;
      case 'ArrowRight':
        if (e.altKey) {
          e.preventDefault();
          goForward();
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
    // Load saved theme
    const savedTheme = localStorage.getItem('qbz-theme');
    if (savedTheme) {
      document.documentElement.setAttribute('data-theme', savedTheme);
    }

    const handleMouseNavigation = (event: MouseEvent) => {
      if (event.button === 3) {
        event.preventDefault();
        goBack();
      } else if (event.button === 4) {
        event.preventDefault();
        goForward();
      }
    };

    document.addEventListener('keydown', handleKeydown);
    window.addEventListener('mouseup', handleMouseNavigation);

    // Initialize download states
    initDownloadStates();
    startDownloadEventListeners();

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

    // Set up track ended callback for auto-advance
    setOnTrackEnded(async () => {
      const nextTrackResult = await nextTrack();
      if (nextTrackResult) {
        await playQueueTrack(nextTrackResult);
      } else {
        setQueueEnded(true);
      }
    });

    // Restore Last.fm session on app startup
    (async () => {
      try {
        const savedApiKey = localStorage.getItem('qbz-lastfm-api-key');
        const savedApiSecret = localStorage.getItem('qbz-lastfm-api-secret');
        const savedSessionKey = localStorage.getItem('qbz-lastfm-session-key');

        // Restore credentials if user-provided
        if (savedApiKey && savedApiSecret) {
          await invoke('lastfm_set_credentials', {
            apiKey: savedApiKey,
            apiSecret: savedApiSecret
          });
        }

        // Restore session if available
        if (savedSessionKey) {
          await invoke('lastfm_set_session', { sessionKey: savedSessionKey });
          console.log('Last.fm session restored on startup');
        }
      } catch (err) {
        console.error('Failed to restore Last.fm session:', err);
      }
    })();

    return () => {
      document.removeEventListener('keydown', handleKeydown);
      window.removeEventListener('mouseup', handleMouseNavigation);
      stopDownloadEventListeners();
      unsubscribeDownloads();
      unsubscribeToast();
      unsubscribeUI();
      unsubscribeAuth();
      unsubscribeNav();
      unsubscribePlayer();
      unsubscribeQueue();
      stopPolling();
    };
  });

  // Sync queue state when opening queue panel
  $effect(() => {
    if (isQueueOpen) {
      syncQueueState();
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
    <!-- Sidebar -->
    <Sidebar
      bind:this={sidebarRef}
      {activeView}
      {selectedPlaylistId}
      onNavigate={navigateTo}
      onPlaylistSelect={handlePlaylistSelect}
      onCreatePlaylist={openCreatePlaylist}
      onSettingsClick={() => navigateTo('settings')}
      onLogout={handleLogout}
      userName={userInfo?.userName || 'User'}
      subscription={userInfo?.subscription || 'Qobuz'}
    />

    <!-- Main Content -->
    <main class="main-content">
      {#if activeView === 'home'}
        <HomeView
          onAlbumClick={handleAlbumClick}
        />
      {:else if activeView === 'search'}
        <SearchView
          onAlbumClick={handleAlbumClick}
          onTrackPlay={handleTrackPlay}
          onTrackPlayNext={handleQobuzTrackPlayNext}
          onTrackPlayLater={handleQobuzTrackPlayLater}
          onTrackAddFavorite={addTrackToFavorites}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'settings'}
        <SettingsView
          onBack={goBack}
          onLogout={handleLogout}
          userName={userInfo?.userName}
          subscription={userInfo?.subscription}
        />
      {:else if activeView === 'album' && selectedAlbum}
        <AlbumDetailView
          album={selectedAlbum}
          onBack={goBack}
          onArtistClick={() => selectedAlbum?.artistId && handleArtistClick(selectedAlbum.artistId)}
          onTrackPlay={handleAlbumTrackPlay}
          onTrackPlayNext={handleAlbumTrackPlayNext}
          onTrackPlayLater={handleAlbumTrackPlayLater}
          onTrackAddFavorite={addTrackToFavorites}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onPlayAll={handlePlayAllAlbum}
          onShuffleAll={handleShuffleAlbum}
          onAddToQueue={handleAddAlbumToQueue}
          onAddTrackToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackDownload={handleTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          getTrackDownloadStatus={getTrackDownloadStatus}
          onDownloadAlbum={handleDownloadAlbum}
          {downloadStateVersion}
        />
      {:else if activeView === 'artist' && selectedArtist}
        <ArtistDetailView
          artist={selectedArtist}
          onBack={goBack}
          onAlbumClick={handleAlbumClick}
          onTrackPlay={handlePlaylistTrackPlay}
          onTrackPlayNext={handleQobuzTrackPlayNext}
          onTrackPlayLater={handleQobuzTrackPlayLater}
          onTrackAddFavorite={addTrackToFavorites}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
        />
      {:else if activeView === 'library'}
        <LocalLibraryView
          onTrackPlay={handleLocalTrackPlay}
          onTrackPlayNext={handleLocalTrackPlayNext}
          onTrackPlayLater={handleLocalTrackPlayLater}
          onSetLocalQueue={handleSetLocalQueue}
        />
      {:else if activeView === 'playlist' && selectedPlaylistId}
        <PlaylistDetailView
          playlistId={selectedPlaylistId}
          onBack={goBack}
          onTrackPlay={handlePlaylistTrackPlay}
          onTrackPlayNext={handleDisplayTrackPlayNext}
          onTrackPlayLater={handleDisplayTrackPlayLater}
          onTrackAddFavorite={addTrackToFavorites}
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
          onLocalTrackPlayNext={handleLocalTrackPlayNext}
          onLocalTrackPlayLater={handleLocalTrackPlayLater}
        />
      {:else if activeView === 'favorites'}
        <FavoritesView
          onAlbumClick={handleAlbumClick}
          onTrackPlay={handlePlaylistTrackPlay}
          onArtistClick={handleArtistClick}
          onTrackPlayNext={handleDisplayTrackPlayNext}
          onTrackPlayLater={handleDisplayTrackPlayLater}
          onTrackAddFavorite={addTrackToFavorites}
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
      {/if}
    </main>

    <!-- Now Playing Bar -->
    {#if currentTrack}
      <NowPlayingBar
        artwork={currentTrack.artwork}
        trackTitle={currentTrack.title}
        artist={currentTrack.artist}
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
      />
    {:else}
      <NowPlayingBar
        onOpenQueue={openQueue}
        onOpenFullScreen={openFullScreen}
        onCast={openCastPicker}
      />
    {/if}

    <!-- Queue Panel -->
    <QueuePanel
      isOpen={isQueueOpen}
      onClose={closeQueue}
      currentTrack={currentQueueTrack ?? undefined}
      upcomingTracks={queue}
      onPlayTrack={handleQueueTrackPlay}
      onClearQueue={handleClearQueue}
      onSaveAsPlaylist={() => showToast('Save as playlist coming soon', 'info')}
    />

    <!-- Full Screen Now Playing -->
    {#if currentTrack}
      <FullScreenNowPlaying
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
        {currentTime}
        {duration}
        onSeek={handleSeek}
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
    height: 100vh;
    overflow: hidden;
    background-color: var(--bg-primary);
  }

  .main-content {
    flex: 1;
    min-width: 0;
    height: calc(100vh - 80px);
    overflow-y: auto;
    padding: 24px 32px;
  }

</style>
