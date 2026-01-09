<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

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

  // Overlays
  import QueuePanel from '$lib/components/QueuePanel.svelte';
  import FullScreenNowPlaying from '$lib/components/FullScreenNowPlaying.svelte';
  import FocusMode from '$lib/components/FocusMode.svelte';

  // Types
  interface QobuzTrack {
    id: number;
    title: string;
    duration: number;
    album?: {
      title: string;
      image?: { small?: string; thumbnail?: string; large?: string };
    };
    performer?: { name: string };
    hires_streamable?: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
  }

  interface QobuzAlbum {
    id: string;
    title: string;
    artist: { name: string };
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
  }

  interface AlbumDetail {
    artwork: string;
    title: string;
    artist: string;
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
  }

  // Auth State
  let isLoggedIn = $state(false);
  let userInfo = $state<{ userName: string; subscription: string } | null>(null);

  // View State
  type ViewType = 'home' | 'search' | 'library' | 'settings' | 'album';
  let activeView = $state<ViewType>('home');
  let viewHistory = $state<ViewType[]>(['home']);
  let selectedAlbum = $state<AlbumDetail | null>(null);

  // Overlay States
  let isQueueOpen = $state(false);
  let isFullScreenOpen = $state(false);
  let isFocusModeOpen = $state(false);

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

  // Toast State
  let toast = $state<{ message: string; type: 'success' | 'error' | 'info' } | null>(null);

  // Navigation Functions
  function navigateTo(view: string) {
    console.log('navigateTo called with:', view, 'current activeView:', activeView);
    const typedView = view as ViewType;
    if (typedView !== activeView) {
      viewHistory = [...viewHistory, typedView];
      activeView = typedView;
      console.log('View changed to:', activeView);
    } else {
      console.log('View unchanged (already on this view)');
    }
  }

  function goBack() {
    if (viewHistory.length > 1) {
      viewHistory = viewHistory.slice(0, -1);
      activeView = viewHistory[viewHistory.length - 1];
      if (activeView !== 'album') {
        selectedAlbum = null;
      }
    }
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
      artwork,
      title: album.title,
      artist: album.artist?.name || 'Unknown Artist',
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
        samplingRate: track.maximum_sampling_rate
      })) || []
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
      quality
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

  function toggleFavorite() {
    isFavorite = !isFavorite;
    showToast(isFavorite ? 'Added to favorites' : 'Removed from favorites', 'success');
  }

  // Skip track handlers - wired to backend queue
  async function handleSkipBack() {
    if (!currentTrack) return;
    // If more than 3 seconds in, restart track; otherwise go to previous
    if (currentTime > 3) {
      handleSeek(0);
      return;
    }

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
    }
  }

  async function handleSkipForward() {
    if (!currentTrack) return;

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
    }
  }

  // Helper to play a track from the queue
  async function playQueueTrack(track: BackendQueueTrack) {
    currentTrack = {
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork: track.artwork_url || '',
      duration: track.duration_secs,
      quality: 'Hi-Res' // Will be updated by actual quality info
    };

    duration = track.duration_secs;
    currentTime = 0;

    try {
      await invoke('play_track', { trackId: track.id });
      isPlaying = true;

      // Update MPRIS
      await invoke('set_media_metadata', {
        title: track.title,
        artist: track.artist,
        album: track.album,
        durationSecs: track.duration_secs,
        coverUrl: track.artwork_url
      });

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
        if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          handleSkipBack();
        }
        break;
      case 'ArrowRight':
        if (e.ctrlKey || e.metaKey) {
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

  async function pollPlaybackState() {
    if (!currentTrack) return;

    try {
      const state = await invoke<PlaybackState>('get_playback_state');

      // Only update if we have a matching track
      if (state.track_id === currentTrack.id) {
        currentTime = state.position;
        isPlaying = state.is_playing;

        // Check if track ended - auto-advance to next
        if (state.duration > 0 && state.position >= state.duration - 1 && !state.is_playing && !isAdvancingTrack) {
          console.log('Track finished, advancing to next...');
          isAdvancingTrack = true;

          try {
            const nextTrackResult = await invoke<BackendQueueTrack | null>('next_track');
            if (nextTrackResult) {
              await playQueueTrack(nextTrackResult);
            } else {
              // Queue ended
              console.log('Queue ended');
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
    document.addEventListener('keydown', handleKeydown);
    return () => document.removeEventListener('keydown', handleKeydown);
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
      {activeView}
      onNavigate={navigateTo}
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
        />
      {:else if activeView === 'settings'}
        <SettingsView onBack={goBack} />
      {:else if activeView === 'album' && selectedAlbum}
        <AlbumDetailView
          album={selectedAlbum}
          onBack={goBack}
          onTrackPlay={handleAlbumTrackPlay}
          onPlayAll={handlePlayAllAlbum}
          onShuffleAll={handleShuffleAlbum}
          onAddToQueue={handleAddAlbumToQueue}
        />
      {:else if activeView === 'library'}
        <div class="placeholder-view">
          <h1>Library</h1>
          <p>Your library will appear here...</p>
        </div>
      {/if}
    </main>

    <!-- Now Playing Bar -->
    {#if currentTrack}
      <NowPlayingBar
        artwork={currentTrack.artwork}
        trackTitle={currentTrack.title}
        artist={currentTrack.artist}
        quality={currentTrack.quality}
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
      />
    {:else}
      <NowPlayingBar
        onOpenQueue={() => (isQueueOpen = true)}
        onOpenFullScreen={() => (isFullScreenOpen = true)}
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
    margin-bottom: 80px;
    overflow-y: auto;
    padding: 24px 32px;
  }

  .placeholder-view {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
  }

  .placeholder-view h1 {
    font-size: 32px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 8px;
  }

  .placeholder-view p {
    font-size: 16px;
  }
</style>
