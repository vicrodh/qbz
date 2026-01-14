<script lang="ts">
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount } from 'svelte';
  import {
    HardDrive, Music, Disc3, Mic2, FolderPlus, Trash2, RefreshCw,
    Settings, X, Play, AlertCircle, ImageDown, Upload, Search, LayoutGrid, List
  } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import AddToPlaylistModal from '../AddToPlaylistModal.svelte';

  // Backend types matching Rust models
  interface LocalTrack {
    id: number;
    file_path: string;
    title: string;
    artist: string;
    album: string;
    album_artist?: string;
    album_group_key?: string;
    album_group_title?: string;
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
  let albumSearch = $state('');
  let albumViewMode = $state<'grid' | 'list'>('grid');
  type AlbumGroupMode = 'alpha' | 'artist';
  let albumGroupMode = $state<AlbumGroupMode>('alpha');
  let showGroupMenu = $state(false);
  let artistSearch = $state('');
  let trackSearch = $state('');
  type TrackGroupMode = 'album' | 'artist' | 'name';
  let trackGroupMode = $state<TrackGroupMode>('album');
  let showTrackGroupMenu = $state(false);
  let trackSearchTimer: ReturnType<typeof setTimeout> | null = null;

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
  let updatingArtwork = $state(false);
  let hasDiscogsCredentials = $state(false);

  // Album detail state (for viewing album tracks)
  let selectedAlbum = $state<LocalAlbum | null>(null);
  let albumTracks = $state<LocalTrack[]>([]);

  // Playlist modal state
  let showPlaylistModal = $state(false);
  let selectedTrackForPlaylist = $state<LocalTrack | null>(null);

  async function handleAddToPlaylist(playlistId: number) {
    if (!selectedTrackForPlaylist) return;

    // Get the current count of local tracks to determine position
    const existingTracks = await invoke<LocalTrack[]>('playlist_get_local_tracks', { playlistId });
    const position = existingTracks.length;

    await invoke('playlist_add_local_track', {
      playlistId,
      localTrackId: selectedTrackForPlaylist.id,
      position
    });
  }

  function openPlaylistPicker(track: LocalTrack) {
    selectedTrackForPlaylist = track;
    showPlaylistModal = true;
  }

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

  async function loadTracks(query = '') {
    loading = true;
    try {
      tracks = await invoke<LocalTrack[]>('library_search', { query, limit: 1000 });
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
      loadTracks(trackSearch.trim());
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

  function applyAlbumArtworkUpdate(groupKey: string, artworkPath: string) {
    albums = albums.map(album =>
      album.id === groupKey ? { ...album, artwork_path: artworkPath } : album
    );
    if (selectedAlbum?.id === groupKey) {
      selectedAlbum = { ...selectedAlbum, artwork_path: artworkPath };
    }
    albumTracks = albumTracks.map(track =>
      track.album_group_key === groupKey ? { ...track, artwork_path: artworkPath } : track
    );
    tracks = tracks.map(track =>
      track.album_group_key === groupKey ? { ...track, artwork_path: artworkPath } : track
    );
  }

  async function handleSetAlbumArtwork() {
    if (!selectedAlbum || updatingArtwork) return;
    try {
      updatingArtwork = true;
      const selected = await open({
        title: 'Select Album Artwork',
        multiple: false,
        directory: false,
        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }]
      });

      if (!selected || typeof selected !== 'string') return;

      const cachedPath = await invoke<string>('library_set_album_artwork', {
        albumGroupKey: selectedAlbum.id,
        artworkPath: selected
      });
      applyAlbumArtworkUpdate(selectedAlbum.id, cachedPath);
    } catch (err) {
      console.error('Failed to set album artwork:', err);
      alert(`Failed to set artwork: ${err}`);
    } finally {
      updatingArtwork = false;
    }
  }

  async function handleAlbumClick(album: LocalAlbum) {
    selectedAlbum = album;
    try {
      albumTracks = await invoke<LocalTrack[]>('library_get_album_tracks', {
        albumGroupKey: album.id
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
      artwork_url: t.artwork_path ? getArtworkUrl(t.artwork_path) : null,
      hires: (t.bit_depth && t.bit_depth > 16) || t.sample_rate > 44100,
      bit_depth: t.bit_depth ?? null,
      sample_rate: t.sample_rate ?? null,
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

  function extractDiscNumber(track: LocalTrack): number {
    if (track.disc_number && track.disc_number > 0) return track.disc_number;

    const album = track.album ?? '';
    const match = album.match(/(?:disc|disk|cd)\s*([0-9]+)/i);
    if (match) {
      const parsed = Number(match[1]);
      if (!Number.isNaN(parsed) && parsed > 0) return parsed;
    }

    return 1;
  }

  function buildAlbumSections(tracks: LocalTrack[]) {
    const sorted = [...tracks].sort((a, b) => {
      const aDisc = extractDiscNumber(a);
      const bDisc = extractDiscNumber(b);
      if (aDisc !== bDisc) return aDisc - bDisc;
      const aTrack = a.track_number ?? 0;
      const bTrack = b.track_number ?? 0;
      if (aTrack !== bTrack) return aTrack - bTrack;
      return a.title.localeCompare(b.title);
    });

    const groups = new Map<number, LocalTrack[]>();
    for (const track of sorted) {
      const disc = extractDiscNumber(track);
      if (!groups.has(disc)) {
        groups.set(disc, []);
      }
      groups.get(disc)?.push(track);
    }

    const discs = [...groups.keys()].sort((a, b) => a - b);
    return discs.map(disc => ({
      disc,
      label: `Disc ${disc}`,
      tracks: groups.get(disc) ?? []
    }));
  }

  function getArtworkUrl(path?: string): string {
    if (!path) return '';
    return convertFileSrc(path);
  }

  function matchesAlbumSearch(album: LocalAlbum, query: string): boolean {
    const needle = query.trim().toLowerCase();
    if (!needle) return true;
    return (
      album.title.toLowerCase().includes(needle) ||
      album.artist.toLowerCase().includes(needle)
    );
  }

  function matchesArtistSearch(artist: LocalArtist, query: string): boolean {
    const needle = query.trim().toLowerCase();
    if (!needle) return true;
    return artist.name.toLowerCase().includes(needle);
  }

  const alphaIndexLetters = ['#', ...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'];

  function alphaGroupKey(title: string): string {
    const trimmed = title.trim();
    if (!trimmed) return '#';
    const first = trimmed[0].toUpperCase();
    return first >= 'A' && first <= 'Z' ? first : '#';
  }

  function slugify(value: string): string {
    return value
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'group';
  }

  function groupIdForKey(prefix: string, key: string): string {
    if (key === '#') {
      return `${prefix}-num`;
    }
    return `${prefix}-${slugify(key)}`;
  }

  function groupAlbums(items: LocalAlbum[], mode: AlbumGroupMode) {
    const prefix = `album-${mode}`;
    const sorted = [...items].sort((a, b) => {
      if (mode === 'artist') {
        const artistCmp = a.artist.localeCompare(b.artist);
        if (artistCmp !== 0) return artistCmp;
        return a.title.localeCompare(b.title);
      }
      return a.title.localeCompare(b.title);
    });

    const groups = new Map<string, LocalAlbum[]>();
    for (const album of sorted) {
      const key = mode === 'artist' ? album.artist : alphaGroupKey(album.title);
      if (!groups.has(key)) {
        groups.set(key, []);
      }
      groups.get(key)?.push(album);
    }

    const keys = [...groups.keys()].sort((a, b) => {
      if (mode === 'alpha') {
        if (a === '#') return -1;
        if (b === '#') return 1;
      }
      return a.localeCompare(b);
    });

    return keys.map(key => ({
      key,
      id: groupIdForKey(prefix, key),
      albums: groups.get(key) ?? []
    }));
  }

  function scrollToGroup(prefix: string, letter: string, available: Set<string>) {
    if (!available.has(letter)) return;
    const id = groupIdForKey(prefix, letter);
    const target = document.getElementById(id);
    target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  function scrollToGroupId(groupId?: string) {
    if (!groupId) return;
    const target = document.getElementById(groupId);
    target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  function scheduleTrackSearch() {
    if (trackSearchTimer) {
      clearTimeout(trackSearchTimer);
    }
    trackSearchTimer = setTimeout(() => {
      loadTracks(trackSearch.trim());
    }, 250);
  }

  function trackSortValue(track: LocalTrack) {
    const disc = track.disc_number ?? 0;
    const trackNumber = track.track_number ?? 0;
    return { disc, trackNumber };
  }

  function normalizeAlbumTitle(title: string): string {
    const trimmed = title.trim();
    if (!trimmed) return trimmed;

    let normalized = trimmed.replace(/\s*[\[(](disc|disk|cd)\s*\d+[\])]\s*$/i, '');
    normalized = normalized.replace(/\s+(disc|disk|cd)\s*\d+\s*$/i, '');
    return normalized.trim() || trimmed;
  }

  function groupTracks(items: LocalTrack[], mode: TrackGroupMode) {
    const prefix = `track-${mode}`;
    const sorted = [...items].sort((a, b) => {
      if (mode === 'album') {
        const albumCmp = a.album.localeCompare(b.album);
        if (albumCmp !== 0) return albumCmp;
        const artistCmp = a.artist.localeCompare(b.artist);
        if (artistCmp !== 0) return artistCmp;
        const aOrder = trackSortValue(a);
        const bOrder = trackSortValue(b);
        if (aOrder.disc !== bOrder.disc) return aOrder.disc - bOrder.disc;
        if (aOrder.trackNumber !== bOrder.trackNumber) return aOrder.trackNumber - bOrder.trackNumber;
        return a.title.localeCompare(b.title);
      }
      if (mode === 'artist') {
        const artistCmp = a.artist.localeCompare(b.artist);
        if (artistCmp !== 0) return artistCmp;
        const albumCmp = a.album.localeCompare(b.album);
        if (albumCmp !== 0) return albumCmp;
        const aOrder = trackSortValue(a);
        const bOrder = trackSortValue(b);
        if (aOrder.disc !== bOrder.disc) return aOrder.disc - bOrder.disc;
        if (aOrder.trackNumber !== bOrder.trackNumber) return aOrder.trackNumber - bOrder.trackNumber;
        return a.title.localeCompare(b.title);
      }
      const titleCmp = a.title.localeCompare(b.title);
      if (titleCmp !== 0) return titleCmp;
      const artistCmp = a.artist.localeCompare(b.artist);
      if (artistCmp !== 0) return artistCmp;
      return a.album.localeCompare(b.album);
    });

    const groups = new Map<string, { title: string; subtitle?: string; tracks: LocalTrack[]; artists: Set<string> }>();
    for (const track of sorted) {
      if (mode === 'album') {
        const title = track.album_group_title?.trim() || normalizeAlbumTitle(track.album);
        const albumArtist = track.album_artist?.trim() || '';
        const groupKey = track.album_group_key?.trim()
          ? track.album_group_key
          : albumArtist
            ? `${title}|||${albumArtist}`
            : title;
        const entry = groups.get(groupKey);
        if (!entry) {
          groups.set(groupKey, {
            title,
            subtitle: albumArtist || undefined,
            tracks: [track],
            artists: new Set([track.artist || 'Unknown Artist'])
          });
        } else {
          entry.tracks.push(track);
          if (albumArtist && !entry.subtitle) {
            entry.subtitle = albumArtist;
          }
          entry.artists.add(track.artist || 'Unknown Artist');
        }
      } else if (mode === 'artist') {
        const key = track.artist || 'Unknown Artist';
        if (!groups.has(key)) {
          groups.set(key, { title: key, tracks: [], artists: new Set([key]) });
        }
        groups.get(key)?.tracks.push(track);
      } else {
        const key = alphaGroupKey(track.title);
        if (!groups.has(key)) {
          groups.set(key, { title: key, tracks: [], artists: new Set() });
        }
        groups.get(key)?.tracks.push(track);
      }
    }

    const keys = [...groups.keys()].sort((a, b) => {
      if (mode === 'name') {
        if (a === '#') return -1;
        if (b === '#') return 1;
      }
      if (mode === 'album') {
        const titleCmp = (groups.get(a)?.title ?? a).localeCompare(groups.get(b)?.title ?? b);
        if (titleCmp !== 0) return titleCmp;
      }
      return a.localeCompare(b);
    });

    return keys.map(key => ({
      key,
      id: groupIdForKey(prefix, key),
      title: groups.get(key)?.title ?? key,
      subtitle: (() => {
        const entry = groups.get(key);
        if (!entry) return undefined;
        if (mode === 'album') {
          if (entry.subtitle) return entry.subtitle;
          const artists = [...entry.artists];
          if (artists.length > 1) return 'Various Artists';
          return artists[0];
        }
        return entry.subtitle;
      })(),
      tracks: groups.get(key)?.tracks ?? []
    }));
  }
</script>

<div class="library-view">
  {#if selectedAlbum}
    {@const albumSections = buildAlbumSections(albumTracks)}
    {@const showDiscHeaders = albumSections.length > 1}
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
            <button
              class="secondary-btn"
              onclick={handleSetAlbumArtwork}
              disabled={updatingArtwork}
              title="Set album artwork"
            >
              <Upload size={14} />
              <span>{updatingArtwork ? 'Updating...' : 'Set Cover'}</span>
            </button>
          </div>
        </div>
      </div>

      <div class="track-list">
        {#each albumSections as section (section.disc)}
          {#if showDiscHeaders}
            <div class="disc-header">{section.label}</div>
          {/if}
          {#each section.tracks as track, index (track.id)}
            <TrackRow
              number={track.track_number ?? index + 1}
              title={track.title}
              artist={track.artist !== selectedAlbum?.artist ? track.artist : undefined}
              duration={formatDuration(track.duration_secs)}
              quality={getQualityBadge(track)}
              hideDownload={true}
              hideFavorite={true}
              onPlay={() => handleTrackPlay(track)}
              menuActions={{
                onPlayNow: () => handleTrackPlay(track),
                onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
                onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined,
                onAddToPlaylist: () => openPlaylistPicker(track)
              }}
            />
          {/each}
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
          {@const filteredAlbums = albumSearch.trim()
            ? albums.filter(album => matchesAlbumSearch(album, albumSearch))
            : albums}

          <div class="album-controls">
            <div class="search-container">
              <Search size={16} class="search-icon" />
              <input
                type="text"
                placeholder="Search albums or artists..."
                bind:value={albumSearch}
                class="search-input"
              />
              {#if albumSearch}
                <button class="clear-search" onclick={() => (albumSearch = '')}>
                  <X size={14} />
                </button>
              {/if}
            </div>

            <div class="dropdown-container">
              <button
                class="control-btn"
                onclick={() => (showGroupMenu = !showGroupMenu)}
                title="Group albums"
              >
                <span>{albumGroupMode === 'alpha' ? 'Group: A-Z' : 'Group: Artist'}</span>
              </button>
              {#if showGroupMenu}
                <div class="dropdown-menu">
                  <button
                    class="dropdown-item"
                    class:selected={albumGroupMode === 'alpha'}
                    onclick={() => { albumGroupMode = 'alpha'; showGroupMenu = false; }}
                  >
                    Alphabetical (A-Z)
                  </button>
                  <button
                    class="dropdown-item"
                    class:selected={albumGroupMode === 'artist'}
                    onclick={() => { albumGroupMode = 'artist'; showGroupMenu = false; }}
                  >
                    Artist
                  </button>
                </div>
              {/if}
            </div>

            <button
              class="control-btn icon-only"
              onclick={() => (albumViewMode = albumViewMode === 'list' ? 'grid' : 'list')}
              title={albumViewMode === 'list' ? 'Grid view' : 'List view'}
            >
              {#if albumViewMode === 'list'}
                <LayoutGrid size={16} />
              {:else}
                <List size={16} />
              {/if}
            </button>

            <span class="album-count">{filteredAlbums.length} albums</span>
          </div>

          {#if filteredAlbums.length === 0}
            <div class="empty">
              <Disc3 size={48} />
              <p>No albums match your search</p>
              <p class="empty-hint">Try a different artist or album name</p>
            </div>
          {:else}
          {@const groupedAlbums = groupAlbums(filteredAlbums, albumGroupMode)}
          {@const alphaGroups = albumGroupMode === 'alpha'
            ? new Set(groupedAlbums.map(group => group.key))
            : new Set<string>()}

            <div class="album-sections">
              <div class="album-group-list">
                {#each groupedAlbums as group (group.id)}
                  <div class="album-group" id={group.id}>
                    <div class="album-group-header">
                      <span class="album-group-title">{group.key}</span>
                      <span class="album-group-count">{group.albums.length}</span>
                    </div>
                    {#if albumViewMode === 'grid'}
                      <div class="album-grid">
                        {#each group.albums as album (album.id)}
                          <AlbumCard
                            artwork={getArtworkUrl(album.artwork_path)}
                            title={album.title}
                            artist={album.artist}
                            quality={getAlbumQualityBadge(album)}
                            onclick={() => handleAlbumClick(album)}
                          />
                        {/each}
                      </div>
                    {:else}
                      <div class="album-list">
                        {#each group.albums as album (album.id)}
                          <div class="album-row" role="button" tabindex="0" onclick={() => handleAlbumClick(album)}>
                            <div class="album-row-art">
                              {#if album.artwork_path}
                                <img src={getArtworkUrl(album.artwork_path)} alt={album.title} loading="lazy" decoding="async" />
                              {:else}
                                <div class="artwork-placeholder">
                                  <Disc3 size={28} />
                                </div>
                              {/if}
                            </div>
                            <div class="album-row-info">
                              <div class="album-row-title truncate">{album.title}</div>
                              <div class="album-row-meta">
                                <span>{album.artist}</span>
                                {#if album.year}<span>{album.year}</span>{/if}
                                <span>{album.track_count} tracks</span>
                                <span>{formatTotalDuration(album.total_duration_secs)}</span>
                              </div>
                            </div>
                            <div class="album-row-quality">
                              <span class="quality-badge" class:hires={isAlbumHiRes(album)}>
                                {getAlbumQualityBadge(album)}
                              </span>
                            </div>
                          </div>
                        {/each}
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>

              {#if albumGroupMode === 'alpha'}
                <div class="alpha-index">
                  {#each alphaIndexLetters as letter}
                    <button
                      class="alpha-letter"
                      class:disabled={!alphaGroups.has(letter)}
                      onclick={() => scrollToGroup('album-alpha', letter, alphaGroups)}
                    >
                      {letter}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      {:else if activeTab === 'artists'}
        {#if artists.length === 0}
          <div class="empty">
            <Mic2 size={48} />
            <p>No artists in library</p>
          </div>
        {:else}
          {@const filteredArtists = artists.filter(artist => matchesArtistSearch(artist, artistSearch))}
          <div class="artist-controls">
            <div class="search-container">
              <Search size={16} class="search-icon" />
              <input
                type="text"
                placeholder="Search artists..."
                bind:value={artistSearch}
                class="search-input"
              />
              {#if artistSearch}
                <button class="clear-search" onclick={() => (artistSearch = '')}>
                  <X size={14} />
                </button>
              {/if}
            </div>
            <span class="album-count">{filteredArtists.length} artists</span>
          </div>

          {#if filteredArtists.length === 0}
            <div class="empty">
              <Mic2 size={48} />
              <p>No artists match your search</p>
            </div>
          {:else}
            <div class="artist-grid">
              {#each filteredArtists as artist (artist.name)}
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
        {/if}
      {:else if activeTab === 'tracks'}
        {#if tracks.length === 0}
          <div class="empty">
            <Music size={48} />
            <p>No tracks in library</p>
          </div>
        {:else}
          <div class="track-controls">
            <div class="search-container">
              <Search size={16} class="search-icon" />
              <input
                type="text"
                placeholder="Search tracks, albums, artists..."
                bind:value={trackSearch}
                oninput={scheduleTrackSearch}
                class="search-input"
              />
              {#if trackSearch}
                <button class="clear-search" onclick={() => { trackSearch = ''; loadTracks(''); }}>
                  <X size={14} />
                </button>
              {/if}
            </div>

            <div class="dropdown-container">
              <button
                class="control-btn"
                onclick={() => (showTrackGroupMenu = !showTrackGroupMenu)}
                title="Group tracks"
              >
                <span>
                  {trackGroupMode === 'album'
                    ? 'Group: Album'
                    : trackGroupMode === 'artist'
                      ? 'Group: Artist'
                      : 'Group: Name'}
                </span>
              </button>
              {#if showTrackGroupMenu}
                <div class="dropdown-menu">
                  <button
                    class="dropdown-item"
                    class:selected={trackGroupMode === 'album'}
                    onclick={() => { trackGroupMode = 'album'; showTrackGroupMenu = false; }}
                  >
                    Album
                  </button>
                  <button
                    class="dropdown-item"
                    class:selected={trackGroupMode === 'artist'}
                    onclick={() => { trackGroupMode = 'artist'; showTrackGroupMenu = false; }}
                  >
                    Artist
                  </button>
                  <button
                    class="dropdown-item"
                    class:selected={trackGroupMode === 'name'}
                    onclick={() => { trackGroupMode = 'name'; showTrackGroupMenu = false; }}
                  >
                    Name (A-Z)
                  </button>
                </div>
              {/if}
            </div>

            <span class="album-count">{tracks.length} tracks</span>
          </div>

          {@const groupedTracks = groupTracks(tracks, trackGroupMode)}
          {@const trackIndexTargets = trackGroupMode === 'artist'
            ? (() => {
                const map = new Map<string, string>();
                for (const group of groupedTracks) {
                  const letter = alphaGroupKey(group.title);
                  if (!map.has(letter)) {
                    map.set(letter, group.id);
                  }
                }
                return map;
              })()
            : new Map<string, string>()}
          {@const trackAlphaGroups = trackGroupMode === 'name'
            ? new Set(groupedTracks.map(group => group.key))
            : trackGroupMode === 'artist'
              ? new Set(trackIndexTargets.keys())
              : new Set<string>()}

          <div class="track-sections">
            <div class="track-group-list">
              {#each groupedTracks as group (group.id)}
                <div class="track-group" id={group.id}>
                  <div class="track-group-header">
                    <div class="track-group-title">{group.title}</div>
                    {#if group.subtitle}
                      <div class="track-group-subtitle">{group.subtitle}</div>
                    {/if}
                    <div class="track-group-count">{group.tracks.length} tracks</div>
                  </div>

                  <div class="track-list">
                    {#if trackGroupMode === 'album'}
                      {@const trackSections = buildAlbumSections(group.tracks)}
                      {@const showTrackDiscHeaders = trackSections.length > 1}
                      {#each trackSections as section (section.disc)}
                        {#if showTrackDiscHeaders}
                          <div class="disc-header">{section.label}</div>
                        {/if}
                        {#each section.tracks as track, index (track.id)}
                          <TrackRow
                            number={track.track_number ?? index + 1}
                            title={track.title}
                            artist={track.artist}
                            duration={formatDuration(track.duration_secs)}
                            quality={getQualityBadge(track)}
                            hideDownload={true}
                            hideFavorite={true}
                            onPlay={() => handleTrackPlay(track)}
                            menuActions={{
                              onPlayNow: () => handleTrackPlay(track),
                              onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
                              onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined,
                              onAddToPlaylist: () => openPlaylistPicker(track)
                            }}
                          />
                        {/each}
                      {/each}
                    {:else}
                      {#each group.tracks as track, index (track.id)}
                        <TrackRow
                          number={track.track_number ?? index + 1}
                          title={track.title}
                          artist={track.artist}
                          duration={formatDuration(track.duration_secs)}
                          quality={getQualityBadge(track)}
                          hideDownload={true}
                          hideFavorite={true}
                          onPlay={() => handleTrackPlay(track)}
                          menuActions={{
                            onPlayNow: () => handleTrackPlay(track),
                            onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
                            onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined,
                            onAddToPlaylist: () => openPlaylistPicker(track)
                          }}
                        />
                      {/each}
                    {/if}
                  </div>
                </div>
              {/each}
            </div>

            {#if trackGroupMode === 'name' || trackGroupMode === 'artist'}
              <div class="alpha-index">
                {#each alphaIndexLetters as letter}
                  <button
                    class="alpha-letter"
                    class:disabled={!trackAlphaGroups.has(letter)}
                    onclick={() => trackGroupMode === 'artist'
                      ? scrollToGroupId(trackIndexTargets.get(letter))
                      : scrollToGroup(`track-${trackGroupMode}`, letter, trackAlphaGroups)}
                  >
                    {letter}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<!-- Add to Playlist Modal -->
<AddToPlaylistModal
  isOpen={showPlaylistModal}
  onClose={() => { showPlaylistModal = false; selectedTrackForPlaylist = null; }}
  onSelect={handleAddToPlaylist}
  trackTitle={selectedTrackForPlaylist?.title}
/>

<style>
  .library-view {
    padding: 24px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  /* Custom scrollbar */
  .library-view::-webkit-scrollbar {
    width: 6px;
  }

  .library-view::-webkit-scrollbar-track {
    background: transparent;
  }

  .library-view::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .library-view::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
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
    font-size: 24px;
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
    border-radius: 8px;
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

  /* Album Controls */
  .album-controls {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }

  .artist-controls {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }

  .search-container {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    padding: 8px 12px;
    flex: 1;
    max-width: 420px;
  }

  .search-icon {
    color: var(--text-muted);
  }

  .search-input {
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    width: 100%;
    outline: none;
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .clear-search {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
  }

  .clear-search:hover {
    color: var(--text-primary);
  }

  .control-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .control-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .control-btn.icon-only {
    width: 36px;
    height: 36px;
    justify-content: center;
    padding: 0;
  }

  .album-count {
    font-size: 12px;
    color: var(--text-muted);
    margin-left: auto;
  }

  .dropdown-container {
    position: relative;
  }

  .dropdown-menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    padding: 6px;
    min-width: 180px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
    z-index: 20;
  }

  .dropdown-item {
    width: 100%;
    text-align: left;
    padding: 8px 10px;
    background: none;
    border: none;
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .dropdown-item:hover {
    background: var(--bg-tertiary);
  }

  .dropdown-item.selected {
    background: var(--bg-tertiary);
    font-weight: 600;
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

  .album-sections {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .album-group-list {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .album-group {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .album-group-header {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .album-group-title {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .album-group-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  /* Album List */
  .album-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .album-row {
    display: grid;
    grid-template-columns: 56px 1fr auto;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    background: var(--bg-secondary);
    border-radius: 10px;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .album-row:hover {
    background: var(--bg-tertiary);
  }

  .album-row-art {
    width: 52px;
    height: 52px;
    border-radius: 8px;
    overflow: hidden;
    flex-shrink: 0;
  }

  .album-row-art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .album-row-info {
    min-width: 0;
  }

  .album-row-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .album-row-meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    font-size: 12px;
    color: var(--text-muted);
  }

  .album-row-meta span + span::before {
    content: "\2022";
    margin: 0 8px;
    color: var(--text-muted);
  }

  .album-row-quality {
    display: flex;
    justify-content: flex-end;
  }

  .album-row-quality .quality-badge {
    font-size: 11px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.85);
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 6px;
    padding: 3px 8px;
  }

  .album-row-quality .quality-badge.hires {
    background: linear-gradient(135deg, #f59e0b 0%, #d97706 100%);
    color: white;
    border-color: transparent;
  }

  /* Track Controls */
  .track-controls {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }

  .track-sections {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .track-group-list {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .track-group {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .track-group-header {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 8px;
  }

  .track-group-title {
    font-size: 14px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .track-group-subtitle {
    font-size: 12px;
    color: var(--text-muted);
  }

  .track-group-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .alpha-index {
    position: sticky;
    top: 120px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px 4px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.2);
  }

  .alpha-letter {
    width: 20px;
    height: 20px;
    padding: 0;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    opacity: 0.9;
  }

  .alpha-letter:hover {
    color: var(--accent-primary);
  }

  .alpha-letter.disabled {
    opacity: 0.25;
    cursor: default;
    pointer-events: none;
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

  .disc-header {
    margin-top: 16px;
    margin-bottom: 8px;
    font-size: 12px;
    font-weight: 700;
    color: var(--text-secondary);
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .track-list .disc-header:first-child {
    margin-top: 0;
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
    font-size: 24px;
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
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .play-btn:hover {
    background: var(--accent-hover);
  }
</style>
