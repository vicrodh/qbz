<script lang="ts">
  import { ArrowLeft, Play, Shuffle, ListMusic, Search, X, ChevronDown, ChevronRight, ChevronUp, ImagePlus, Edit3, BarChart2, Heart, CloudDownload, ListPlus, GripVertical } from 'lucide-svelte';
  import AlbumMenu from '../AlbumMenu.svelte';
  import PlaylistCollage from '../PlaylistCollage.svelte';
  import PlaylistModal from '../PlaylistModal.svelte';
  import TrackReplacementModal from '../TrackReplacementModal.svelte';
  import ViewTransition from '../ViewTransition.svelte';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { invoke } from '@tauri-apps/api/core';
  import { open, ask } from '@tauri-apps/plugin-dialog';
  import TrackRow from '../TrackRow.svelte';
  import PlaylistSuggestions from '../PlaylistSuggestions.svelte';
  import { extractAdaptiveArtists } from '$lib/services/playlistSuggestionsService';
  import { type OfflineCacheStatus, cacheTrackForOffline, getOfflineCacheState } from '$lib/stores/offlineCacheState';
  import {
    subscribe as subscribeOffline,
    getStatus as getOfflineStatus,
    type OfflineStatus
  } from '$lib/stores/offlineStore';
  import { consumeContextTrackFocus, setPlaybackContext } from '$lib/stores/playbackContextStore';
  import { saveScrollPosition, getSavedScrollPosition } from '$lib/stores/navigationStore';
  import { isTrackUnavailable, clearTrackUnavailable, subscribe as subscribeUnavailable } from '$lib/stores/unavailableTracksStore';
  import { isBlacklisted as isArtistBlacklisted } from '$lib/stores/artistBlacklistStore';
  import { showToast } from '$lib/stores/toastStore';
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { onMount, tick } from 'svelte';

  interface PlaylistTrack {
    id: number;
    title: string;
    duration: number;
    track_number: number;
    performer?: { id?: number; name: string };
    album?: {
      id: string;
      title: string;
      image: { small?: string; thumbnail?: string; large?: string };
      label?: { id: number; name: string };
    };
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
    isrc?: string;
    playlist_track_id?: number; // Qobuz playlist-specific ID for removal
    streamable?: boolean; // Whether track is available on Qobuz (false = removed)
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

  interface PlaylistWithTrackIds {
    id: number;
    name: string;
    description?: string;
    owner: { id: number; name: string };
    images?: string[];
    tracks_count: number;
    duration: number;
    is_public: boolean;
    track_ids: number[];
    images150?: string[];
    images300?: string[];
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
    label?: string;           // Record label name from Qobuz
    addedIndex?: number;      // Original position in playlist (proxy for date added)
    customPosition?: number;  // User-defined position for custom arrange mode
    streamable?: boolean;     // Whether track is available on Qobuz (false = removed)
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

  // Local track with playlist position (for mixed ordering)
  interface PlaylistLocalTrack {
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
    playlist_position: number;
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
    is_favorite?: boolean;
    folder_id?: string | null;
  }

  interface PlaylistStats {
    qobuz_playlist_id: number;
    play_count: number;
    last_played_at?: number;
  }

  type SortField = 'default' | 'title' | 'artist' | 'album' | 'duration' | 'added' | 'label' | 'custom';
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
    onTrackShowInfo?: (trackId: number) => void;
    onTrackDownload?: (track: DisplayTrack) => void;
    onTrackRemoveDownload?: (trackId: number) => void;
    onTrackReDownload?: (track: DisplayTrack) => void;
    getTrackOfflineCacheStatus?: (trackId: number) => { status: OfflineCacheStatus; progress: number };
    downloadStateVersion?: number;
    onLocalTrackPlay?: (track: LocalLibraryTrack) => void;
    onLocalTrackPlayNext?: (track: LocalLibraryTrack) => void;
    onLocalTrackPlayLater?: (track: LocalLibraryTrack) => void;
    onSetLocalQueue?: (trackIds: number[]) => void;
    onPlaylistCountUpdate?: (playlistId: number, qobuzCount: number, localCount: number) => void;
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
    onTrackShowInfo,
    onTrackDownload,
    onTrackRemoveDownload,
    onTrackReDownload,
    getTrackOfflineCacheStatus,
    downloadStateVersion,
    onLocalTrackPlay,
    onLocalTrackPlayNext,
    onLocalTrackPlayLater,
    onSetLocalQueue,
    onPlaylistCountUpdate,
    onPlaylistUpdated,
    onPlaylistDeleted,
    activeTrackId = null,
    isPlaybackActive = false
  }: Props = $props();

  let playlist = $state<Playlist | null>(null);
  let tracks = $state.raw<DisplayTrack[]>([]);
  let localTracks = $state<PlaylistLocalTrack[]>([]);
  let localTracksMap = $state<Map<number, PlaylistLocalTrack>>(new Map());
  let hasLocalTracks = $derived(localTracks.length > 0);

  // Total counts including local tracks
  let totalTrackCount = $derived((playlist?.tracks_count ?? 0) + localTracks.length);
  let localTracksDuration = $derived(localTracks.reduce((sum, track) => sum + track.duration_secs, 0));
  let totalDuration = $derived((playlist?.duration ?? 0) + localTracksDuration);

  // Suggestions computation gating: ALL playlists require manual activation.
  // >=2000 tracks: never compute (Qobuz limit, can't add more)
  // <2000 tracks: compute only when user manually activates suggestions
  const SUGGESTIONS_AUTO_THRESHOLD = 0;
  const SUGGESTIONS_MAX_TRACKS = 2000;
  let suggestionsActivated = $state(false);

  // Whether suggestions should compute at all for this playlist
  const suggestionsEnabled = $derived(
    (playlist?.tracks_count ?? 0) < SUGGESTIONS_MAX_TRACKS
  );
  // Whether the computation should run now
  const suggestionsReady = $derived(
    suggestionsEnabled && (
      (playlist?.tracks_count ?? 0) <= SUGGESTIONS_AUTO_THRESHOLD || suggestionsActivated
    )
  );

  // Playlist suggestions: adaptive artist selection (quantity scales with playlist size,
  // mix of top artists for coherence + random artists for discovery)
  // Only computed when suggestionsReady is true to avoid blocking large playlist loads
  const playlistArtists = $derived(
    suggestionsReady ? extractAdaptiveArtists(tracks.filter(track => !track.isLocal)) : []
  );
  // Track IDs to exclude from suggestions (already in playlist)
  const excludeTrackIds = $derived(
    suggestionsReady ? tracks.filter(track => !track.isLocal).map(track => track.id) : []
  );

  let loading = $state(true);
  let spinnerFading = $state(false);
  let error = $state<string | null>(null);
  let tracksLoadedCount = $state(0); // Progressive loading: how many tracks have full data
  let scrollContainer: HTMLDivElement | null = $state(null);

  // Virtual scrolling for track list
  const TRACK_ROW_HEIGHT = 56; // px, matches TrackRow component
  const VIRTUAL_BUFFER = 5; // extra rows above/below viewport
  let trackListEl: HTMLDivElement | null = $state(null);
  let trackListScrollTop = $state(0); // scroll position relative to track list top
  let trackListViewHeight = $state(800); // visible height of scroll container

  // Offline mode state
  let offlineStatus = $state<OfflineStatus>(getOfflineStatus());
  let tracksWithLocalCopies = $state<Set<number>>(new Set());

  // Track unavailable state version (increments to force re-render)
  let unavailableVersion = $state(0);

  // Local settings state
  let searchQuery = $state('');
  let sortBy = $state<SortField>('default');
  let sortOrder = $state<SortOrder>('asc');
  let customArtworkPath = $state<string | null>(null);
  let showSortMenu = $state(false);
  let playlistSettings = $state<PlaylistSettings | null>(null);
  let playlistStats = $state<PlaylistStats | null>(null);
  let editModalOpen = $state(false);
  let isFavorite = $state(false);

  // Custom order state
  let customOrderMap = $state<Map<string, number>>(new Map());  // "trackId:isLocal" -> position
  let customOrderLoading = $state(false);
  let isCustomOrderMode = $derived(sortBy === 'custom');

  // Drag and drop state
  let draggedTrackIdx = $state<number | null>(null);
  let dragOverIdx = $state<number | null>(null);

  // Batch selection state (for custom order mode)
  let selectedTrackKeys = $state<Set<string>>(new Set());  // Set of "trackId:isLocal" keys
  let isSelectionMode = $derived(isCustomOrderMode && selectedTrackKeys.size > 0);

  // User ownership state (to show "Copy to Library" button for non-owned playlists)
  let currentUserId = $state<number | null>(null);
  let isOwnPlaylist = $derived(playlist !== null && currentUserId !== null && playlist.owner.id === currentUserId);
  let isCopying = $state(false);
  let isCopied = $state(false);

  // Track replacement modal state
  let replacementModalOpen = $state(false);
  let trackToReplace = $state<DisplayTrack | null>(null);

  // Track copied playlists in localStorage
  const COPIED_PLAYLISTS_KEY = 'qbz_copied_playlists';

  function getCopiedPlaylists(): Set<number> {
    try {
      const stored = localStorage.getItem(COPIED_PLAYLISTS_KEY);
      return stored ? new Set(JSON.parse(stored)) : new Set();
    } catch {
      return new Set();
    }
  }

  function markPlaylistAsCopied(id: number) {
    const copied = getCopiedPlaylists();
    copied.add(id);
    localStorage.setItem(COPIED_PLAYLISTS_KEY, JSON.stringify([...copied]));
    isCopied = true;
  }

  function isPlaylistCopied(id: number): boolean {
    return getCopiedPlaylists().has(id);
  }

  // Show copy button only if: not own playlist AND not already copied
  let showCopyButton = $derived(!isOwnPlaylist && playlist !== null && !isCopied);

  async function scrollToTrack(trackId: number) {
    await tick();
    // With virtualized list, find track index and scroll to its computed position
    const trackIndex = displayTracks.findIndex(trk => trk.id === trackId);
    if (trackIndex >= 0 && scrollContainer && trackListEl) {
      const trackOffset = trackIndex * TRACK_ROW_HEIGHT;
      const trackListTop = trackListEl.offsetTop;
      const targetScroll = trackListTop + trackOffset - (scrollContainer.clientHeight / 2) + (TRACK_ROW_HEIGHT / 2);
      scrollContainer.scrollTo({ top: Math.max(0, targetScroll), behavior: 'smooth' });
    }
  }

  // Subscribe to offline status changes and fetch current user ID
  onMount(() => {
    // Fetch current user ID for ownership check
    invoke<number | null>('get_current_user_id').then(userId => {
      currentUserId = userId;
    }).catch(err => {
      console.warn('Failed to get current user ID:', err);
    });

    const unsubscribeOffline = subscribeOffline(() => {
      offlineStatus = getOfflineStatus();
      // Re-check local copies when offline status changes
      if (offlineStatus.isOffline && tracks.length > 0) {
        checkTracksLocalStatus();
      }
    });

    // Subscribe to unavailable tracks store
    const unsubscribeUnavailable = subscribeUnavailable(() => {
      unavailableVersion++;
    });

    // Restore scroll position and initialize virtual scroll dimensions
    requestAnimationFrame(() => {
      if (scrollContainer) {
        trackListViewHeight = scrollContainer.clientHeight;
        const saved = getSavedScrollPosition('playlist');
        if (saved > 0) {
          scrollContainer.scrollTop = saved;
        }
      }
    });

    // Track scroll container resize for virtual scrolling
    let resizeObserver: ResizeObserver | null = null;
    if (scrollContainer) {
      resizeObserver = new ResizeObserver((entries) => {
        for (const entry of entries) {
          trackListViewHeight = entry.contentRect.height;
        }
      });
      resizeObserver.observe(scrollContainer);
    }

    return () => {
      unsubscribeOffline();
      unsubscribeUnavailable();
      resizeObserver?.disconnect();
    };
  });

  // Check if this playlist was already copied when playlistId changes
  $effect(() => {
    isCopied = isPlaylistCopied(playlistId);
  });

  $effect(() => {
    if (!playlist || displayTracks.length === 0) return;
    const targetId = consumeContextTrackFocus('playlist', playlist.id.toString());
    if (targetId !== null) {
      void scrollToTrack(targetId);
    }
  });

  // Check if a track was removed from Qobuz (streamable: false or marked in unavailable store)
  function isTrackRemovedFromQobuz(track: DisplayTrack): boolean {
    if (track.isLocal) return false;
    // Check API streamable flag
    if (track.streamable === false) return true;
    // Check local unavailable store (marked during playback errors)
    // Reference unavailableVersion to trigger reactivity
    void unavailableVersion;
    return isTrackUnavailable(track.id);
  }

  // Check if a track is available (has local copy when offline, always available when online, unless removed from Qobuz)
  function isTrackAvailable(track: DisplayTrack): boolean {
    // Tracks removed from Qobuz are never available
    if (isTrackRemovedFromQobuz(track)) return false;
    // When online, Qobuz tracks are available
    if (!offlineStatus.isOffline) return true;
    // Local tracks are always available
    if (track.isLocal) return true;
    // When offline, check if we have a local copy
    return tracksWithLocalCopies.has(track.id);
  }

  // Check which tracks have local copies (for offline mode)
  async function checkTracksLocalStatus() {
    if (!offlineStatus.isOffline || tracks.length === 0) {
      tracksWithLocalCopies = new Set();
      return;
    }

    try {
      const qobuzTrackIds = tracks.filter(trk => !trk.isLocal).map(trk => trk.id);
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

  // Helper to notify parent of track counts (called imperatively, not reactively)
  function notifyParentOfCounts() {
    if (playlist) {
      const qobuzCount = playlist.tracks_count ?? 0;
      const localCount = localTracks.length;
      onPlaylistCountUpdate?.(playlistId, qobuzCount, localCount);
    }
  }

  // Reload playlist when playlistId changes
  $effect(() => {
    // Access playlistId to create dependency
    const id = playlistId;
    const t0 = performance.now();
    console.log(`[Perf] ---- playlist switch START (id=${id}) ----`);
    // FIRST: clear old data so no $derived recomputes against stale 800+ item arrays
    tracks = [];
    localTracks = [];
    localTracksMap = new Map();
    playlist = null;
    console.log(`[Perf] clear old data: ${(performance.now() - t0).toFixed(1)}ms`);
    // Reset suggestions state for new playlist
    suggestionsActivated = false;
    // Reset virtual scroll state so new playlist renders from the top
    trackListScrollTop = 0;
    if (scrollContainer) {
      scrollContainer.scrollTop = 0;
    }
    console.log(`[Perf] reset state: ${(performance.now() - t0).toFixed(1)}ms`);
    // Load all data and notify parent when done
    const loadStart = performance.now();
    (async () => {
      await Promise.all([loadPlaylist(), loadLocalTracks()]);
      console.log(`[Perf] all loads resolved: ${(performance.now() - t0).toFixed(1)}ms (network: ${(performance.now() - loadStart).toFixed(1)}ms)`);
      notifyParentOfCounts();
    })();
    loadSettings();
    loadStats();
    console.log(`[Perf] effect sync done: ${(performance.now() - t0).toFixed(1)}ms`);
  });

  // Check local track status after loading tracks and when offline
  $effect(() => {
    if (offlineStatus.isOffline && tracks.length > 0) {
      checkTracksLocalStatus();
    }
  });


  async function loadLocalTracks() {
    const _lt0 = performance.now();
    try {
      // Check if this is a pending playlist (negative ID)
      if (playlistId < 0) {
        // For pending playlists, load local tracks from the pending playlist data
        const pendingId = -playlistId;
        const pendingPlaylists = await invoke<import('$lib/stores/offlineStore').PendingPlaylist[]>('get_pending_playlists');
        const pending = pendingPlaylists.find(p => p.id === pendingId);

        if (pending && pending.localTrackIds.length > 0) {
          // Load the actual local track data
          const localTrackData = await invoke<LocalLibraryTrack[]>('library_get_tracks_by_ids', {
            trackIds: pending.localTrackIds
          });

          // Convert to PlaylistLocalTrack format with positions
          localTracks = localTrackData.map((track, idx) => ({
            ...track,
            playlist_position: pending.trackIds.length + idx // Local tracks come after Qobuz tracks
          }));
          localTracksMap = new Map(localTracks.map(trk => [trk.id, trk]));
        } else {
          localTracks = [];
          localTracksMap = new Map();
        }
      } else {
        // Regular playlist - use existing command
        const result = await invoke<PlaylistLocalTrack[]>('playlist_get_local_tracks_with_position', { playlistId });
        localTracks = result;
        localTracksMap = new Map(result.map(trk => [trk.id, trk]));
      }
    } catch (err) {
      console.error('Failed to load local tracks:', err);
      localTracks = [];
      localTracksMap = new Map();
    } finally {
      console.log(`[Perf] loadLocalTracks() done: ${(performance.now() - _lt0).toFixed(1)}ms`);
    }
  }

  /** Convert a raw PlaylistTrack from the API into a DisplayTrack */
  function mapPlaylistTrack(track: PlaylistTrack, idx: number): DisplayTrack {
    return {
      id: track.id,
      number: idx + 1,
      title: track.title,
      artist: track.performer?.name,
      album: track.album?.title,
      albumArt: track.album?.image?.large || track.album?.image?.thumbnail || track.album?.image?.small,
      albumId: track.album?.id,
      artistId: track.performer?.id,
      duration: formatDuration(track.duration),
      durationSeconds: track.duration,
      hires: track.hires,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate,
      isrc: track.isrc,
      playlistTrackId: track.playlist_track_id,
      label: track.album?.label?.name,
      addedIndex: idx,
      streamable: track.streamable,
    };
  }

  /**
   * Playlist loading — adaptive strategy:
   * - Small playlists (≤500 tracks): single get_playlist call (full data, no placeholders)
   * - Large playlists (>500 tracks): progressive via get_playlist_track_ids + get_tracks_batch
   * - Pending playlists (negative ID): offline path
   */
  const PROGRESSIVE_THRESHOLD = 500;

  async function loadPlaylist() {
    const _t0 = performance.now();
    console.log(`[Perf] loadPlaylist() START`);
    loading = true;
    error = null;
    tracksLoadedCount = 0;
    try {
      if (playlistId < 0) {
        // === Pending (offline) playlist ===
        const pendingId = -playlistId;
        const pendingPlaylists = await invoke<import('$lib/stores/offlineStore').PendingPlaylist[]>('get_pending_playlists');
        const pending = pendingPlaylists.find(p => p.id === pendingId);

        if (!pending) {
          throw new Error('Pending playlist not found');
        }

        playlist = {
          id: playlistId,
          name: pending.name,
          description: pending.description || undefined,
          owner: { id: 0, name: 'You (Offline)' },
          images: [],
          tracks_count: pending.trackIds.length,
          duration: 0,
          is_public: pending.isPublic,
          tracks: { items: [], total: 0 }
        };

        if (pending.trackIds.length > 0) {
          try {
            const qobuzTracks = await invoke<PlaylistTrack[]>('get_tracks_by_ids', {
              trackIds: pending.trackIds
            });
            tracks = qobuzTracks.map((track, idx) => mapPlaylistTrack(track, idx));
            tracksLoadedCount = tracks.length;
            playlist.duration = qobuzTracks.reduce((sum, track) => sum + track.duration, 0);
          } catch (err) {
            console.error('Failed to load Qobuz tracks for pending playlist:', err);
            tracks = [];
          }
        } else {
          tracks = [];
        }
      } else {
        // === Regular playlist — decide strategy based on track count ===

        // Phase 1: lightweight metadata + track IDs (always fast)
        console.log(`[Perf] invoke get_playlist_track_ids START (+${(performance.now() - _t0).toFixed(1)}ms)`);
        const meta = await invoke<PlaylistWithTrackIds>('get_playlist_track_ids', { playlistId });
        console.log(`[Perf] invoke get_playlist_track_ids DONE (+${(performance.now() - _t0).toFixed(1)}ms) — ${meta.track_ids.length} IDs`);

        // Set playlist metadata immediately
        playlist = {
          id: meta.id,
          name: meta.name,
          description: meta.description || undefined,
          owner: meta.owner,
          images: meta.images300 ?? meta.images150 ?? meta.images,
          tracks_count: meta.tracks_count,
          duration: meta.duration,
          is_public: meta.is_public,
        };

        const allTrackIds = meta.track_ids;

        if (allTrackIds.length <= PROGRESSIVE_THRESHOLD) {
          // --- Small playlist: single get_playlist call, no placeholders ---
          console.log(`[Perf] small playlist (${allTrackIds.length} ≤ ${PROGRESSIVE_THRESHOLD}), using get_playlist`);
          const fullPlaylist = await invoke<Playlist>('get_playlist', { playlistId });
          console.log(`[Perf] get_playlist DONE (+${(performance.now() - _t0).toFixed(1)}ms)`);

          // Use images from meta (collage thumbnails) if the full playlist doesn't have them
          playlist = {
            ...fullPlaylist,
            images: fullPlaylist.images?.length ? fullPlaylist.images : (meta.images300 ?? meta.images150 ?? meta.images),
          };

          tracks = (fullPlaylist.tracks?.items ?? []).map((track, idx) => mapPlaylistTrack(track, idx));
          tracksLoadedCount = tracks.length;
        } else {
          // --- Large playlist: progressive loading ---
          // No placeholders. Start with first batch of real tracks, grow the array.
          // Each batch does ONE array reassignment (not N individual mutations)
          // and yields to the browser so the UI stays responsive.
          console.log(`[Perf] large playlist (${allTrackIds.length} > ${PROGRESSIVE_THRESHOLD}), progressive load`);

          const BATCH_SIZE = 50;
          const CONCURRENCY = 4;
          const batches: number[][] = [];
          for (let i = 0; i < allTrackIds.length; i += BATCH_SIZE) {
            batches.push(allTrackIds.slice(i, i + BATCH_SIZE));
          }

          console.log(`[Perf] progressive: ${batches.length} batches of ${BATCH_SIZE} (+${(performance.now() - _t0).toFixed(1)}ms)`);

          // Load first group (up to 4×50 = 200 tracks) under the spinner
          const firstGroup = batches.slice(0, CONCURRENCY);
          const firstResults = await Promise.all(
            firstGroup.map(batch =>
              invoke<PlaylistTrack[]>('get_tracks_batch', { trackIds: batch })
                .catch(err => {
                  console.warn('[Perf] batch fetch failed:', err);
                  return [] as PlaylistTrack[];
                })
            )
          );

          // Set tracks to ONLY the loaded tracks (no placeholders = no proxy bloat)
          const loaded: DisplayTrack[] = [];
          for (const batchTracks of firstResults) {
            for (const apiTrack of batchTracks) {
              loaded.push(mapPlaylistTrack(apiTrack, loaded.length));
            }
          }
          tracks = loaded;
          tracksLoadedCount = loaded.length;
          console.log(`[Perf] first group: ${loaded.length}/${allTrackIds.length} tracks (+${(performance.now() - _t0).toFixed(1)}ms)`);

          // Spinner dismisses via finally block — viewport has real data now.

          // Load remaining batches in background, appending to tracks.
          // Key: one `tracks = newArray` per group (single reactive update),
          // plus a setTimeout(0) yield between groups so the browser can paint.
          if (batches.length > CONCURRENCY) {
            const remaining = batches.slice(CONCURRENCY);
            (async () => {
              for (let g = 0; g < remaining.length; g += CONCURRENCY) {
                const group = remaining.slice(g, g + CONCURRENCY);

                const results = await Promise.all(
                  group.map(batch =>
                    invoke<PlaylistTrack[]>('get_tracks_batch', { trackIds: batch })
                      .catch(err => {
                        console.warn('[Perf] batch fetch failed:', err);
                        return [] as PlaylistTrack[];
                      })
                  )
                );

                // Build new batch of DisplayTracks
                const currentLen = tracks.length;
                const newTracks: DisplayTrack[] = [];
                for (const batchTracks of results) {
                  for (const apiTrack of batchTracks) {
                    newTracks.push(mapPlaylistTrack(apiTrack, currentLen + newTracks.length));
                  }
                }

                // Single reactive assignment: append batch
                tracks = [...tracks, ...newTracks];
                tracksLoadedCount = tracks.length;
                console.log(`[Perf] loaded ${tracksLoadedCount}/${allTrackIds.length} tracks (+${(performance.now() - _t0).toFixed(1)}ms)`);

                // Yield to browser — let it paint before next group
                await new Promise(r => setTimeout(r, 0));
              }
            })();
          }
        }
      }
    } catch (err) {
      console.error('Failed to load playlist:', err);
      error = String(err);
    } finally {
      console.log(`[Perf] loadPlaylist() done (+${(performance.now() - _t0).toFixed(1)}ms)`);
      if (playlist) {
        console.log(`[Perf] "${playlist.name}" — ${tracks.length} tracks (${tracksLoadedCount} loaded)`);
      }
      if (loading) {
        spinnerFading = true;
        const finalT0 = _t0;
        setTimeout(() => {
          loading = false;
          spinnerFading = false;
          requestAnimationFrame(() => {
            if (scrollContainer) {
              trackListViewHeight = scrollContainer.clientHeight;
            }
            trackListScrollTop = 0;
          });
        }, 200);
      }
    }
  }

  async function loadSettings() {
    const _st0 = performance.now();
    // Reset state before loading new playlist settings
    sortBy = 'default';
    sortOrder = 'asc';
    customArtworkPath = null;
    searchQuery = '';
    playlistSettings = null;
    isFavorite = false;
    customOrderMap = new Map();

    // Skip loading settings for pending playlists
    if (playlistId < 0) {
      return;
    }

    try {
      const settings = await invoke<PlaylistSettings | null>('playlist_get_settings', { playlistId });
      playlistSettings = settings;
      if (settings) {
        sortBy = (settings.sort_by as SortField) || 'default';
        sortOrder = (settings.sort_order as SortOrder) || 'asc';
        customArtworkPath = settings.custom_artwork_path || null;
        searchQuery = settings.last_search_query || '';
        isFavorite = settings.is_favorite ?? false;

        // Load custom order if in custom mode
        if (sortBy === 'custom') {
          await loadOrInitCustomOrder();
        }
      }
    } catch (err) {
      console.error('Failed to load playlist settings:', err);
    } finally {
      console.log(`[Perf] loadSettings() done: ${(performance.now() - _st0).toFixed(1)}ms`);
    }
  }

  async function loadStats() {
    const _stat0 = performance.now();
    // Skip loading stats for pending playlists
    if (playlistId < 0) {
      playlistStats = null;
      return;
    }

    try {
      const stats = await invoke<PlaylistStats | null>('playlist_get_stats', { playlistId });
      playlistStats = stats;
    } catch (err) {
      console.error('Failed to load playlist stats:', err);
    } finally {
      console.log(`[Perf] loadStats() done: ${(performance.now() - _stat0).toFixed(1)}ms`);
    }
  }

  async function toggleFavorite() {
    const newValue = !isFavorite;
    isFavorite = newValue; // Optimistic update
    try {
      await invoke('playlist_set_favorite', { playlistId, favorite: newValue });
    } catch (err) {
      console.error('Failed to toggle favorite:', err);
      isFavorite = !newValue; // Revert on error
    }
  }

  async function copyPlaylistToLibrary() {
    if (isCopying || !playlist) return;

    isCopying = true;
    try {
      const newPlaylist = await invoke<Playlist>('subscribe_playlist', { playlistId: playlist.id });
      // Mark as copied so button disappears
      markPlaylistAsCopied(playlist.id);
      // Notify parent to refresh sidebar
      onPlaylistUpdated?.();
      console.log('Playlist copied successfully:', newPlaylist);
    } catch (err) {
      console.error('Failed to copy playlist:', err);
    } finally {
      isCopying = false;
    }
  }

  async function selectSort(field: SortField) {
    // Default and custom don't have direction toggles
    if (field === 'default' || field === 'custom') {
      sortBy = field;
      sortOrder = 'asc';
    } else if (sortBy === field) {
      // Toggle direction if same field
      sortOrder = sortOrder === 'asc' ? 'desc' : 'asc';
    } else {
      // New field, set default direction
      sortBy = field;
      sortOrder = field === 'added' ? 'desc' : 'asc'; // Added defaults to newest first
    }
    showSortMenu = false;

    // When switching to custom mode, load or initialize custom order
    if (field === 'custom') {
      await loadOrInitCustomOrder();
    }

    try {
      await invoke('playlist_set_sort', { playlistId, sortBy, sortOrder });
    } catch (err) {
      console.error('Failed to save sort settings:', err);
    }
  }

  // Load custom order from backend or initialize if not exists
  async function loadOrInitCustomOrder() {
    if (playlistId < 0) return; // Skip for pending playlists

    customOrderLoading = true;
    try {
      // Check if custom order exists
      const hasOrder = await invoke<boolean>('playlist_has_custom_order', { playlistId });

      if (hasOrder) {
        // Load existing custom order
        const orders = await invoke<[number, boolean, number][]>('playlist_get_custom_order', { playlistId });
        const newMap = new Map<string, number>();
        for (const [trackId, isLocal, position] of orders) {
          newMap.set(`${trackId}:${isLocal}`, position);
        }
        customOrderMap = newMap;
      } else {
        // Initialize from current track arrangement
        await initCustomOrderFromCurrentTracks();
      }
    } catch (err) {
      console.error('Failed to load custom order:', err);
    } finally {
      customOrderLoading = false;
    }
  }

  // Initialize custom order from the current track list
  async function initCustomOrderFromCurrentTracks() {
    // Get all tracks in current display order (before custom sort applied)
    const allTracks = [...tracks];
    const localTracksInPlaylist = localTracks.map((t, idx) => ({
      ...t,
      playlist_position: idx
    }));

    // Build track ID list: (trackId, isLocal)
    const trackIds: [number, boolean][] = [];

    // Add Qobuz tracks first (in original order)
    for (const trk of allTracks) {
      if (!trk.isLocal) {
        trackIds.push([trk.id, false]);
      }
    }

    // Add local tracks (by their position)
    for (const trk of localTracksInPlaylist) {
      trackIds.push([trk.id, true]);
    }

    // Save to backend
    try {
      await invoke('playlist_init_custom_order', { playlistId, trackIds });

      // Update local state
      const newMap = new Map<string, number>();
      for (let i = 0; i < trackIds.length; i++) {
        const [trackId, isLocal] = trackIds[i];
        newMap.set(`${trackId}:${isLocal}`, i);
      }
      customOrderMap = newMap;
    } catch (err) {
      console.error('Failed to initialize custom order:', err);
    }
  }

  // Move a track to a new position
  async function moveTrack(trackId: number, isLocal: boolean, fromIndex: number, toIndex: number) {
    if (fromIndex === toIndex) return;

    // Optimistic update: reorder the map
    const key = `${trackId}:${isLocal}`;
    const newMap = new Map(customOrderMap);

    // Adjust positions
    for (const [k, pos] of newMap) {
      if (k === key) {
        newMap.set(k, toIndex);
      } else if (fromIndex < toIndex) {
        // Moving down: shift tracks between from+1 and to up
        if (pos > fromIndex && pos <= toIndex) {
          newMap.set(k, pos - 1);
        }
      } else {
        // Moving up: shift tracks between to and from-1 down
        if (pos >= toIndex && pos < fromIndex) {
          newMap.set(k, pos + 1);
        }
      }
    }
    customOrderMap = newMap;

    // Persist to backend
    try {
      await invoke('playlist_move_track', {
        playlistId,
        trackId: Math.abs(trackId),
        isLocal,
        newPosition: toIndex
      });
    } catch (err) {
      console.error('Failed to move track:', err);
      // Reload to get consistent state
      await loadOrInitCustomOrder();
    }
  }

  // Helper to move track up one position
  function moveTrackUp(track: DisplayTrack, currentIndex: number) {
    if (currentIndex === 0) return;
    const isLocal = track.isLocal ?? false;
    const trackId = isLocal ? Math.abs(track.id) : track.id;
    moveTrack(trackId, isLocal, currentIndex, currentIndex - 1);
  }

  // Helper to move track down one position
  function moveTrackDown(track: DisplayTrack, currentIndex: number) {
    if (currentIndex >= displayTracks.length - 1) return;
    const isLocal = track.isLocal ?? false;
    const trackId = isLocal ? Math.abs(track.id) : track.id;
    moveTrack(trackId, isLocal, currentIndex, currentIndex + 1);
  }

  // Drag and drop handlers
  function handleDragStart(e: DragEvent, idx: number) {
    if (!isCustomOrderMode) return;
    draggedTrackIdx = idx;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', String(idx));
    }
  }

  function handleDragOver(e: DragEvent, idx: number) {
    if (!isCustomOrderMode || draggedTrackIdx === null) return;
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = 'move';
    }
    dragOverIdx = idx;
  }

  function handleDragLeave() {
    dragOverIdx = null;
  }

  function handleDragEnd() {
    draggedTrackIdx = null;
    dragOverIdx = null;
  }

  function handleDrop(e: DragEvent, toIdx: number) {
    e.preventDefault();
    if (!isCustomOrderMode || draggedTrackIdx === null) return;

    const fromIdx = draggedTrackIdx;
    if (fromIdx !== toIdx) {
      const track = displayTracks[fromIdx];
      const isLocal = track.isLocal ?? false;
      const trackId = isLocal ? Math.abs(track.id) : track.id;
      moveTrack(trackId, isLocal, fromIdx, toIdx);
    }

    draggedTrackIdx = null;
    dragOverIdx = null;
  }

  // === Batch Selection Functions ===

  function getTrackKey(track: DisplayTrack): string {
    const isLocal = track.isLocal ?? false;
    const trackId = isLocal ? Math.abs(track.id) : track.id;
    return `${trackId}:${isLocal}`;
  }

  function toggleTrackSelection(track: DisplayTrack) {
    const key = getTrackKey(track);
    const newSet = new Set(selectedTrackKeys);
    if (newSet.has(key)) {
      newSet.delete(key);
    } else {
      newSet.add(key);
    }
    selectedTrackKeys = newSet;
  }

  function clearSelection() {
    selectedTrackKeys = new Set();
  }

  function selectAllTracks() {
    const newSet = new Set<string>();
    for (const track of displayTracks) {
      newSet.add(getTrackKey(track));
    }
    selectedTrackKeys = newSet;
  }

  // Move all selected tracks up one position (as a group)
  async function moveSelectedUp() {
    if (selectedTrackKeys.size === 0) return;

    // Get indices of selected tracks (sorted)
    const selectedIndices: number[] = [];
    displayTracks.forEach((track, idx) => {
      if (selectedTrackKeys.has(getTrackKey(track))) {
        selectedIndices.push(idx);
      }
    });
    selectedIndices.sort((a, b) => a - b);

    // Can't move up if first selected is already at top
    if (selectedIndices[0] === 0) return;

    // Build new order: swap each selected with the one above
    const currentOrder = displayTracks.map(trk => ({
      id: trk.isLocal ? Math.abs(trk.id) : trk.id,
      isLocal: trk.isLocal ?? false
    }));

    // Move from top to bottom to avoid conflicts
    for (const idx of selectedIndices) {
      const newIdx = idx - 1;
      [currentOrder[newIdx], currentOrder[idx]] = [currentOrder[idx], currentOrder[newIdx]];
    }

    // Save new order
    const orders: [number, boolean, number][] = currentOrder.map((item, pos) => [item.id, item.isLocal, pos]);
    try {
      await invoke('playlist_set_custom_order', { playlistId, orders });
      // Update local map
      const newMap = new Map<string, number>();
      orders.forEach(([id, isLocal, pos]) => {
        newMap.set(`${id}:${isLocal}`, pos);
      });
      customOrderMap = newMap;
    } catch (err) {
      console.error('Failed to move selected tracks:', err);
    }
  }

  // Move all selected tracks down one position (as a group)
  async function moveSelectedDown() {
    if (selectedTrackKeys.size === 0) return;

    // Get indices of selected tracks (sorted descending for moving down)
    const selectedIndices: number[] = [];
    displayTracks.forEach((track, idx) => {
      if (selectedTrackKeys.has(getTrackKey(track))) {
        selectedIndices.push(idx);
      }
    });
    selectedIndices.sort((a, b) => b - a);  // Descending

    // Can't move down if last selected is already at bottom
    if (selectedIndices[0] === displayTracks.length - 1) return;

    // Build new order: swap each selected with the one below
    const currentOrder = displayTracks.map(trk => ({
      id: trk.isLocal ? Math.abs(trk.id) : trk.id,
      isLocal: trk.isLocal ?? false
    }));

    // Move from bottom to top to avoid conflicts
    for (const idx of selectedIndices) {
      const newIdx = idx + 1;
      [currentOrder[idx], currentOrder[newIdx]] = [currentOrder[newIdx], currentOrder[idx]];
    }

    // Save new order
    const orders: [number, boolean, number][] = currentOrder.map((item, pos) => [item.id, item.isLocal, pos]);
    try {
      await invoke('playlist_set_custom_order', { playlistId, orders });
      // Update local map
      const newMap = new Map<string, number>();
      orders.forEach(([id, isLocal, pos]) => {
        newMap.set(`${id}:${isLocal}`, pos);
      });
      customOrderMap = newMap;
    } catch (err) {
      console.error('Failed to move selected tracks:', err);
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
  function localTrackToDisplay(track: PlaylistLocalTrack, index: number): DisplayTrack {
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
      samplingRate: track.sample_rate / 1000, // Convert Hz to kHz for display
      isLocal: true,
      localTrackId: track.id,
      artworkPath: track.artwork_path
    };
  }

  // Filtered and sorted tracks (merged Qobuz + local by position)
  let displayTracks = $derived.by(() => {
    // Fast path: no local tracks, no search, default sort → skip copy entirely
    if (localTracks.length === 0 && !searchQuery.trim() && sortBy === 'default') {
      return tracks;
    }

    // Full path: interleave with local tracks, filter, sort
    const result: DisplayTrack[] = [];

    // Create a map of local track positions
    const localByPosition = new Map<number, PlaylistLocalTrack>();
    for (const lt of localTracks) {
      localByPosition.set(lt.playlist_position, lt);
    }

    // Calculate total count: must reach the highest local position
    const maxLocalPosition = localTracks.length > 0
      ? Math.max(...localTracks.map(lt => lt.playlist_position))
      : -1;
    const minTotalCount = tracks.length + localTracks.length;
    const totalCount = Math.max(minTotalCount, maxLocalPosition + 1);

    // Interleave: iterate through positions, use local if exists, else use next Qobuz track
    let qobuzIdx = 0;
    for (let pos = 0; pos < totalCount; pos++) {
      const localTrack = localByPosition.get(pos);
      if (localTrack) {
        result.push(localTrackToDisplay(localTrack, result.length));
      } else if (qobuzIdx < tracks.length) {
        // Use Qobuz track
        result.push({ ...tracks[qobuzIdx], number: result.length + 1 });
        qobuzIdx++;
      }
      // Skip positions with no track (gaps)
    }

    // If any Qobuz tracks remain, append them
    while (qobuzIdx < tracks.length) {
      result.push({ ...tracks[qobuzIdx], number: result.length + 1 });
      qobuzIdx++;
    }

    // Filter by search query
    let filtered = result;
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase();
      filtered = result.filter(trk =>
        trk.title.toLowerCase().includes(query) ||
        (trk.artist?.toLowerCase().includes(query)) ||
        (trk.album?.toLowerCase().includes(query))
      );
    }

    // Sort (only if not default)
    if (sortBy !== 'default') {
      filtered.sort((a, b) => {
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
          case 'added':
            // Use original index as proxy for date added
            // ASC = newest first (higher index = more recent), DESC = oldest first
            cmp = (b.addedIndex ?? 0) - (a.addedIndex ?? 0);
            break;
          case 'label':
            const labelA = a.label || '';
            const labelB = b.label || '';
            // Tracks without label (local tracks) go to end
            if (!labelA && labelB) return 1;
            if (labelA && !labelB) return -1;
            cmp = labelA.localeCompare(labelB);
            break;
          case 'custom':
            // Get positions from customOrderMap
            const aIsLocal = a.isLocal ?? false;
            const bIsLocal = b.isLocal ?? false;
            const aKey = `${aIsLocal ? Math.abs(a.id) : a.id}:${aIsLocal}`;
            const bKey = `${bIsLocal ? Math.abs(b.id) : b.id}:${bIsLocal}`;
            const aPos = customOrderMap.get(aKey) ?? a.addedIndex ?? 0;
            const bPos = customOrderMap.get(bKey) ?? b.addedIndex ?? 0;
            cmp = aPos - bPos;
            break;
        }
        return sortOrder === 'desc' ? -cmp : cmp;
      });
    }

    return filtered;
  });

  // Virtual scrolling: total height of the track list
  const trackListTotalHeight = $derived(displayTracks.length * TRACK_ROW_HEIGHT);

  // Virtual scrolling: compute visible track index range
  const visibleTrackRange = $derived.by(() => {
    const count = displayTracks.length;
    if (count === 0) return { start: 0, end: 0 };

    // Calculate which tracks are visible based on scroll position relative to track list
    const firstVisible = Math.floor(trackListScrollTop / TRACK_ROW_HEIGHT);
    const visibleCount = Math.ceil(trackListViewHeight / TRACK_ROW_HEIGHT);
    const lastVisible = firstVisible + visibleCount;

    const start = Math.max(0, firstVisible - VIRTUAL_BUFFER);
    const end = Math.min(count, lastVisible + VIRTUAL_BUFFER);

    return { start, end };
  });

  // The slice of tracks to actually render
  const visibleDisplayTracks = $derived(
    displayTracks.slice(visibleTrackRange.start, visibleTrackRange.end)
  );

  // Handle scroll from the parent .playlist-detail container
  function handlePlaylistScroll(e: Event) {
    const container = e.target as HTMLElement;
    // Save scroll position for navigation restoration
    saveScrollPosition('playlist', container.scrollTop);
    // Update virtual scroll state relative to the track list position
    if (trackListEl) {
      const trackListTop = trackListEl.offsetTop;
      trackListScrollTop = Math.max(0, container.scrollTop - trackListTop);
      trackListViewHeight = container.clientHeight;
    }
  }

  const sortOptions: { field: SortField; label: string }[] = [
    { field: 'default', label: 'Default' },
    { field: 'title', label: 'Title' },
    { field: 'artist', label: 'Artist' },
    { field: 'album', label: 'Album' },
    { field: 'duration', label: 'Duration' },
    { field: 'added', label: 'Added Recently' },
    { field: 'label', label: 'Label' },
    { field: 'custom', label: 'Custom Order' },
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

  function resolveArtworkPath(path: string): string {
    if (path.startsWith('http://') || path.startsWith('https://')) {
      return path;
    }
    return `asset://localhost/${encodeURIComponent(path)}`;
  }

  function getPlaylistImage(): string {
    // Use custom artwork if set (URL or local file path)
    if (customArtworkPath) {
      return resolveArtworkPath(customArtworkPath);
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

  function buildQueueTracks(tracks: DisplayTrack[]) {
    // Filter out blacklisted artists before building queue
    const filteredTracks = tracks.filter(trk => {
      if (trk.isLocal) return true; // Local tracks are never blacklisted
      if (!trk.artistId) return true; // No artist ID, can't check blacklist
      return !isArtistBlacklisted(trk.artistId);
    });

    const queueTracks = filteredTracks.map(trk => ({
      id: trk.isLocal ? Math.abs(trk.id) : trk.id,
      title: trk.title,
      artist: trk.artist || 'Unknown Artist',
      album: trk.album || playlist?.name || 'Playlist',
      duration_secs: trk.durationSeconds,
      artwork_url: trk.albumArt || getPlaylistImage(),
      hires: trk.hires ?? false,
      bit_depth: trk.bitDepth ?? null,
      sample_rate: trk.samplingRate != null ? (trk.isLocal ? trk.samplingRate * 1000 : trk.samplingRate) : null,
      is_local: trk.isLocal ?? false,
      album_id: trk.isLocal ? null : (trk.albumId || null),
      artist_id: trk.isLocal ? null : (trk.artistId ?? null),
    }));

    const localIds = filteredTracks
      .filter(trk => trk.isLocal)
      .map(trk => Math.abs(trk.id));

    return { queueTracks, localIds };
  }

  async function setPlaylistQueue(startIndex: number) {
    const allTracks = displayTracks;
    if (allTracks.length === 0) return;
    const { queueTracks, localIds } = buildQueueTracks(allTracks);
    await invoke('set_queue', { tracks: queueTracks, startIndex });
    if (localIds.length > 0) {
      onSetLocalQueue?.(localIds);
    }
  }

  async function handleTrackClick(track: DisplayTrack, trackIndex: number) {
    // Create playlist context before playing
    if (playlist) {
      const trackIds = displayTracks
        .filter(trk => !trk.isLocal) // Only Qobuz tracks in context
        .map(trk => trk.id);

      const contextIndex = trackIds.indexOf(track.id);
      
      if (contextIndex >= 0 && trackIds.length > 0) {
        await setPlaybackContext(
          'playlist',
          playlist.id.toString(),
          playlist.name,
          'qobuz',
          trackIds,
          contextIndex
        );
        console.log(`[Playlist] Context created: "${playlist.name}", ${trackIds.length} tracks, starting at ${contextIndex}`);
      }
    }

    // Handle playback
    try {
      await setPlaylistQueue(trackIndex);
    } catch (err) {
      console.error('Failed to set queue:', err);
    }

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
        notifyParentOfCounts();
      } else if (track.playlistTrackId) {
        // Remove Qobuz track using playlist_track_id (available from full playlist load)
        await invoke('remove_tracks_from_playlist', {
          playlistId,
          playlistTrackIds: [track.playlistTrackId]
        });
        await loadPlaylist();
        notifyParentOfCounts();
      } else {
        // Progressive loading path: no playlist_track_id available, resolve by track ID
        await invoke('remove_tracks_from_playlist', {
          playlistId,
          trackIds: [track.id]
        });
        await loadPlaylist();
        notifyParentOfCounts();
      }
      // Notify parent to refresh sidebar counts
      onPlaylistUpdated?.();
    } catch (err) {
      console.error('Failed to remove track from playlist:', err);
    }
  }

  // Open replacement modal for an unavailable track
  function openReplacementModal(track: DisplayTrack) {
    trackToReplace = track;
    replacementModalOpen = true;
  }

  // Handle track replacement selection
  interface ReplacementTrack {
    id: number;
    title: string;
    duration: number;
    performer?: { id?: number; name: string };
    album?: {
      id: string;
      title: string;
      image?: { small?: string; thumbnail?: string; large?: string };
    };
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
  }

  async function handleTrackReplacement(newTrack: ReplacementTrack) {
    if (!trackToReplace) {
      console.error('No track to replace');
      return;
    }

    try {
      // Get the current position of the track being replaced
      const currentIndex = displayTracks.findIndex(trk => trk.id === trackToReplace!.id);

      // Remove the old track (supports both playlist_track_id and track_id resolution)
      if (trackToReplace.playlistTrackId) {
        await invoke('remove_tracks_from_playlist', {
          playlistId,
          playlistTrackIds: [trackToReplace.playlistTrackId]
        });
      } else {
        await invoke('remove_tracks_from_playlist', {
          playlistId,
          trackIds: [trackToReplace.id]
        });
      }

      // Add the new track
      await invoke('add_tracks_to_playlist', {
        playlistId,
        trackIds: [newTrack.id]
      });

      // Clear the unavailable status for the old track (if it was in the store)
      clearTrackUnavailable(trackToReplace.id);

      // Reload the playlist to get updated data
      await loadPlaylist();
      notifyParentOfCounts();
      onPlaylistUpdated?.();

      // Show success message
      showToast($t('playlist.trackReplaced'), 'success');

      // Close modal
      replacementModalOpen = false;
      trackToReplace = null;

      console.log(`[Playlist] Track replaced: ${trackToReplace?.title} -> ${newTrack.title} at position ${currentIndex}`);
    } catch (err) {
      console.error('Failed to replace track:', err);
      showToast($t('playlist.trackReplaceFailed'), 'error');
    }
  }

  // Preview a replacement track
  function handlePreviewReplacement(track: ReplacementTrack) {
    if (!onTrackPlay) return;

    const displayTrack: DisplayTrack = {
      id: track.id,
      number: 0,
      title: track.title,
      artist: track.performer?.name,
      album: track.album?.title,
      albumArt: track.album?.image?.large || track.album?.image?.thumbnail,
      albumId: track.album?.id,
      artistId: track.performer?.id,
      duration: formatDuration(track.duration),
      durationSeconds: track.duration,
      hires: track.hires,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate
    };

    onTrackPlay(displayTrack);
  }

  // Add a suggested track to the playlist
  async function handleAddSuggestedTrack(suggestedTrack: import('$lib/services/playlistSuggestionsService').SuggestedTrack) {
    try {
      // Add to Qobuz playlist
      await invoke('add_tracks_to_playlist', {
        playlistId,
        trackIds: [suggestedTrack.track_id]
      });

      // Add to local tracks array immediately (no reload needed)
      const newTrack: DisplayTrack = {
        id: suggestedTrack.track_id,
        number: tracks.length + 1,
        title: suggestedTrack.title,
        artist: suggestedTrack.artist_name,
        artistId: suggestedTrack.artist_id,
        album: suggestedTrack.album_title,
        albumId: suggestedTrack.album_id,
        albumArt: suggestedTrack.album_image_url,
        duration: formatDuration(suggestedTrack.duration),
        durationSeconds: suggestedTrack.duration,
        addedIndex: tracks.length, // Latest added
      };

      // Append to tracks array
      tracks = [...tracks, newTrack];

      // Update playlist count
      if (playlist) {
        playlist.tracks_count = (playlist.tracks_count || 0) + 1;
        playlist.duration = (playlist.duration || 0) + suggestedTrack.duration;
      }

      // Notify parent (sidebar count update, etc.)
      notifyParentOfCounts();
      onPlaylistUpdated?.();
    } catch (err) {
      console.error('Failed to add suggested track:', err);
      throw err; // Re-throw so the suggestions component knows it failed
    }
  }

  // Preview a suggested track
  function handlePreviewSuggestedTrack(track: import('$lib/services/playlistSuggestionsService').SuggestedTrack) {
    if (!onTrackPlay) return;

    // Convert SuggestedTrack to DisplayTrack format
    const displayTrack: DisplayTrack = {
      id: track.track_id,
      number: 0,
      title: track.title,
      artist: track.artist_name,
      album: track.album_title,
      albumArt: track.album_image_url,
      albumId: track.album_id,
      artistId: track.artist_id,
      duration: formatDuration(track.duration),
      durationSeconds: track.duration
    };

    onTrackPlay(displayTrack);
  }

  async function handlePlayAll() {
    // Get all display tracks (Qobuz + local, respecting search/sort)
    const allTracks = displayTracks;
    if (allTracks.length === 0) return;

    // Filter out blacklisted tracks
    const playableTracks = allTracks.filter(trk => {
      if (trk.isLocal) return true;
      if (!trk.artistId) return true;
      return !isArtistBlacklisted(trk.artistId);
    });

    if (playableTracks.length === 0) return;

    // Set playback context for playlist
    if (playlist) {
      const trackIds = playableTracks
        .filter(trk => !trk.isLocal) // Only Qobuz tracks in context
        .map(trk => trk.id);

      if (trackIds.length > 0) {
        await setPlaybackContext(
          'playlist',
          playlist.id.toString(),
          playlist.name,
          'qobuz',
          trackIds,
          0
        );
        console.log(`[Playlist] Context created via Play All: "${playlist.name}", ${trackIds.length} tracks`);
      }
    }

    try {
      await setPlaylistQueue(0);

      // Play first playable track (handle local vs Qobuz)
      const firstTrack = playableTracks[0];
      if (firstTrack.isLocal && onLocalTrackPlay) {
        const localTrack = localTracks.find(trk => trk.id === Math.abs(firstTrack.id));
        if (localTrack) onLocalTrackPlay(localTrack);
      } else if (onTrackPlay) {
        onTrackPlay(firstTrack);
      }

      // Increment play count
      const stats = await invoke<PlaylistStats>('playlist_increment_play_count', { playlistId });
      playlistStats = stats;
    } catch (err) {
      console.error('Failed to set queue:', err);
    }
  }

  async function handleEditSuccess() {
    editModalOpen = false;
    await loadPlaylist(); // Reload playlist data
    loadSettings(); // Reload settings (including hidden status)
    notifyParentOfCounts();
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
    const allTracks = displayTracks;
    if (allTracks.length === 0) return;

    // Filter out blacklisted tracks
    const playableTracks = allTracks.filter(trk => {
      if (trk.isLocal) return true;
      if (!trk.artistId) return true;
      return !isArtistBlacklisted(trk.artistId);
    });

    if (playableTracks.length === 0) return;

    // Collect local track IDs to add to set
    const localIds = playableTracks
      .filter(trk => trk.isLocal)
      .map(trk => Math.abs(trk.id));

    // Add in reverse order so first track ends up right after current
    for (let i = playableTracks.length - 1; i >= 0; i--) {
      const trk = playableTracks[i];
      try {
        await invoke('add_to_queue_next', {
          track: {
            id: trk.isLocal ? Math.abs(trk.id) : trk.id,
            title: trk.title,
            artist: trk.artist || 'Unknown Artist',
            album: trk.album || playlist?.name || 'Playlist',
            duration_secs: trk.durationSeconds,
            artwork_url: trk.albumArt || getPlaylistImage(),
            hires: trk.hires ?? false,
            bit_depth: trk.bitDepth ?? null,
            sample_rate: trk.samplingRate != null ? (trk.isLocal ? trk.samplingRate * 1000 : trk.samplingRate) : null,
            is_local: trk.isLocal ?? false,
            album_id: trk.isLocal ? null : (trk.albumId || null),
            artist_id: trk.isLocal ? null : (trk.artistId ?? null),
          }
        });
      } catch (err) {
        console.error('Failed to add track next:', err);
      }
    }

    // Tell parent about local tracks added to queue
    if (localIds.length > 0) {
      onSetLocalQueue?.(localIds);
    }
  }

  async function handlePlayAllLater() {
    const allTracks = displayTracks;
    if (allTracks.length === 0) return;

    // Filter out blacklisted tracks
    const playableTracks = allTracks.filter(trk => {
      if (trk.isLocal) return true;
      if (!trk.artistId) return true;
      return !isArtistBlacklisted(trk.artistId);
    });

    if (playableTracks.length === 0) return;

    const queueTracks = playableTracks.map(trk => ({
      id: trk.isLocal ? Math.abs(trk.id) : trk.id,
      title: trk.title,
      artist: trk.artist || 'Unknown Artist',
      album: trk.album || playlist?.name || 'Playlist',
      duration_secs: trk.durationSeconds,
      artwork_url: trk.albumArt || getPlaylistImage(),
      hires: trk.hires ?? false,
      bit_depth: trk.bitDepth ?? null,
      sample_rate: trk.samplingRate != null ? (trk.isLocal ? trk.samplingRate * 1000 : trk.samplingRate) : null,
      is_local: trk.isLocal ?? false,
      album_id: trk.isLocal ? null : (trk.albumId || null),
      artist_id: trk.isLocal ? null : (trk.artistId ?? null),
    }));

    // Collect local track IDs
    const localIds = playableTracks
      .filter(trk => trk.isLocal)
      .map(trk => Math.abs(trk.id));

    try {
      await invoke('add_tracks_to_queue', { tracks: queueTracks });

      // Tell parent about local tracks added to queue
      if (localIds.length > 0) {
        onSetLocalQueue?.(localIds);
      }
    } catch (err) {
      console.error('Failed to add to queue:', err);
    }
  }

  function sharePlaylistQobuz() {
    if (!playlist?.id) return;
    const url = `https://play.qobuz.com/playlist/${playlist.id}`;
    writeText(url);
  }

  async function handleMakePlaylistOffline() {
    const translate = get(t);
    // Filter to Qobuz-only tracks (not local)
    const qobuzTracks = displayTracks.filter(track => !track.isLocal);

    // Filter out already-cached tracks
    const tracksToCache = qobuzTracks.filter(track => {
      const status = getOfflineCacheState(track.id).status;
      return status === 'none' || status === 'failed';
    });

    if (tracksToCache.length === 0) {
      showToast(translate('toast.allTracksOffline'), 'info');
      return;
    }

    // Warn for large playlists
    if (tracksToCache.length > 300) {
      const confirmed = await ask(
        translate('playlist.makeOfflineConfirmDesc', { values: { count: tracksToCache.length } }),
        {
          title: translate('playlist.makeOfflineConfirmTitle'),
          kind: 'warning'
        }
      );
      if (!confirmed) return;
    }

    const playlistName = playlist?.name || '';
    showToast(translate('playlist.preparingPlaylistOffline', { values: { count: tracksToCache.length, name: playlistName } }), 'info');

    for (const track of tracksToCache) {
      try {
        await cacheTrackForOffline({
          id: track.id,
          title: track.title,
          artist: track.artist || 'Unknown',
          album: track.album,
          albumId: track.albumId,
          durationSecs: track.durationSeconds,
          quality: '-',
          bitDepth: track.bitDepth,
          sampleRate: track.samplingRate,
        });
      } catch (err) {
        console.error(`Failed to queue offline cache for "${track.title}":`, err);
      }
    }
  }
</script>

<ViewTransition duration={200} distance={12} direction="down">
<div class="playlist-detail" bind:this={scrollContainer} onscroll={handlePlaylistScroll}>
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
    <div class="loading" class:fading={spinnerFading}>
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
    <ViewTransition duration={200} distance={12} direction="up">
    <!-- Playlist Header -->
    <div class="playlist-header">
      <!-- Playlist Artwork - Collage or Custom -->
      <div class="artwork-container">
        {#if customArtworkPath}
          <div class="artwork custom-artwork">
            <img src={resolveArtworkPath(customArtworkPath)} alt={playlist.name} />
            <div class="artwork-overlay">
              <button class="artwork-btn artwork-clear" onclick={clearCustomArtwork} title="Remove custom artwork">
                <X size={20} />
              </button>
            </div>
          </div>
        {:else}
          <div class="collage-wrapper">
            {#if playlist.images && playlist.images.length > 0}
              <img class="playlist-api-image" src={playlist.images[0]} alt={playlist.name} width="200" height="200" />
            {:else}
              <PlaylistCollage
                artworks={tracks.slice(0, 4).map(trk => trk.albumArt).filter((a): a is string => !!a)}
                size={200}
              />
            {/if}
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
            class="action-btn-circle primary"
            onclick={handlePlayAll}
            title="Play"
          >
            <Play size={20} fill="currentColor" color="currentColor" />
          </button>
          <button
            class="action-btn-circle"
            onclick={handleShuffle}
            title="Shuffle"
          >
            <Shuffle size={18} />
          </button>
          <button
            class="action-btn-circle"
            class:is-active={isFavorite}
            onclick={toggleFavorite}
            title={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
          >
            <Heart
              size={18}
              color={isFavorite ? 'var(--accent-primary)' : 'currentColor'}
              fill={isFavorite ? 'var(--accent-primary)' : 'none'}
            />
          </button>
          {#if showCopyButton}
            <button
              class="action-btn-circle"
              class:is-loading={isCopying}
              onclick={copyPlaylistToLibrary}
              disabled={isCopying}
              title="Copy to My Library"
            >
              <ListPlus size={18} />
            </button>
          {/if}
          <AlbumMenu
            onPlayNext={handlePlayAllNext}
            onPlayLater={handlePlayAllLater}
            onShareQobuz={sharePlaylistQobuz}
            onMakeOffline={handleMakePlaylistOffline}
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
                onclick={() => selectSort(option.field)}
              >
                <span>{option.label}</span>
                {#if sortBy === option.field && option.field !== 'default' && option.field !== 'custom'}
                  <span class="sort-indicator">{sortOrder === 'asc' ? '↑' : '↓'}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>

    </div>

    <!-- Track List (virtualized) -->
    <div class="track-list" bind:this={trackListEl}>
      {#if isCustomOrderMode}
        <div class="batch-controls">
          <div class="batch-left">
            {#if selectedTrackKeys.size > 0}
              <span class="selection-count">{selectedTrackKeys.size} selected</span>
              <button class="batch-btn" onclick={clearSelection}>Clear</button>
            {:else}
              <button class="batch-btn" onclick={selectAllTracks}>Select All</button>
            {/if}
          </div>
          {#if selectedTrackKeys.size > 0}
            <div class="batch-right">
              <button class="batch-btn" onclick={moveSelectedUp} title="Move selected up">
                <ChevronUp size={14} /> Move Up
              </button>
              <button class="batch-btn" onclick={moveSelectedDown} title="Move selected down">
                <ChevronDown size={14} /> Move Down
              </button>
            </div>
          {/if}
        </div>
      {/if}
      <div class="track-list-header">
        {#if isCustomOrderMode}
          <div class="col-checkbox"></div>
        {/if}
        <div class="col-number">#</div>
        <div class="col-title">Title</div>
        <div class="col-album">Album</div>
        <div class="col-duration">Duration</div>
        <div class="col-quality">Quality</div>
        <div class="col-icon"><Heart size={14} /></div>
        <div class="col-icon"><CloudDownload size={14} /></div>
        <div class="col-spacer"></div>
      </div>

      <div class="virtual-track-content" style="height: {trackListTotalHeight}px;">
        {#each visibleDisplayTracks as track, loopIdx (`${visibleTrackRange.start + loopIdx}-${track.id}-${downloadStateVersion}`)}
          {@const idx = visibleTrackRange.start + loopIdx}
          {@const downloadInfo = track.isLocal ? { status: 'none' as const, progress: 0 } : (getTrackOfflineCacheStatus?.(track.id) ?? { status: 'none' as const, progress: 0 })}
          {@const isActiveTrack = (
            track.isLocal
              ? (track.localTrackId !== undefined && activeTrackId === track.localTrackId)
              : activeTrackId === track.id
          )}
          {@const isTrackPlaying = isActiveTrack && isPlaybackActive}
          {@const available = isTrackAvailable(track)}
          {@const removedFromQobuz = isTrackRemovedFromQobuz(track)}
          {@const trackBlacklisted = !track.isLocal && track.artistId ? isArtistBlacklisted(track.artistId) : false}
          <div
            class="track-row-wrapper virtual-track-item"
            class:unavailable={!available}
            class:removed-from-qobuz={removedFromQobuz}
            class:custom-order-mode={isCustomOrderMode}
            class:dragging={draggedTrackIdx === idx}
            class:drag-over={dragOverIdx === idx && draggedTrackIdx !== idx}
            style="transform: translateY({idx * TRACK_ROW_HEIGHT}px); height: {TRACK_ROW_HEIGHT}px;"
            data-track-id={track.isLocal ? undefined : track.id}
            title={removedFromQobuz ? $t('player.trackUnavailable') : (!available ? $t('offline.trackNotAvailable') : undefined)}
            draggable={isCustomOrderMode && !removedFromQobuz}
            ondragstart={(e) => handleDragStart(e, idx)}
            ondragover={(e) => handleDragOver(e, idx)}
            ondragleave={handleDragLeave}
            ondragend={handleDragEnd}
            ondrop={(e) => handleDrop(e, idx)}
          >
            {#if isCustomOrderMode}
              {@const trackKey = getTrackKey(track)}
              <label class="track-checkbox" onclick={(e) => e.stopPropagation()}>
                <input
                  type="checkbox"
                  checked={selectedTrackKeys.has(trackKey)}
                  onchange={() => toggleTrackSelection(track)}
                />
              </label>
              <div class="reorder-controls">
                <button
                  class="reorder-btn"
                  onclick={() => moveTrackUp(track, idx)}
                  disabled={idx === 0}
                  title="Move up"
                >
                  <ChevronUp size={16} />
                </button>
                <div class="drag-handle" title="Drag to reorder">
                  <GripVertical size={16} />
                </div>
                <button
                  class="reorder-btn"
                  onclick={() => moveTrackDown(track, idx)}
                  disabled={idx === displayTracks.length - 1}
                  title="Move down"
                >
                  <ChevronDown size={16} />
                </button>
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
              isPlaying={isTrackPlaying}
              isLocal={track.isLocal}
              isUnavailable={removedFromQobuz && isOwnPlaylist}
              unavailableTooltip={removedFromQobuz ? $t('player.trackUnavailable') : undefined}
              isBlacklisted={trackBlacklisted}
              hideFavorite={track.isLocal || removedFromQobuz || trackBlacklisted}
              hideDownload={track.isLocal || removedFromQobuz || trackBlacklisted}
              downloadStatus={downloadInfo.status}
              downloadProgress={downloadInfo.progress}
              onPlay={available && !trackBlacklisted ? () => handleTrackClick(track, idx) : undefined}
              onDownload={available && !track.isLocal && !trackBlacklisted && onTrackDownload ? () => onTrackDownload(track) : undefined}
              onRemoveDownload={available && !track.isLocal && !trackBlacklisted && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
              menuActions={removedFromQobuz ? (isOwnPlaylist ? {
                onRemoveFromPlaylist: () => removeTrackFromPlaylist(track),
                onFindReplacement: () => openReplacementModal(track)
              } : {}) : trackBlacklisted ? {
                onGoToAlbum: !track.isLocal && track.albumId && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.albumId!) : undefined,
                onGoToArtist: !track.isLocal && track.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(track.artistId!) : undefined,
                onShowInfo: !track.isLocal && onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined
              } : available ? {
                onPlayNow: () => handleTrackClick(track, idx),
                onPlayNext: track.isLocal ? () => handleTrackPlayNext(track) : (onTrackPlayNext ? () => onTrackPlayNext(track) : undefined),
                onPlayLater: track.isLocal ? () => handleTrackPlayLater(track) : (onTrackPlayLater ? () => onTrackPlayLater(track) : undefined),
                onAddToPlaylist: !track.isLocal && onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined,
                onRemoveFromPlaylist: () => removeTrackFromPlaylist(track),
                onShareQobuz: !track.isLocal && onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
                onShareSonglink: !track.isLocal && onTrackShareSonglink ? () => onTrackShareSonglink(track) : undefined,
                onGoToAlbum: !track.isLocal && track.albumId && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.albumId!) : undefined,
                onGoToArtist: !track.isLocal && track.artistId && onTrackGoToArtist ? () => onTrackGoToArtist(track.artistId!) : undefined,
                onShowInfo: !track.isLocal && onTrackShowInfo ? () => onTrackShowInfo(track.id) : undefined,
                onDownload: !track.isLocal && onTrackDownload ? () => onTrackDownload(track) : undefined,
                isTrackDownloaded: !track.isLocal ? downloadInfo.status === 'ready' : false,
                onReDownload: !track.isLocal && downloadInfo.status === 'ready' && onTrackReDownload ? () => onTrackReDownload(track) : undefined,
                onRemoveDownload: !track.isLocal && downloadInfo.status === 'ready' && onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined
              } : {}}
            />
          </div>
        {/each}
      </div>

      {#if displayTracks.length === 0 && searchQuery}
        <div class="no-results">
          <p>No tracks match "{searchQuery}"</p>
        </div>
      {/if}
    </div>

    <!-- Bottom spacer when no suggestions will render (prevents back-to-top covering last track actions) -->
    {#if !isOwnPlaylist || !suggestionsEnabled || searchQuery}
      <div class="track-list-bottom-spacer"></div>
    {/if}

    <!-- Playlist Suggestions -->
    <!-- >=2000 tracks: hidden (Qobuz limit reached) -->
    <!-- >500 tracks: manual launch button shown here, computation deferred -->
    <!-- <=500 tracks: auto-loaded -->
    {#if playlist && !searchQuery && isOwnPlaylist && suggestionsEnabled}
      {#if suggestionsReady && playlistArtists.length > 0}
        <PlaylistSuggestions
          playlistId={playlistId}
          artists={playlistArtists}
          excludeTrackIds={excludeTrackIds}
          trackCount={playlist.tracks_count ?? 0}
          existingTracks={tracks.filter(trk => !trk.isLocal).map(trk => ({ title: trk.title, artist: trk.artist }))}
          onAddTrack={handleAddSuggestedTrack}
          onGoToAlbum={onTrackGoToAlbum}
          onGoToArtist={onTrackGoToArtist}
          onPreviewTrack={handlePreviewSuggestedTrack}
          showReasons={false}
        />
      {:else if !suggestionsReady}
        <div class="suggestions-manual-launch">
          <p class="suggestions-manual-hint">{$t('playlist.suggestionsLargePlaylist')}</p>
          <button class="suggestions-manual-btn" onclick={() => suggestionsActivated = true}>
            {$t('playlist.suggestSongs')}
          </button>
        </div>
      {/if}
    {/if}
    </ViewTransition>
  {/if}

</div>
</ViewTransition>

<!-- Edit Playlist Modal -->
{#if playlist}
  <PlaylistModal
    isOpen={editModalOpen}
    mode="edit"
    playlist={{ id: playlist.id, name: playlist.name, tracks_count: playlist.tracks_count }}
    isHidden={playlistSettings?.hidden ?? false}
    currentFolderId={playlistSettings?.folder_id ?? null}
    onClose={() => editModalOpen = false}
    onSuccess={handleEditSuccess}
    onDelete={handleDelete}
  />
{/if}

<!-- Track Replacement Modal -->
<TrackReplacementModal
  isOpen={replacementModalOpen}
  trackTitle={trackToReplace?.title ?? ''}
  trackArtist={trackToReplace?.artist}
  onClose={() => { replacementModalOpen = false; trackToReplace = null; }}
  onSelect={handleTrackReplacement}
  onPreview={handlePreviewReplacement}
/>

<style>
  .playlist-detail {
    padding: 24px;
    padding-left: 18px;
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

  .loading {
    opacity: 1;
    transition: opacity 200ms ease-out;
  }

  .loading.fading {
    opacity: 0;
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
    background: var(--alpha-10);
    border: 1px solid var(--alpha-30);
    border-radius: 50%;
    color: white;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .artwork-btn:hover {
    background: var(--alpha-20);
    border-color: var(--alpha-50);
  }

  .artwork-btn.artwork-clear {
    width: 36px;
    height: 36px;
    background: var(--danger-bg);
    border-color: var(--danger-border);
  }

  .artwork-btn.artwork-clear:hover {
    background: var(--danger-hover);
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
    gap: 12px;
  }

  /* Style AlbumMenu trigger to match action buttons */
  .actions :global(.album-menu .menu-trigger) {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    border: none;
    box-shadow: inset 0 0 0 1px var(--border-strong);
    color: var(--text-muted);
  }

  .actions :global(.album-menu .menu-trigger:hover) {
    background: var(--bg-hover);
    color: var(--text-primary);
    box-shadow: inset 0 0 0 1px var(--text-primary);
  }


  .track-list {
    margin-top: 24px;
  }

  .track-list-header {
    width: 100%;
    height: 40px;
    padding: 0 16px;
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    font-size: 12px;
    text-transform: uppercase;
    color: #666666;
    font-weight: 400;
    box-sizing: border-box;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 8px;
  }

  .col-number {
    width: 48px;
    text-align: center;
  }

  .col-title {
    flex: 1;
    min-width: 0;
  }

  .col-album {
    flex: 1;
    min-width: 0;
  }

  .col-duration {
    width: 80px;
    text-align: center;
  }

  .col-quality {
    width: 80px;
    text-align: center;
  }

  .col-icon {
    width: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    opacity: 0.5;
  }

  .col-spacer {
    width: 28px;
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
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px;
    background-color: var(--bg-tertiary);
    border: none;
    border-radius: 8px;
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
    transition: color 150ms ease;
    min-width: 200px;
    white-space: nowrap;
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
    min-width: 200px;
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  .sort-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
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
    white-space: nowrap;
  }

  .sort-option:hover {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .sort-option.active {
    color: var(--accent-primary);
  }

  .sort-indicator {
    font-size: 11px;
    font-weight: 600;
    margin-left: 8px;
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


  .virtual-track-content {
    position: relative;
    width: 100%;
  }

  .virtual-track-item {
    position: absolute;
    left: 0;
    right: 0;
    will-change: transform;
  }

  .track-row-wrapper {
    display: flex;
    align-items: center;
  }

  .track-row-wrapper :global(.track-row) {
    flex: 1;
  }

  /* Unavailable track styles (offline mode) */
  .track-row-wrapper.unavailable {
    opacity: 0.4;
    pointer-events: none;
    user-select: none;
  }

  .track-row-wrapper.unavailable :global(.track-row) {
    filter: grayscale(100%);
  }

  /* Track removed from Qobuz - allow limited interactions (remove from playlist) */
  .track-row-wrapper.removed-from-qobuz {
    opacity: 0.5;
    /* Keep wrapper interactive for context menu */
    pointer-events: auto;
  }

  .track-row-wrapper.removed-from-qobuz :global(.track-row) {
    filter: grayscale(100%);
  }

  /* Disable play hover effect for removed tracks */
  .track-row-wrapper.removed-from-qobuz :global(.track-row .track-number),
  .track-row-wrapper.removed-from-qobuz :global(.track-row .play-button) {
    pointer-events: none;
  }

  /* Custom order mode */
  .track-row-wrapper.custom-order-mode {
    padding-left: 0;
  }

  .reorder-controls {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 4px;
    margin-right: 8px;
    flex-shrink: 0;
  }

  .reorder-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary, #888);
    cursor: pointer;
    padding: 2px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background-color 0.15s, color 0.15s;
  }

  .reorder-btn:hover:not(:disabled) {
    background: var(--hover-bg, rgba(255, 255, 255, 0.1));
    color: var(--text-primary, #fff);
  }

  .reorder-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .drag-handle {
    color: var(--text-secondary, #888);
    cursor: grab;
    padding: 2px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .drag-handle:active {
    cursor: grabbing;
  }

  /* Drag and drop styles */
  .track-row-wrapper.dragging {
    opacity: 0.5;
    background: var(--drag-bg, rgba(99, 102, 241, 0.2));
  }

  .track-row-wrapper.drag-over {
    border-top: 2px solid var(--accent-color, #6366f1);
    margin-top: -2px;
  }

  .track-row-wrapper[draggable="true"] {
    cursor: grab;
  }

  .track-row-wrapper[draggable="true"]:active {
    cursor: grabbing;
  }

  /* Batch selection controls */
  .batch-controls {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 16px;
    background: var(--bg-tertiary);
    border-radius: 8px;
    margin-bottom: 8px;
  }

  .batch-left, .batch-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .selection-count {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .batch-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .batch-btn:hover {
    background: var(--hover-bg, rgba(255, 255, 255, 0.1));
    color: var(--text-primary);
  }

  .col-checkbox {
    width: 24px;
    flex-shrink: 0;
  }

  .track-checkbox {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    flex-shrink: 0;
    cursor: pointer;
  }

  .track-checkbox input[type="checkbox"] {
    width: 16px;
    height: 16px;
    cursor: pointer;
    accent-color: var(--accent-color, #6366f1);
  }

  .suggestions-manual-launch {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 24px 16px;
    margin-top: 32px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .suggestions-manual-hint {
    font-size: 13px;
    color: var(--text-muted);
    text-align: center;
    margin: 0;
  }

  .suggestions-manual-btn {
    padding: 10px 20px;
    background: var(--bg-tertiary);
    border: 1px solid var(--alpha-8);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background 150ms ease, border-color 150ms ease;
  }

  .suggestions-manual-btn:hover {
    background: var(--alpha-12);
    border-color: var(--alpha-20);
  }

  /* Playlist API image (pre-made collage from Qobuz) */
  .playlist-api-image {
    border-radius: 8px;
    object-fit: cover;
    display: block;
  }

  .track-list-bottom-spacer {
    height: 64px;
  }


</style>
