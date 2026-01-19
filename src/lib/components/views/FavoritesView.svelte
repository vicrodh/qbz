<script lang="ts">
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import { Heart, Play, Disc3, Mic2, Music, Search, X, LayoutGrid, List, ChevronDown, ListMusic, Edit3, Star, Folder, Library } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import TrackRow from '../TrackRow.svelte';
  import PlaylistCollage from '../PlaylistCollage.svelte';
  import FavoritesEditModal from '../FavoritesEditModal.svelte';
  import { type DownloadStatus } from '$lib/stores/downloadState';
  import { consumeContextTrackFocus, setPlaybackContext } from '$lib/stores/playbackContextStore';
  import { normalizeFavoritesTabOrder } from '$lib/utils/favorites';
  import type { FavoritesPreferences } from '$lib/types';

  interface FavoriteAlbum {
    id: string;
    title: string;
    artist: { id: number; name: string };
    image: { small?: string; thumbnail?: string; large?: string };
    release_date_original?: string;
    hires: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
  }

  interface FavoriteTrack {
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

  interface FavoriteArtist {
    id: number;
    name: string;
    image?: { small?: string; thumbnail?: string; large?: string };
    albums_count?: number;
  }

  interface FavoritePlaylist {
    id: number;
    name: string;
    tracks_count: number;
    images?: string[];
    duration: number;
    owner: { id: number; name: string };
  }

  interface Props {
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    checkAlbumFullyDownloaded?: (albumId: string) => Promise<boolean>;
    downloadStateVersion?: number;
    onTrackPlay?: (track: DisplayTrack) => void;
    onArtistClick?: (artistId: number) => void;
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
    onPlaylistSelect?: (playlistId: number) => void;
    selectedTab?: TabType;
    onTabNavigate?: (tab: TabType) => void;
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
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
  }

  let {
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    checkAlbumFullyDownloaded,
    downloadStateVersion,
    onTrackPlay,
    onArtistClick,
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
    onPlaylistSelect,
    selectedTab,
    onTabNavigate,
    activeTrackId = null,
    isPlaybackActive = false
  }: Props = $props();

  type TabType = 'tracks' | 'albums' | 'artists' | 'playlists';
  let activeTab = $state<TabType>('tracks');
  let preferencesLoaded = $state(false);

  const tabLabels: Record<TabType, string> = {
    tracks: 'Tracks',
    albums: 'Albums',
    artists: 'Artists',
    playlists: 'Playlists',
  };

  let favoriteAlbums = $state<FavoriteAlbum[]>([]);
  let favoriteTracks = $state<FavoriteTrack[]>([]);
  let favoriteArtists = $state<FavoriteArtist[]>([]);
  let favoritePlaylists = $state<FavoritePlaylist[]>([]);

  let loading = $state(false);
  let loadingPlaylists = $state(false);
  let editModalOpen = $state(false);
  let scrollContainer: HTMLDivElement | null = $state(null);
  let favoritesPreferences = $state<FavoritesPreferences>({
    custom_icon_path: null,
    custom_icon_preset: 'heart',
    icon_background: null,
    tab_order: ['tracks', 'albums', 'artists', 'playlists'],
  });

  // Download status tracking
  let albumDownloadStatuses = $state<Map<string, boolean>>(new Map());

  async function loadAlbumDownloadStatus(albumId: string) {
    if (!checkAlbumFullyDownloaded) return false;
    try {
      const isDownloaded = await checkAlbumFullyDownloaded(albumId);
      albumDownloadStatuses.set(albumId, isDownloaded);
      return isDownloaded;
    } catch {
      return false;
    }
  }

  async function loadAllAlbumDownloadStatuses(albums: { id: string }[]) {
    if (!checkAlbumFullyDownloaded || albums.length === 0) return;
    await Promise.all(albums.map(album => loadAlbumDownloadStatus(album.id)));
  }

  function isAlbumDownloaded(albumId: string): boolean {
    void downloadStateVersion;
    return albumDownloadStatuses.get(albumId) || false;
  }

  let error = $state<string | null>(null);

  // Search state for each tab
  let trackSearch = $state('');
  let albumSearch = $state('');
  let artistSearch = $state('');
  let playlistSearch = $state('');
  let searchExpanded = $state(false);

  let albumViewMode = $state<'grid' | 'list'>('grid');
  type AlbumGroupMode = 'alpha' | 'artist';
  let albumGroupMode = $state<AlbumGroupMode>('alpha');
  let showAlbumGroupMenu = $state(false);
  let albumGroupingEnabled = $state(false);

  type TrackGroupMode = 'album' | 'artist' | 'name';
  let trackGroupMode = $state<TrackGroupMode>('album');
  let showTrackGroupMenu = $state(false);
  let trackGroupingEnabled = $state(false);

  let showArtistGroupMenu = $state(false);
  let artistGroupingEnabled = $state(false);
  function resolveCustomIconSrc(path: string | null): string | null {
    if (!path) return null;
    if (path.startsWith('asset://') || path.startsWith('http://asset.localhost') || path.startsWith('https://asset.localhost')) {
      return path;
    }
    if (path.startsWith('file://')) {
      return path;
    }
    return convertFileSrc(path);
  }

  let customIconSrc = $derived.by(() => resolveCustomIconSrc(favoritesPreferences.custom_icon_path));

  async function scrollToTrack(trackId: number) {
    await tick();
    const target = scrollContainer?.querySelector<HTMLElement>(`[data-track-id="${trackId}"]`);
    target?.scrollIntoView({ block: 'center' });
  }

  // Filtered lists based on search
  let filteredTracks = $derived.by(() => {
    if (!trackSearch.trim()) return favoriteTracks;
    const query = trackSearch.toLowerCase();
    return favoriteTracks.filter(t =>
      t.title.toLowerCase().includes(query) ||
      t.performer?.name?.toLowerCase().includes(query) ||
      t.album?.title?.toLowerCase().includes(query)
    );
  });

  let trackIndexMap = $derived.by(() => {
    return new Map(filteredTracks.map((track, index) => [track.id, index]));
  });

  let filteredAlbums = $derived.by(() => {
    if (!albumSearch.trim()) return favoriteAlbums;
    const query = albumSearch.toLowerCase();
    return favoriteAlbums.filter(a =>
      a.title.toLowerCase().includes(query) ||
      a.artist.name.toLowerCase().includes(query)
    );
  });

  let filteredArtists = $derived.by(() => {
    if (!artistSearch.trim()) return favoriteArtists;
    const query = artistSearch.toLowerCase();
    return favoriteArtists.filter(a =>
      a.name.toLowerCase().includes(query)
    );
  });

  let filteredPlaylists = $derived.by(() => {
    if (!playlistSearch.trim()) return favoritePlaylists;
    const query = playlistSearch.toLowerCase();
    return favoritePlaylists.filter(p =>
      p.name.toLowerCase().includes(query) ||
      p.owner.name.toLowerCase().includes(query)
    );
  });

  function loadStoredBool(key: string, fallback = false): boolean {
    try {
      const value = localStorage.getItem(key);
      if (value === null) return fallback;
      return value === 'true';
    } catch {
      return fallback;
    }
  }

  function loadStoredString<T extends string>(key: string, fallback: T, options: T[]): T {
    try {
      const value = localStorage.getItem(key);
      if (value && (options as string[]).includes(value)) {
        return value as T;
      }
    } catch {
      return fallback;
    }
    return fallback;
  }

  function getCurrentSearchValue(): string {
    switch (activeTab) {
      case 'tracks': return trackSearch;
      case 'albums': return albumSearch;
      case 'artists': return artistSearch;
      case 'playlists': return playlistSearch;
      default: return '';
    }
  }

  function setCurrentSearchValue(value: string) {
    switch (activeTab) {
      case 'tracks': trackSearch = value; break;
      case 'albums': albumSearch = value; break;
      case 'artists': artistSearch = value; break;
      case 'playlists': playlistSearch = value; break;
    }
  }

  function clearCurrentSearch() {
    setCurrentSearchValue('');
    searchExpanded = false;
  }

  function getTabIcon(tab: TabType) {
    switch (tab) {
      case 'tracks': return Music;
      case 'albums': return Disc3;
      case 'artists': return Mic2;
      case 'playlists': return ListMusic;
    }
  }

  function getTabLabel(tab: TabType): string {
    return tabLabels[tab] || tab.charAt(0).toUpperCase() + tab.slice(1);
  }

  onMount(() => {
    albumViewMode = loadStoredString('qbz-favorites-album-view', 'grid', ['grid', 'list']);
    albumGroupMode = loadStoredString('qbz-favorites-album-group', 'alpha', ['alpha', 'artist']);
    trackGroupMode = loadStoredString('qbz-favorites-track-group', 'album', ['album', 'artist', 'name']);
    albumGroupingEnabled = loadStoredBool('qbz-favorites-album-group-enabled', false);
    trackGroupingEnabled = loadStoredBool('qbz-favorites-track-group-enabled', false);
    artistGroupingEnabled = loadStoredBool('qbz-favorites-artist-group-enabled', false);
    loadFavoritesPreferences().then(() => {
      preferencesLoaded = true;
      if (selectedTab) {
        activeTab = selectedTab;
      } else {
        activeTab = favoritesPreferences.tab_order[0] as TabType;
      }
      loadTabIfNeeded(activeTab);
    });
  });

  async function loadFavoritesPreferences() {
    try {
      const prefs = await invoke<FavoritesPreferences>('get_favorites_preferences');
      favoritesPreferences = {
        ...prefs,
        tab_order: normalizeFavoritesTabOrder(prefs.tab_order)
      };
    } catch (err) {
      console.error('Failed to load favorites preferences:', err);
      favoritesPreferences = {
        ...favoritesPreferences,
        tab_order: normalizeFavoritesTabOrder(favoritesPreferences.tab_order)
      };
    }
  }

  function handlePreferencesSaved(prefs: FavoritesPreferences) {
    favoritesPreferences = {
      ...prefs,
      tab_order: normalizeFavoritesTabOrder(prefs.tab_order)
    };
  }


  $effect(() => {
    try {
      localStorage.setItem('qbz-favorites-album-view', albumViewMode);
      localStorage.setItem('qbz-favorites-album-group', albumGroupMode);
      localStorage.setItem('qbz-favorites-track-group', trackGroupMode);
      localStorage.setItem('qbz-favorites-album-group-enabled', String(albumGroupingEnabled));
      localStorage.setItem('qbz-favorites-track-group-enabled', String(trackGroupingEnabled));
      localStorage.setItem('qbz-favorites-artist-group-enabled', String(artistGroupingEnabled));
    } catch {
      // localStorage not available
    }
  });

  $effect(() => {
    if (preferencesLoaded && selectedTab && selectedTab !== activeTab) {
      activeTab = selectedTab;
      loadTabIfNeeded(activeTab);
    }
  });

  $effect(() => {
    if (!preferencesLoaded || activeTab !== 'tracks' || favoriteTracks.length === 0) return;
    const targetId = consumeContextTrackFocus('favorites', 'favorites');
    if (targetId !== null) {
      void scrollToTrack(targetId);
    }
  });

  const FAVORITES_PAGE_SIZE = 200;
  const FAVORITES_MAX_PAGES = 50;

  function extractFavoritesPayload(result: any, type: TabType) {
    const payload = result?.[type];
    const items = Array.isArray(payload?.items) ? payload.items : [];
    const total = typeof payload?.total === 'number'
      ? payload.total
      : typeof payload?.count === 'number'
        ? payload.count
        : null;
    return { items, total };
  }

  async function fetchAllFavorites(type: TabType) {
    let offset = 0;
    let page = 0;
    let total: number | null = null;
    const collected: any[] = [];

    while (page < FAVORITES_MAX_PAGES) {
      const result = await invoke<any>('get_favorites', {
        favType: type,
        limit: FAVORITES_PAGE_SIZE,
        offset
      });
      const { items, total: batchTotal } = extractFavoritesPayload(result, type);
      if (!items.length) break;

      collected.push(...items);
      offset += items.length;
      total = total ?? batchTotal;

      if (total !== null && offset >= total) break;
      if (items.length < FAVORITES_PAGE_SIZE) break;

      page += 1;
    }

    return collected;
  }

  async function loadFavorites(type: TabType) {
    loading = true;
    error = null;
    try {
      const items = await fetchAllFavorites(type);

      if (type === 'tracks') {
        favoriteTracks = items as FavoriteTrack[];
      } else if (type === 'albums') {
        favoriteAlbums = items as FavoriteAlbum[];
        await loadAllAlbumDownloadStatuses(favoriteAlbums);
      } else if (type === 'artists') {
        favoriteArtists = items as FavoriteArtist[];
      }
    } catch (err) {
      console.error(`Failed to load ${type} favorites:`, err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function loadFavoritePlaylists() {
    loadingPlaylists = true;
    error = null;
    try {
      // Get IDs of favorited playlists from local DB
      const favoriteIds = await invoke<number[]>('playlist_get_favorites');
      if (favoriteIds.length === 0) {
        favoritePlaylists = [];
        return;
      }
      // Fetch full playlist data for each favorited playlist
      const playlists: FavoritePlaylist[] = [];
      for (const id of favoriteIds) {
        try {
          const playlist = await invoke<FavoritePlaylist>('get_playlist', { playlistId: id });
          playlists.push(playlist);
        } catch (err) {
          console.warn(`Failed to load playlist ${id}:`, err);
        }
      }
      favoritePlaylists = playlists;
    } catch (err) {
      console.error('Failed to load favorite playlists:', err);
      error = String(err);
    } finally {
      loadingPlaylists = false;
    }
  }

  function loadTabIfNeeded(tab: TabType) {
    if (tab === 'tracks' && favoriteTracks.length === 0) {
      loadFavorites(tab);
    } else if (tab === 'albums' && favoriteAlbums.length === 0) {
      loadFavorites(tab);
    } else if (tab === 'artists' && favoriteArtists.length === 0) {
      loadFavorites(tab);
    } else if (tab === 'playlists' && favoritePlaylists.length === 0) {
      loadFavoritePlaylists();
    }
  }

  function handleTabChange(tab: TabType) {
    activeTab = tab;
    showAlbumGroupMenu = false;
    showTrackGroupMenu = false;
    showArtistGroupMenu = false;
    onTabNavigate?.(tab);
    loadTabIfNeeded(tab);
  }

  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  function getQualityLabel(item: { hires?: boolean; maximum_bit_depth?: number; maximum_sampling_rate?: number }): string {
    if (item.hires && item.maximum_bit_depth && item.maximum_sampling_rate) {
      return `${item.maximum_bit_depth}bit/${item.maximum_sampling_rate}kHz`;
    }
    return item.hires ? 'Hi-Res' : 'CD Quality';
  }

  const alphaIndexLetters = ['#', ...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'];

  function alphaGroupKey(value: string): string {
    const trimmed = value.trim();
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

  function scrollToGroup(prefix: string, letter: string, available: Set<string>) {
    if (!available.has(letter)) return;
    const id = groupIdForKey(prefix, letter);
    const target = document.getElementById(id);
    target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  function getAlbumYear(album: FavoriteAlbum): string | null {
    if (!album.release_date_original) return null;
    return album.release_date_original.slice(0, 4);
  }

  function getAlbumQualityLabel(album: FavoriteAlbum): string {
    return album.hires ? 'Hi-Res' : 'CD Quality';
  }

  function groupAlbums(items: FavoriteAlbum[], mode: AlbumGroupMode) {
    const prefix = `album-${mode}`;
    const sorted = [...items].sort((a, b) => {
      if (mode === 'artist') {
        const artistCmp = a.artist.name.localeCompare(b.artist.name);
        if (artistCmp !== 0) return artistCmp;
      }
      return a.title.localeCompare(b.title);
    });

    const groups = new Map<string, FavoriteAlbum[]>();
    for (const album of sorted) {
      const key = mode === 'artist' ? album.artist.name : alphaGroupKey(album.title);
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

  function groupArtists(items: FavoriteArtist[]) {
    const prefix = 'artist-alpha';
    const sorted = [...items].sort((a, b) => a.name.localeCompare(b.name));
    const groups = new Map<string, FavoriteArtist[]>();
    for (const artist of sorted) {
      const key = alphaGroupKey(artist.name);
      if (!groups.has(key)) {
        groups.set(key, []);
      }
      groups.get(key)?.push(artist);
    }

    const keys = [...groups.keys()].sort((a, b) => {
      if (a === '#') return -1;
      if (b === '#') return 1;
      return a.localeCompare(b);
    });

    return keys.map(key => ({
      key,
      id: groupIdForKey(prefix, key),
      artists: groups.get(key) ?? []
    }));
  }

  function groupTracks(items: FavoriteTrack[], mode: TrackGroupMode) {
    const prefix = `track-${mode}`;
    const sorted = [...items].sort((a, b) => {
      if (mode === 'album') {
        const albumCmp = (a.album?.title || '').localeCompare(b.album?.title || '');
        if (albumCmp !== 0) return albumCmp;
        const trackCmp = (a.track_number || 0) - (b.track_number || 0);
        if (trackCmp !== 0) return trackCmp;
        return a.title.localeCompare(b.title);
      }
      if (mode === 'artist') {
        const artistCmp = (a.performer?.name || '').localeCompare(b.performer?.name || '');
        if (artistCmp !== 0) return artistCmp;
        const albumCmp = (a.album?.title || '').localeCompare(b.album?.title || '');
        if (albumCmp !== 0) return albumCmp;
        const trackCmp = (a.track_number || 0) - (b.track_number || 0);
        if (trackCmp !== 0) return trackCmp;
        return a.title.localeCompare(b.title);
      }
      const titleCmp = a.title.localeCompare(b.title);
      if (titleCmp !== 0) return titleCmp;
      return (a.performer?.name || '').localeCompare(b.performer?.name || '');
    });

    const groups = new Map<string, { title: string; subtitle?: string; tracks: FavoriteTrack[]; artists: Set<string> }>();
    for (const track of sorted) {
      if (mode === 'album') {
        const title = track.album?.title || 'Unknown Album';
        const groupKey = track.album?.id || title;
        const artistName = track.performer?.name || 'Unknown Artist';
        const entry = groups.get(groupKey);
        if (!entry) {
          groups.set(groupKey, {
            title,
            subtitle: artistName,
            tracks: [track],
            artists: new Set([artistName])
          });
        } else {
          entry.tracks.push(track);
          entry.artists.add(artistName);
        }
      } else if (mode === 'artist') {
        const key = track.performer?.name || 'Unknown Artist';
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

    return keys.map(key => {
      const entry = groups.get(key);
      if (!entry) {
        return { key, id: groupIdForKey(prefix, key), title: key, tracks: [] as FavoriteTrack[] };
      }
      let subtitle = entry.subtitle;
      if (mode === 'album') {
        const artists = [...entry.artists];
        subtitle = artists.length > 1 ? 'Various Artists' : artists[0];
      }
      return {
        key,
        id: groupIdForKey(prefix, key),
        title: entry.title,
        subtitle,
        tracks: entry.tracks
      };
    });
  }

  function buildDisplayTrack(track: FavoriteTrack, index: number): DisplayTrack {
    return {
      id: track.id,
      number: index + 1,
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
    };
  }

  function buildFavoritesQueueTracks(tracks: FavoriteTrack[]) {
    return tracks.map(t => ({
      id: t.id,
      title: t.title,
      artist: t.performer?.name || 'Unknown Artist',
      album: t.album?.title || 'Favorites',
      duration_secs: t.duration,
      artwork_url: t.album?.image?.large || t.album?.image?.thumbnail || t.album?.image?.small || '',
      hires: t.hires ?? false,
      bit_depth: t.maximum_bit_depth ?? null,
      sample_rate: t.maximum_sampling_rate ?? null,
    }));
  }

  function setFavoritesContext(trackIds: number[], index: number) {
    if (trackIds.length === 0) return;
    setPlaybackContext(
      'favorites',
      'favorites',
      'Favorites',
      'qobuz',
      trackIds,
      index
    );
  }

  async function setFavoritesQueue(startIndex: number) {
    if (filteredTracks.length === 0) return;
    const queueTracks = buildFavoritesQueueTracks(filteredTracks);
    await invoke('set_queue', { tracks: queueTracks, startIndex });
  }

  async function handleTrackClick(track: FavoriteTrack, index: number) {
    const trackIds = filteredTracks.map(t => t.id);
    setFavoritesContext(trackIds, index);

    try {
      await setFavoritesQueue(index);
    } catch (err) {
      console.error('Failed to set queue:', err);
    }

    if (onTrackPlay) {
      onTrackPlay(buildDisplayTrack(track, index));
    }
  }

  async function handlePlayAllTracks() {
    if (filteredTracks.length === 0 || !onTrackPlay) return;

    try {
      await setFavoritesQueue(0);
      setFavoritesContext(filteredTracks.map(t => t.id), 0);
      onTrackPlay(buildDisplayTrack(filteredTracks[0], 0));
    } catch (err) {
      console.error('Failed to set queue:', err);
    }
  }

</script>

<div class="favorites-view" bind:this={scrollContainer}>
  <!-- Header -->
  <div class="header">
    <div
      class="header-icon"
      style={favoritesPreferences.icon_background ? `background: ${favoritesPreferences.icon_background};` : ''}
    >
      {#if customIconSrc}
        <img
          src={customIconSrc}
          alt="Custom Icon"
          class="custom-icon-img"
        />
      {:else if favoritesPreferences.custom_icon_preset}
        {#if favoritesPreferences.custom_icon_preset === 'heart'}
          <Heart size={32} fill="var(--accent-primary)" color="var(--accent-primary)" />
        {:else if favoritesPreferences.custom_icon_preset === 'star'}
          <svelte:component this={Star} size={32} fill="var(--accent-primary)" color="var(--accent-primary)" />
        {:else if favoritesPreferences.custom_icon_preset === 'music'}
          <Music size={32} color="var(--accent-primary)" />
        {:else if favoritesPreferences.custom_icon_preset === 'folder'}
          <svelte:component this={Folder} size={32} color="var(--accent-primary)" />
        {:else if favoritesPreferences.custom_icon_preset === 'disc'}
          <Disc3 size={32} color="var(--accent-primary)" />
        {:else if favoritesPreferences.custom_icon_preset === 'library'}
          <svelte:component this={Library} size={32} color="var(--accent-primary)" />
        {/if}
      {:else}
        <Heart size={32} fill="var(--accent-primary)" color="var(--accent-primary)" />
      {/if}
    </div>
    <div class="header-content">
      <h1>Favorites</h1>
      <p class="subtitle">Your liked tracks, albums, and artists</p>
    </div>
    <button class="edit-btn" onclick={() => editModalOpen = true} title="Edit Favorites settings">
      <Edit3 size={16} />
    </button>
  </div>

  <!-- Navigation Bar (Artist-style) -->
  <div class="favorites-nav">
    <div class="nav-left">
      {#each favoritesPreferences.tab_order as tab}
        <button
          class="nav-link"
          class:active={activeTab === tab}
          onclick={() => handleTabChange(tab as TabType)}
        >
          <svelte:component this={getTabIcon(tab as TabType)} size={16} />
          <span>{getTabLabel(tab as TabType)}</span>
        </button>
      {/each}
    </div>
    <div class="nav-right">
      {#if !searchExpanded}
        <button class="search-icon-btn" onclick={() => searchExpanded = true} title="Search">
          <Search size={16} />
        </button>
      {:else}
        <div class="search-expanded">
          <Search size={16} class="search-icon-inline" />
          <input
            type="text"
            placeholder={`Search ${getTabLabel(activeTab).toLowerCase()}...`}
            value={getCurrentSearchValue()}
            oninput={(e) => setCurrentSearchValue(e.currentTarget.value)}
            class="search-input-inline"
            autofocus
          />
          {#if getCurrentSearchValue()}
            <button class="search-clear-btn" onclick={clearCurrentSearch} title="Clear">
              <X size={14} />
            </button>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <!-- Toolbar with actions -->
  <div class="toolbar">
    {#if activeTab === 'albums'}
      <div class="toolbar-controls">
        <div class="dropdown-container">
          <button class="control-btn" onclick={() => (showAlbumGroupMenu = !showAlbumGroupMenu)}>
            <span>
              {albumGroupingEnabled
                ? albumGroupMode === 'alpha'
                  ? 'Group: A-Z'
                  : 'Group: Artist'
                : 'Group: Off'}
            </span>
            <ChevronDown size={14} />
          </button>
          {#if showAlbumGroupMenu}
            <div class="dropdown-menu">
              <button
                class="dropdown-item"
                class:selected={!albumGroupingEnabled}
                onclick={() => { albumGroupingEnabled = false; showAlbumGroupMenu = false; }}
              >
                Off
              </button>
              <button
                class="dropdown-item"
                class:selected={albumGroupingEnabled && albumGroupMode === 'alpha'}
                onclick={() => { albumGroupMode = 'alpha'; albumGroupingEnabled = true; showAlbumGroupMenu = false; }}
              >
                Alphabetical (A-Z)
              </button>
              <button
                class="dropdown-item"
                class:selected={albumGroupingEnabled && albumGroupMode === 'artist'}
                onclick={() => { albumGroupMode = 'artist'; albumGroupingEnabled = true; showAlbumGroupMenu = false; }}
              >
                Artist
              </button>
            </div>
          {/if}
        </div>
        <button
          class="icon-btn"
          onclick={() => (albumViewMode = albumViewMode === 'grid' ? 'list' : 'grid')}
          title={albumViewMode === 'grid' ? 'List view' : 'Grid view'}
        >
          {#if albumViewMode === 'grid'}
            <List size={16} />
          {:else}
            <LayoutGrid size={16} />
          {/if}
        </button>
      </div>
    {:else if activeTab === 'tracks'}
      <div class="toolbar-controls">
        <div class="dropdown-container">
          <button class="control-btn" onclick={() => (showTrackGroupMenu = !showTrackGroupMenu)}>
            <span>
              {trackGroupingEnabled
                ? trackGroupMode === 'album'
                  ? 'Group: Album'
                  : trackGroupMode === 'artist'
                    ? 'Group: Artist'
                    : 'Group: Name'
                : 'Group: Off'}
            </span>
            <ChevronDown size={14} />
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
      </div>
    {:else if activeTab === 'artists'}
      <div class="toolbar-controls">
        <div class="dropdown-container">
          <button class="control-btn" onclick={() => (showArtistGroupMenu = !showArtistGroupMenu)}>
            <span>{artistGroupingEnabled ? 'Group: A-Z' : 'Group: Off'}</span>
            <ChevronDown size={14} />
          </button>
          {#if showArtistGroupMenu}
            <div class="dropdown-menu">
              <button
                class="dropdown-item"
                class:selected={!artistGroupingEnabled}
                onclick={() => { artistGroupingEnabled = false; showArtistGroupMenu = false; }}
              >
                Off
              </button>
              <button
                class="dropdown-item"
                class:selected={artistGroupingEnabled}
                onclick={() => { artistGroupingEnabled = true; showArtistGroupMenu = false; }}
              >
                Alphabetical (A-Z)
              </button>
            </div>
          {/if}
        </div>
      </div>
    {/if}

    <!-- Actions (for tracks tab) -->
    {#if activeTab === 'tracks' && filteredTracks.length > 0}
      <div class="actions">
        <button class="play-btn" onclick={handlePlayAllTracks}>
          <Play size={16} fill="white" />
          <span>Play All</span>
        </button>
      </div>
    {/if}

    <!-- Results count -->
    <span class="results-count">
      {#if activeTab === 'tracks'}
        {filteredTracks.length}{trackSearch ? ` / ${favoriteTracks.length}` : ''} tracks
      {:else if activeTab === 'albums'}
        {filteredAlbums.length}{albumSearch ? ` / ${favoriteAlbums.length}` : ''} albums
      {:else if activeTab === 'artists'}
        {filteredArtists.length}{artistSearch ? ` / ${favoriteArtists.length}` : ''} artists
      {:else}
        {filteredPlaylists.length}{playlistSearch ? ` / ${favoritePlaylists.length}` : ''} playlists
      {/if}
    </span>
  </div>

  <!-- Content -->
  <div class="content">
    {#if loading}
      <div class="loading">
        <div class="spinner"></div>
        <p>Loading favorites...</p>
      </div>
    {:else if error}
      <div class="error">
        <p>Failed to load favorites</p>
        <p class="error-detail">{error}</p>
        <button class="retry-btn" onclick={() => loadTabIfNeeded(activeTab)}>Retry</button>
      </div>
    {:else if activeTab === 'tracks'}
      {#if favoriteTracks.length === 0}
        <div class="empty">
          <Music size={48} />
          <p>No favorite tracks yet</p>
          <p class="empty-hint">Like tracks to see them here</p>
        </div>
      {:else if filteredTracks.length === 0}
        <div class="empty">
          <Search size={48} />
          <p>No tracks match "{trackSearch}"</p>
        </div>
      {:else if trackGroupingEnabled}
        {@const groupedTracks = groupTracks(filteredTracks, trackGroupMode)}
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
                  {#each group.tracks as track, index (`${track.id}-${downloadStateVersion}`)}
                    {@const globalIndex = trackIndexMap.get(track.id) ?? index}
                    {@const displayTrack = buildDisplayTrack(track, globalIndex)}
                    {@const downloadInfo = getTrackDownloadStatus?.(track.id) ?? { status: 'none' as const, progress: 0 }}
                    {@const isActiveTrack = isPlaybackActive && activeTrackId === track.id}
                    <TrackRow
                      trackId={track.id}
                      number={track.track_number || index + 1}
                      title={track.title}
                      artist={track.performer?.name}
                      duration={formatDuration(track.duration)}
                      quality={getQualityLabel(track)}
                      isPlaying={isActiveTrack}
                      isFavoriteOverride={true}
                      downloadStatus={downloadInfo.status}
                      downloadProgress={downloadInfo.progress}
                      onPlay={() => handleTrackClick(track, globalIndex)}
                      onDownload={onTrackDownload ? () => onTrackDownload(displayTrack) : undefined}
                      onRemoveDownload={onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
                      menuActions={{
                        onPlayNow: () => handleTrackClick(track, globalIndex),
                        onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(displayTrack) : undefined,
                        onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(displayTrack) : undefined,
                        onAddToPlaylist: onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined,
                        onShareQobuz: onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
                        onShareSonglink: onTrackShareSonglink ? () => onTrackShareSonglink(displayTrack) : undefined,
                        onGoToAlbum: track.album?.id && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.album!.id) : undefined,
                        onGoToArtist: track.performer?.id && onTrackGoToArtist ? () => onTrackGoToArtist(track.performer!.id!) : undefined
                      }}
                    />
                  {/each}
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
                  onclick={() => scrollToGroup(trackGroupMode === 'name' ? 'track-name' : 'track-artist', letter, trackAlphaGroups)}
                >
                  {letter}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <div class="track-list">
          {#each filteredTracks as track, index (`${track.id}-${downloadStateVersion}`)}
            {@const displayTrack = buildDisplayTrack(track, index)}
            {@const downloadInfo = getTrackDownloadStatus?.(track.id) ?? { status: 'none' as const, progress: 0 }}
            {@const isActiveTrack = isPlaybackActive && activeTrackId === track.id}
            <TrackRow
              trackId={track.id}
              number={index + 1}
              title={track.title}
              artist={track.performer?.name}
              duration={formatDuration(track.duration)}
              quality={getQualityLabel(track)}
              isPlaying={isActiveTrack}
              isFavoriteOverride={true}
              downloadStatus={downloadInfo.status}
              downloadProgress={downloadInfo.progress}
              onPlay={() => handleTrackClick(track, index)}
              onDownload={onTrackDownload ? () => onTrackDownload(displayTrack) : undefined}
              onRemoveDownload={onTrackRemoveDownload ? () => onTrackRemoveDownload(track.id) : undefined}
              menuActions={{
                onPlayNow: () => handleTrackClick(track, index),
                onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(displayTrack) : undefined,
                onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(displayTrack) : undefined,
                onAddToPlaylist: onTrackAddToPlaylist ? () => onTrackAddToPlaylist(track.id) : undefined,
                onShareQobuz: onTrackShareQobuz ? () => onTrackShareQobuz(track.id) : undefined,
                onShareSonglink: onTrackShareSonglink ? () => onTrackShareSonglink(displayTrack) : undefined,
                onGoToAlbum: track.album?.id && onTrackGoToAlbum ? () => onTrackGoToAlbum(track.album!.id) : undefined,
                onGoToArtist: track.performer?.id && onTrackGoToArtist ? () => onTrackGoToArtist(track.performer!.id!) : undefined
              }}
            />
          {/each}
        </div>
      {/if}
    {:else if activeTab === 'albums'}
      {#if favoriteAlbums.length === 0}
        <div class="empty">
          <Disc3 size={48} />
          <p>No favorite albums yet</p>
          <p class="empty-hint">Like albums to see them here</p>
        </div>
      {:else if filteredAlbums.length === 0}
        <div class="empty">
          <Search size={48} />
          <p>No albums match "{albumSearch}"</p>
        </div>
      {:else if albumGroupingEnabled}
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
                        albumId={album.id}
                        artwork={album.image?.large || album.image?.thumbnail || ''}
                        title={album.title}
                        artist={album.artist.name}
                        quality={getQualityLabel(album)}
                        onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
                        onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
                        onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
                        onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
                        onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
                        onDownload={onAlbumDownload ? () => onAlbumDownload(album.id) : undefined}
                        isAlbumFullyDownloaded={isAlbumDownloaded(album.id)}
                        onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(album.id) : undefined}
                        onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(album.id) : undefined}
                        {downloadStateVersion}
                        onclick={() => { onAlbumClick?.(album.id); loadAlbumDownloadStatus(album.id); }}
                      />
                    {/each}
                  </div>
                {:else}
                  <div class="album-list">
                    {#each group.albums as album (album.id)}
                      <div class="album-row" role="button" tabindex="0" onclick={() => onAlbumClick?.(album.id)}>
                        <div class="album-row-art">
                          {#if album.image?.thumbnail || album.image?.small || album.image?.large}
                            <img src={album.image?.thumbnail || album.image?.small || album.image?.large} alt={album.title} loading="lazy" decoding="async" />
                          {:else}
                            <div class="artwork-placeholder">
                              <Disc3 size={28} />
                            </div>
                          {/if}
                        </div>
                        <div class="album-row-info">
                          <div class="album-row-title truncate">{album.title}</div>
                          <div class="album-row-meta">
                            <span>{album.artist.name}</span>
                            {#if getAlbumYear(album)}<span>{getAlbumYear(album)}</span>{/if}
                          </div>
                        </div>
                        <div class="album-row-quality">
                          <span class="quality-badge" class:hires={album.hires}>
                            {getAlbumQualityLabel(album)}
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
      {:else}
        {#if albumViewMode === 'grid'}
          <div class="album-grid">
            {#each filteredAlbums as album (album.id)}
              <AlbumCard
                albumId={album.id}
                artwork={album.image?.large || album.image?.thumbnail || ''}
                title={album.title}
                artist={album.artist.name}
                quality={getQualityLabel(album)}
                onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
                onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
                onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
                onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
                onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
                onDownload={onAlbumDownload ? () => onAlbumDownload(album.id) : undefined}
                isAlbumFullyDownloaded={isAlbumDownloaded(album.id)}
                onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(album.id) : undefined}
                onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(album.id) : undefined}
                        {downloadStateVersion}
                onclick={() => { onAlbumClick?.(album.id); loadAlbumDownloadStatus(album.id); }}
              />
            {/each}
          </div>
        {:else}
          <div class="album-list">
            {#each filteredAlbums as album (album.id)}
              <div class="album-row" role="button" tabindex="0" onclick={() => onAlbumClick?.(album.id)}>
                <div class="album-row-art">
                  {#if album.image?.thumbnail || album.image?.small || album.image?.large}
                    <img src={album.image?.thumbnail || album.image?.small || album.image?.large} alt={album.title} loading="lazy" decoding="async" />
                  {:else}
                    <div class="artwork-placeholder">
                      <Disc3 size={28} />
                    </div>
                  {/if}
                </div>
                <div class="album-row-info">
                  <div class="album-row-title truncate">{album.title}</div>
                  <div class="album-row-meta">
                    <span>{album.artist.name}</span>
                    {#if getAlbumYear(album)}<span>{getAlbumYear(album)}</span>{/if}
                  </div>
                </div>
                <div class="album-row-quality">
                  <span class="quality-badge" class:hires={album.hires}>
                    {getAlbumQualityLabel(album)}
                  </span>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    {:else if activeTab === 'artists'}
      {#if favoriteArtists.length === 0}
        <div class="empty">
          <Mic2 size={48} />
          <p>No favorite artists yet</p>
          <p class="empty-hint">Like artists to see them here</p>
        </div>
      {:else if filteredArtists.length === 0}
        <div class="empty">
          <Search size={48} />
          <p>No artists match "{artistSearch}"</p>
        </div>
      {:else if artistGroupingEnabled}
        {@const groupedArtists = groupArtists(filteredArtists)}
        {@const artistAlphaGroups = new Set(groupedArtists.map(group => group.key))}

        <div class="artist-sections">
          <div class="artist-group-list">
            {#each groupedArtists as group (group.id)}
              <div class="artist-group" id={group.id}>
                <div class="artist-group-header">
                  <span class="artist-group-title">{group.key}</span>
                  <span class="artist-group-count">{group.artists.length}</span>
                </div>
                <div class="artist-grid">
                  {#each group.artists as artist (artist.id)}
                    <button class="artist-card" onclick={() => onArtistClick?.(artist.id)}>
                      <div class="artist-image">
                        {#if artist.image?.large || artist.image?.thumbnail}
                          <img src={artist.image?.large || artist.image?.thumbnail} alt={artist.name} />
                        {:else}
                          <div class="artist-placeholder">
                            <Mic2 size={32} />
                          </div>
                        {/if}
                      </div>
                      <div class="artist-name">{artist.name}</div>
                      {#if artist.albums_count}
                        <div class="artist-albums">{artist.albums_count} albums</div>
                      {/if}
                    </button>
                  {/each}
                </div>
              </div>
            {/each}
          </div>

          <div class="alpha-index">
            {#each alphaIndexLetters as letter}
              <button
                class="alpha-letter"
                class:disabled={!artistAlphaGroups.has(letter)}
                onclick={() => scrollToGroup('artist-alpha', letter, artistAlphaGroups)}
              >
                {letter}
              </button>
            {/each}
          </div>
        </div>
      {:else}
        <div class="artist-grid">
          {#each filteredArtists as artist (artist.id)}
            <button class="artist-card" onclick={() => onArtistClick?.(artist.id)}>
              <div class="artist-image">
                {#if artist.image?.large || artist.image?.thumbnail}
                  <img src={artist.image?.large || artist.image?.thumbnail} alt={artist.name} />
                {:else}
                  <div class="artist-placeholder">
                    <Mic2 size={32} />
                  </div>
                {/if}
              </div>
              <div class="artist-name">{artist.name}</div>
              {#if artist.albums_count}
                <div class="artist-albums">{artist.albums_count} albums</div>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    {:else if activeTab === 'playlists'}
      {#if loadingPlaylists}
        <div class="loading">
          <div class="spinner"></div>
          <p>Loading playlists...</p>
        </div>
      {:else if favoritePlaylists.length === 0}
        <div class="empty">
          <ListMusic size={48} />
          <p>No favorite playlists yet</p>
          <p class="empty-hint">Click the heart icon on playlists to add them here</p>
        </div>
      {:else if filteredPlaylists.length === 0}
        <div class="empty">
          <Search size={48} />
          <p>No playlists match "{playlistSearch}"</p>
        </div>
      {:else}
        <div class="playlist-grid">
          {#each filteredPlaylists as playlist (playlist.id)}
            <button class="playlist-card" onclick={() => onPlaylistSelect?.(playlist.id)}>
              <div class="playlist-artwork">
                <PlaylistCollage artworks={playlist.images ?? []} size={140} />
              </div>
              <div class="playlist-name">{playlist.name}</div>
              <div class="playlist-meta">{playlist.tracks_count} tracks</div>
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>

<FavoritesEditModal
  isOpen={editModalOpen}
  onClose={() => editModalOpen = false}
  onSave={handlePreferencesSaved}
  initialPreferences={favoritesPreferences}
/>

<style>
  .favorites-view {
    padding: 24px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }

  /* Custom scrollbar */
  .favorites-view::-webkit-scrollbar {
    width: 6px;
  }

  .favorites-view::-webkit-scrollbar-track {
    background: transparent;
  }

  .favorites-view::-webkit-scrollbar-thumb {
    background: var(--bg-tertiary);
    border-radius: 3px;
  }

  .favorites-view::-webkit-scrollbar-thumb:hover {
    background: var(--text-muted);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 32px;
    position: relative;
  }

  .header-icon {
    width: 80px;
    height: 80px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--accent-primary) 0%, #ff6b9d 100%);
    border-radius: 16px;
    overflow: hidden;
  }

  .custom-icon-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
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

  .edit-btn {
    position: absolute;
    top: 0;
    right: 0;
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .edit-btn:hover {
    color: var(--accent-primary);
  }

  .favorites-nav {
    position: sticky;
    top: -24px;
    z-index: 4;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 12px 24px;
    margin: 0 -32px 16px -32px;
    background-color: var(--bg-primary);
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .nav-left {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 20px;
  }

  .nav-link {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 0;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: color 150ms ease, border-color 150ms ease;
  }

  .nav-link:hover {
    color: var(--text-secondary);
  }

  .nav-link.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-primary);
  }

  .nav-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .search-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    transition: all 150ms ease;
  }

  .search-icon-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .search-expanded {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    min-width: 240px;
  }

  .search-icon-inline {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .search-input-inline {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 13px;
  }

  .search-input-inline::placeholder {
    color: var(--text-muted);
  }

  .search-clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .search-clear-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .search-clear-btn:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-bottom: 24px;
  }

  .toolbar-controls {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .dropdown-container {
    position: relative;
  }

  .control-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 12px;
    cursor: pointer;
  }

  .control-btn:hover {
    color: var(--text-primary);
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: 8px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    cursor: pointer;
  }

  .icon-btn:hover {
    color: var(--text-primary);
  }

  .dropdown-menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    min-width: 170px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    z-index: 10;
  }

  .dropdown-item {
    width: 100%;
    text-align: left;
    padding: 8px 10px;
    background: none;
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    font-size: 12px;
  }

  .dropdown-item:hover,
  .dropdown-item.selected {
    background: var(--bg-tertiary);
    color: var(--text-primary);
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

  .results-count {
    margin-left: auto;
    font-size: 13px;
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    gap: 12px;
  }

  .play-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 24px;
    background-color: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .play-btn:hover {
    background-color: var(--accent-hover);
  }


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

  .empty-hint {
    font-size: 13px;
    margin-top: 8px;
  }

  .track-list {
    display: flex;
    flex-direction: column;
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
    justify-content: space-between;
    gap: 12px;
  }

  .track-group-title {
    font-size: 14px;
    font-weight: 600;
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

  .album-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .album-row {
    display: grid;
    grid-template-columns: 56px 1fr auto;
    gap: 12px;
    align-items: center;
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

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    color: var(--text-muted);
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

  .artist-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 24px;
  }

  .artist-sections {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .artist-group-list {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .artist-group-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    color: var(--text-muted);
    font-size: 12px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    margin-bottom: 8px;
  }

  .artist-group-title {
    font-weight: 600;
  }

  .artist-group-count {
    font-size: 12px;
  }

  .artist-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 16px;
    background-color: var(--bg-secondary);
    border: none;
    border-radius: 12px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .artist-card:hover {
    background-color: var(--bg-tertiary);
  }

  .artist-image {
    width: 120px;
    height: 120px;
    border-radius: 50%;
    overflow: hidden;
    margin-bottom: 12px;
    background-color: var(--bg-tertiary);
  }

  .artist-image img {
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
    color: var(--text-muted);
  }

  .artist-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
  }

  .artist-albums {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
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

  /* Playlist grid styles */
  .playlist-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 24px;
  }

  .playlist-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 16px;
    background-color: var(--bg-secondary);
    border: none;
    border-radius: 12px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .playlist-card:hover {
    background-color: var(--bg-tertiary);
  }

  .playlist-artwork {
    width: 140px;
    height: 140px;
    border-radius: 8px;
    overflow: hidden;
    margin-bottom: 12px;
  }

  .playlist-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playlist-meta {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 4px;
  }
</style>
