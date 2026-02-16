<script lang="ts">
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { getThumbnailUrl, getCachedThumbnailUrl } from '$lib/services/thumbnailService';
  import { open, ask } from '@tauri-apps/plugin-dialog';
  import { onMount, onDestroy, tick } from 'svelte';
  import {
    HardDrive, Music, Disc3, Mic2, FolderPlus, Trash2, RefreshCw,
    Settings, ArrowLeft, X, Play, AlertCircle, ImageDown, Upload, Search, LayoutGrid, List, Edit3,
    Network, Power, PowerOff, ChevronLeft, ChevronRight, Shuffle, SlidersHorizontal, ArrowUpDown, ChevronDown, Check
  } from 'lucide-svelte';
  import FolderSettingsModal from '../FolderSettingsModal.svelte';
  import LocalLibraryTagEditorModal from '../LocalLibraryTagEditorModal.svelte';
  import ViewTransition from '../ViewTransition.svelte';
  import { t } from '$lib/i18n';
  import { getUserItem } from '$lib/utils/userStorage';
  import { downloadSettingsVersion } from '$lib/stores/downloadSettingsStore';
  import { showToast } from '$lib/stores/toastStore';
  import AlbumCard from '../AlbumCard.svelte';
  import VirtualizedAlbumList from '../VirtualizedAlbumList.svelte';
  import VirtualizedArtistGrid from '../VirtualizedArtistGrid.svelte';
  import VirtualizedArtistList from '../VirtualizedArtistList.svelte';
  import VirtualizedTrackList from '../VirtualizedTrackList.svelte';
  import {
    isVirtualizationEnabled,
    shouldUsePerformanceMode,
    subscribe as subscribePerformance
  } from '$lib/stores/libraryPerformanceStore';
  import TrackRow from '../TrackRow.svelte';
  import {
    subscribe as subscribeNav,
    selectLocalAlbum,
    clearLocalAlbum,
    getSelectedLocalAlbumId,
    goBack as navGoBack,
    navigateTo,
    getNavigationState
  } from '$lib/stores/navigationStore';
  import {
    subscribe as subscribeOffline,
    isOffline as checkIsOffline,
    getOfflineReason,
    getSettings as getOfflineSettings
  } from '$lib/stores/offlineStore';
  import {
    setPlaybackContext
  } from '$lib/stores/playbackContextStore';

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
    catalog_number?: string;
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
    source?: string; // 'user' | 'qobuz_download' | 'plex'
  }

  interface LocalAlbum {
    id: string;
    title: string;
    artist: string;
    all_artists?: string; // Comma-separated list of all contributing artists
    year?: number;
    catalog_number?: string;
    artwork_path?: string;
    track_count: number;
    total_duration_secs: number;
    format: string;
    bit_depth?: number;
    sample_rate: number;
    directory_path: string;
    source?: string; // 'user' | 'qobuz_download' | 'plex'
    likely_single_file_album?: boolean;
  }

  interface PlexCachedAlbum {
    id: string;
    title: string;
    artist: string;
    artworkPath?: string;
    trackCount: number;
    totalDurationSecs: number;
    format: string;
    bitDepth?: number;
    sampleRate: number;
    source: string;
    likelySingleFileAlbum?: boolean;
  }

  interface PlexCachedTrack {
    id: number;
    ratingKey: string;
    title: string;
    artist: string;
    album: string;
    durationSecs: number;
    format: string;
    bitDepth?: number;
    sampleRate: number;
    artworkPath?: string;
    source: string;
    albumKey: string;
  }

  interface PlexMusicSection {
    key: string;
    title: string;
  }

  interface PlexTrack {
    ratingKey: string;
    title: string;
    artist?: string;
    album?: string;
    durationMs?: number;
    artworkPath?: string;
    partKey?: string;
    container?: string;
    codec?: string;
    channels?: number;
    bitrateKbps?: number;
    samplingRateHz?: number;
    bitDepth?: number;
  }

  interface PlexTrackMetadata {
    ratingKey: string;
    container?: string;
    codec?: string;
    samplingRateHz?: number;
    bitDepth?: number;
  }

  interface PlexTrackQualityUpdate {
    ratingKey: string;
    container?: string;
    samplingRateHz?: number;
    bitDepth?: number;
  }

  interface LocalArtist {
    name: string;
    album_count: number;
    track_count: number;
  }

  interface ArtistSearchResult {
    id: number;
    name: string;
    image?: { small?: string; thumbnail?: string; large?: string };
  }

  interface SearchResults<T> {
    items: T[];
    total: number;
    offset: number;
    limit: number;
  }

  interface LibraryStats {
    track_count: number;
    album_count: number;
    artist_count: number;
    total_duration_secs: number;
    total_size_bytes: number;
  }

  interface ScanProgress {
    status: 'Idle' | 'Scanning' | 'Complete' | 'Cancelled' | 'Error';
    total_files: number;
    processed_files: number;
    current_file?: string;
    errors: { file_path: string; error: string }[];
  }

  interface LibraryFolder {
    id: number;
    path: string;
    alias: string | null;
    enabled: boolean;
    isNetwork: boolean;
    networkFsType: string | null;
    userOverrideNetwork: boolean;
    lastScan: number | null;
  }

  interface DiscogsImageOption {
    url: string;
    width: number;
    height: number;
    image_type: string;
    release_title?: string;
    release_year?: number;
  }

  interface Props {
    onAlbumClick?: (album: LocalAlbum) => void;
    onQobuzArtistClick?: (artistId: number) => void;
    onTrackPlay?: (track: LocalTrack) => void;
    onTrackPlayNext?: (track: LocalTrack) => void;
    onTrackPlayLater?: (track: LocalTrack) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onSetLocalQueue?: (trackIds: number[]) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
  }

  const PLEX_METADATA_WRITE_KEY = 'qbz-plex-poc-metadata-write-enabled';

  let {
    onAlbumClick,
    onQobuzArtistClick,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddToPlaylist,
    onSetLocalQueue,
    activeTrackId = null,
    isPlaybackActive = false
  }: Props = $props();

  // View state
  type TabType = 'albums' | 'artists' | 'tracks';
  let activeTab = $state<TabType>('albums');
  let showSettings = $state(false);
  let showHiddenAlbums = $state(false);
  let albumSearch = $state('');
  let folderSearch = $state('');
  let albumViewMode = $state<'grid' | 'list'>('grid');
  type AlbumGroupMode = 'alpha' | 'artist';
  let albumGroupMode = $state<AlbumGroupMode>('alpha');
  let albumGroupingEnabled = $state(false);
  let showGroupMenu = $state(false);

  // Quality/Format filter with checkboxes (AND between sections, OR within section)
  let showFilterPanel = $state(false);
  let filterPanelRef: HTMLDivElement | null = $state(null);
  let filterPanelTimeout: ReturnType<typeof setTimeout> | null = null;

  function startFilterPanelTimer() {
    clearFilterPanelTimer();
    filterPanelTimeout = setTimeout(() => {
      showFilterPanel = false;
    }, 3000);
  }

  function clearFilterPanelTimer() {
    if (filterPanelTimeout) {
      clearTimeout(filterPanelTimeout);
      filterPanelTimeout = null;
    }
  }

  function handleFilterPanelActivity() {
    if (showFilterPanel) {
      startFilterPanelTimer();
    }
  }

  function handleClickOutsideFilterPanel(event: MouseEvent) {
    if (showFilterPanel && filterPanelRef && !filterPanelRef.contains(event.target as Node)) {
      showFilterPanel = false;
      clearFilterPanelTimer();
    }
  }

  // Effect to manage filter panel auto-close
  $effect(() => {
    if (showFilterPanel) {
      startFilterPanelTimer();
      document.addEventListener('click', handleClickOutsideFilterPanel, true);
    } else {
      clearFilterPanelTimer();
      document.removeEventListener('click', handleClickOutsideFilterPanel, true);
    }
    return () => {
      clearFilterPanelTimer();
      document.removeEventListener('click', handleClickOutsideFilterPanel, true);
    };
  });

  // Effect to close sort menu on click outside
  $effect(() => {
    if (showSortMenu) {
      const handleClickOutside = (event: MouseEvent) => {
        const target = event.target as HTMLElement;
        if (!target.closest('.sort-btn') && !target.closest('.sort-menu')) {
          showSortMenu = false;
        }
      };
      document.addEventListener('click', handleClickOutside, true);
      return () => document.removeEventListener('click', handleClickOutside, true);
    }
  });

  // Quality tier filters (OR within this group)
  let filterHiRes = $state(false);
  let filterCdQuality = $state(false);
  let filterLossy = $state(false);

  // Format filters (OR within this group)
  let filterFlac = $state(false);
  let filterAlac = $state(false);
  let filterApe = $state(false);
  let filterWav = $state(false);
  let filterMp3 = $state(false);
  let filterAac = $state(false);
  let filterOther = $state(false);

  // Source filters (OR within this group)
  let filterLocalFiles = $state(false);
  let filterOfflineCache = $state(false);

  const LOSSLESS_FORMATS = ['flac', 'wav', 'aiff', 'alac', 'ape', 'dsd', 'dsf', 'dff'];
  const LOSSY_FORMATS = ['mp3', 'aac', 'm4a', 'ogg', 'opus', 'wma'];

  // Derived: check if any filter is active
  let hasActiveFilters = $derived(
    filterHiRes || filterCdQuality || filterLossy ||
    filterFlac || filterAlac || filterApe || filterWav || filterMp3 || filterAac || filterOther ||
    filterLocalFiles || filterOfflineCache
  );

  // Count active filters for badge
  let activeFilterCount = $derived(
    [filterHiRes, filterCdQuality, filterLossy, filterFlac, filterAlac, filterApe, filterWav, filterMp3, filterAac, filterOther, filterLocalFiles, filterOfflineCache]
      .filter(Boolean).length
  );

  function matchesQualityFilters(album: LocalAlbum): boolean {
    const format = album.format.toLowerCase();
    const isLossless = LOSSLESS_FORMATS.includes(format);
    const bitDepth = album.bit_depth ?? 16;

    // Check quality tier (OR logic - pass if any selected matches, or none selected)
    const qualityFiltersActive = filterHiRes || filterCdQuality || filterLossy;
    let passesQuality = !qualityFiltersActive; // Pass if no quality filters

    if (qualityFiltersActive) {
      if (filterHiRes && isLossless && (bitDepth >= 24 || album.sample_rate > 48000)) {
        passesQuality = true;
      }
      if (filterCdQuality && isLossless && bitDepth <= 16 && album.sample_rate <= 48000) {
        passesQuality = true;
      }
      if (filterLossy && LOSSY_FORMATS.includes(format)) {
        passesQuality = true;
      }
    }

    // Check format (OR logic - pass if any selected matches, or none selected)
    const formatFiltersActive = filterFlac || filterAlac || filterApe || filterWav || filterMp3 || filterAac || filterOther;
    let passesFormat = !formatFiltersActive; // Pass if no format filters

    if (formatFiltersActive) {
      if (filterFlac && format === 'flac') passesFormat = true;
      if (filterAlac && (format === 'alac' || format === 'm4a')) passesFormat = true;
      if (filterApe && format === 'ape') passesFormat = true;
      if (filterWav && (format === 'wav' || format === 'wave')) passesFormat = true;
      if (filterMp3 && format === 'mp3') passesFormat = true;
      if (filterAac && (format === 'aac' || format === 'm4a')) passesFormat = true;
      if (filterOther && !['flac', 'alac', 'ape', 'wav', 'wave', 'mp3', 'aac', 'm4a'].includes(format)) passesFormat = true;
    }

    // Check source (OR logic - pass if any selected matches, or none selected)
    const sourceFiltersActive = filterLocalFiles || filterOfflineCache;
    let passesSource = !sourceFiltersActive; // Pass if no source filters

    if (sourceFiltersActive) {
      const source = album.source ?? 'user';
      if (filterLocalFiles && (source === 'user' || source === 'plex')) passesSource = true;
      if (filterOfflineCache && source === 'qobuz_download') passesSource = true;
    }

    // AND between sections
    return passesQuality && passesFormat && passesSource;
  }

  function clearAllFilters() {
    filterHiRes = false;
    filterCdQuality = false;
    filterLossy = false;
    filterFlac = false;
    filterAlac = false;
    filterApe = false;
    filterWav = false;
    filterMp3 = false;
    filterAac = false;
    filterOther = false;
    filterLocalFiles = false;
    filterOfflineCache = false;
  }

  // Album sorting state
  type SortBy = 'title' | 'year' | 'artist';
  type SortDirection = 'asc' | 'desc';
  let sortBy = $state<SortBy>('title');
  let sortDirection = $state<SortDirection>('asc');
  let showSortMenu = $state(false);

  const sortOptions: { value: SortBy; label: string }[] = [
    { value: 'title', label: 'Album Name' },
    { value: 'year', label: 'Release Year' },
    { value: 'artist', label: 'Artist Name' }
  ];

  function getSortLabel(): string {
    const option = sortOptions.find(o => o.value === sortBy);
    return option?.label || 'Album Name';
  }

  function toggleSortDirection() {
    sortDirection = sortDirection === 'asc' ? 'desc' : 'asc';
  }

  function selectSort(value: SortBy) {
    if (sortBy === value) {
      toggleSortDirection();
    } else {
      sortBy = value;
      sortDirection = 'asc';
    }
    showSortMenu = false;
  }

  function sortAlbums(items: LocalAlbum[]): LocalAlbum[] {
    const sorted = [...items];
    const dir = sortDirection === 'asc' ? 1 : -1;

    sorted.sort((a, b) => {
      switch (sortBy) {
        case 'title':
          return a.title.localeCompare(b.title) * dir;
        case 'year': {
          // Albums without year go to the end
          const yearA = a.year ?? (sortDirection === 'asc' ? 9999 : 0);
          const yearB = b.year ?? (sortDirection === 'asc' ? 9999 : 0);
          if (yearA !== yearB) return (yearA - yearB) * dir;
          // Secondary sort by title
          return a.title.localeCompare(b.title);
        }
        case 'artist':
          const artistCompare = a.artist.localeCompare(b.artist) * dir;
          if (artistCompare !== 0) return artistCompare;
          // Secondary sort by title
          return a.title.localeCompare(b.title);
        default:
          return 0;
      }
    });

    return sorted;
  }

  // Performance mode state
  let useVirtualization = $state(isVirtualizationEnabled());
  let virtualizedScrollTarget = $state<string | undefined>(undefined);

  // Artist view state
  let artistSearch = $state('');
  let artistViewMode = $state<'grid' | 'list'>('grid');
  let artistGroupingEnabled = $state(true); // Enable alpha grouping by default
  let showArtistGroupMenu = $state(false);
  // Selected artist for the two-column layout
  let selectedArtistName = $state<string | null>(null);
  // Reference to the artist list scroll container
  let artistListScrollRef: HTMLDivElement | null = $state(null);
  let artistImageFetchInProgress = false; // Guard against concurrent fetches
  let artistImageFetchAborted = false; // Flag to abort fetching
  let trackSearch = $state('');
  let tracksHydrationRequestId = 0;
  let searchOpen = $state(false);
  let searchInputEl = $state<HTMLInputElement | undefined>(undefined);
  type TrackGroupMode = 'album' | 'artist' | 'name';
  let trackGroupMode = $state<TrackGroupMode>('album');
  let trackGroupingEnabled = $state(false);
  let showTrackGroupMenu = $state(false);
  let trackSearchTimer: ReturnType<typeof setTimeout> | null = null;
  // Reference to virtualized track list for programmatic scrolling
  let virtualizedTrackListRef = $state<{ scrollToGroup: (groupId: string) => void } | undefined>(undefined);
  let albumSearchTimer: ReturnType<typeof setTimeout> | null = null;
  let artistSearchTimer: ReturnType<typeof setTimeout> | null = null;
  let debouncedAlbumSearch = $state('');
  let debouncedArtistSearch = $state('');

  // Data state
  let albums = $state<LocalAlbum[]>([]);
  let hiddenAlbums = $state<LocalAlbum[]>([]);
  let artists = $state<LocalArtist[]>([]);
  let tracks = $state<LocalTrack[]>([]);
  let stats = $state<LibraryStats | null>(null);
  let folders = $state<LibraryFolder[]>([]);
  let scanProgress = $state<ScanProgress | null>(null);

  // Reactive counters based on filtered data
  // Note: filteredArtistCount is defined after mergedArtists below
  let filteredAlbumCount = $derived(albums.length);
  let filteredTrackCount = $derived.by(() => {
    // When in tracks view with search results, use actual filtered count
    if (activeTab === 'tracks' && tracks.length > 0) {
      return tracks.length;
    }
    // When in albums view, calculate from filtered albums
    if (activeTab === 'albums') {
      return albums.reduce((sum, album) => sum + album.track_count, 0);
    }
    // When in artists view, calculate from filtered artists
    if (activeTab === 'artists') {
      return artists.reduce((sum, artist) => sum + artist.track_count, 0);
    }
    // Fallback for tracks view when no search results - calculate from albums
    // This ensures the counter respects filters even when tracks aren't loaded
    return albums.reduce((sum, album) => sum + album.track_count, 0);
  });

  // Merge artists with same normalized name (e.g., "Alice in Chains" and "Alice In Chains")
  // Priority goes to the variant with most albums, then tracks
  // Returns both merged list and canonical name mappings
  let artistMergeResult = $derived.by(() => {
    const byNormalized = new Map<string, LocalArtist[]>();

    // Group by normalized name (skip empty names)
    for (const artist of artists) {
      if (!artist.name || !artist.name.trim()) continue;
      const normalized = normalizeArtistName(artist.name);
      if (!normalized) continue; // Skip if normalized name is empty
      if (!byNormalized.has(normalized)) {
        byNormalized.set(normalized, []);
      }
      byNormalized.get(normalized)!.push(artist);
    }

    // Build a map of normalized artist -> unique album IDs
    // This gives us accurate album counts even when same album has tracks with different artist spellings
    const artistAlbumIds = new Map<string, Set<string>>();
    for (const album of albums) {
      const normalizedAlbumArtist = normalizeArtistName(album.artist);
      const isVariousArtists = normalizedAlbumArtist === 'various artists';

      // For all albums with all_artists, credit each artist
      if (album.all_artists) {
        const allArtistsList = album.all_artists.split(',').map(a => normalizeArtistName(a.trim()));
        for (const normalizedArtist of allArtistsList) {
          if (!normalizedArtist || normalizedArtist === 'various artists') continue;
          if (!artistAlbumIds.has(normalizedArtist)) {
            artistAlbumIds.set(normalizedArtist, new Set());
          }
          artistAlbumIds.get(normalizedArtist)!.add(album.id);
        }
      } else if (!isVariousArtists) {
        // Fallback to album.artist (only if not Various Artists)
        if (!artistAlbumIds.has(normalizedAlbumArtist)) {
          artistAlbumIds.set(normalizedAlbumArtist, new Set());
        }
        artistAlbumIds.get(normalizedAlbumArtist)!.add(album.id);
      }
    }

    // Helper to get album count for an artist (handles multi-artist names)
    function getAlbumCountForArtist(artistName: string, normalized: string): number {
      // First try direct lookup
      const direct = artistAlbumIds.get(normalized);
      if (direct && direct.size > 0) return direct.size;

      // For multi-artist names like "A, B, C", find albums where ANY part appears
      const parts = artistName.split(/,|&|feat\.|ft\.|featuring|with/i)
        .map(p => normalizeArtistName(p.trim()))
        .filter(p => p.length > 0);

      if (parts.length > 1) {
        // Union of all albums where ANY artist appears
        const unionAlbums = new Set<string>();
        for (const part of parts) {
          const partAlbums = artistAlbumIds.get(part);
          if (partAlbums) {
            for (const albumId of partAlbums) {
              unionAlbums.add(albumId);
            }
          }
        }
        return unionAlbums.size;
      }

      return 0;
    }

    // Merge groups and build canonical mappings
    const merged: LocalArtist[] = [];
    const newCanonicalMappings = new Map<string, string>();

    for (const [normalized, variants] of byNormalized) {
      if (variants.length === 1) {
        // Single variant - use actual album count
        const actualAlbumCount = getAlbumCountForArtist(variants[0].name, normalized);
        merged.push({
          ...variants[0],
          album_count: actualAlbumCount || variants[0].album_count
        });
        continue;
      }

      // Find the canonical variant (most albums, then most tracks)
      const sorted = [...variants].sort((a, b) => {
        if (b.album_count !== a.album_count) return b.album_count - a.album_count;
        return b.track_count - a.track_count;
      });
      const canonical = sorted[0];

      // Get actual unique album count using helper
      const actualAlbumCount = getAlbumCountForArtist(canonical.name, normalized);
      // Sum track counts (tracks are unique per artist spelling in DB)
      const totalTracks = variants.reduce((sum, v) => sum + v.track_count, 0);

      // Create merged artist with canonical name and accurate album count
      merged.push({
        name: canonical.name,
        album_count: actualAlbumCount || canonical.album_count,
        track_count: totalTracks
      });

      // Map all variants to canonical name
      for (const variant of variants) {
        if (variant.name !== canonical.name) {
          newCanonicalMappings.set(variant.name, canonical.name);
        }
      }
    }

    return { merged, canonicalMappings: newCanonicalMappings };
  });

  // Extract merged artists list
  let mergedArtists = $derived(artistMergeResult.merged);

  // Artist count uses merged artists (after deduplication)
  let filteredArtistCount = $derived(mergedArtists.length);

  // Combined canonical names: merge mappings + Qobuz mappings
  // This is a derived that combines both sources without causing infinite loops
  let allCanonicalNames = $derived.by(() => {
    const combined = new Map(canonicalNames); // Start with Qobuz mappings
    // Add merge mappings (these take precedence for case normalization)
    for (const [variant, canonical] of artistMergeResult.canonicalMappings) {
      combined.set(variant, canonical);
    }
    return combined;
  });

  // Memoized filtered artists (uses merged artists)
  let filteredArtistsMemo = $derived.by(() => {
    if (!debouncedArtistSearch) return mergedArtists;
    const needle = debouncedArtistSearch.toLowerCase();
    return mergedArtists.filter(artist => artist.name.toLowerCase().includes(needle));
  });

  // Memoized grouped artists with alpha index and display names
  let groupedArtistsMemo = $derived.by(() => {
    const filtered = filteredArtistsMemo;

    // Add display names from canonical names mapping
    const withDisplayNames = filtered.map(artist => ({
      ...artist,
      displayName: allCanonicalNames.get(artist.name) || artist.name
    }));

    if (!artistGroupingEnabled) {
      return {
        grouped: [{ key: '', id: 'ungrouped', artists: withDisplayNames }],
        alphaGroups: new Set<string>()
      };
    }

    // Group by first letter of DISPLAY name (canonical name if available)
    const groups = new Map<string, (LocalArtist & { displayName: string })[]>();
    for (const artist of withDisplayNames) {
      const key = alphaGroupKey(artist.displayName);
      let group = groups.get(key);
      if (!group) {
        group = [];
        groups.set(key, group);
      }
      group.push(artist);
    }

    // Sort keys (# at end)
    const keys = [...groups.keys()].sort((a, b) => {
      if (a === '#') return 1;
      if (b === '#') return -1;
      return a.localeCompare(b);
    });

    const grouped = keys.map(key => ({
      key,
      id: `artist-alpha-${key}`,
      // Sort artists within group by display name
      artists: (groups.get(key) ?? []).sort((a, b) =>
        a.displayName.localeCompare(b.displayName)
      )
    }));

    return {
      grouped,
      alphaGroups: new Set(keys)
    };
  });

  // Memoized filtered and grouped albums
  let filteredAndGroupedAlbums = $derived.by(() => {
    // Filter albums by search and quality
    let filtered = albums;

    // Apply search filter
    if (debouncedAlbumSearch) {
      filtered = filtered.filter(album => matchesAlbumSearchFast(album, debouncedAlbumSearch));
    }

    // Apply quality/format filters (checkboxes)
    if (hasActiveFilters) {
      filtered = filtered.filter(album => matchesQualityFilters(album));
    }

    // Apply sorting
    filtered = sortAlbums(filtered);

    // Group if enabled
    if (!albumGroupingEnabled) {
      return {
        filtered,
        grouped: [{ key: '', id: 'ungrouped', albums: filtered }],
        alphaGroups: new Set<string>()
      };
    }

    const grouped = groupAlbumsOptimized(filtered, albumGroupMode);
    const alphaGroups = albumGroupMode === 'alpha'
      ? new Set(grouped.map(g => g.key))
      : new Set<string>();

    return { filtered, grouped, alphaGroups };
  });

  // Albums for the selected artist (used in artist view two-column layout)
  // Includes multi-artist albums where the selected artist appears
  let selectedArtistAlbums = $derived.by(() => {
    if (!selectedArtistName) return [];
    const normalizedSelected = normalizeArtistName(selectedArtistName);

    // Special case: when user selects "Various Artists" from the list,
    // show all albums where the album artist is "Various Artists"
    if (normalizedSelected === 'various artists') {
      return albums.filter(album =>
        normalizeArtistName(album.artist) === 'various artists'
      );
    }

    // Split selected artist into parts for multi-artist matching
    // e.g., "Katsutoshi Kitagawa, Mina Kubota" -> ["katsutoshi kitagawa", "mina kubota"]
    const selectedParts = selectedArtistName.split(/,|&|feat\.|ft\.|featuring|with/i)
      .map(p => normalizeArtistName(p.trim()))
      .filter(p => p.length > 0);

    return albums.filter(album => {
      const normalizedArtist = normalizeArtistName(album.artist);
      const allArtistsList = album.all_artists
        ? album.all_artists.split(',').map(a => normalizeArtistName(a.trim()))
        : [];

      // For "Various Artists" albums: include if ANY part of the artist name is in all_artists
      if (normalizedArtist === 'various artists') {
        if (allArtistsList.length === 0) return false;
        // Check exact match first
        if (allArtistsList.includes(normalizedSelected)) return true;
        // Check if ANY part of a multi-artist name is present
        if (selectedParts.length > 1 && selectedParts.some(part => allArtistsList.includes(part))) {
          return true;
        }
        return false;
      }

      // Exact match on display artist
      if (normalizedArtist === normalizedSelected) return true;

      // Check all_artists field for EXACT matches
      if (allArtistsList.includes(normalizedSelected)) return true;

      // Check if ANY part of a multi-artist name is present in all_artists
      if (selectedParts.length > 1 && allArtistsList.length > 0) {
        if (selectedParts.some(part => allArtistsList.includes(part))) return true;
      }

      // Also check individual parts split by common separators in album.artist
      const artistParts = album.artist.split(/,|&|feat\.|ft\.|featuring|with/i)
        .map(p => normalizeArtistName(p.trim()))
        .filter(p => p.length > 0);
      return artistParts.includes(normalizedSelected);
    });
  });

  // Fast album search without function call overhead
  function matchesAlbumSearchFast(album: LocalAlbum, needle: string): boolean {
    const lowerNeedle = needle.toLowerCase();
    return (
      album.title.toLowerCase().includes(lowerNeedle) ||
      album.artist.toLowerCase().includes(lowerNeedle)
    );
  }

  // Optimized grouping that avoids unnecessary allocations
  function groupAlbumsOptimized(items: LocalAlbum[], mode: AlbumGroupMode) {
    const prefix = `album-${mode}`;

    // Build groups without sorting first (sort within groups)
    const groups = new Map<string, LocalAlbum[]>();
    for (const album of items) {
      // Use canonical name for artist grouping to merge "Alice in Chains" and "Alice In Chains"
      const key = mode === 'artist'
        ? (allCanonicalNames.get(album.artist) || album.artist)
        : alphaGroupKey(album.title);
      let group = groups.get(key);
      if (!group) {
        group = [];
        groups.set(key, group);
      }
      group.push(album);
    }

    // Sort keys
    const keys = [...groups.keys()].sort((a, b) => {
      if (mode === 'alpha') {
        if (a === '#') return 1;
        if (b === '#') return -1;
      }
      return a.localeCompare(b);
    });

    // Sort albums within each group and build result
    return keys.map(key => {
      const albumsInGroup = groups.get(key) ?? [];
      albumsInGroup.sort((a, b) => a.title.localeCompare(b.title));
      return {
        key,
        id: groupIdForKey(prefix, key),
        albums: albumsInGroup
      };
    });
  }

  // Memoized filtered and grouped tracks
  let groupedTracksMemo = $derived.by(() => {
    if (!trackGroupingEnabled) {
      return {
        grouped: [{ id: 'ungrouped', title: '', subtitle: '', tracks, key: '' }],
        alphaGroups: new Set<string>(),
        indexTargets: new Map<string, string>()
      };
    }

    const grouped = groupTracks(tracks, trackGroupMode);

    // Build alpha index targets for artist mode
    let indexTargets = new Map<string, string>();
    if (trackGroupMode === 'artist') {
      for (const group of grouped) {
        const letter = alphaGroupKey(group.title);
        if (!indexTargets.has(letter)) {
          indexTargets.set(letter, group.id);
        }
      }
    }

    // Build alpha groups set
    const alphaGroups = trackGroupMode === 'name'
      ? new Set(grouped.map(group => group.key))
      : trackGroupMode === 'artist'
        ? new Set(indexTargets.keys())
        : new Set<string>();

    return { grouped, alphaGroups, indexTargets };
  });

  // Loading state
  let loading = $state(false);
  let scanning = $state(false);
  let error = $state<string | null>(null);
  let plexRepairAttempted = $state(false);
  let plexRepairInProgress = $state(false);
  let plexRepairQueued = $state(false);
  let fetchingArtwork = $state(false);
  let updatingArtwork = $state(false);
  let hasDiscogsCredentials = $state(false);
  let isOffline = $state(checkIsOffline());

  // Album detail state (for viewing album tracks)
  let selectedAlbum = $state<LocalAlbum | null>(null);
  let albumTracks = $state<LocalTrack[]>([]);

  // Qobuz artist images cache (artist name -> image URL)
  let artistImages = $state<Map<string, string>>(new Map());

  // Canonical artist names mapping (local name -> Qobuz/Discogs canonical name)
  let canonicalNames = $state<Map<string, string>>(new Map());

  // Album edit modal state
  let showAlbumEditModal = $state(false);
  let showTagEditorModal = $state(false);
  let refreshingAlbumMetadata = $state(false);
  let albumMetadataRefreshed = $state(false);
  let editingAlbumHidden = $state(false);
  let discogsImageOptions = $state<DiscogsImageOption[]>([]);
  let selectedDiscogsImage = $state<string | null>(null);
  let fetchingDiscogsImages = $state(false);
  let discogsFetchSuccessful = $state(false);
  let discogsImagePage = $state(0);
  const IMAGES_PER_PAGE = 3;

  // Folder selection state (by folder ID)
  let selectedFolders = $state<Set<number>>(new Set());

  // Folder settings modal state
  let showFolderSettingsModal = $state(false);
  let editingFolder = $state<LibraryFolder | null>(null);

  // Folder accessibility cache
  let folderAccessibility = $state<Map<number, boolean>>(new Map());

  let unsubscribeNav: (() => void) | null = null;
  let unsubscribeOffline: (() => void) | null = null;
  let unsubscribePerformance: (() => void) | null = null;

  // Reactive effect: reload library when download settings change
  $effect(() => {
    // Access the store value to create a reactive dependency
    const version = $downloadSettingsVersion;

    // Skip the initial mount (version 0)
    if (version > 0) {
      console.log('Download settings changed, reloading library data');
      loadLibraryData();
    }
  });

  // Reactive effect: reload library when offline state changes
  let previousOfflineState = $state<boolean | undefined>(undefined);
  $effect(() => {
    // Skip the initial mount
    if (previousOfflineState !== undefined && previousOfflineState !== isOffline) {
      console.log('Offline state changed to:', isOffline, '- reloading library data');
      loadLibraryData();
    }
    previousOfflineState = isOffline;
  });

  // Reactive effect: update virtualization state when album or track count changes
  // Use max of both to ensure large track libraries also trigger virtualization
  $effect(() => {
    const itemCount = Math.max(albums.length, tracks.length);
    useVirtualization = isVirtualizationEnabled() && shouldUsePerformanceMode(itemCount);
  });

  onMount(async () => {
    await loadLibraryData();
    // Load folders (now safe in offline mode - uses library_get_folders instead)
    loadFolders(); // Load in background - doesn't block UI
    checkDiscogsCredentials();

    // Subscribe to offline state changes
    unsubscribeOffline = subscribeOffline(() => {
      isOffline = checkIsOffline();
    });

    // Subscribe to performance settings changes
    unsubscribePerformance = subscribePerformance(() => {
      const itemCount = Math.max(albums.length, tracks.length);
      useVirtualization = isVirtualizationEnabled() && shouldUsePerformanceMode(itemCount);
    });

    // Subscribe to navigation changes for back/forward support
    unsubscribeNav = subscribeNav(() => {
      const navState = getNavigationState();

      // When navigating to library-album, load the album if we have an ID
      if (navState.activeView === 'library-album' && navState.selectedLocalAlbumId) {
        const albumId = navState.selectedLocalAlbumId;
        // Find album in current list or load it
        const album = albums.find(a => a.id === albumId);
        if (album && (!selectedAlbum || selectedAlbum.id !== albumId)) {
          loadAlbumById(albumId);
        }
      }

      // When navigating back to library (from library-album), clear album selection
      if (navState.activeView === 'library' && selectedAlbum) {
        selectedAlbum = null;
        albumTracks = [];
      }
    });

    // Check if we should show an album on initial load (forward navigation)
    const initialNavState = getNavigationState();
    if (initialNavState.activeView === 'library-album' && initialNavState.selectedLocalAlbumId) {
      loadAlbumById(initialNavState.selectedLocalAlbumId);
    }
  });

  onDestroy(() => {
    // Abort any ongoing artist image fetch
    artistImageFetchAborted = true;

    if (unsubscribeNav) {
      unsubscribeNav();
    }
    if (unsubscribeOffline) {
      unsubscribeOffline();
    }
    if (unsubscribePerformance) {
      unsubscribePerformance();
    }
  });

  async function loadAlbumById(albumId: string) {
    try {
      // Find album in current list
      let album = albums.find(a => a.id === albumId);

      // If not found in loaded albums, we need to fetch album list first
      if (!album) {
        const includePlex = isPlexLibraryEnabled();
        const [localAlbums, plexAlbumsRaw] = await Promise.all([
          invoke<LocalAlbum[]>('v2_library_get_albums', {
            includeHidden: false,
            excludeNetworkFolders: shouldExcludeNetworkFolders()
          }),
          includePlex
            ? invoke<PlexCachedAlbum[]>('v2_plex_cache_get_albums').catch(() => [])
            : Promise.resolve([])
        ]);
        const allAlbums = [...localAlbums, ...plexAlbumsRaw.map(mapPlexAlbum)];
        albums = allAlbums;
        album = allAlbums.find(a => a.id === albumId);
      }

      if (album) {
        selectedAlbum = album;
        albumTracks = await fetchAlbumTracks(album);
      }
    } catch (err) {
      console.error('Failed to load album:', err);
    }
  }

  async function checkDiscogsCredentials() {
    try {
      hasDiscogsCredentials = await invoke<boolean>('discogs_has_credentials');
    } catch {
      hasDiscogsCredentials = false;
    }
  }

  /**
   * Determine if we should hide network folder content based on offline state
   * - Not offline: Show everything
   * - Offline real (no_network): Always hide network content
   * - Offline manual: Hide network content ONLY if user disabled the setting
   */
  function shouldExcludeNetworkFolders(): boolean {
    try {
      console.log('[LocalLibrary] shouldExcludeNetworkFolders called, isOffline:', isOffline);
      if (!isOffline) return false;

      const reason = getOfflineReason();
      const offlineSettings = getOfflineSettings();
      console.log('[LocalLibrary] Offline reason:', reason, 'Settings:', offlineSettings);

      if (reason === 'no_network') {
        // No internet connection - always hide network folders
        console.log('[LocalLibrary] Excluding network folders (no_network)');
        return true;
      }

      if (reason === 'manual_override') {
        // Manual offline mode - respect user preference
        const exclude = !offlineSettings.showNetworkFoldersInManualOffline;
        console.log('[LocalLibrary] Excluding network folders (manual):', exclude);
        return exclude;
      }

      // Default: hide network content when offline
      console.log('[LocalLibrary] Excluding network folders (default)');
      return true;
    } catch (err) {
      console.error('[LocalLibrary] Error in shouldExcludeNetworkFolders:', err);
      return false; // On error, don't filter
    }
  }

  function isPlexLibraryEnabled(): boolean {
    if (getUserItem('qbz-plex-enabled') !== 'true') return false;
    // Disable Plex when no network is detected — Plex streams from a server
    // manual_override keeps Plex active: local Plex server may still be reachable
    if (isOffline && getOfflineReason() === 'no_network') return false;
    return true;
  }

  function buildPlexArtworkUrl(path: string): string {
    const baseUrl = getUserItem('qbz-plex-poc-base-url') || '';
    const token = getUserItem('qbz-plex-poc-token') || '';
    if (!baseUrl || !token) return path;
    const base = baseUrl.replace(/\/+$/, '');
    const separator = path.includes('?') ? '&' : '?';
    return `${base}${path}${separator}X-Plex-Token=${encodeURIComponent(token)}`;
  }

  function normalizePlexAlbumTitle(artist: string, album: string): string {
    const artistName = artist.trim();
    const albumTitle = album.trim();
    if (!artistName || !albumTitle) return albumTitle;

    for (const sep of [' - ', ' — ', ' – ', ': ']) {
      const prefix = `${artistName}${sep}`;
      if (albumTitle.startsWith(prefix)) {
        return albumTitle.slice(prefix.length).trim();
      }
    }
    return albumTitle;
  }

  function isLikelyLegacyPlexCache(plexAlbums: PlexCachedAlbum[]): boolean {
    if (plexAlbums.length < 20) return false;
    const suspicious = plexAlbums.filter(album =>
      album.title.trim().toLowerCase() === album.artist.trim().toLowerCase()
    ).length;
    return suspicious >= 20 || suspicious / plexAlbums.length >= 0.2;
  }

  async function repairLegacyPlexCache(): Promise<boolean> {
    if (plexRepairInProgress) return false;

    const baseUrl = getUserItem('qbz-plex-poc-base-url') || '';
    const token = getUserItem('qbz-plex-poc-token') || '';
    if (!baseUrl || !token) return false;

    plexRepairInProgress = true;
    try {
      const startedAt = performance.now();
      const sections = await invoke<PlexMusicSection[]>('v2_plex_get_music_sections', {
        baseUrl,
        token
      });
      await invoke('v2_plex_cache_save_sections', { serverId: null, sections });

      const sectionMetrics = await Promise.all(
        sections.map(async (section) => {
          const sectionStart = performance.now();
          const sectionTracks = await invoke<PlexTrack[]>('v2_plex_get_section_tracks', {
            baseUrl,
            token,
            sectionKey: section.key
          });
          await invoke('v2_plex_cache_save_tracks', {
            serverId: null,
            sectionKey: section.key,
            tracks: sectionTracks
          });
          return {
            key: section.key,
            tracks: sectionTracks.length,
            ms: performance.now() - sectionStart
          };
        })
      );

      const totalTracks = sectionMetrics.reduce((sum, item) => sum + item.tracks, 0);
      const elapsedMs = performance.now() - startedAt;
      console.log('[PlexBench] cache repair completed', {
        sections: sections.length,
        totalTracks,
        elapsedMs: Number(elapsedMs.toFixed(1)),
        slowestSections: sectionMetrics
          .slice()
          .sort((a, b) => b.ms - a.ms)
          .slice(0, 3)
          .map((item) => ({ key: item.key, tracks: item.tracks, ms: Number(item.ms.toFixed(1)) }))
      });

      return true;
    } catch (err) {
      console.error('[LocalLibrary] Failed to auto-repair Plex cache:', err);
      return false;
    } finally {
      plexRepairInProgress = false;
    }
  }

  async function queuePlexRepairInBackground() {
    if (plexRepairQueued || plexRepairInProgress) return;
    plexRepairQueued = true;
    try {
      const repaired = await repairLegacyPlexCache();
      if (repaired) {
        await loadLibraryData({ background: true });
      }
    } finally {
      plexRepairQueued = false;
    }
  }

  function mapPlexAlbum(plexAlbum: PlexCachedAlbum): LocalAlbum {
    const artist = plexAlbum.artist?.trim() || 'Unknown Artist';
    const title = normalizePlexAlbumTitle(artist, plexAlbum.title || 'Unknown Album');
    return {
      id: plexAlbum.id,
      title: title || 'Unknown Album',
      artist,
      artwork_path: plexAlbum.artworkPath,
      track_count: plexAlbum.trackCount,
      total_duration_secs: plexAlbum.totalDurationSecs,
      format: plexAlbum.format,
      bit_depth: plexAlbum.bitDepth,
      sample_rate: plexAlbum.sampleRate,
      directory_path: '',
      source: 'plex',
      likely_single_file_album: plexAlbum.likelySingleFileAlbum
    };
  }

  function mapPlexTrack(plexTrack: PlexCachedTrack): LocalTrack {
    const artist = plexTrack.artist?.trim() || 'Unknown Artist';
    const album = normalizePlexAlbumTitle(artist, plexTrack.album || 'Unknown Album');
    return {
      id: plexTrack.id,
      file_path: plexTrack.ratingKey,
      title: plexTrack.title,
      artist,
      album: album || 'Unknown Album',
      album_artist: artist,
      album_group_key: plexTrack.albumKey,
      album_group_title: album || 'Unknown Album',
      track_number: undefined,
      disc_number: undefined,
      duration_secs: plexTrack.durationSecs,
      format: plexTrack.format,
      bit_depth: plexTrack.bitDepth,
      sample_rate: plexTrack.sampleRate,
      channels: 2,
      file_size_bytes: 0,
      artwork_path: plexTrack.artworkPath,
      last_modified: 0,
      indexed_at: 0,
      source: 'plex'
    };
  }

  async function loadLibraryData(options: { background?: boolean } = {}) {
    const background = options.background === true;
    const startedAt = performance.now();
    console.log('[LocalLibrary] loadLibraryData START, isOffline:', isOffline, 'background:', background);
    if (!background) {
      loading = true;
      error = null;
    }
    try {
      const excludeNetwork = shouldExcludeNetworkFolders();
      console.log('[LocalLibrary] Calling library_get_albums with excludeNetwork:', excludeNetwork);

      const fetchStart = performance.now();
      const includePlex = isPlexLibraryEnabled();
      const [albumsResult, statsResult, plexAlbumsRaw] = await Promise.all([
        invoke<LocalAlbum[]>('v2_library_get_albums', {
          includeHidden: false,
          excludeNetworkFolders: excludeNetwork
        }),
        invoke<LibraryStats>('library_get_stats'),
        includePlex
          ? invoke<PlexCachedAlbum[]>('v2_plex_cache_get_albums').catch(() => [])
          : Promise.resolve([])
      ]);
      const fetchMs = performance.now() - fetchStart;
      let workingPlexAlbumsRaw = plexAlbumsRaw;
      const shouldAttemptRepair =
        includePlex &&
        !plexRepairAttempted &&
        (isLikelyLegacyPlexCache(workingPlexAlbumsRaw) || workingPlexAlbumsRaw.length <= 1);
      if (shouldAttemptRepair) {
        plexRepairAttempted = true;
        void queuePlexRepairInBackground();
      }
      const mapStart = performance.now();
      const plexAlbums = workingPlexAlbumsRaw.map(mapPlexAlbum);
      const mapMs = performance.now() - mapStart;
      console.log('[LocalLibrary] Received albums:', albumsResult.length, 'plex albums:', plexAlbums.length);
      console.log('[LocalLibrary] Received stats:', statsResult);

      const plexTrackCount = plexAlbums.reduce((sum, album) => sum + album.track_count, 0);
      const plexDurationSecs = plexAlbums.reduce((sum, album) => sum + album.total_duration_secs, 0);

      albums = [...albumsResult, ...plexAlbums];
      stats = {
        ...statsResult,
        track_count: statsResult.track_count + plexTrackCount,
        album_count: statsResult.album_count + plexAlbums.length,
        total_duration_secs: statsResult.total_duration_secs + plexDurationSecs
      };
      console.log('[PlexBench] loadLibraryData', {
        background,
        localAlbums: albumsResult.length,
        plexAlbums: plexAlbums.length,
        fetchMs: Number(fetchMs.toFixed(1)),
        mapMs: Number(mapMs.toFixed(1)),
        totalMs: Number((performance.now() - startedAt).toFixed(1)),
        repairQueued: shouldAttemptRepair
      });

      // Background hydration: fetch quality for Plex tracks with missing data
      if (includePlex && plexAlbums.length > 0) {
        void hydrateUnhydratedPlexTracks();
      }
    } catch (err) {
      console.error('[LocalLibrary] Failed to load library:', err);
      if (!background) {
        error = String(err);
      }
    } finally {
      if (!background) {
        console.log('[LocalLibrary] Setting loading = false');
        loading = false;
      }
    }
  }

  async function loadFolders() {
    try {
      console.log('[LocalLibrary] loadFolders START, isOffline:', isOffline);

      if (isOffline) {
        // When offline, get folders without calling is_network_path (blocks offline)
        // Use basic folder list command instead
        folders = await invoke<LibraryFolder[]>('library_get_folders');
        console.log('[LocalLibrary] Received folders (offline mode):', folders.length);

        // Mark all network folders as inaccessible, local folders as accessible
        for (const folder of folders) {
          folderAccessibility.set(folder.id, !folder.isNetwork);
        }
        folderAccessibility = new Map(folderAccessibility);
      } else {
        // When online, use the full metadata call with network detection
        const timeoutPromise = new Promise<LibraryFolder[]>((_, reject) =>
          setTimeout(() => reject(new Error('Folder loading timeout')), 5000)
        );
        const foldersPromise = invoke<LibraryFolder[]>('library_get_folders_with_metadata');

        folders = await Promise.race([foldersPromise, timeoutPromise]);
        console.log('[LocalLibrary] Received folders (online mode):', folders.length);

        // Check accessibility for network folders
        for (const folder of folders) {
          if (folder.isNetwork) {
            checkFolderAccessibility(folder);
          } else {
            folderAccessibility.set(folder.id, true);
          }
        }
        folderAccessibility = new Map(folderAccessibility);
      }
    } catch (err) {
      console.error('[LocalLibrary] Failed to load folders (timeout or error):', err);
      // Continue anyway - folders are not critical for basic library functionality
    }
  }

  async function checkFolderAccessibility(folder: LibraryFolder) {
    try {
      const accessible = await invoke<boolean>('v2_library_check_folder_accessible', { path: folder.path });
      folderAccessibility.set(folder.id, accessible);
      folderAccessibility = new Map(folderAccessibility);
    } catch (err) {
      console.error('Failed to check folder accessibility:', err);
      folderAccessibility.set(folder.id, false);
      folderAccessibility = new Map(folderAccessibility);
    }
  }

  async function loadArtists() {
    console.log('[LocalLibrary] loadArtists START');
    loading = true;
    try {
      console.log('[LocalLibrary] Calling library_get_artists + plex_cache_get_albums');
      const includePlex = isPlexLibraryEnabled();
      const [localArtists, plexAlbumsRaw] = await Promise.all([
        invoke<LocalArtist[]>('library_get_artists', {
          excludeNetworkFolders: shouldExcludeNetworkFolders()
        }),
        includePlex
          ? invoke<PlexCachedAlbum[]>('v2_plex_cache_get_albums').catch(() => [])
          : Promise.resolve([])
      ]);

      const plexArtistMap = new Map<string, { albumCount: number; trackCount: number }>();
      for (const rawAlbum of plexAlbumsRaw) {
        const name = (rawAlbum.artist || '').trim();
        if (!name) continue;
        const current = plexArtistMap.get(name) ?? { albumCount: 0, trackCount: 0 };
        current.albumCount += 1;
        current.trackCount += rawAlbum.trackCount ?? 0;
        plexArtistMap.set(name, current);
      }

      const mergedArtistMap = new Map<string, LocalArtist>();
      for (const localArtist of localArtists) {
        mergedArtistMap.set(localArtist.name, { ...localArtist });
      }
      for (const [name, metrics] of plexArtistMap) {
        const existing = mergedArtistMap.get(name);
        if (existing) {
          existing.album_count += metrics.albumCount;
          existing.track_count += metrics.trackCount;
        } else {
          mergedArtistMap.set(name, {
            name,
            album_count: metrics.albumCount,
            track_count: metrics.trackCount
          });
        }
      }

      artists = Array.from(mergedArtistMap.values()).sort((a, b) => a.name.localeCompare(b.name));
      console.log('[LocalLibrary] Received artists:', artists.length, 'local:', localArtists.length, 'plex:', plexArtistMap.size);
      // Load cached artist images from database
      await loadCachedArtistImages();
      // Fetch missing images in background if enabled
      const fetchEnabled = getUserItem('qbz-fetch-artist-images') !== 'false';
      if (fetchEnabled) {
        fetchMissingArtistImages();
      }
    } catch (err) {
      console.error('[LocalLibrary] Failed to load artists:', err);
      error = String(err);
    } finally {
      console.log('[LocalLibrary] loadArtists COMPLETE');
      loading = false;
    }
  }

  async function loadTracks(query = '') {
    console.log('[LocalLibrary] loadTracks START, query:', query);
    const requestId = ++tracksHydrationRequestId;
    loading = true;
    try {
      console.log('[LocalLibrary] Calling library_search + plex_cache_search_tracks');
      const includePlex = isPlexLibraryEnabled();
      const [localTracks, plexTracksRaw] = await Promise.all([
        invoke<LocalTrack[]>('library_search', {
          query,
          limit: 0, // 0 = no limit, virtualization handles any list size
          excludeNetworkFolders: shouldExcludeNetworkFolders()
        }),
        includePlex
          ? invoke<PlexCachedTrack[]>('v2_plex_cache_search_tracks', {
              query
            }).catch(() => [])
          : Promise.resolve([])
      ]);
      const mappedPlexTracks = plexTracksRaw.map(mapPlexTrack);
      tracks = [...localTracks, ...mappedPlexTracks];
      console.log('[LocalLibrary] Received tracks:', tracks.length, 'local:', localTracks.length, 'plex:', plexTracksRaw.length);

      // Hydrate Plex quality in the background; don't block rendering track lists.
      // Guard with requestId to avoid stale updates after a newer search.
      void hydratePlexTrackQuality(mappedPlexTracks)
        .then((hydratedPlexTracks) => {
          if (requestId !== tracksHydrationRequestId) return;
          tracks = [...localTracks, ...hydratedPlexTracks];
        })
        .catch((error) => {
          console.warn('[LocalLibrary] Background Plex quality hydration failed:', error);
        });
    } catch (err) {
      console.error('[LocalLibrary] Failed to load tracks:', err);
      error = String(err);
    } finally {
      console.log('[LocalLibrary] loadTracks COMPLETE');
      loading = false;
    }
  }

  function handleTabChange(tab: TabType) {
    activeTab = tab;

    // If we're viewing an album, navigate back to library
    const navState = getNavigationState();
    if (navState.activeView === 'library-album') {
      clearLocalAlbum();
      navigateTo('library');
    }

    // Clear local state
    selectedAlbum = null;
    albumTracks = [];

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

      const newFolder = await invoke<LibraryFolder>('library_add_folder', { path: selected });
      await loadFolders();

      // Show warning if network folder detected
      if (newFolder.isNetwork) {
        alert($t('library.networkFolderDetected'));
      }
    } catch (err) {
      console.error('Failed to add folder:', err);
    }
  }

  async function handleRemoveFolder(folder: LibraryFolder) {
    const displayName = folder.alias || folder.path;
    if (!confirm(`Remove "${displayName}" from library? This will remove all indexed tracks from this folder.`)) {
      return;
    }

    try {
      await invoke('v2_library_remove_folder', { path: folder.path });
      selectedFolders.delete(folder.id);
      selectedFolders = new Set(selectedFolders);
      await loadFolders();
      await loadLibraryData();
    } catch (err) {
      console.error('Failed to remove folder:', err);
      alert(`Failed to remove folder: ${err}`);
    }
  }

  function toggleFolderSelection(folderId: number) {
    if (selectedFolders.has(folderId)) {
      selectedFolders.delete(folderId);
    } else {
      selectedFolders.add(folderId);
    }
    selectedFolders = new Set(selectedFolders);
  }

  function handleEditFolder() {
    if (selectedFolders.size !== 1) return;
    const folderId = Array.from(selectedFolders)[0];
    editingFolder = folders.find(f => f.id === folderId) || null;
    if (editingFolder) {
      showFolderSettingsModal = true;
    }
  }

  function handleFolderSettingsSave(updatedFolder: LibraryFolder) {
    // Update folder in list
    const index = folders.findIndex(f => f.id === updatedFolder.id);
    if (index !== -1) {
      folders[index] = updatedFolder;
      folders = [...folders];
    }
    editingFolder = null;
  }

  async function handleScanSingleFolder(folderId: number) {
    try {
      scanning = true;
      await invoke('v2_library_scan_folder', { folderId });
      // Start polling for progress
      const progressInterval = setInterval(async () => {
        scanProgress = await invoke<ScanProgress>('v2_library_get_scan_progress');
        if (scanProgress.status === 'Complete' || scanProgress.status === 'Cancelled' || scanProgress.status === 'Error') {
          clearInterval(progressInterval);
          scanning = false;
          await loadLibraryData();
          await loadFolders();
        }
      }, 500);
    } catch (err) {
      console.error('Failed to scan folder:', err);
      scanning = false;
      alert(`Failed to scan folder: ${err}`);
    }
  }

  async function handleRemoveSelectedFolders() {
    if (selectedFolders.size === 0) return;

    const count = selectedFolders.size;
    if (!confirm(`Remove ${count} selected folder${count > 1 ? 's' : ''}? This will remove all indexed tracks from these folders.`)) return;

    try {
      for (const folderId of selectedFolders) {
        const folder = folders.find(f => f.id === folderId);
        if (folder) {
          await invoke('v2_library_remove_folder', { path: folder.path });
        }
      }
      selectedFolders.clear();
      selectedFolders = new Set(selectedFolders);
      await loadFolders();
      await loadLibraryData();
    } catch (err) {
      console.error('Failed to remove folders:', err);
      alert(`Failed to remove folders: ${err}`);
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
        scanProgress = await invoke<ScanProgress>('v2_library_get_scan_progress');
        if (scanProgress.status === 'Complete' || scanProgress.status === 'Cancelled' || scanProgress.status === 'Error') {
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
      await invoke('v2_library_scan');
    } catch (err) {
      console.error('Scan failed:', err);
      scanning = false;
      clearInterval(pollInterval);
    }
  }

  async function handleStopScan() {
    try {
      await invoke('v2_library_stop_scan');
    } catch (err) {
      console.error('Failed to stop scan:', err);
    }
  }

  let clearingLibrary = $state(false);
  let cleaningUpMissingFiles = $state(false);
  let cleanupStatus = $state('');

  async function handleCleanupMissingFiles() {
    if (cleaningUpMissingFiles) return;

    cleaningUpMissingFiles = true;
    cleanupStatus = 'Scanning track paths...';
    try {
      const result = await invoke<{ checked: number; removed: number }>('library_cleanup_missing_files');

      if (result.removed > 0) {
        cleanupStatus = `Removed ${result.removed} of ${result.checked} tracks`;
        showToast($t('toast.removedMissingFiles', { values: { count: result.removed } }), 'success');
        // Reload library data
        cleanupStatus = 'Refreshing library...';
        await loadLibraryData();
        await loadArtists();
      } else {
        cleanupStatus = `Checked ${result.checked} tracks - all OK`;
        showToast($t('toast.noMissingFilesFound'), 'info');
      }
    } catch (err) {
      console.error('Failed to cleanup missing files:', err);
      cleanupStatus = 'Error during cleanup';
      showToast($t('toast.failedCleanupMissing'), 'error');
    } finally {
      cleaningUpMissingFiles = false;
      // Clear status after a delay
      setTimeout(() => {
        cleanupStatus = '';
      }, 3000);
    }
  }

  async function handleClearLibrary(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();

    if (clearingLibrary) return;

    // First confirmation using Tauri dialog (async, properly sequential)
    const firstConfirm = await ask(
      'This will remove ALL indexed tracks from the database.\n' +
      'Your audio files will NOT be deleted.\n\n' +
      'You will need to re-scan your folders after this.',
      {
        title: 'Clear entire library?',
        kind: 'warning',
        okLabel: 'Continue',
        cancelLabel: 'Cancel'
      }
    );

    if (!firstConfirm) {
      return;
    }

    // Second confirmation - only shown after first is confirmed
    const secondConfirm = await ask(
      'This action cannot be undone.',
      {
        title: 'Are you absolutely sure?',
        kind: 'warning',
        okLabel: 'Clear Library',
        cancelLabel: 'Cancel'
      }
    );

    if (!secondConfirm) {
      return;
    }

    // Only proceed if both confirmations passed
    clearingLibrary = true;

    try {
      await invoke('v2_library_clear');
      await loadLibraryData();
      albums = [];
      artists = [];
      tracks = [];
    } catch (err) {
      console.error('Failed to clear library:', err);
      alert(`Failed to clear library: ${err}`);
    } finally {
      clearingLibrary = false;
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

      const cachedPath = await invoke<string>('v2_library_set_album_artwork', {
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
    // Use navigation store for proper back/forward support
    selectLocalAlbum(album.id);
    // Also load album data immediately for responsive UI
    selectedAlbum = album;
    try {
      albumTracks = await fetchAlbumTracks(album);
    } catch (err) {
      console.error('Failed to load album tracks:', err);
    }
  }

  async function handleAlbumPlayFromGrid(album: LocalAlbum) {
    const tracks = await fetchAlbumTracks(album);
    if (!tracks.length) return;

    await setQueueForAlbumTracks(tracks);
    await handleTrackPlay(tracks[0]);
  }

  async function handleAlbumQueueNextFromGrid(album: LocalAlbum) {
    if (!onTrackPlayNext) return;
    const tracks = await fetchAlbumTracks(album);
    if (!tracks.length) return;
    for (let i = tracks.length - 1; i >= 0; i--) {
      onTrackPlayNext(tracks[i]);
    }
  }

  async function handleAlbumQueueLaterFromGrid(album: LocalAlbum) {
    if (!onTrackPlayLater) return;
    const tracks = await fetchAlbumTracks(album);
    if (!tracks.length) return;
    for (const track of tracks) {
      onTrackPlayLater(track);
    }
  }

  async function handleTrackPlay(track: LocalTrack) {
    try {
      const trackSource = track.source === 'plex' ? 'plex' : 'local';
      if (selectedAlbum && albumTracks.length > 0) {
        const trackIndex = albumTracks.findIndex((albumTrack) => albumTrack.id === track.id);
        const trackIds = albumTracks.map(track => track.id);

        // Set playback context for Local Library album
        await setPlaybackContext(
          'local_library',
          selectedAlbum.id,
          selectedAlbum.title,
          selectedAlbum.source === 'plex' ? 'plex' : 'local',
          trackIds,
          trackIndex >= 0 ? trackIndex : 0
        );

        await setQueueForAlbumTracks(albumTracks, trackIndex >= 0 ? trackIndex : 0);
      } else if (activeTab === 'tracks' && tracks.length > 0) {
        const orderedTracks = getDisplayedTrackOrder();
        const sameSourceTracks = orderedTracks.filter(item => (item.source === 'plex') === (trackSource === 'plex'));
        const trackIndex = sameSourceTracks.findIndex(item => item.id === track.id);
        const trackIds = sameSourceTracks.map(item => item.id);

        // Set playback context for Local Library tracks view
        await setPlaybackContext(
          'local_library',
          trackSource === 'plex' ? 'plex-tracks' : 'local-tracks',
          'Local Tracks',
          trackSource,
          trackIds,
          trackIndex >= 0 ? trackIndex : 0
        );

        await setQueueForLocalTracks(sameSourceTracks, trackIndex >= 0 ? trackIndex : 0);
      }

      if (onTrackPlay) {
        await Promise.resolve(onTrackPlay(track));
      } else if (trackSource === 'local') {
        await invoke('v2_library_play_track', { trackId: track.id });
      } else {
        throw new Error('Plex playback handler not available');
      }
    } catch (err) {
      console.error('Failed to play track:', err);
      alert(`Failed to play: ${err}`);
    }
  }

  async function handlePlayAllAlbum() {
    if (!selectedAlbum || albumTracks.length === 0) return;

    try {
      await handleTrackPlay(albumTracks[0]);
    } catch (err) {
      console.error('Failed to play album:', err);
    }
  }

  async function handleShuffleAllAlbum() {
    if (!selectedAlbum || albumTracks.length === 0) return;

    try {
      console.log('[LocalLibrary Shuffle] Starting shuffle with', albumTracks.length, 'tracks');

      // Enable shuffle mode first (V2)
      await invoke('v2_set_shuffle', { enabled: true });

      // Pick a random track to start with
      const randomIndex = Math.floor(Math.random() * albumTracks.length);
      const randomTrack = albumTracks[randomIndex];

      console.log('[LocalLibrary Shuffle] Starting from random track index:', randomIndex, 'track:', randomTrack.title);

      // Play from random track (queue will be shuffled by backend)
      await handleTrackPlay(randomTrack);
    } catch (err) {
      console.error('Failed to shuffle album:', err);
    }
  }

  async function fetchAlbumTracks(album: LocalAlbum): Promise<LocalTrack[]> {
    try {
      if (album.source === 'plex') {
        if (!isPlexLibraryEnabled()) {
          return [];
        }
        const plexTracks = await invoke<PlexCachedTrack[]>('v2_plex_cache_get_album_tracks', {
          albumKey: album.id
        });
        const mappedTracks = plexTracks.map(mapPlexTrack);
        return await hydratePlexTrackQuality(mappedTracks);
      }

      return await invoke<LocalTrack[]>('library_get_album_tracks', {
        albumGroupKey: album.id
      });
    } catch (err) {
      console.error('Failed to load album tracks:', err);
      return [];
    }
  }

  function isLikelyFallbackPlexQuality(track: LocalTrack): boolean {
    if (track.source !== 'plex') return false;
    const format = (track.format || '').toLowerCase();
    const bitDepth = track.bit_depth ?? 0;
    const sampleRate = track.sample_rate ?? 0;
    return format === 'flac' && bitDepth <= 16 && sampleRate <= 44100;
  }

  async function hydratePlexTrackQuality(tracks: LocalTrack[]): Promise<LocalTrack[]> {
    const baseUrl = getUserItem('qbz-plex-poc-base-url') || '';
    const token = getUserItem('qbz-plex-poc-token') || '';
    if (!baseUrl || !token || tracks.length === 0) return tracks;

    const candidates = tracks.filter((track) => isLikelyFallbackPlexQuality(track));
    if (candidates.length === 0) return tracks;

    const metadataEntries = await Promise.all(
      candidates.map(async (track) => {
        try {
          const metadata = await invoke<PlexTrackMetadata>('v2_plex_get_track_metadata', {
            baseUrl,
            token,
            ratingKey: track.file_path
          });
          return [track.file_path, metadata] as const;
        } catch (error) {
          console.warn('[LocalLibrary] Failed to hydrate Plex track metadata for', track.file_path, error);
          return null;
        }
      })
    );

    const metadataByRatingKey = new Map<string, PlexTrackMetadata>();
    const qualityUpdates: PlexTrackQualityUpdate[] = [];
    for (const entry of metadataEntries) {
      if (!entry) continue;
      metadataByRatingKey.set(entry[0], entry[1]);
      qualityUpdates.push({
        ratingKey: entry[0],
        container: entry[1].container ?? entry[1].codec,
        samplingRateHz: entry[1].samplingRateHz,
        bitDepth: entry[1].bitDepth
      });
    }
    if (metadataByRatingKey.size === 0) return tracks;

    if (qualityUpdates.length > 0) {
      invoke<number>('v2_plex_cache_update_track_quality', { updates: qualityUpdates }).catch((error) => {
        console.warn('[LocalLibrary] Failed to persist Plex track quality updates:', error);
      });
    }

    return tracks.map((track) => {
      const metadata = metadataByRatingKey.get(track.file_path);
      if (!metadata) return track;
      return {
        ...track,
        format: (metadata.container ?? metadata.codec ?? track.format).toLowerCase(),
        bit_depth: metadata.bitDepth ?? track.bit_depth,
        sample_rate: metadata.samplingRateHz ?? track.sample_rate
      };
    });
  }

  async function hydrateUnhydratedPlexTracks(): Promise<void> {
    try {
      const baseUrl = getUserItem('qbz-plex-poc-base-url') || '';
      const token = getUserItem('qbz-plex-poc-token') || '';
      if (!baseUrl || !token) return;

      const ratingKeys = await invoke<string[]>('v2_plex_cache_get_tracks_needing_hydration', { limit: 50 });
      if (ratingKeys.length === 0) return;
      console.log('[LocalLibrary] Background hydration: fetching quality for', ratingKeys.length, 'tracks');

      const qualityUpdates: PlexTrackQualityUpdate[] = [];
      // Fetch metadata in small batches to avoid overwhelming the Plex server
      const BATCH_SIZE = 5;
      for (let i = 0; i < ratingKeys.length; i += BATCH_SIZE) {
        const batch = ratingKeys.slice(i, i + BATCH_SIZE);
        const results = await Promise.all(
          batch.map(async (ratingKey) => {
            try {
              const metadata = await invoke<PlexTrackMetadata>('v2_plex_get_track_metadata', {
                baseUrl, token, ratingKey
              });
              return { ratingKey, metadata };
            } catch {
              return null;
            }
          })
        );
        for (const result of results) {
          if (!result) continue;
          qualityUpdates.push({
            ratingKey: result.ratingKey,
            container: result.metadata.container ?? result.metadata.codec,
            samplingRateHz: result.metadata.samplingRateHz,
            bitDepth: result.metadata.bitDepth
          });
        }
      }

      if (qualityUpdates.length === 0) return;

      // Persist hydrated quality to cache
      await invoke<number>('v2_plex_cache_update_track_quality', { updates: qualityUpdates });
      console.log('[LocalLibrary] Background hydration: persisted quality for', qualityUpdates.length, 'tracks');

      // Refresh album list to show updated quality badges
      const plexAlbumsRaw = await invoke<PlexCachedAlbum[]>('v2_plex_cache_get_albums').catch(() => []);
      if (plexAlbumsRaw.length > 0) {
        const plexAlbums = plexAlbumsRaw.map(mapPlexAlbum);
        const localOnly = albums.filter(a => a.source !== 'plex');
        albums = [...localOnly, ...plexAlbums];
        console.log('[LocalLibrary] Background hydration: refreshed album list with updated quality');
      }
    } catch (err) {
      console.warn('[LocalLibrary] Background hydration failed:', err);
    }
  }

  async function setQueueForLocalTracks(tracks: LocalTrack[], startIndex = 0) {
    console.log('[LocalLibrary Queue] Setting queue with', tracks.length, 'tracks, startIndex:', startIndex);

    const queueTracks = tracks.map(track => ({
      source: track.source === 'plex' ? 'plex' : 'local',
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      duration_secs: track.duration_secs,
      artwork_url: track.artwork_path ? getArtworkUrl(track.artwork_path) : null,
      hires: (track.bit_depth && track.bit_depth > 16) || track.sample_rate > 44100,
      bit_depth: track.bit_depth ?? null,
      sample_rate: track.sample_rate ?? null,
      is_local: track.source !== 'plex',
      album_id: null,  // Local tracks don't have Qobuz IDs
      artist_id: null,
    }));

    console.log('[LocalLibrary Queue] Mapped to', queueTracks.length, 'queue tracks');
    console.log('[LocalLibrary Queue] Track IDs:', queueTracks.map(track => track.id));

    await invoke('v2_set_queue', { tracks: queueTracks, startIndex });
    onSetLocalQueue?.(tracks.filter(track => track.source !== 'plex').map(track => track.id));

    console.log('[LocalLibrary Queue] Queue set successfully');
  }

  async function setQueueForAlbumTracks(tracks: LocalTrack[], startIndex = 0) {
    await setQueueForLocalTracks(tracks, startIndex);
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

  async function handleHideAlbum(album: LocalAlbum) {
    try {
      await invoke('v2_library_set_album_hidden', { albumGroupKey: album.id, hidden: true });
      await loadLibraryData();
    } catch (err) {
      console.error('Failed to hide album:', err);
      alert(`Failed to hide album: ${err}`);
    }
  }

  async function handleShowAlbum(album: LocalAlbum) {
    try {
      await invoke('v2_library_set_album_hidden', { albumGroupKey: album.id, hidden: false });
      await loadHiddenAlbums();
      await loadLibraryData();
    } catch (err) {
      console.error('Failed to show album:', err);
      alert(`Failed to show album: ${err}`);
    }
  }

  async function loadHiddenAlbums() {
    try {
      hiddenAlbums = await invoke<LocalAlbum[]>('v2_library_get_albums', {
        includeHidden: true,
        excludeNetworkFolders: shouldExcludeNetworkFolders()
      });
      const visibleAlbumIds = new Set(albums.map(a => a.id));
      hiddenAlbums = hiddenAlbums.filter(a => !visibleAlbumIds.has(a.id));
    } catch (err) {
      console.error('Failed to load hidden albums:', err);
    }
  }

  async function toggleHiddenAlbumsView() {
    showHiddenAlbums = !showHiddenAlbums;
    if (showHiddenAlbums && hiddenAlbums.length === 0) {
      await loadHiddenAlbums();
    }
  }

  function openAlbumEditModal() {
    if (!selectedAlbum) return;
    if (selectedAlbum.source === 'plex' && getUserItem(PLEX_METADATA_WRITE_KEY) !== 'true') {
      showToast($t('settings.integrations.plexWriteDisabledNotice'), 'info');
      return;
    }
    editingAlbumHidden = false;
    albumMetadataRefreshed = false;
    discogsImageOptions = [];
    selectedDiscogsImage = null;
    discogsFetchSuccessful = false;
    showAlbumEditModal = true;
  }

  function openTagEditorFromAlbumSettings() {
    if (!selectedAlbum) return;
    showAlbumEditModal = false;
    showTagEditorModal = true;
  }

  async function handleTagEditorSaved() {
    if (!selectedAlbum) return;
    await loadLibraryData();
    await loadAlbumById(selectedAlbum.id);
  }

  async function handleRefreshAlbumMetadataFromFiles() {
    if (!selectedAlbum || refreshingAlbumMetadata) return;

    const confirmed = await ask(
      'This will re-read embedded metadata from the audio files and discard QBZ sidecar overrides for this album.',
      {
        title: 'Refresh metadata from files?',
        kind: 'warning',
        okLabel: 'Refresh',
        cancelLabel: 'Cancel'
      }
    );
    if (!confirmed) return;

    try {
      refreshingAlbumMetadata = true;
      albumMetadataRefreshed = false;
      await invoke('v2_library_refresh_album_metadata_from_files', { albumGroupKey: selectedAlbum.id });
      showToast($t('toast.metadataRefreshed'), 'success');
      albumMetadataRefreshed = true;
      await handleTagEditorSaved();
    } catch (err) {
      console.error('Failed to refresh metadata:', err);
      alert(`Failed to refresh metadata: ${err}`);
    } finally {
      refreshingAlbumMetadata = false;
    }
  }

  async function fetchDiscogsArtwork() {
    if (!selectedAlbum || fetchingDiscogsImages) return;

    try {
      fetchingDiscogsImages = true;
      discogsImageOptions = [];
      selectedDiscogsImage = null;
      discogsImagePage = 0;
      discogsFetchSuccessful = false;

      const options = await invoke<DiscogsImageOption[]>('discogs_search_artwork', {
        artist: selectedAlbum.artist,
        album: selectedAlbum.title,
        catalogNumber: selectedAlbum.catalog_number || null
      });

      discogsImageOptions = options;
      discogsFetchSuccessful = options.length > 0;
      if (options.length === 0) {
        alert('No artwork found on Discogs for this album.');
      }
      console.log(`Found ${options.length} Discogs artwork options`);
    } catch (err) {
      console.error('Failed to fetch Discogs artwork:', err);
      alert(`Failed to fetch Discogs artwork: ${err}`);
    } finally {
      fetchingDiscogsImages = false;
    }
  }

  // Computed values for Discogs image pagination
  const paginatedDiscogsImages = $derived(
    discogsImageOptions.slice(
      discogsImagePage * IMAGES_PER_PAGE,
      (discogsImagePage + 1) * IMAGES_PER_PAGE
    )
  );

  const hasMoreDiscogsPages = $derived(
    discogsImageOptions.length > (discogsImagePage + 1) * IMAGES_PER_PAGE
  );

  const hasPrevDiscogsPages = $derived(discogsImagePage > 0);

  function nextDiscogsPage() {
    if (hasMoreDiscogsPages) {
      discogsImagePage++;
    }
  }

  function prevDiscogsPage() {
    if (hasPrevDiscogsPages) {
      discogsImagePage--;
    }
  }

  async function saveAlbumEdit() {
    if (!selectedAlbum) return;

    try {
      // If a Discogs image was selected, download and set it
      if (selectedDiscogsImage) {
        console.log('Downloading Discogs artwork from:', selectedDiscogsImage);

        const localPath = await invoke<string>('discogs_download_artwork', {
          imageUrl: selectedDiscogsImage,
          artist: selectedAlbum.artist,
          album: selectedAlbum.title
        });

        console.log('Downloaded to:', localPath);

        await invoke('v2_library_set_album_artwork', {
          albumGroupKey: selectedAlbum.id,
          artworkPath: localPath
        });

        console.log('Set album artwork successfully');
        applyAlbumArtworkUpdate(selectedAlbum.id, localPath);
      }

      await invoke('v2_library_set_album_hidden', {
        albumGroupKey: selectedAlbum.id,
        hidden: editingAlbumHidden
      });

      // Reset Discogs state
      discogsImageOptions = [];
      selectedDiscogsImage = null;
      discogsImagePage = 0;

      showAlbumEditModal = false;

      if (editingAlbumHidden) {
        clearLocalAlbum();
        navGoBack();
        await loadLibraryData();
      }
    } catch (err) {
      console.error('Failed to save album settings:', err);
      alert(`Failed to save settings: ${err}`);
    }
  }

  function getQualityBadge(track: LocalTrack): string {
    const format = track.format.toUpperCase();
    const bitDepth = track.bit_depth && track.bit_depth > 0 ? String(track.bit_depth) : '--';
    const sampleRate = track.sample_rate > 0
      ? Number((track.sample_rate / 1000).toFixed(1)).toString()
      : '--';

    // Format: "FLAC 24/96" style that audiophiles love
    return `${format} ${bitDepth}/${sampleRate}`;
  }

  function isHiRes(track: LocalTrack): boolean {
    return (track.bit_depth ?? 16) >= 24 || track.sample_rate > 48000;
  }

  function formatSampleRate(hz: number): string {
    if (hz <= 0) return '-- kHz';
    return `${(hz / 1000).toFixed(1)} kHz`;
  }

  function formatBitDepth(bits?: number): string {
    return bits && bits > 0 ? `${bits}-bit` : '--bit';
  }

  function getAlbumQualityBadge(album: LocalAlbum): string {
    const format = album.format.toUpperCase();
    const bitDepth = album.bit_depth && album.bit_depth > 0 ? String(album.bit_depth) : '--';
    const sampleRate = album.sample_rate > 0
      ? Number((album.sample_rate / 1000).toFixed(1)).toString()
      : '--';
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

  // Memoization cache for artwork URLs to avoid repeated convertFileSrc calls
  const artworkUrlCache = new Map<string, string>();
  // Thumbnail URL cache (separate from full-res cache)
  let thumbnailUrlCache = $state<Map<string, string>>(new Map());
  // Track pending thumbnail requests to avoid duplicates
  const pendingThumbnails = new Set<string>();

  function getArtworkUrl(path?: string): string {
    if (!path) return '';
    if (/^https?:\/\//i.test(path)) return path;
    if (path.startsWith('/library/')) return buildPlexArtworkUrl(path);

    // For grid/list views, prefer thumbnails
    const cachedThumb = thumbnailUrlCache.get(path);
    if (cachedThumb) return cachedThumb;

    // Start thumbnail generation in background if not already pending
    if (!pendingThumbnails.has(path)) {
      pendingThumbnails.add(path);
      getThumbnailUrl(path).then(thumbUrl => {
        pendingThumbnails.delete(path);
        // Update the reactive cache to trigger re-render
        thumbnailUrlCache = new Map(thumbnailUrlCache).set(path, thumbUrl);
      }).catch(() => {
        pendingThumbnails.delete(path);
        // On error, cache the original URL
        const fallbackUrl = convertFileSrc(path);
        thumbnailUrlCache = new Map(thumbnailUrlCache).set(path, fallbackUrl);
      });
    }

    // Return full image while thumbnail loads
    const cached = artworkUrlCache.get(path);
    if (cached) return cached;

    const url = convertFileSrc(path);
    artworkUrlCache.set(path, url);
    return url;
  }

  // For album detail view, always use full resolution
  function getFullArtworkUrl(path?: string): string {
    if (!path) return '';
    if (/^https?:\/\//i.test(path)) return path;
    if (path.startsWith('/library/')) return buildPlexArtworkUrl(path);
    const cached = artworkUrlCache.get(path);
    if (cached) return cached;
    const url = convertFileSrc(path);
    artworkUrlCache.set(path, url);
    return url;
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
    // Use encodeURIComponent instead of slugify to preserve case sensitivity
    // This prevents collisions like "Aki" and "AKI" both becoming "aki"
    return `${prefix}-${encodeURIComponent(key)}`;
  }

  function groupAlbums(items: LocalAlbum[], mode: AlbumGroupMode) {
    const prefix = `album-${mode}`;
    const sorted = [...items].sort((a, b) => {
      if (mode === 'artist') {
        // Use canonical names for sorting to keep "Alice in Chains" and "Alice In Chains" together
        const aArtist = allCanonicalNames.get(a.artist) || a.artist;
        const bArtist = allCanonicalNames.get(b.artist) || b.artist;
        const artistCmp = aArtist.localeCompare(bArtist);
        if (artistCmp !== 0) return artistCmp;
        return a.title.localeCompare(b.title);
      }
      return a.title.localeCompare(b.title);
    });

    const groups = new Map<string, LocalAlbum[]>();
    for (const album of sorted) {
      // Use canonical name for artist grouping
      const key = mode === 'artist'
        ? (allCanonicalNames.get(album.artist) || album.artist)
        : alphaGroupKey(album.title);
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

  // Scroll within the artist list container (for two-column layout)
  function scrollToArtistGroup(letter: string, available: Set<string>) {
    if (!available.has(letter) || !artistListScrollRef) return;
    const id = groupIdForKey('artist-alpha', letter);
    const target = document.getElementById(id);
    if (target && artistListScrollRef) {
      const containerTop = artistListScrollRef.getBoundingClientRect().top;
      const targetTop = target.getBoundingClientRect().top;
      const scrollOffset = targetTop - containerTop + artistListScrollRef.scrollTop;
      artistListScrollRef.scrollTo({ top: scrollOffset, behavior: 'smooth' });
    }
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

  function toggleSearch() {
    searchOpen = !searchOpen;
    if (searchOpen) {
      // Focus input after it's rendered
      setTimeout(() => searchInputEl?.focus(), 50);
    } else {
      // Clear search when closing
      if (activeTab === 'albums') {
        albumSearch = '';
        debouncedAlbumSearch = '';
        if (albumSearchTimer) clearTimeout(albumSearchTimer);
      } else if (activeTab === 'artists') {
        artistSearch = '';
        debouncedArtistSearch = '';
        if (artistSearchTimer) clearTimeout(artistSearchTimer);
      } else if (activeTab === 'tracks') {
        trackSearch = '';
        loadTracks('');
      }
    }
  }

  function getCurrentSearchValue(): string {
    if (activeTab === 'albums') return albumSearch;
    if (activeTab === 'artists') return artistSearch;
    return trackSearch;
  }

  function getCurrentSearchPlaceholder(): string {
    if (activeTab === 'albums') return 'Search albums or artists...';
    if (activeTab === 'artists') return 'Search artists...';
    return 'Search tracks, albums, artists...';
  }

  function handleSearchInput(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    if (activeTab === 'albums') {
      albumSearch = value;
      scheduleAlbumSearch();
    } else if (activeTab === 'artists') {
      artistSearch = value;
      scheduleArtistSearch();
    } else if (activeTab === 'tracks') {
      trackSearch = value;
      scheduleTrackSearch();
    }
  }

  function scheduleAlbumSearch() {
    if (albumSearchTimer) {
      clearTimeout(albumSearchTimer);
    }
    albumSearchTimer = setTimeout(() => {
      debouncedAlbumSearch = albumSearch.trim();
    }, 150);
  }

  function scheduleArtistSearch() {
    if (artistSearchTimer) {
      clearTimeout(artistSearchTimer);
    }
    artistSearchTimer = setTimeout(() => {
      debouncedArtistSearch = artistSearch.trim();
    }, 150);
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

  function normalizeArtistName(name: string): string {
    return name
      .toLowerCase()
      .normalize('NFKD')
      .replace(/[\u0300-\u036f]/g, '')
      .replace(/[^a-z0-9]+/g, ' ')
      .trim();
  }

  async function resolveQobuzArtistId(name: string): Promise<number | null> {
    const query = name.trim();
    if (!query) return null;

    const results = await invoke<SearchResults<ArtistSearchResult>>('v2_search_artists', {
      query,
      limit: 5,
      offset: 0
    });

    if (!results.items.length) return null;

    const normalizedQuery = normalizeArtistName(query);
    const exactMatch = results.items.find(
      artist => normalizeArtistName(artist.name) === normalizedQuery
    );
    return (exactMatch ?? results.items[0]).id;
  }

  async function handleLocalArtistClick(name?: string) {
    if (!name) return;
    // Note: "Various Artists" is allowed when clicking from artist list,
    // but blocked in album header via HTML condition

    // Close album detail view if open and navigate to library
    if (selectedAlbum) {
      clearLocalAlbum();
      navigateTo('library'); // This adds to history so back button works
    }

    // Switch to artists tab
    activeTab = 'artists';

    // Load artists if not already loaded
    if (artists.length === 0) {
      await loadArtists();
    }

    // Select artist to show their albums in the right column
    selectedArtistName = name;

    // Scroll to the artist card after DOM updates
    // Use multiple ticks + small delay to ensure content is fully rendered
    await tick();
    setTimeout(() => {
      const artistId = `local-artist-${normalizeArtistName(name)}`;
      const artistCard = document.getElementById(artistId);
      if (artistCard) {
        artistCard.scrollIntoView({ behavior: 'smooth', block: 'center' });
      }
    }, 150);
  }

  /**
   * Load cached artist images and canonical names from database.
   */
  async function loadCachedArtistImages(): Promise<void> {
    try {
      const artistNames = artists.map(a => a.name);
      const cachedImages = await invoke<Array<{
        artist_name: string;
        image_url: string | null;
        source: string | null;
        custom_image_path: string | null;
        canonical_name: string | null;
      }>>('library_get_artist_images', { artistNames });

      for (const cached of cachedImages) {
        const imageUrl = cached.custom_image_path
          ? convertFileSrc(cached.custom_image_path)
          : cached.image_url;
        if (imageUrl) {
          artistImages.set(cached.artist_name, imageUrl);
        }
        // Store canonical name if available and different
        if (cached.canonical_name && cached.canonical_name !== cached.artist_name) {
          canonicalNames.set(cached.artist_name, cached.canonical_name);
        }
      }
      // Trigger re-render
      artistImages = new Map(artistImages);
      canonicalNames = new Map(canonicalNames);
    } catch (err) {
      console.debug('Failed to load cached artist images:', err);
    }
  }

  /**
   * Get the display name for an artist (canonical if available, otherwise original).
   */
  function getArtistDisplayName(name: string): string {
    return allCanonicalNames.get(name) || name;
  }

  /**
   * Fetch missing artist images from Qobuz only (Discogs disabled due to rate limiting).
   * Fetches sequentially with delays to avoid API abuse.
   */
  async function fetchMissingArtistImages(): Promise<void> {
    // Guard against concurrent executions
    if (artistImageFetchInProgress) {
      console.log('[LocalLibrary] Artist image fetch already in progress, skipping');
      return;
    }

    // Don't fetch external artwork when offline
    if (isOffline) {
      console.log('[LocalLibrary] Skipping artist image fetch - offline mode');
      return;
    }

    // Reset abort flag
    artistImageFetchAborted = false;
    artistImageFetchInProgress = true;

    try {
      // Filter out artists we already have images for and "Various Artists"
      const toFetch = artists.filter(artist => {
        const normalized = normalizeArtistName(artist.name);
        return normalized !== 'various artists' && !artistImages.has(artist.name);
      });

      if (toFetch.length === 0) return;

      // Limit to first 50 artists per session to avoid overwhelming APIs
      const maxFetch = 50;
      const limitedFetch = toFetch.slice(0, maxFetch);

      console.log(`[LocalLibrary] Fetching images for ${limitedFetch.length} artists (limited from ${toFetch.length})`);

      // Fetch SEQUENTIALLY with delays - no parallel requests
      const requestDelay = 1000; // 1 second between each request

      for (let i = 0; i < limitedFetch.length; i++) {
        // Check abort conditions
        if (artistImageFetchAborted || isOffline) {
          console.log('[LocalLibrary] Artist image fetch aborted');
          break;
        }

        // Add delay between requests (not before the first one)
        if (i > 0) {
          await new Promise(resolve => setTimeout(resolve, requestDelay));
        }

        const artist = limitedFetch[i];
        const name = artist.name;

        try {
          // Only use Qobuz - Discogs causes too many issues with rate limiting
          const results = await invoke<SearchResults<ArtistSearchResult>>('v2_search_artists', {
            query: name.trim(),
            limit: 3,
            offset: 0
          });

          if (results.items.length > 0) {
            const normalizedQuery = normalizeArtistName(name);
            const exactMatch = results.items.find(
              item => normalizeArtistName(item.name) === normalizedQuery
            );
            const match = exactMatch ?? results.items[0];
            const imageUrl = match.image?.large || match.image?.thumbnail || match.image?.small;
            // Store canonical name from Qobuz (properly capitalized)
            const canonicalName = match.name;

            if (imageUrl) {
              // Cache in database with canonical name
              await invoke('v2_library_cache_artist_image', {
                artistName: name,
                imageUrl,
                source: 'qobuz',
                canonicalName
              });
              artistImages.set(name, imageUrl);
              // Also store canonical name mapping
              if (canonicalName && canonicalName !== name) {
                canonicalNames.set(name, canonicalName);
              }
              // Update state periodically (every 5 artists)
              if (i % 5 === 4) {
                artistImages = new Map(artistImages);
                canonicalNames = new Map(canonicalNames);
              }
            }
          }
        } catch (err) {
          console.debug('Failed to fetch image for artist:', name, err);
        }
      }

      // Final state update
      artistImages = new Map(artistImages);
    } finally {
      artistImageFetchInProgress = false;
    }
  }

  /**
   * Legacy function - kept for compatibility but now calls new implementation.
   * @deprecated Use fetchMissingArtistImages() instead
   */
  async function fetchArtistImages(artistNames: string[]): Promise<void> {
    await fetchMissingArtistImages();
  }

  /**
   * Upload custom artist image
   */
  async function handleUploadArtistImage(artistName: string, event?: Event) {
    event?.stopPropagation();
    try {
      const selected = await open({
        multiple: false,
        filters: [{
          name: 'Image',
          extensions: ['jpg', 'jpeg', 'png', 'webp']
        }]
      });

      if (!selected) return;

      const imagePath = Array.isArray(selected) ? selected[0] : selected;
      
      // Save to database
      await invoke('v2_library_set_custom_artist_image', {
        artistName,
        customImagePath: imagePath
      });

      // Update local state
      const imageUrl = convertFileSrc(imagePath);
      artistImages.set(artistName, imageUrl);
      artistImages = new Map(artistImages);
    } catch (err) {
      console.error('Failed to upload custom artist image:', err);
    }
  }

  function handleLocalAlbumLink(track: LocalTrack) {
    if (!track.album_group_key) return;
    const album = albums.find(item => item.id === track.album_group_key);
    if (album) {
      handleAlbumClick(album);
    }
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
        // Use canonical names for sorting to keep variants together
        const aArtist = allCanonicalNames.get(a.artist) || a.artist;
        const bArtist = allCanonicalNames.get(b.artist) || b.artist;
        const artistCmp = aArtist.localeCompare(bArtist);
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
        const rawArtist = track.artist || 'Unknown Artist';
        // Use canonical name for grouping to merge "Alice in Chains" and "Alice In Chains"
        const canonicalArtist = allCanonicalNames.get(rawArtist) || rawArtist;
        if (!groups.has(canonicalArtist)) {
          groups.set(canonicalArtist, { title: canonicalArtist, tracks: [], artists: new Set([rawArtist]) });
        } else {
          groups.get(canonicalArtist)?.artists.add(rawArtist);
        }
        groups.get(canonicalArtist)?.tracks.push(track);
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

  function getDisplayedTrackOrder(): LocalTrack[] {
    if (!trackGroupingEnabled) return tracks;
    const grouped = groupTracks(tracks, trackGroupMode);
    if (trackGroupMode === 'album') {
      const ordered: LocalTrack[] = [];
      for (const group of grouped) {
        const sections = buildAlbumSections(group.tracks);
        for (const section of sections) {
          ordered.push(...section.tracks);
        }
      }
      return ordered;
    }
    return grouped.flatMap(group => group.tracks);
  }
</script>

<ViewTransition duration={200} distance={12} direction="down">
<div class="library-view" class:virtualized-active={!selectedAlbum && ((activeTab === 'albums' && !showHiddenAlbums && albums.length > 0) || (activeTab === 'artists' && artists.length > 0) || (activeTab === 'tracks' && tracks.length > 0))}>
  {#if selectedAlbum}
    {@const albumSections = buildAlbumSections(albumTracks)}
    {@const showDiscHeaders = albumSections.length > 1}
    <!-- Album Detail View -->
    <div class="album-detail">
      <div class="nav-row">
        <button class="back-btn" onclick={() => { clearLocalAlbum(); navGoBack(); }}>
          <ArrowLeft size={16} />
          <span>{$t('library.backToLibrary')}</span>
        </button>
        <button
          class="edit-btn"
          onclick={openAlbumEditModal}
          title={selectedAlbum?.source === 'plex' && getUserItem(PLEX_METADATA_WRITE_KEY) !== 'true'
            ? $t('settings.integrations.plexWriteDisabledNotice')
            : $t('actions.edit')}
          disabled={selectedAlbum?.source === 'plex' && getUserItem(PLEX_METADATA_WRITE_KEY) !== 'true'}
        >
          <Edit3 size={16} />
        </button>
      </div>

      <div class="album-header">
        <div class="album-artwork">
          {#if selectedAlbum.artwork_path}
            <img src={getFullArtworkUrl(selectedAlbum.artwork_path)} alt={selectedAlbum.title} />
          {:else}
            <div class="artwork-placeholder">
              <Disc3 size={64} />
            </div>
          {/if}
        </div>
        <div class="album-info">
          <h1>{selectedAlbum.title}</h1>
          {#if normalizeArtistName(selectedAlbum.artist) !== 'various artists'}
            <button class="artist artist-link" type="button" onclick={() => handleLocalArtistClick(selectedAlbum?.artist)}>
              {selectedAlbum.artist}
            </button>
          {:else}
            <p class="artist">{selectedAlbum.artist}</p>
          {/if}
          <p class="meta">
            {#if selectedAlbum.catalog_number}Cat# {selectedAlbum.catalog_number} &bull; {/if}
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
          {#if selectedAlbum.likely_single_file_album}
            <div class="single-file-notice">
              <AlertCircle size={14} />
              <span>{$t('library.singleFileAlbumNotice')}</span>
            </div>
          {/if}
          <div class="album-actions">
            <button class="action-btn-circle primary" onclick={handlePlayAllAlbum} title={$t('actions.playAll')}>
              <Play size={20} fill="currentColor" color="currentColor" />
            </button>
            <button class="action-btn-circle" onclick={handleShuffleAllAlbum} title={$t('actions.shuffle')}>
              <Shuffle size={18} />
            </button>
          </div>
        </div>
      </div>

      <div class="track-list">
        <div class="track-list-header">
          <div class="col-number">#</div>
          <div class="col-title">Title</div>
          <div class="col-duration">Duration</div>
          <div class="col-quality">Quality</div>
          <div class="col-spacer"></div>
          <div class="col-spacer"></div>
          <div class="col-spacer"></div>
        </div>
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
              isPlaying={isPlaybackActive && activeTrackId === track.id}
              isLocal={true}
              localSource={track.source === 'plex' ? 'plex' : 'local'}
              hideDownload={true}
              hideFavorite={true}
              onArtistClick={track.artist && track.artist !== selectedAlbum?.artist
                ? () => handleLocalArtistClick(track.artist)
                : undefined}
              onPlay={() => handleTrackPlay(track)}
              menuActions={{
                onPlayNow: () => handleTrackPlay(track),
                onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(track) : undefined,
                onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(track) : undefined,
                onAddToPlaylist: onTrackAddToPlaylist && track.source !== 'plex' ? () => onTrackAddToPlaylist(track.id) : undefined
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
        <h1>{$t('library.title')}</h1>
        {#if stats}
          <p class="subtitle">
            {stats.album_count} albums &bull; {stats.track_count} tracks &bull;
            {formatTotalDuration(stats.total_duration_secs)} &bull;
            {formatFileSize(stats.total_size_bytes)}
          </p>
        {:else}
          <p class="subtitle">{$t('library.yourCollection')}</p>
        {/if}
      </div>
      <div class="header-actions">
        <button class="icon-btn" onclick={handleScan} disabled={scanning} title={$t('library.scanLibrary')}>
          <RefreshCw size={20} class={scanning ? 'spinning' : ''} />
        </button>
        <button class="icon-btn" onclick={() => (showSettings = !showSettings)} title={$t('settings.library.title')}>
          <Settings size={20} />
        </button>
      </div>
    </div>

    <!-- Offline Notice Banner -->
    {#if isOffline}
      <div class="offline-notice">
        <AlertCircle size={16} />
        <span>{$t('library.playlistOfflineMode')}</span>
      </div>
    {/if}

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
          <button class="stop-scan-btn" onclick={handleStopScan} title="Stop scanning">
            <X size={14} />
            <span>Stop</span>
          </button>
        </div>
      </div>
    {/if}

    <!-- Settings Panel -->
    {#if showSettings}
      <div class="settings-panel">
        <div class="settings-header">
          <h3>{$t('library.folders')}</h3>
          <div class="folder-actions">
            <div class="folder-search">
              <Search size={14} />
              <input
                type="text"
                placeholder="Filter folders..."
                bind:value={folderSearch}
              />
            </div>
            <button class="icon-btn" onclick={handleAddFolder} title={$t('library.addFolder')}>
              <FolderPlus size={16} />
            </button>
            <button
              class="icon-btn"
              onclick={handleEditFolder}
              disabled={selectedFolders.size !== 1}
              title={selectedFolders.size === 1 ? $t('library.editFolder') : $t('library.editFolderHint')}
            >
              <Edit3 size={16} />
            </button>
            <button
              class="icon-btn"
              onclick={handleRemoveSelectedFolders}
              disabled={selectedFolders.size === 0}
              title={$t('library.removeSelectedFolders')}
            >
              <Trash2 size={16} />
            </button>
          </div>
        </div>

        {#if folders.length === 0}
          <div class="no-folders">
            <p>{$t('library.noFolders')}</p>
          </div>
        {:else}
          <div class="folder-table">
            {#each folders.filter(f => !folderSearch || (f.alias ?? f.path).toLowerCase().includes(folderSearch.toLowerCase())) as folder (folder.id)}
              {@const accessible = folderAccessibility.get(folder.id) ?? true}
              <div
                class="folder-row"
                class:selected={selectedFolders.has(folder.id)}
                class:disabled={!folder.enabled}
                class:inaccessible={!accessible}
                onclick={() => toggleFolderSelection(folder.id)}
                role="checkbox"
                aria-checked={selectedFolders.has(folder.id)}
                tabindex="0"
                onkeydown={(e) => e.key === 'Enter' || e.key === ' ' ? toggleFolderSelection(folder.id) : null}
              >
                <div class="folder-checkbox">
                  <input
                    type="checkbox"
                    checked={selectedFolders.has(folder.id)}
                    onclick={(e) => e.stopPropagation()}
                    onchange={() => toggleFolderSelection(folder.id)}
                  />
                </div>
                <div class="folder-icon">
                  {#if folder.isNetwork}
                    <Network size={14} class={accessible ? 'network-connected' : 'network-disconnected'} />
                  {:else}
                    <HardDrive size={14} />
                  {/if}
                </div>
                <div class="folder-info" title={folder.alias ? folder.path : ''}>
                  {#if folder.alias}
                    <span class="folder-alias">{folder.alias}</span>
                  {:else}
                    <span class="folder-path">{folder.path}</span>
                  {/if}
                </div>
                {#if !folder.enabled}
                  <span class="folder-badge disabled-badge">{$t('library.disabled')}</span>
                {:else if folder.isNetwork && !accessible}
                  <span class="folder-badge offline-badge">{$t('library.unavailable')}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <div class="settings-actions">
          <button class="btn btn-secondary" onclick={toggleHiddenAlbumsView}>
            <span>{showHiddenAlbums ? 'Show Active Albums' : 'View Hidden Albums'}</span>
            {#if hiddenAlbums.length > 0}
              <span class="count">({hiddenAlbums.length})</span>
            {/if}
          </button>
          {#if hasDiscogsCredentials}
            <button
              class="btn btn-secondary"
              onclick={handleFetchMissingArtwork}
              disabled={fetchingArtwork || isOffline}
              title={isOffline ? $t('library.artworkOffline') : ''}
            >
              <ImageDown size={14} class={fetchingArtwork ? 'spinning' : ''} />
              <span>{fetchingArtwork ? $t('library.fetchingArtwork') : $t('library.fetchMissingArtwork')}</span>
            </button>
          {:else if isOffline}
            <div class="discogs-hint">
              <span>{$t('library.artworkOffline')}</span>
            </div>
          {:else}
            <div class="discogs-hint">
              <span>Configure Discogs API for automatic artwork fetching</span>
            </div>
          {/if}
        </div>

        <div class="settings-bottom-row">
          <div class="maintenance-section">
            <div class="maintenance-label">Maintenance</div>
            <button
              class="btn btn-secondary"
              onclick={handleCleanupMissingFiles}
              disabled={cleaningUpMissingFiles}
            >
              <RefreshCw size={14} class={cleaningUpMissingFiles ? 'spinning' : ''} />
              <span>{cleaningUpMissingFiles ? 'Cleaning up...' : 'Cleanup missing files'}</span>
            </button>
            {#if cleanupStatus}
              <div class="cleanup-status">{cleanupStatus}</div>
            {/if}
          </div>

          <div class="settings-divider"></div>

          <div class="danger-zone">
            <div class="danger-zone-label">Danger Zone</div>
            <button class="danger-btn-small" onclick={(e) => handleClearLibrary(e)} disabled={clearingLibrary}>
              <Trash2 size={12} />
              <span>{clearingLibrary ? 'Clearing...' : 'Clear Library Database'}</span>
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- Tabs Navigation -->
    <div class="jump-nav">
      <div class="jump-nav-left">
        <div class="jump-links">
          <button
            class="jump-link"
            class:active={activeTab === 'albums'}
            onclick={() => handleTabChange('albums')}
          >
            Albums
          </button>
          <button
            class="jump-link"
            class:active={activeTab === 'artists'}
            onclick={() => handleTabChange('artists')}
          >
            Artists
          </button>
          <button
            class="jump-link"
            class:active={activeTab === 'tracks'}
            onclick={() => handleTabChange('tracks')}
          >
            Tracks
          </button>
        </div>
      </div>
      <div class="page-search" class:open={searchOpen}>
        {#if searchOpen}
          <div class="search-input-container">
            <input
              type="text"
              class="search-input-sticky"
              placeholder={getCurrentSearchPlaceholder()}
              value={getCurrentSearchValue()}
              bind:this={searchInputEl}
              oninput={handleSearchInput}
              onkeydown={(e) => {
                if (e.key === 'Escape') toggleSearch();
              }}
            />
            <div class="search-controls">
              <button class="search-close-btn" onclick={toggleSearch} title="Close search">
                <X size={16} />
              </button>
            </div>
          </div>
        {:else}
          <button class="search-toggle" onclick={toggleSearch} title="Search">
            <Search size={18} />
          </button>
        {/if}
      </div>
    </div>

    <!-- Content -->
    <div class="content">
      {#if loading}
        <div class="loading">
          <div class="spinner"></div>
          <p>{$t('library.loadingLibrary')}</p>
        </div>
      {:else if error}
        <div class="error">
          <AlertCircle size={48} />
          <p>{$t('library.failedLoadLibrary')}</p>
          <p class="error-detail">{error}</p>
          <button class="retry-btn" onclick={() => loadLibraryData()}>{$t('actions.retry')}</button>
        </div>
      {:else if activeTab === 'albums'}
        {#key activeTab}
        <ViewTransition duration={200} distance={12} direction="up">
        {#if showHiddenAlbums}
          <!-- Hidden Albums View -->
          <div class="albums-section">
            <div class="section-header">
              <h3>Hidden Albums ({hiddenAlbums.length})</h3>
              <button class="btn btn-secondary" onclick={toggleHiddenAlbumsView}>
                <span>{$t('library.backToActiveAlbums')}</span>
              </button>
            </div>
            {#if hiddenAlbums.length === 0}
              <div class="empty-state">
                <Disc3 size={64} />
                <p>{$t('library.noHiddenAlbums')}</p>
              </div>
            {:else}
              <div class="album-list">
                {#each hiddenAlbums as album (album.id)}
                  <div class="album-row" role="button" tabindex="0">
                    <div class="album-row-art" onclick={() => handleAlbumClick(album)}>
                      {#if album.artwork_path}
                        <img src={getArtworkUrl(album.artwork_path)} alt={album.title} loading="lazy" decoding="async" />
                      {:else}
                        <div class="artwork-placeholder">
                          <Disc3 size={28} />
                        </div>
                      {/if}
                    </div>
                    <div class="album-row-info" onclick={() => handleAlbumClick(album)}>
                      <div class="album-row-title">{album.title}</div>
                      <div class="album-row-artist">{album.artist}</div>
                      <div class="album-row-meta">
                        {#if album.year}<span>{album.year}</span><span class="separator">•</span>{/if}
                        <span>{album.track_count} tracks</span>
                        <span class="separator">•</span>
                        <span>{formatTotalDuration(album.total_duration_secs)}</span>
                        <span class="separator">•</span>
                        <span class="quality-badge" class:hires={isAlbumHiRes(album)}>{getAlbumQualityBadge(album)}</span>
                      </div>
                    </div>
                    <button class="show-album-btn" onclick={() => handleShowAlbum(album)} title="Show album">
                      <span>Show</span>
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <!-- Active Albums View -->
        {#if albums.length === 0}
          <div class="empty">
            <Disc3 size={48} />
            <p>{$t('library.noAlbumsInLibrary')}</p>
            <p class="empty-hint">{$t('library.addFoldersHint')}</p>
          </div>
        {:else}
          <!-- Use memoized filtered and grouped albums -->
          {@const { filtered: filteredAlbums, grouped: groupedAlbums, alphaGroups } = filteredAndGroupedAlbums}

          <div class="album-controls">
            <div class="dropdown-container">
              <button
                class="control-btn"
                onclick={() => (showGroupMenu = !showGroupMenu)}
                title="Group albums"
              >
                <span>{!albumGroupingEnabled
                  ? 'Group: Off'
                  : albumGroupMode === 'alpha'
                    ? 'Group: A-Z'
                    : 'Group: Artist'}</span>
              </button>
              {#if showGroupMenu}
                <div class="dropdown-menu">
                  <button
                    class="dropdown-item"
                    class:selected={!albumGroupingEnabled}
                    onclick={() => { albumGroupingEnabled = false; showGroupMenu = false; }}
                  >
                    Off
                  </button>
                  <button
                    class="dropdown-item"
                    class:selected={albumGroupingEnabled && albumGroupMode === 'alpha'}
                    onclick={() => { albumGroupMode = 'alpha'; albumGroupingEnabled = true; showGroupMenu = false; }}
                  >
                    Alphabetical (A-Z)
                  </button>
                  <button
                    class="dropdown-item"
                    class:selected={albumGroupingEnabled && albumGroupMode === 'artist'}
                    onclick={() => { albumGroupMode = 'artist'; albumGroupingEnabled = true; showGroupMenu = false; }}
                  >
                    Artist
                  </button>
                </div>
              {/if}
            </div>

            <!-- Quality/Format Filter -->
            <div class="dropdown-container" bind:this={filterPanelRef}>
              <button
                class="control-btn icon-only"
                class:active={hasActiveFilters}
                onclick={() => (showFilterPanel = !showFilterPanel)}
                title="Filter by quality/format"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M4.22657 2C2.50087 2 1.58526 4.03892 2.73175 5.32873L8.99972 12.3802V19C8.99972 19.3788 9.21373 19.725 9.55251 19.8944L13.5525 21.8944C13.8625 22.0494 14.2306 22.0329 14.5255 21.8507C14.8203 21.6684 14.9997 21.3466 14.9997 21V12.3802L21.2677 5.32873C22.4142 4.03893 21.4986 2 19.7729 2H4.22657Z"/>
                </svg>
                {#if activeFilterCount > 0}
                  <span class="filter-badge">{activeFilterCount}</span>
                {/if}
              </button>
              {#if showFilterPanel}
                <div
                  class="filter-panel"
                  onmouseenter={clearFilterPanelTimer}
                  onmouseleave={startFilterPanelTimer}
                  onclick={handleFilterPanelActivity}
                >
                  <div class="filter-panel-header">
                    <span>{$t('library.filters')}</span>
                    {#if hasActiveFilters}
                      <button class="clear-filters-btn" onclick={clearAllFilters}>{$t('library.clearAllFilters')}</button>
                    {/if}
                  </div>

                  <div class="filter-section">
                    <div class="filter-section-label">{$t('library.quality')}</div>
                    <div class="filter-checkboxes">
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterHiRes} />
                        <span class="checkmark"></span>
                        <span class="label-text">Hi-Res</span>
                        <span class="label-hint">24bit+</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterCdQuality} />
                        <span class="checkmark"></span>
                        <span class="label-text">CD Quality</span>
                        <span class="label-hint">16bit</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterLossy} />
                        <span class="checkmark"></span>
                        <span class="label-text">Lossy</span>
                      </label>
                    </div>
                  </div>

                  <div class="filter-section">
                    <div class="filter-section-label">Format</div>
                    <div class="filter-checkboxes format-grid">
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterFlac} />
                        <span class="checkmark"></span>
                        <span class="label-text">FLAC</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterAlac} />
                        <span class="checkmark"></span>
                        <span class="label-text">ALAC</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterApe} />
                        <span class="checkmark"></span>
                        <span class="label-text">APE</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterWav} />
                        <span class="checkmark"></span>
                        <span class="label-text">WAV</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterMp3} />
                        <span class="checkmark"></span>
                        <span class="label-text">MP3</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterAac} />
                        <span class="checkmark"></span>
                        <span class="label-text">AAC</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterOther} />
                        <span class="checkmark"></span>
                        <span class="label-text">Other</span>
                      </label>
                    </div>
                  </div>

                  <div class="filter-section">
                    <div class="filter-section-label">Source</div>
                    <div class="filter-checkboxes source-row">
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterLocalFiles} />
                        <span class="checkmark"></span>
                        <HardDrive size={14} class="filter-icon" />
                        <span class="label-text">Local</span>
                      </label>
                      <label class="filter-checkbox">
                        <input type="checkbox" bind:checked={filterOfflineCache} />
                        <span class="checkmark"></span>
                        <img src="/qobuz-logo-filled.svg" alt="" class="filter-icon qobuz-icon" />
                        <span class="label-text">Offline cache</span>
                      </label>
                    </div>
                  </div>
                </div>
              {/if}
            </div>

            <!-- Sort dropdown -->
            <div class="dropdown-container">
              <button
                class="control-btn icon-only"
                onclick={() => (showSortMenu = !showSortMenu)}
                title="Sort albums"
              >
                <ArrowUpDown size={14} />
              </button>
              {#if showSortMenu}
                <div class="sort-menu">
                  {#each sortOptions as option}
                    <button
                      class="dropdown-item"
                      class:selected={sortBy === option.value}
                      onclick={() => selectSort(option.value)}
                    >
                      <span>{option.label}</span>
                      {#if sortBy === option.value}
                        <span class="sort-indicator">{sortDirection === 'asc' ? '↑' : '↓'}</span>
                      {/if}
                    </button>
                  {/each}
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

            {#if albumGroupingEnabled && albumGroupMode === 'alpha'}
              <div class="alpha-index-inline">
                {#each alphaIndexLetters as letter}
                  <button
                    class="alpha-letter"
                    class:disabled={!alphaGroups.has(letter)}
                    onclick={() => {
                      const groupId = groupIdForKey('album-alpha', letter);
                      virtualizedScrollTarget = alphaGroups.has(letter) ? groupId : undefined;
                    }}
                  >
                    {letter}
                  </button>
                {/each}
              </div>
            {/if}
          </div>

          {#if filteredAlbums.length === 0}
            <div class="empty">
              <Disc3 size={48} />
              <p>{$t('library.noAlbumsMatch')}</p>
              <p class="empty-hint">{$t('library.tryDifferentSearch')}</p>
            </div>
          {:else}
            <!-- Always use virtualization for albums - handles any library size efficiently -->
            <div class="album-sections virtualized">
              <div class="virtualized-container">
                <VirtualizedAlbumList
                  groups={groupedAlbums}
                  viewMode={albumViewMode}
                  showGroupHeaders={albumGroupingEnabled}
                  {getArtworkUrl}
                  getQualityBadge={getAlbumQualityBadge}
                  isHiRes={isAlbumHiRes}
                  formatDuration={formatTotalDuration}
                  onAlbumClick={handleAlbumClick}
                  onAlbumPlay={handleAlbumPlayFromGrid}
                  onAlbumQueueNext={handleAlbumQueueNextFromGrid}
                  onAlbumQueueLater={handleAlbumQueueLaterFromGrid}
                  scrollToGroupId={virtualizedScrollTarget}
                  showSourceBadge={true}
                />
              </div>
            </div>
          {/if}
        {/if}
        {/if}
        </ViewTransition>
        {/key}
      {:else if activeTab === 'artists'}
        {#key activeTab}
        <ViewTransition duration={200} distance={12} direction="up">
        {#if artists.length === 0}
          <div class="empty">
            <Mic2 size={48} />
            <p>{$t('library.noArtistsInLibrary')}</p>
          </div>
        {:else}
          {@const { grouped: groupedArtists, alphaGroups: artistAlphaGroups } = groupedArtistsMemo}
          {@const filteredArtists = filteredArtistsMemo}

          <div class="artists-tab-container">
          <!-- Horizontal alphabetical index at the top -->
          <div class="artist-alpha-index-row">
            <div class="alpha-index-inline">
              {#each alphaIndexLetters as letter}
                <button
                  class="alpha-letter"
                  class:disabled={!artistAlphaGroups.has(letter)}
                  onclick={() => scrollToArtistGroup(letter, artistAlphaGroups)}
                >
                  {letter}
                </button>
              {/each}
            </div>
          </div>

          <!-- Two-column layout: Artists | Albums -->
          <div class="artist-two-column-layout">
            <!-- Left column: Artist cards (single column with scroll) -->
            <div class="artist-column">
              <div class="artist-column-header">
                <span class="artist-count">{filteredArtists.length} artists</span>
              </div>
              {#if filteredArtists.length === 0}
                <div class="empty-small">
                  <Mic2 size={32} />
                  <p>{$t('library.noArtistsMatch')}</p>
                </div>
              {:else}
                <div class="artist-list-scroll" bind:this={artistListScrollRef}>
                  {#each groupedArtists as group}
                    {#if group.key}
                      <div class="artist-group-header" id={group.id}>
                        {group.key}
                      </div>
                    {/if}
                    {#each group.artists as artist}
                      {@const displayName = getArtistDisplayName(artist.name)}
                      {@const artistImage = artistImages.get(artist.name)}
                      <button
                        id="local-artist-{normalizeArtistName(artist.name)}"
                        class="artist-card-compact"
                        class:selected={selectedArtistName === artist.name}
                        onclick={() => handleLocalArtistClick(artist.name)}
                      >
                        <div class="artist-card-image">
                          {#if artistImage}
                            <img src={artistImage} alt={displayName} />
                          {:else}
                            <div class="artist-placeholder">
                              <Mic2 size={24} />
                            </div>
                          {/if}
                        </div>
                        <div class="artist-card-info">
                          <span class="artist-name">{displayName}</span>
                          <span class="artist-meta">{artist.album_count} albums &bull; {artist.track_count} tracks</span>
                        </div>
                      </button>
                    {/each}
                  {/each}
                </div>
              {/if}
            </div>

            <!-- Right column: Selected artist's albums -->
            <div class="artist-albums-column">
              {#if selectedArtistName}
                <div class="artist-albums-header">
                  <h3>{getArtistDisplayName(selectedArtistName)}</h3>
                  <span class="album-count">{selectedArtistAlbums.length} albums</span>
                </div>
                {#if selectedArtistAlbums.length === 0}
                  <div class="empty-small">
                    <Disc3 size={32} />
                    <p>{$t('library.noAlbumsFound')}</p>
                  </div>
                {:else}
                  <div class="artist-albums-grid">
                    {#each selectedArtistAlbums as album}
                      <AlbumCard
                        artwork={getArtworkUrl(album.artwork_path)}
                        title={album.title}
                        artist={album.artist}
                        quality={getAlbumQualityBadge(album)}
                        size="large"
                        showFavorite={false}
                        showGenre={false}
                        onPlay={() => handleAlbumPlayFromGrid(album)}
                        onPlayNext={() => handleAlbumQueueNextFromGrid(album)}
                        onPlayLater={() => handleAlbumQueueLaterFromGrid(album)}
                        onclick={() => handleAlbumClick(album)}
                        sourceBadge={album.source === 'plex' ? 'plex' : (album.source === 'qobuz_download' ? 'qobuz_download' : 'user')}
                      />
                    {/each}
                  </div>
                {/if}
              {:else}
                <div class="empty-small centered">
                  <Mic2 size={48} />
                  <p>{$t('library.selectArtist')}</p>
                </div>
              {/if}
            </div>
          </div>
          </div>
        {/if}
        </ViewTransition>
        {/key}
      {:else if activeTab === 'tracks'}
        {#key activeTab}
        <ViewTransition duration={200} distance={12} direction="up">
        {#if tracks.length === 0}
          <div class="empty">
            <Music size={48} />
            <p>{$t('library.noTracksInLibrary')}</p>
          </div>
        {:else}
          {@const { grouped: groupedTracks, alphaGroups: trackAlphaGroups, indexTargets: trackIndexTargets } = groupedTracksMemo}
          <div class="track-controls">
            <div class="dropdown-container">
              <button
                class="control-btn"
                onclick={() => (showTrackGroupMenu = !showTrackGroupMenu)}
                title="Group tracks"
              >
                <span>
                  {!trackGroupingEnabled
                    ? 'Group: Off'
                    : trackGroupMode === 'album'
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
                    class:selected={!trackGroupingEnabled}
                    onclick={() => { trackGroupingEnabled = false; showTrackGroupMenu = false; }}
                  >
                    Off
                  </button>
                  <button
                    class="dropdown-item"
                    class:selected={trackGroupingEnabled && trackGroupMode === 'album'}
                    onclick={() => { trackGroupMode = 'album'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
                  >
                    Album
                  </button>
                  <button
                    class="dropdown-item"
                    class:selected={trackGroupingEnabled && trackGroupMode === 'artist'}
                    onclick={() => { trackGroupMode = 'artist'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
                  >
                    Artist
                  </button>
                  <button
                    class="dropdown-item"
                    class:selected={trackGroupingEnabled && trackGroupMode === 'name'}
                    onclick={() => { trackGroupMode = 'name'; trackGroupingEnabled = true; showTrackGroupMenu = false; }}
                  >
                    Name (A-Z)
                  </button>
                </div>
              {/if}
            </div>

            {#if trackGroupingEnabled && (trackGroupMode === 'name' || trackGroupMode === 'artist')}
              <div class="alpha-index-inline">
                {#each alphaIndexLetters as letter}
                  <button
                    class="alpha-letter"
                    class:disabled={!trackAlphaGroups.has(letter)}
                    onclick={() => {
                      if (!trackAlphaGroups.has(letter)) return;
                      const groupId = trackGroupMode === 'artist'
                        ? trackIndexTargets.get(letter)
                        : groupIdForKey(`track-${trackGroupMode}`, letter);
                      if (groupId) {
                        virtualizedTrackListRef?.scrollToGroup(groupId);
                      }
                    }}
                  >
                    {letter}
                  </button>
                {/each}
              </div>
            {/if}
          </div>

          <!-- Always use virtualization for tracks - handles any library size efficiently -->
          <div class="track-sections virtualized">
            <div class="virtualized-container">
              <VirtualizedTrackList
                bind:this={virtualizedTrackListRef}
                groups={groupedTracks}
                groupingEnabled={trackGroupingEnabled}
                groupMode={trackGroupMode}
                {activeTrackId}
                {isPlaybackActive}
                {formatDuration}
                {getQualityBadge}
                {buildAlbumSections}
                onTrackPlay={handleTrackPlay}
                onArtistClick={handleLocalArtistClick}
                onAlbumClick={handleLocalAlbumLink}
                onTrackPlayNext={onTrackPlayNext}
                onTrackPlayLater={onTrackPlayLater}
                onTrackAddToPlaylist={onTrackAddToPlaylist}
              />
            </div>
          </div>
        {/if}
        </ViewTransition>
        {/key}
      {/if}
    </div>
  {/if}
</div>
</ViewTransition>

<!-- Album Settings Modal -->
{#if showAlbumEditModal && selectedAlbum}
  <div class="modal-overlay" onclick={() => showAlbumEditModal = false}>
    <div class="modal" onclick={(e: MouseEvent) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>Album Settings</h2>
        <button class="close-btn" onclick={() => showAlbumEditModal = false}>
          <X size={20} />
        </button>
      </div>
      
      <div class="modal-body">
        <div class="album-header-grid">
          <div class="album-text">
            <div class="album-title">{selectedAlbum.title}</div>
            <div class="album-artist">{selectedAlbum.artist}</div>
          </div>
          <div class="album-settings-actions">
            <button
              class="album-action-btn"
              onclick={openTagEditorFromAlbumSettings}
              title={$t('library.editAlbumMetadataTitle')}
            >
              <Edit3 size={18} />
              <span>{$t('library.editAlbumInfo')}</span>
            </button>
            <button
              class="album-action-btn"
              onclick={handleRefreshAlbumMetadataFromFiles}
              disabled={refreshingAlbumMetadata}
              title="Re-read embedded file tags and discard QBZ sidecar overrides"
            >
              {#if albumMetadataRefreshed && !refreshingAlbumMetadata}
                <Check size={18} />
              {:else}
                <RefreshCw size={18} class={refreshingAlbumMetadata ? 'spinning' : ''} />
              {/if}
              <span>{refreshingAlbumMetadata ? 'Refreshing...' : 'Refresh metadata'}</span>
            </button>
            </div>
          </div>

          <div class="form-group">
            <div class="artwork-layout-header" class:discogs-active={discogsFetchSuccessful}>
              <label>Album Artwork</label>
              {#if discogsFetchSuccessful}
                <div class="discogs-layout-label">Select Artwork from Discogs</div>
              {/if}
            </div>

            <div class="artwork-layout" class:discogs-active={discogsFetchSuccessful}>
              <div class="artwork-left">
                <div class="artwork-row">
                  {#if selectedAlbum.artwork_path}
                    <img
                      src={getArtworkUrl(selectedAlbum.artwork_path)}
                      alt="Current artwork"
                      class="artwork-preview"
                    />
                  {:else}
                    <div class="artwork-preview artwork-placeholder-mini">
                      <Disc3 size={24} />
                    </div>
                  {/if}

                  <div class="artwork-actions">
                    <button class="discogs-btn" onclick={handleSetAlbumArtwork} disabled={updatingArtwork}>
                      <Upload size={14} />
                      <span>{updatingArtwork ? 'Updating...' : 'Upload cover'}</span>
                    </button>
                    <button class="discogs-btn" onclick={fetchDiscogsArtwork} disabled={fetchingDiscogsImages}>
                      <img src="/discogs_icon.svg" alt="Discogs" class="discogs-icon" />
                      <span>{fetchingDiscogsImages ? 'Fetching...' : 'Get from Discogs'}</span>
                    </button>
                  </div>
                </div>
              </div>

              {#if discogsFetchSuccessful}
                <div class="discogs-panel">
                  {#if discogsImageOptions.length > IMAGES_PER_PAGE}
                    <div class="discogs-header">
                      <div class="carousel-controls">
                        <button
                          class="carousel-btn"
                          onclick={prevDiscogsPage}
                          disabled={!hasPrevDiscogsPages}
                          title="Previous"
                        >
                          <ChevronLeft size={16} />
                        </button>
                        <span class="page-indicator">
                          {discogsImagePage + 1} / {Math.ceil(discogsImageOptions.length / IMAGES_PER_PAGE)}
                        </span>
                        <button
                          class="carousel-btn"
                          onclick={nextDiscogsPage}
                          disabled={!hasMoreDiscogsPages}
                          title="Next"
                        >
                          <ChevronRight size={16} />
                        </button>
                      </div>
                    </div>
                  {/if}

                  <div class="discogs-options discogs-options-compact">
                    {#each paginatedDiscogsImages as option, i}
                      <button
                        class="discogs-option"
                        class:selected={selectedDiscogsImage === option.url}
                        onclick={() => selectedDiscogsImage = option.url}
                        title={option.release_title ? `${option.release_title}${option.release_year ? ` (${option.release_year})` : ''}` : ''}
                      >
                        <img src={option.url} alt={`Option ${discogsImagePage * IMAGES_PER_PAGE + i + 1}`} />
                        <div class="option-info">
                          {#if option.release_title}
                            <div class="release-title">
                              {option.release_title}{#if option.release_year} ({option.release_year}){/if}
                            </div>
                          {/if}
                          <div class="image-dims">{option.width}x{option.height}</div>
                        </div>
                      </button>
                    {/each}
                  </div>
                </div>
              {/if}
            </div>
          </div>

      </div>

      <div class="modal-footer">
        <div class="footer-left">
          <label class="toggle-label footer-toggle">
            <input type="checkbox" bind:checked={editingAlbumHidden} />
            <span>Hide this album from library</span>
          </label>
          <p class="form-hint footer-hint">Hidden albums can be viewed from Settings</p>
        </div>

        <div class="footer-actions">
          <button class="btn btn-secondary" onclick={() => showAlbumEditModal = false}>
            Cancel
          </button>
          <button class="btn btn-primary" onclick={saveAlbumEdit}>
            Save
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

<!-- LocalLibrary Tag Editor Modal -->
<LocalLibraryTagEditorModal
  isOpen={showTagEditorModal}
  album={selectedAlbum}
  tracks={albumTracks}
  onClose={() => (showTagEditorModal = false)}
  onSaved={handleTagEditorSaved}
/>

<!-- Folder Settings Modal -->
<FolderSettingsModal
  isOpen={showFolderSettingsModal}
  folder={editingFolder}
  onClose={() => { showFolderSettingsModal = false; editingFolder = null; }}
  onSave={handleFolderSettingsSave}
  onScanFolder={handleScanSingleFolder}
/>

<style>
  .library-view {
    padding: 0 24px 100px 18px;
    padding-right: 8px;
    overflow-y: auto;
    height: 100%;
  }

  .library-view.virtualized-active {
    overflow: hidden;
    padding-bottom: 0;
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

  /* Offline Notice */
  .offline-notice {
    background: rgba(251, 191, 36, 0.1);
    border: 1px solid rgba(251, 191, 36, 0.3);
    border-radius: 8px;
    padding: 12px 16px;
    margin-bottom: 24px;
    display: flex;
    align-items: center;
    gap: 12px;
    color: #fbbf24;
    font-size: 14px;
  }

  .offline-notice span {
    color: var(--text-primary);
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

  .stop-scan-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 12px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .stop-scan-btn:hover {
    background: var(--error);
    border-color: var(--error);
    color: white;
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

  .no-folders {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 14px;
    background: var(--bg-tertiary);
    border-radius: 8px;
  }

  .folder-table {
    max-height: 150px;
    overflow-y: auto;
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    background: var(--bg-secondary);
  }

  .folder-table::-webkit-scrollbar {
    width: 6px;
  }

  .folder-table::-webkit-scrollbar-track {
    background: transparent;
  }

  .folder-table::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .folder-table::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .folder-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--bg-tertiary);
    transition: background 150ms ease;
    cursor: pointer;
    min-height: 36px;
  }

  .folder-row:last-child {
    border-bottom: none;
  }

  .folder-row:hover {
    background: var(--bg-tertiary);
  }

  .folder-row.selected {
    background: rgba(59, 130, 246, 0.15);
  }

  .folder-row.selected:hover {
    background: rgba(59, 130, 246, 0.2);
  }

  .folder-row.disabled {
    opacity: 0.5;
  }

  .folder-row.inaccessible {
    border-left: 3px solid #ef4444;
  }

  .folder-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    color: var(--text-muted);
  }

  .folder-icon :global(.network-connected) {
    color: #22c55e;
  }

  .folder-icon :global(.network-disconnected) {
    color: #ef4444;
  }

  .folder-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .folder-alias {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .folder-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.02em;
    padding: 2px 6px;
    border-radius: 4px;
  }

  .disabled-badge {
    background: var(--bg-tertiary);
    color: var(--text-muted);
  }

  .offline-badge {
    background: rgba(239, 68, 68, 0.1);
    color: #ef4444;
  }

  .folder-checkbox {
    display: flex;
    align-items: center;
    cursor: pointer;
    margin: 0;
  }

  .folder-checkbox input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-primary);
    cursor: pointer;
    margin: 0;
  }

  .folder-path {
    flex: 1;
    font-size: 13px;
    color: var(--text-primary);
    font-family: var(--font-mono, 'Courier New', monospace);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .folder-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .folder-search {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-muted);
  }

  .folder-search input {
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 12px;
    width: 120px;
  }

  .folder-search input::placeholder {
    color: var(--text-muted);
  }

  .no-folders {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 14px;
  }

  .folder-path {
    font-size: 13px;
    color: var(--text-primary);
    font-family: var(--font-mono);
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

  .discogs-hint {
    font-size: 12px;
    color: var(--text-muted);
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    flex: 1;
  }

  .settings-bottom-row {
    display: flex;
    gap: 24px;
    margin-top: 20px;
    padding-top: 16px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .settings-divider {
    width: 1px;
    background: var(--bg-tertiary);
    align-self: stretch;
  }

  .maintenance-section {
    flex: 1;
    display: flex;
    flex-direction: column;
  }

  .maintenance-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 12px;
  }

  .cleanup-status {
    margin-top: 8px;
    font-size: 12px;
    color: var(--text-secondary);
    padding: 6px 10px;
    background: var(--bg-tertiary);
    border-radius: 4px;
  }

  .danger-zone {
    flex: 1;
    display: flex;
    flex-direction: column;
  }

  .danger-zone-label {
    font-size: 12px;
    font-weight: 600;
    color: #ef4444;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 12px;
  }

  .danger-btn-small {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: transparent;
    color: #ef4444;
    border: 1px solid #ef4444;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .danger-btn-small:hover {
    background: #ef4444;
    color: white;
  }

  /* Tabs */
  /* Sticky Navigation */
  .jump-nav {
    position: sticky;
    top: 0;
    z-index: 10; /* Above content (z:1-2) but below dropdowns (z:50+) */
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 12px 24px;
    margin: 0 -8px 16px -24px;
    width: calc(100% + 32px);
    background: var(--bg-primary);
    border-bottom: 1px solid var(--alpha-6);
    box-shadow: 0 4px 8px -4px rgba(0, 0, 0, 0.5);
  }

  .jump-nav-left {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }

  .jump-links {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
  }

  .jump-link {
    padding: 4px 0;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: color 150ms ease, border-color 150ms ease;
  }

  .jump-link:hover {
    color: var(--text-secondary);
  }

  .jump-link.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-primary);
  }

  /* Page Search in Nav */
  .page-search {
    display: flex;
    align-items: center;
  }

  .search-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 6px;
    cursor: pointer;
    transition: color 150ms ease;
  }

  .search-toggle:hover {
    color: var(--text-primary);
  }

  .search-input-container {
    display: flex;
    align-items: center;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 0 4px 0 12px;
    animation: slideInFromRight 200ms ease-out;
  }

  @keyframes slideInFromRight {
    from {
      opacity: 0;
      transform: translateX(20px);
    }
    to {
      opacity: 1;
      transform: translateX(0);
    }
  }

  .search-input-sticky {
    width: 180px;
    padding: 6px 0;
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 13px;
    outline: none;
  }

  .search-input-sticky::placeholder {
    color: var(--text-muted);
  }

  .search-controls {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: 8px;
  }

  .search-close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    border-radius: 4px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .search-close-btn:hover {
    color: var(--text-primary);
    background-color: var(--bg-tertiary);
  }

  /* Album Controls */
  .album-controls {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
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

  .control-btn.active {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: white;
  }

  .control-btn.active:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
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
    max-height: 260px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--text-muted) transparent;
  }

  .dropdown-menu::-webkit-scrollbar {
    width: 8px;
  }

  .dropdown-menu::-webkit-scrollbar-track {
    background: transparent;
  }

  .dropdown-menu::-webkit-scrollbar-thumb {
    background: var(--text-muted);
    border-radius: 9999px;
  }

  .dropdown-menu::-webkit-scrollbar-thumb:hover {
    background: var(--text-secondary);
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

  /* Filter Panel */
  .filter-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    background: var(--accent-primary);
    color: white;
    font-size: 11px;
    font-weight: 600;
    border-radius: 9px;
    margin-left: 4px;
  }

  .filter-panel {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 12px;
    padding: 12px;
    min-width: 420px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
    z-index: 20;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px 16px;
  }

  .filter-panel-header {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 4px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .filter-panel-header span {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .clear-filters-btn {
    background: none;
    border: none;
    padding: 4px 8px;
    font-size: 12px;
    color: var(--accent-primary);
    cursor: pointer;
    border-radius: 4px;
    transition: background 150ms ease;
  }

  .clear-filters-btn:hover {
    background: var(--bg-tertiary);
  }

  .filter-section {
    margin-bottom: 0;
  }

  .filter-section:last-child {
    grid-column: 1 / -1;
  }

  .filter-section-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .filter-checkboxes {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .filter-checkboxes.format-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 6px 12px;
  }

  .filter-checkboxes.source-row {
    flex-direction: row;
    gap: 12px;
  }

  .filter-checkbox {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    padding: 6px 8px;
    border-radius: 6px;
    transition: background 150ms ease;
  }

  .filter-checkbox:hover {
    background: var(--bg-tertiary);
  }

  .filter-checkbox input {
    display: none;
  }

  .filter-checkbox .checkmark {
    width: 16px;
    height: 16px;
    border: 2px solid var(--text-muted);
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .filter-checkbox input:checked + .checkmark {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .filter-checkbox input:checked + .checkmark::after {
    content: '';
    width: 4px;
    height: 8px;
    border: solid white;
    border-width: 0 2px 2px 0;
    transform: rotate(45deg) translateY(-1px);
  }

  .filter-checkbox .label-text {
    font-size: 12px;
    color: var(--text-primary);
  }

  .filter-checkbox .label-hint {
    font-size: 11px;
    color: var(--text-muted);
    margin-left: auto;
  }

  .filter-checkbox .filter-icon {
    color: var(--text-muted);
    margin-right: 4px;
  }

  .filter-checkbox .filter-icon.qobuz-icon {
    width: 14px;
    height: 14px;
  }

  /* Sort dropdown */
  .sort-menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    padding: 6px;
    min-width: 160px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
    z-index: 20;
  }

  .sort-menu .dropdown-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 8px 12px;
    font-size: 13px;
    color: var(--text-secondary);
    background: transparent;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    transition: all 150ms ease;
  }

  .sort-menu .dropdown-item:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .sort-menu .dropdown-item.selected {
    color: var(--accent-primary);
    font-weight: 500;
  }

  .sort-menu .sort-indicator {
    font-size: 12px;
    opacity: 0.8;
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
  .album-sections {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .album-sections.virtualized {
    flex: 1;
    height: calc(100vh - 280px); /* Adjust based on header/controls height */
    min-height: 400px;
  }

  .virtualized-container {
    flex: 1;
    height: 100%;
    min-width: 0;
    overflow: hidden;
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

  .album-row-meta .quality-badge {
    font-size: 11px;
    font-weight: 600;
    color: var(--alpha-85);
    background: var(--alpha-10);
    border: 1px solid var(--alpha-15);
    border-radius: 6px;
    padding: 3px 8px;
    min-width: 90px;
    text-align: center;
    box-sizing: border-box;
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
    overflow-x: hidden;
  }

  .track-sections.virtualized {
    flex: 1;
    height: calc(100vh - 280px);
    min-height: 400px;
    overflow: hidden;
  }

  .alpha-index-inline {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    flex: 1;
    justify-content: center;
  }

  .alpha-index-inline .alpha-letter {
    width: 22px;
    height: 22px;
    font-size: 10px;
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
    color: var(--text-muted);
    font-weight: 400;
    box-sizing: border-box;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 8px;
  }

  .track-list-header .col-number {
    width: 48px;
    text-align: center;
  }

  .track-list-header .col-title {
    flex: 1;
    min-width: 0;
  }

  .track-list-header .col-duration {
    width: 80px;
    text-align: center;
  }

  .track-list-header .col-quality {
    width: 80px;
    text-align: center;
  }

  .track-list-header .col-spacer {
    width: 28px;
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
    font-size: 18px;
    font-weight: 500;
    color: var(--text-primary);
    margin: 0 0 8px 0;
  }

  .album-info .artist-link {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
    font-size: 18px;
    font-weight: 500;
    color: var(--accent-primary);
  }

  .album-info .artist-link:hover {
    color: var(--text-primary);
    text-decoration: underline;
    text-underline-offset: 2px;
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

  .single-file-notice {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-radius: 6px;
    padding: 6px 10px;
    margin-bottom: 8px;
    max-width: fit-content;
  }

  .album-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 4px;
  }

  /* Nav row for album detail */
  .nav-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
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

  /* Modal */
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }

  .modal {
    --album-settings-cover-size: 94px;
    --discogs-panel-height: calc(var(--album-settings-cover-size) + 56px);
    width: 100%;
    max-width: 704px;
    max-height: 90vh;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border-radius: 16px;
    border: 1px solid var(--bg-tertiary);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 20px 24px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .modal-header h2 {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
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

  .close-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .modal-body {
    padding: 18px 20px;
    overflow-y: auto;
  }

  .album-header-grid {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 16px;
    align-items: start;
    margin-bottom: 18px;
  }

  .album-title {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    line-height: 1.2;
  }

  .album-artist {
    margin-top: 6px;
    font-size: 18px;
    font-weight: 400;
    color: var(--text-muted);
    line-height: 1.25;
    word-break: break-word;
  }

  .album-settings-actions {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .album-action-btn {
    width: 190px;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 10px;
    padding: 0 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 10px;
    color: var(--text-primary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .album-action-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .album-action-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .form-group {
    margin-bottom: 20px;
  }

  .form-group label {
    display: block;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    margin-bottom: 8px;
  }

  .artwork-layout-header {
    display: grid;
    grid-template-columns: 1fr;
    gap: 16px;
    align-items: end;
    margin-bottom: 8px;
  }

  .artwork-layout-header.discogs-active {
    grid-template-columns: 1fr 1fr;
  }

  .artwork-layout-header label {
    margin-bottom: 0;
  }

  .discogs-layout-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    margin-left: -8px;
  }

  .toggle-label {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
  }

  .toggle-label input[type="checkbox"] {
    width: 18px;
    height: 18px;
    accent-color: var(--accent-primary);
    cursor: pointer;
  }

  .toggle-label span {
    font-size: 14px;
    color: var(--text-primary);
  }

  .form-hint {
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .artwork-row {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .artwork-layout {
    display: grid;
    grid-template-columns: 1fr;
    gap: 16px;
    align-items: start;
  }

  .artwork-layout.discogs-active {
    grid-template-columns: 1fr 1fr;
  }

  .discogs-panel {
    min-width: 0;
    border: 1px solid var(--bg-tertiary);
    border-radius: 10px;
    padding: 10px 12px;
    background: var(--bg-secondary);
    height: var(--discogs-panel-height);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    margin-left: -8px;
    width: calc(100% + 8px);
  }

  .discogs-hint {
    margin-top: 8px;
  }

  .artwork-preview {
    width: var(--album-settings-cover-size);
    height: var(--album-settings-cover-size);
    border-radius: 6px;
    object-fit: cover;
    background: var(--bg-tertiary);
  }

  .artwork-placeholder-mini {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .artwork-actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .discogs-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: var(--bg-tertiary);
    border: 1px solid var(--bg-quaternary);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .discogs-btn:hover:not(:disabled) {
    background: var(--bg-quaternary);
    border-color: var(--text-muted);
  }

  .discogs-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .discogs-icon {
    width: 16px;
    height: 16px;
    filter: invert(1) brightness(0.8);
  }

  .discogs-options {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(80px, 1fr));
    gap: 12px;
    margin-top: 8px;
  }

  .discogs-options-compact {
    grid-template-columns: repeat(3, var(--album-settings-cover-size));
    grid-template-rows: var(--album-settings-cover-size);
    justify-content: start;
    gap: 10px;
    margin-top: 0;
    flex: 1;
    overflow: hidden;
  }

  .discogs-panel .discogs-option:hover {
    transform: none;
  }

  .discogs-option {
    position: relative;
    aspect-ratio: 1;
    padding: 0;
    background: var(--bg-tertiary);
    border: 2px solid transparent;
    border-radius: 8px;
    overflow: hidden;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .discogs-option:hover {
    border-color: var(--text-muted);
    transform: scale(1.05);
  }

  .discogs-option.selected {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent);
  }

  .discogs-option img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .discogs-option .option-info {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 4px 6px;
    background: rgba(0, 0, 0, 0.8);
    color: white;
    font-size: 10px;
    text-align: center;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .discogs-option .release-title {
    font-weight: 500;
    line-height: 1.2;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .discogs-option .image-dims {
    opacity: 0.8;
    font-size: 9px;
  }

  .discogs-header {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    margin-bottom: 8px;
  }

  .carousel-controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .carousel-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: var(--bg-tertiary);
    border: 1px solid var(--bg-quaternary);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .carousel-btn:hover:not(:disabled) {
    background: var(--bg-quaternary);
    border-color: var(--text-muted);
  }

  .carousel-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .page-indicator {
    font-size: 12px;
    color: var(--text-muted);
    min-width: 40px;
    text-align: center;
  }

  .modal-footer {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 12px;
    padding: 16px 24px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .footer-left {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 60%;
  }

  .footer-actions {
    display: flex;
    gap: 12px;
  }

  .footer-hint {
    margin-top: 0;
  }

  /* Artist View - Two Column Layout */
  .artist-alpha-index-row {
    padding: 8px 0 12px;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 12px;
  }

  .artists-tab-container {
    display: flex;
    flex-direction: column;
    height: calc(100vh - 320px); /* Header + tabs + player */
    min-height: 400px;
  }

  .artist-two-column-layout {
    display: flex;
    gap: 0;
    flex: 1;
    min-height: 0;
    margin: 0 -24px 0 -18px; /* Negative margins to extend to edges */
    padding: 0;
  }

  .artist-column {
    width: 240px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    background: transparent;
    border-right: 1px solid var(--bg-tertiary);
    overflow: hidden;
    padding-left: 18px;
  }

  .artist-column-header {
    padding: 8px 16px 8px 0;
  }

  .artist-count {
    font-size: 12px;
    color: var(--text-muted);
  }

  .artist-list-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 12px 4px 0;
    /* Smooth scrolling optimizations */
    -webkit-overflow-scrolling: touch;
    scroll-behavior: smooth;
    overscroll-behavior: contain;
    /* GPU acceleration for smoother scrolling */
    will-change: scroll-position;
    contain: strict;
  }

  .artist-group-header {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    padding: 12px 8px 4px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .artist-card-compact {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 8px;
    /* Performance optimizations */
    contain: layout style;
    background: transparent;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: background 150ms ease;
    text-align: left;
  }

  .artist-card-compact:hover {
    background: var(--bg-tertiary);
  }

  .artist-card-compact.selected {
    background: var(--accent-primary);
  }

  .artist-card-compact.selected .artist-name {
    color: white;
  }

  .artist-card-compact.selected .artist-meta {
    color: rgba(255, 255, 255, 0.7);
  }

  .artist-card-image {
    width: 48px;
    height: 48px;
    flex-shrink: 0;
    border-radius: 50%;
    overflow: hidden;
  }

  .artist-card-image img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artist-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-primary) 100%);
    color: var(--text-muted);
  }

  .artist-card-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .artist-card-info .artist-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist-card-info .artist-meta {
    font-size: 11px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist-albums-column {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
    padding: 0 24px 0 24px;
  }

  .artist-albums-header {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 16px;
  }

  .artist-albums-header h3 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .artist-albums-header .album-count {
    font-size: 13px;
    color: var(--text-muted);
  }

  .artist-albums-grid {
    flex: 1;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 16px;
    align-content: start;
    padding-right: 8px;
    /* Smooth scrolling optimizations */
    -webkit-overflow-scrolling: touch;
    overscroll-behavior: contain;
    contain: strict;
  }

  .empty-small {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 32px;
    color: var(--text-muted);
    text-align: center;
  }

  .empty-small.centered {
    flex: 1;
  }

  .empty-small p {
    margin: 0;
    font-size: 13px;
  }
</style>
