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

  // Queue State
  let queue = $state<QueueTrack[]>([]);

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

    // Try to play the track
    try {
      console.log('Invoking play_track with trackId:', track.id);
      showToast(`Loading: ${track.title}`, 'info');
      await invoke('play_track', { trackId: track.id });
      console.log('play_track invoke succeeded');
      isPlaying = true;
      showToast(`Playing: ${track.title}`, 'success');
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

  function toggleShuffle() {
    isShuffle = !isShuffle;
    showToast(isShuffle ? 'Shuffle enabled' : 'Shuffle disabled', 'info');
  }

  function toggleRepeat() {
    if (repeatMode === 'off') repeatMode = 'all';
    else if (repeatMode === 'all') repeatMode = 'one';
    else repeatMode = 'off';

    const messages = { off: 'Repeat off', all: 'Repeat all', one: 'Repeat one' };
    showToast(messages[repeatMode], 'info');
  }

  function toggleFavorite() {
    isFavorite = !isFavorite;
    showToast(isFavorite ? 'Added to favorites' : 'Removed from favorites', 'success');
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

  // Playback progress timer
  let progressInterval: ReturnType<typeof setInterval> | null = null;

  $effect(() => {
    if (isPlaying && currentTrack) {
      progressInterval = setInterval(() => {
        if (currentTime < duration) {
          currentTime += 1;
        } else {
          isPlaying = false;
        }
      }, 1000);
    } else if (progressInterval) {
      clearInterval(progressInterval);
      progressInterval = null;
    }

    return () => {
      if (progressInterval) clearInterval(progressInterval);
    };
  });

  onMount(() => {
    document.addEventListener('keydown', handleKeydown);
    return () => document.removeEventListener('keydown', handleKeydown);
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
      onClearQueue={() => {
        queue = [];
        showToast('Queue cleared', 'info');
      }}
      onSaveAsPlaylist={() => showToast('Saved as playlist', 'success')}
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
