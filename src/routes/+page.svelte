<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';

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

  // Types
  interface QobuzTrack {
    id: number;
    title: string;
    duration: number;
    album?: {
      id?: string;
      title: string;
      image?: { small?: string; thumbnail?: string; large?: string };
    };
    performer?: { id?: number; name: string };
    hires_streamable?: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    isrc?: string;
  }

  interface QobuzAlbum {
    id: string;
    title: string;
    artist: { id?: number; name: string };
    image: { small?: string; thumbnail?: string; large?: string };
    release_date_original?: string;
    hires_streamable?: boolean;
    tracks_count?: number;
    duration?: number;
    label?: { name: string };
    genre?: { name: string };
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    tracks?: { items: QobuzTrack[] };
  }

  interface QobuzArtist {
    id: number;
    name: string;
    image?: { small?: string; thumbnail?: string; large?: string };
    albums_count?: number;
    biography?: {
      summary?: string;
      content?: string;
      source?: string;
    };
    albums?: {
      items: QobuzAlbum[];
      total: number;
      offset: number;
      limit: number;
    };
  }

  interface ArtistDetail {
    id: number;
    name: string;
    image?: string;
    albumsCount?: number;
    biography?: {
      summary?: string;
      content?: string;
      source?: string;
    };
    albums: {
      id: string;
      title: string;
      artwork: string;
      year?: string;
      quality: string;
    }[];
    totalAlbums: number;
  }

  interface Track {
    id: number;
    number: number;
    title: string;
    artist?: string;
    duration: string;
    durationSeconds: number;
    quality?: string;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
    albumId?: string;
    artistId?: number;
    isrc?: string;
  }

  interface AlbumDetail {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    artistId?: number;
    year: string;
    label: string;
    genre: string;
    quality: string;
    trackCount: number;
    duration: string;
    tracks: Track[];
  }

  interface QueueTrack {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    duration: string;
  }

  // Backend queue track format
  interface BackendQueueTrack {
    id: number;
    title: string;
    artist: string;
    album: string;
    duration_secs: number;
    artwork_url: string | null;
  }

  interface SongLinkResponse {
    pageUrl: string;
    title?: string;
    artist?: string;
    thumbnailUrl?: string;
    platforms: Record<string, string>;
    identifier: string;
    contentType: string;
  }

  interface BackendQueueState {
    current_track: BackendQueueTrack | null;
    current_index: number | null;
    upcoming: BackendQueueTrack[];
    history: BackendQueueTrack[];
    shuffle: boolean;
    repeat: 'Off' | 'All' | 'One';
    total_tracks: number;
  }

  interface PlayingTrack {
    id: number;
    title: string;
    artist: string;
    album: string;
    artwork: string;
    duration: number;
    quality: string;
    bitDepth?: number;
    samplingRate?: number;
  }

  // Auth State
  let isLoggedIn = $state(false);
  let userInfo = $state<{ userName: string; subscription: string } | null>(null);

  // View State
  type ViewType = 'home' | 'search' | 'library' | 'settings' | 'album' | 'artist' | 'playlist' | 'favorites';
  let activeView = $state<ViewType>('home');
  let viewHistory = $state<ViewType[]>(['home']);
  let forwardHistory = $state<ViewType[]>([]);
  let selectedAlbum = $state<AlbumDetail | null>(null);
  let selectedArtist = $state<ArtistDetail | null>(null);
  let selectedPlaylistId = $state<number | null>(null);

  // Overlay States
  let isQueueOpen = $state(false);
  let isFullScreenOpen = $state(false);
  let isFocusModeOpen = $state(false);
  let isCastPickerOpen = $state(false);

  // Playlist Modal State
  let isPlaylistModalOpen = $state(false);
  let playlistModalMode = $state<'create' | 'edit' | 'addTrack'>('create');
  let playlistModalTrackIds = $state<number[]>([]);
  let userPlaylists = $state<{ id: number; name: string; tracks_count: number }[]>([]);

  // Sidebar reference for refreshing playlists
  let sidebarRef: { getPlaylists: () => { id: number; name: string; tracks_count: number }[], refreshPlaylists: () => void } | undefined;

  // Playback State
  let currentTrack = $state<PlayingTrack | null>(null);
  let isPlaying = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let volume = $state(75);
  let isShuffle = $state(false);
  let repeatMode = $state<'off' | 'all' | 'one'>('off');
  let isFavorite = $state(false);

  // Queue State (synced from backend)
  let queue = $state<QueueTrack[]>([]);
  let queueTotalTracks = $state(0);

  // Local library track IDs in current queue (for distinguishing from Qobuz tracks)
  let localTrackIds = $state<Set<number>>(new Set());

  // Toast State
  let toast = $state<{ message: string; type: 'success' | 'error' | 'info' } | null>(null);

  // Navigation Functions
  function navigateTo(view: string) {
    console.log('navigateTo called with:', view, 'current activeView:', activeView);
    const typedView = view as ViewType;
    if (typedView !== activeView) {
      viewHistory = [...viewHistory, typedView];
      forwardHistory = [];
      activeView = typedView;
      console.log('View changed to:', activeView);
    } else {
      console.log('View unchanged (already on this view)');
    }
  }

  function goBack() {
    if (viewHistory.length > 1) {
      const lastView = viewHistory[viewHistory.length - 1];
      viewHistory = viewHistory.slice(0, -1);
      forwardHistory = [...forwardHistory, lastView];
      activeView = viewHistory[viewHistory.length - 1];
    }
  }

  function goForward() {
    if (forwardHistory.length > 0) {
      const nextView = forwardHistory[forwardHistory.length - 1];
      forwardHistory = forwardHistory.slice(0, -1);
      viewHistory = [...viewHistory, nextView];
      activeView = nextView;
    }
  }

  function handlePlaylistSelect(playlistId: number) {
    selectedPlaylistId = playlistId;
    navigateTo('playlist');
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
    const artwork = track.artwork_path ? `file://${track.artwork_path}` : null;
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
    try {
      await invoke('add_to_queue_next', { track: queueTrack });
      if (isLocal) {
        localTrackIds = new Set([...localTrackIds, queueTrack.id]);
      }
      await syncQueueState();
      showToast('Queued to play next', 'success');
    } catch (err) {
      console.error('Failed to queue track next:', err);
      showToast('Failed to queue track', 'error');
    }
  }

  async function queueTrackLater(queueTrack: BackendQueueTrack, isLocal = false) {
    try {
      await invoke('add_to_queue', { track: queueTrack });
      if (isLocal) {
        localTrackIds = new Set([...localTrackIds, queueTrack.id]);
      }
      await syncQueueState();
      showToast('Added to queue', 'success');
    } catch (err) {
      console.error('Failed to queue track:', err);
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

    currentTrack = {
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

    duration = track.duration;
    currentTime = 0;

    // Try to play the track
    try {
      console.log('Invoking play_track with trackId:', track.id);
      showToast(`Loading: ${track.title}`, 'info');
      await invoke('play_track', { trackId: track.id });
      console.log('play_track invoke succeeded');
      isPlaying = true;
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
      isFavorite = await checkTrackFavorite(track.id);
    } catch (err) {
      console.error('Failed to play track:', err);
      showToast(`Playback error: ${err}`, 'error');
      isPlaying = false;
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

    currentTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist || selectedAlbum?.artist || 'Unknown Artist',
      album: selectedAlbum?.title || '',
      artwork,
      duration: track.durationSeconds,
      quality
    };

    duration = track.durationSeconds;
    currentTime = 0;

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

      // Set the queue starting at the clicked track
      try {
        await invoke('set_queue', {
          tracks: queueTracks,
          startIndex: trackIndex >= 0 ? trackIndex : 0
        });
        // Clear local track IDs when playing Qobuz tracks
        localTrackIds = new Set();
        console.log(`Queue set with ${queueTracks.length} tracks, starting at index ${trackIndex}`);
        // Sync queue state to update UI
        await syncQueueState();
      } catch (err) {
        console.error('Failed to set queue:', err);
      }
    }

    // Try to play the track
    try {
      console.log('Invoking play_track with trackId:', track.id);
      showToast(`Loading: ${track.title}`, 'info');
      await invoke('play_track', { trackId: track.id });
      console.log('play_track invoke succeeded');
      isPlaying = true;
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
      isPlaying = false;
    }
  }

  function togglePlay() {
    if (!currentTrack) return;
    isPlaying = !isPlaying;

    // TODO: invoke pause/resume commands
    if (isPlaying) {
      invoke('resume_playback').catch(console.error);
    } else {
      invoke('pause_playback').catch(console.error);
    }
  }

  function handleSeek(time: number) {
    currentTime = Math.max(0, Math.min(duration, time));
    invoke('seek', { position: time }).catch(console.error);
  }

  function handleVolumeChange(newVolume: number) {
    volume = Math.max(0, Math.min(100, newVolume));
    invoke('set_volume', { volume: newVolume / 100 }).catch(console.error);
  }

  async function toggleShuffle() {
    isShuffle = !isShuffle;
    try {
      await invoke('set_shuffle', { enabled: isShuffle });
      showToast(isShuffle ? 'Shuffle enabled' : 'Shuffle disabled', 'info');
    } catch (err) {
      console.error('Failed to set shuffle:', err);
      isShuffle = !isShuffle; // Revert
    }
  }

  async function toggleRepeat() {
    const nextMode = repeatMode === 'off' ? 'all' : repeatMode === 'all' ? 'one' : 'off';
    try {
      await invoke('set_repeat', { mode: nextMode });
      repeatMode = nextMode;
      const messages = { off: 'Repeat off', all: 'Repeat all', one: 'Repeat one' };
      showToast(messages[repeatMode], 'info');
    } catch (err) {
      console.error('Failed to set repeat:', err);
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
      isFavorite = newFavoriteState;
    } catch (err) {
      console.error('Failed to toggle favorite:', err);
      showToast('Failed to update favorites', 'error');
    }
  }

  // Skip track handlers - wired to backend queue
  async function handleSkipBack() {
    if (!currentTrack || isSkipping) return;
    // If more than 3 seconds in, restart track; otherwise go to previous
    if (currentTime > 3) {
      handleSeek(0);
      return;
    }

    isSkipping = true;
    try {
      const prevTrack = await invoke<BackendQueueTrack | null>('previous_track');
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
      isSkipping = false;
    }
  }

  async function handleSkipForward() {
    if (!currentTrack || isSkipping) return;

    isSkipping = true;
    try {
      const nextTrackResult = await invoke<BackendQueueTrack | null>('next_track');
      if (nextTrackResult) {
        await playQueueTrack(nextTrackResult);
      } else {
        // No next track - stop playback
        await invoke('stop_playback');
        isPlaying = false;
        showToast('Queue ended', 'info');
      }
    } catch (err) {
      console.error('Failed to go to next track:', err);
      showToast('Failed to go to next track', 'error');
    } finally {
      isSkipping = false;
    }
  }

  // Helper to play a track from the queue
  async function playQueueTrack(track: BackendQueueTrack) {
    const isLocalTrack = localTrackIds.has(track.id);

    // Reset queue ended flag when playing a new track
    queueEnded = false;

    currentTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork: track.artwork_url || '',
      duration: track.duration_secs,
      quality: isLocalTrack ? 'Local' : 'Hi-Res'
    };

    duration = track.duration_secs;
    currentTime = 0;

    try {
      // Use appropriate playback command based on track source
      if (isLocalTrack) {
        await invoke('library_play_track', { trackId: track.id });
      } else {
        await invoke('play_track', { trackId: track.id });
      }
      isPlaying = true;

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
      if (!isLocalTrack) {
        isFavorite = await checkTrackFavorite(track.id);
      } else {
        isFavorite = false;
      }

      // Refresh queue state
      await syncQueueState();
    } catch (err) {
      console.error('Failed to play queue track:', err);
      showToast(`Playback error: ${err}`, 'error');
      isPlaying = false;
    }
  }

  // Sync queue state from backend
  async function syncQueueState() {
    try {
      const queueState = await invoke<BackendQueueState>('get_queue_state');

      // Convert backend queue tracks to frontend format
      queue = queueState.upcoming.map(t => ({
        id: String(t.id),
        artwork: t.artwork_url || '',
        title: t.title,
        artist: t.artist,
        duration: formatDuration(t.duration_secs)
      }));

      queueTotalTracks = queueState.total_tracks;
      isShuffle = queueState.shuffle;
      repeatMode = queueState.repeat.toLowerCase() as 'off' | 'all' | 'one';
    } catch (err) {
      console.error('Failed to sync queue state:', err);
    }
  }

  // Play a specific track from the queue panel
  async function handleQueueTrackPlay(trackId: string) {
    try {
      // Find the index in the queue
      const queueState = await invoke<BackendQueueState>('get_queue_state');
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

        const track = await invoke<BackendQueueTrack | null>('play_queue_index', { index: actualIndex });
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
    try {
      await invoke('clear_queue');
      queue = [];
      queueTotalTracks = 0;
      showToast('Queue cleared', 'info');
    } catch (err) {
      console.error('Failed to clear queue:', err);
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

    try {
      await invoke('add_tracks_to_queue', { tracks: queueTracks });
      await syncQueueState();
      showToast(`Added ${queueTracks.length} tracks to queue`, 'success');
    } catch (err) {
      console.error('Failed to add to queue:', err);
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

  // Handle playing a track from playlist view
  interface PlaylistTrack {
    id: number;
    number: number;
    title: string;
    artist?: string;
    album?: string;
    albumArt?: string;
    duration: string;
    durationSeconds: number;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
    albumId?: string;
    artistId?: number;
    isrc?: string;
  }

  async function handlePlaylistTrackPlay(track: PlaylistTrack) {
    console.log('Playing playlist track:', track);

    const quality = track.hires && track.bitDepth && track.samplingRate
      ? `${track.bitDepth}bit/${track.samplingRate}kHz`
      : 'CD Quality';

    currentTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist || 'Unknown Artist',
      album: track.album || 'Playlist',
      artwork: track.albumArt || '',
      duration: track.durationSeconds,
      quality
    };

    duration = track.durationSeconds;
    currentTime = 0;

    try {
      await invoke('play_track', { trackId: track.id });
      isPlaying = true;

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

      await syncQueueState();
    } catch (err) {
      console.error('Failed to play track:', err);
      showToast(`Playback error: ${err}`, 'error');
      isPlaying = false;
    }
  }

  // Handle playing a track from local library view
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

  async function handleLocalTrackPlay(track: LocalLibraryTrack) {
    console.log('Playing local track:', track);

    const artwork = track.artwork_path ? `file://${track.artwork_path}` : '';
    const quality = track.bit_depth && track.sample_rate
      ? (track.bit_depth >= 24 || track.sample_rate > 48000
        ? `${track.bit_depth}bit/${track.sample_rate / 1000}kHz`
        : track.format)
      : track.format;

    currentTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork,
      duration: track.duration_secs,
      quality
    };

    duration = track.duration_secs;
    currentTime = 0;
    isPlaying = true;
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
  }

  // Handle setting queue from local library (tracks need different playback command)
  function handleSetLocalQueue(trackIds: number[]) {
    // Clear Qobuz tracks and set local track IDs
    localTrackIds = new Set(trackIds);
    console.log(`Set ${trackIds.length} local track IDs in queue`);
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
    playlistModalMode = 'create';
    playlistModalTrackIds = [];
    isPlaylistModalOpen = true;
  }

  function openAddToPlaylist(trackIds: number[]) {
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    playlistModalMode = 'addTrack';
    playlistModalTrackIds = trackIds;
    isPlaylistModalOpen = true;
  }

  function handlePlaylistCreated() {
    if (playlistModalMode === 'addTrack') {
      showToast('Track added to playlist', 'success');
    } else {
      showToast('Playlist created', 'success');
    }
    sidebarRef?.refreshPlaylists();
  }

  // Toast Functions
  function showToast(message: string, type: 'success' | 'error' | 'info' = 'info') {
    toast = { message, type };
  }

  function hideToast() {
    toast = null;
  }

  // Auth Handlers
  function handleLoginSuccess(info: { userName: string; subscription: string }) {
    isLoggedIn = true;
    userInfo = info;
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
      isLoggedIn = false;
      userInfo = null;
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
          isFocusModeOpen = !isFocusModeOpen;
        }
        break;
      case 'q':
        isQueueOpen = !isQueueOpen;
        break;
      case 'Escape':
        if (isFocusModeOpen) isFocusModeOpen = false;
        else if (isFullScreenOpen) isFullScreenOpen = false;
        else if (isQueueOpen) isQueueOpen = false;
        break;
    }
  }

  // Playback state polling - sync with backend every 500ms
  interface PlaybackState {
    is_playing: boolean;
    position: number;
    duration: number;
    track_id: number;
    volume: number;
  }

  let pollInterval: ReturnType<typeof setInterval> | null = null;

  let isAdvancingTrack = false; // Prevent multiple advances
  let isSkipping = false; // Prevent concurrent skip operations
  let queueEnded = false; // Prevent spam when queue has no more tracks

  async function pollPlaybackState() {
    if (!currentTrack) return;

    try {
      const state = await invoke<PlaybackState>('get_playback_state');

      // Only update if we have a matching track
      if (state.track_id === currentTrack.id) {
        currentTime = state.position;
        isPlaying = state.is_playing;

        // Check if track ended - auto-advance to next
        // Don't try if queue already ended or we're already advancing
        if (state.duration > 0 && state.position >= state.duration - 1 && !state.is_playing && !isAdvancingTrack && !queueEnded) {
          console.log('Track finished, advancing to next...');
          isAdvancingTrack = true;

          try {
            const nextTrackResult = await invoke<BackendQueueTrack | null>('next_track');
            if (nextTrackResult) {
              await playQueueTrack(nextTrackResult);
            } else {
              // Queue ended - set flag to prevent further attempts
              console.log('Queue ended');
              queueEnded = true;
            }
          } catch (err) {
            console.error('Failed to auto-advance:', err);
          } finally {
            isAdvancingTrack = false;
          }
        }
      }
    } catch (err) {
      console.error('Failed to poll playback state:', err);
    }
  }

  $effect(() => {
    if (currentTrack) {
      // Start polling when we have a track
      pollInterval = setInterval(pollPlaybackState, 500);
      // Also poll immediately
      pollPlaybackState();
    } else if (pollInterval) {
      clearInterval(pollInterval);
      pollInterval = null;
    }

    return () => {
      if (pollInterval) clearInterval(pollInterval);
    };
  });

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

    return () => {
      document.removeEventListener('keydown', handleKeydown);
      window.removeEventListener('mouseup', handleMouseNavigation);
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
        onOpenQueue={() => (isQueueOpen = true)}
        onOpenFullScreen={() => (isFullScreenOpen = true)}
        onCast={() => (isCastPickerOpen = true)}
      />
    {:else}
      <NowPlayingBar
        onOpenQueue={() => (isQueueOpen = true)}
        onOpenFullScreen={() => (isFullScreenOpen = true)}
        onCast={() => (isCastPickerOpen = true)}
      />
    {/if}

    <!-- Queue Panel -->
    <QueuePanel
      isOpen={isQueueOpen}
      onClose={() => (isQueueOpen = false)}
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
        onClose={() => (isFullScreenOpen = false)}
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
          isFullScreenOpen = false;
          isQueueOpen = true;
        }}
        onOpenFocusMode={() => {
          isFullScreenOpen = false;
          isFocusModeOpen = true;
        }}
        onCast={() => (isCastPickerOpen = true)}
      />
    {/if}

    <!-- Focus Mode -->
    {#if currentTrack}
      <FocusMode
        isOpen={isFocusModeOpen}
        onClose={() => (isFocusModeOpen = false)}
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
      onClose={() => (isPlaylistModalOpen = false)}
      onSuccess={handlePlaylistCreated}
    />

    <!-- Cast Picker -->
    <CastPicker
      isOpen={isCastPickerOpen}
      onClose={() => (isCastPickerOpen = false)}
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
