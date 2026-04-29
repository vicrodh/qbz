<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import {
    Play, Shuffle, MoreHorizontal, ArrowLeft, Disc, Music2, ListMusic, Trash2,
    Check, ChevronRight, ChevronDown, RotateCcw, LoaderCircle, ArrowUpDown, Filter,
    SquareCheckBig, List, LayoutGrid, AlignJustify, Search, X
  } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    getCollection,
    enqueueCollection,
    removeItem as removeCollectionItem,
    renameCollection,
    setDescription,
    setPlayMode,
    setKind,
    deleteCollection,
    uploadCustomCover,
    removeCustomCover,
    type MixtapeCollection,
    type MixtapeCollectionItem,
    type CollectionKind,
    type CollectionPlayMode,
    type ItemType,
  } from '$lib/stores/mixtapeCollectionsStore';
  import CollectionMosaic from '../CollectionMosaic.svelte';
  import QualityBadgeStatic from '../QualityBadgeStatic.svelte';
  import BulkActionBar from '../BulkActionBar.svelte';
  import TrackRow from '../TrackRow.svelte';
  import TrackMixModal from '../TrackMixModal.svelte';
  import DjIcon from '$lib/icons/DjIcon.svelte';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { preloadImages } from '$lib/services/imageCacheService';
  import { showToast } from '$lib/stores/toastStore';
  import { getUserItem, setUserItem, removeUserItem } from '$lib/utils/userStorage';
  import { applyShiftRange, isSelectAllShortcut } from '$lib/utils/multiSelect';
  import { openAddToMixtape } from '$lib/stores/addToMixtapeModalStore';
  import { playTrack } from '$lib/services/playbackService';
  import { playQueueIndex } from '$lib/stores/queueStore';
  import {
    releaseTypeOverrides,
    loadReleaseTypeOverrides,
    setReleaseTypeOverride,
    clearReleaseTypeOverride,
    hasReleaseTypeOverride,
    RELEASE_TYPE_CHOICES,
    type ReleaseType,
  } from '$lib/stores/releaseTypeOverridesStore';

  interface Props {
    collectionId: string;
    onBack?: () => void;
    /** Navigate to an item's detail page (album / track-album / playlist). */
    onOpenItem?: (
      source: 'qobuz' | 'local',
      itemType: ItemType,
      sourceItemId: string,
    ) => void;
    /** Navigate to the artist page by name (runtime-resolved). */
    onOpenArtist?: (source: 'qobuz' | 'local', artistName: string) => void;
    /** Play / queue-next / queue-later a single collection item. */
    onPlayItem?: (item: MixtapeCollectionItem) => void;
    onPlayItemNext?: (item: MixtapeCollectionItem) => void;
    onAddItemToQueueLater?: (item: MixtapeCollectionItem) => void;
    /** Bulk actions for multi-selected items. */
    onBulkPlayNext?: (items: MixtapeCollectionItem[]) => void;
    onBulkPlayLater?: (items: MixtapeCollectionItem[]) => void;
    onBulkAddToPlaylist?: (items: MixtapeCollectionItem[]) => void;
    /** Play a specific track inside an expanded album, starting playback
     *  there (with the rest of the album queued behind it). */
    onPlayTrackFromItem?: (item: MixtapeCollectionItem, trackId: number) => void;
    /** Queue-next / queue-later a single Qobuz track id. */
    onPlayTrackNext?: (trackId: number) => void;
    onPlayTrackLater?: (trackId: number) => void;
  }
  let {
    collectionId,
    onBack,
    onOpenItem,
    onOpenArtist,
    onPlayItem,
    onPlayItemNext,
    onAddItemToQueueLater,
    onBulkPlayNext,
    onBulkPlayLater,
    onBulkAddToPlaylist,
    onPlayTrackFromItem,
    onPlayTrackNext,
    onPlayTrackLater,
  }: Props = $props();

  // Selection state — only active while selectMode is on. Toggle lives in
  // the hero actions (SquareCheckBig button), same pattern as
  // ArtistDetailView's popular-tracks multi-select.
  let selectMode = $state(false);
  let selectedPositions = $state<Set<number>>(new Set());
  let lastSelectedPosition = $state<number | null>(null);
  const hasSelection = $derived(selectedPositions.size > 0);

  function toggleSelectMode() {
    selectMode = !selectMode;
    if (!selectMode) {
      selectedPositions = new Set();
      lastSelectedPosition = null;
    }
  }

  $effect(() => {
    if (!selectMode) return;
    const handler = (e: KeyboardEvent) => {
      if (!isSelectAllShortcut(e)) return;
      e.preventDefault();
      if (!collection) return;
      selectedPositions = new Set(collection.items.map((it) => it.position));
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  });

  // ── View mode ─────────────────────────────────────────────────────────────
  // 'list'     — default album-row listing (no chevron, no tracks inline)
  // 'grid'     — each item rendered as an AlbumCard-like tile
  // 'expanded' — same as list, but every album/playlist has its tracks
  //              rendered directly underneath (no chevron — tracks are
  //              always visible, just like LocalLibrary's Tracks page
  //              with "Group by album" active)
  type ViewMode = 'list' | 'grid' | 'expanded';
  let viewMode = $state<ViewMode>('list');

  // ── Inline expand — show tracks under album/playlist items ────────────────
  // Expanded state keyed by `${source}|${source_item_id}`. Tracks are lazy-
  // fetched on first expand and cached so subsequent toggles are instant.
  interface ExpandedTrack {
    id: number;
    number: number;
    title: string;
    artist?: string;
    duration: number; // seconds (for internal math)
    durationStr: string; // formatted "m:ss" for TrackRow
    quality?: string; // e.g. "FLAC 24/96" — shown in TrackRow quality column
    parental_warning?: boolean;
    isLocal?: boolean;
    localSource?: 'local' | 'plex';
  }

  let expandedTracks = $state<Record<string, ExpandedTrack[]>>({});
  let expandLoading = $state<Set<string>>(new Set());

  function itemKey(item: MixtapeCollectionItem): string {
    return `${item.source}|${item.source_item_id}`;
  }

  function canExpand(item: MixtapeCollectionItem): boolean {
    // Only album + playlist rows have tracks to show. Single-track items
    // have nothing useful to expand.
    return item.item_type === 'album' || item.item_type === 'playlist';
  }

  async function ensureTracksLoaded(item: MixtapeCollectionItem) {
    if (!canExpand(item)) return;
    const key = itemKey(item);
    if (expandedTracks[key] || expandLoading.has(key)) return;

    const loading = new Set(expandLoading);
    loading.add(key);
    expandLoading = loading;

    try {
      const tracks = await fetchTracksForItem(item);
      console.log(
        '[MixtapeCollectionDetailView] fetched tracks for',
        key,
        'item_type=', item.item_type,
        'source=', item.source,
        'source_item_id=', item.source_item_id,
        '→', tracks.length, 'tracks',
      );
      expandedTracks = { ...expandedTracks, [key]: tracks };
    } catch (err) {
      console.error(
        '[MixtapeCollectionDetailView] fetch tracks failed for',
        key,
        '(item_type=', item.item_type, 'source=', item.source,
        'source_item_id=', item.source_item_id, ')',
        err,
      );
      // Empty array so the spinner clears and the "No results" state shows.
      expandedTracks = { ...expandedTracks, [key]: [] };
    } finally {
      const done = new Set(expandLoading);
      done.delete(key);
      expandLoading = done;
    }
  }

  // Expanded mode: only fetch tracks for items currently in the virtual
  // window (or that will be shortly — WINDOW_BUFFER rows above/below). Large
  // collections used to trigger 60+ concurrent backend calls on mode switch
  // and mount 600+ TrackRow components before the user could even scroll.
  // Already-fetched items are skipped; the cache persists so re-scrolling
  // back to a row is instant.
  $effect(() => {
    if (viewMode !== 'expanded' || !collection) return;
    const { firstIdx, lastIdx } = virtualWindow;
    const slice = visibleItems.slice(firstIdx, lastIdx);
    for (const it of slice) {
      if (canExpand(it)) {
        void ensureTracksLoaded(it);
      }
    }
  });

  async function fetchTracksForItem(item: MixtapeCollectionItem): Promise<ExpandedTrack[]> {
    if (item.item_type === 'album' && item.source === 'qobuz') {
      interface RawTrack {
        id: number;
        track_number?: number;
        title: string;
        duration?: number;
        performer?: { name?: string };
        parental_warning?: boolean;
      }
      const album = await invoke<{
        tracks?: { items?: RawTrack[] } | RawTrack[];
        maximum_bit_depth?: number;
        maximum_sampling_rate?: number;
      }>('v2_get_album', { albumId: item.source_item_id });

      const raw = Array.isArray(album.tracks)
        ? album.tracks
        : album.tracks?.items ?? [];

      const qualityStr = album.maximum_bit_depth && album.maximum_sampling_rate
        ? `FLAC ${album.maximum_bit_depth}/${album.maximum_sampling_rate}`
        : undefined;

      return raw.map((t, i) => ({
        id: t.id,
        number: t.track_number ?? i + 1,
        title: t.title,
        artist: t.performer?.name,
        duration: t.duration ?? 0,
        durationStr: formatSec(t.duration ?? 0),
        quality: qualityStr,
        parental_warning: t.parental_warning,
      }));
    }

    // Local library album (includes plex-synced rows — source='plex' on
    // the local_tracks side). Uses the same v2_library_get_album_tracks
    // LocalLibrary uses when opening an album.
    if (item.item_type === 'album' && item.source === 'local') {
      // Plex items store their album_key as source_item_id (e.g. "plex:123…").
      // These live in the Plex cache DB, not local_tracks, so route to the
      // Plex-specific command. Same detection LocalLibraryView.fetchAlbumTracks
      // uses (album.source === 'plex'), here by id prefix and resolvedFor.
      const kind = resolvedFor(item).kind;
      const isPlexItem = kind === 'plex' || item.source_item_id.startsWith('plex:');
      if (isPlexItem) {
        interface RawPlexTrack {
          id: number;
          ratingKey: string;
          title: string;
          artist: string;
          durationSecs: number;
          format: string;
          bitDepth?: number;
          sampleRate: number;
          trackNumber?: number;
        }
        const tracks = await invoke<RawPlexTrack[]>('v2_plex_cache_get_album_tracks', {
          albumKey: item.source_item_id,
        });
        return tracks.map((t, i) => {
          const secs = Number(t.durationSecs) || 0;
          const fmt = (t.format ?? '').toUpperCase() || 'FLAC';
          const bd = t.bitDepth && t.bitDepth > 0 ? String(t.bitDepth) : '--';
          const sr = t.sampleRate && t.sampleRate > 0
            ? Number((t.sampleRate / 1000).toFixed(1)).toString()
            : '--';
          return {
            id: t.id,
            number: t.trackNumber ?? i + 1,
            title: t.title,
            artist: t.artist,
            duration: secs,
            durationStr: formatSec(secs),
            quality: `${fmt} ${bd}/${sr}`,
            parental_warning: false,
            isLocal: true,
            localSource: 'plex' as const,
          };
        });
      }

      interface RawLocalTrack {
        id: number;
        track_number?: number;
        title: string;
        artist: string;
        duration_secs: number;
        format: string;
        bit_depth?: number;
        sample_rate: number;
      }
      interface RawLocalTrackExtended extends RawLocalTrack {
        source?: string;
      }
      let tracks = await invoke<RawLocalTrackExtended[]>('v2_library_get_album_tracks', {
        albumGroupKey: item.source_item_id,
      });

      // Fallback: if the stored group_key no longer matches any tracks (e.g.
      // the library was re-indexed and the group_key composition changed
      // after this item was added to the collection), look the album up by
      // title + artist in the current albums list and retry.
      if (!tracks || tracks.length === 0) {
        try {
          interface RawLocalAlbum {
            id: string;
            title: string;
            artist: string;
            all_artists?: string;
          }
          const albums = await invoke<RawLocalAlbum[]>('v2_library_get_albums', {
            includeHidden: false,
            excludeNetworkFolders: false,
          });
          const needleTitle = item.title.toLowerCase().trim();
          const needleArtist = (item.subtitle ?? '').toLowerCase().trim();
          const match = albums.find((a) => {
            const t = a.title.toLowerCase().trim();
            const ar = a.artist.toLowerCase().trim();
            const allAr = (a.all_artists ?? '').toLowerCase();
            if (t !== needleTitle) return false;
            if (!needleArtist) return true;
            return ar === needleArtist || allAr.includes(needleArtist);
          });
          if (match && match.id !== item.source_item_id) {
            console.log(
              '[MixtapeCollectionDetailView] local album group_key re-resolved',
              item.source_item_id, '→', match.id,
            );
            tracks = await invoke<RawLocalTrackExtended[]>(
              'v2_library_get_album_tracks',
              { albumGroupKey: match.id },
            );
          }
        } catch (err) {
          console.warn(
            '[MixtapeCollectionDetailView] group_key fallback failed:',
            err,
          );
        }
      }
      return tracks.map((t, i) => {
        const secs = Number(t.duration_secs) || 0;
        const fmt = (t.format ?? '').toUpperCase() || 'FLAC';
        const bd = t.bit_depth && t.bit_depth > 0 ? String(t.bit_depth) : '--';
        const sr = t.sample_rate && t.sample_rate > 0
          ? Number((t.sample_rate / 1000).toFixed(1)).toString()
          : '--';
        return {
          id: t.id,
          number: t.track_number ?? i + 1,
          title: t.title,
          artist: t.artist,
          duration: secs,
          durationStr: formatSec(secs),
          quality: `${fmt} ${bd}/${sr}`,
          parental_warning: false,
          isLocal: true,
          localSource: t.source === 'plex' ? 'plex' as const : 'local' as const,
        };
      });
    }

    // Plex cache / playlist paths still follow-ups.
    return [];
  }

  function formatSec(seconds: number): string {
    if (!seconds || seconds < 0) return '--:--';
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60).toString().padStart(2, '0');
    return `${m}:${s}`;
  }

  // ── Sort + filter state ───────────────────────────────────────────────────
  type SortBy = 'position' | 'name' | 'year' | 'tracks';
  type SortDir = 'asc' | 'desc';
  type TypeFilter = 'all' | 'album' | 'track' | 'playlist';

  let sortBy = $state<SortBy>('position');
  let sortDir = $state<SortDir>('asc');
  let showSortMenu = $state(false);
  let typeFilter = $state<TypeFilter>('all');
  let showFilterMenu = $state(false);

  // Source filter — multi-select, maps to the resolved item kind (qobuz /
  // plex / local, see resolveItems). Empty set means "all pass" (no filter).
  let sourceFilter = $state<Set<SourceKind>>(new Set());
  function toggleSourceFilter(kind: SourceKind) {
    const next = new Set(sourceFilter);
    if (next.has(kind)) next.delete(kind);
    else next.add(kind);
    sourceFilter = next;
  }
  const hasAnyFilter = $derived(
    typeFilter !== 'all' ||
    sourceFilter.size > 0 ||
    sortBy !== 'position' ||
    sortDir !== 'asc',
  );

  // Search query — matches against item title / subtitle always, and against
  // track titles when tracks are loaded (expanded view or cached from a
  // previous expand). An album passes if the album itself matches OR any of
  // its tracks match; in the latter case, only the matching tracks render.
  let searchQuery = $state('');
  const normalizedSearch = $derived(searchQuery.trim().toLowerCase());

  // Auto-load tracks while a non-empty search is active, so a user typing
  // a song title gets results even when the collection has never been
  // switched into Expanded view. Same fetch path as the Expanded mode
  // effect — already-fetched items skip.
  $effect(() => {
    if (!normalizedSearch || !collection) return;
    for (const it of collection.items) {
      if (canExpand(it)) {
        void ensureTracksLoaded(it);
      }
    }
  });

  function itemMatchesSearch(it: MixtapeCollectionItem): boolean {
    if (!normalizedSearch) return true;
    if (it.title.toLowerCase().includes(normalizedSearch)) return true;
    if (it.subtitle?.toLowerCase().includes(normalizedSearch)) return true;
    // Track-level match: any loaded track whose title contains the query.
    const cached = expandedTracks[itemKey(it)];
    if (cached?.some((track) => track.title.toLowerCase().includes(normalizedSearch))) {
      return true;
    }
    return false;
  }

  function filteredTracksFor(it: MixtapeCollectionItem): ExpandedTrack[] {
    const all = expandedTracks[itemKey(it)] ?? [];
    if (!normalizedSearch) return all;
    // If the album title itself matched, don't narrow — show all its tracks.
    if (
      it.title.toLowerCase().includes(normalizedSearch) ||
      it.subtitle?.toLowerCase().includes(normalizedSearch)
    ) {
      return all;
    }
    return all.filter((track) => track.title.toLowerCase().includes(normalizedSearch));
  }

  function selectSort(value: SortBy) {
    if (sortBy === value) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortBy = value;
      sortDir = 'asc';
    }
    showSortMenu = false;
  }

  const sortOptions: { value: SortBy; label: string }[] = $derived([
    { value: 'position', label: $t('collectionDetail.sortByPosition') || 'Position' },
    { value: 'name', label: $t('collectionDetail.sortByName') || 'Name' },
    { value: 'year', label: $t('collectionDetail.sortByYear') || 'Year' },
    { value: 'tracks', label: $t('collectionDetail.sortByTracks') || 'Tracks' },
  ]);

  const typeFilterOptions: { value: TypeFilter; label: string }[] = $derived([
    { value: 'all', label: $t('collectionDetail.filterAll') || 'All' },
    { value: 'album', label: $t('itemType.album') },
    { value: 'track', label: $t('itemType.track') },
    { value: 'playlist', label: $t('itemType.playlist') },
  ]);

  /**
   * Visible items = filter + sort applied. Sort is non-destructive — the
   * underlying `collection.items` keeps its persisted order so the Play
   * action still plays "in order" according to the user's curated sequence.
   */
  const visibleItems = $derived.by(() => {
    if (!collection) return [] as MixtapeCollectionItem[];
    let filtered = typeFilter === 'all'
      ? collection.items.slice()
      : collection.items.filter((it) => it.item_type === typeFilter);
    if (sourceFilter.size > 0) {
      filtered = filtered.filter((it) => sourceFilter.has(resolvedFor(it).kind));
    }
    if (normalizedSearch) {
      filtered = filtered.filter(itemMatchesSearch);
    }
    const dir = sortDir === 'asc' ? 1 : -1;
    filtered.sort((a, b) => {
      switch (sortBy) {
        case 'name':
          return a.title.localeCompare(b.title) * dir;
        case 'year': {
          const ya = a.year ?? 0;
          const yb = b.year ?? 0;
          return (ya - yb) * dir;
        }
        case 'tracks': {
          const ta = a.track_count ?? 0;
          const tb = b.track_count ?? 0;
          return (ta - tb) * dir;
        }
        default:
          return (a.position - b.position) * dir;
      }
    });
    return filtered;
  });

  // ── Virtualization ────────────────────────────────────────────────────────
  // Fixed-height windowing for list/grid modes. Large collections (60+ items)
  // were rendering every row + every artwork <img> up front — scroll felt
  // laggy and initial paint took seconds. We keep the whole `.detail-view`
  // as the scroller so the hero + toolbar can share its scroll (toolbar
  // stays sticky; hero can scroll away), and compute a visible index range
  // from scrollTop relative to the list's offsetTop inside that scroller.
  //
  // Expanded mode isn't virtualized (row heights vary with loaded tracks),
  // but auto-track-fetching is gated to the virtual window so only visible
  // items trigger backend calls.
  // Row height MUST match the CSS rule .item-row { min-height: 56px } below.
  // The variable-height case (subtitle wrapping to 2 lines) pushed actual row
  // height up to ~60px, so 52px as a constant left ~8px of unaccounted space
  // per row — over 60 rows that's ~480px of scroll-height error, enough that
  // the scrollbar can't reach the true bottom. Fixing CSS + constant together
  // is the cheapest path to smooth scroll without variable-height windowing.
  const LIST_ROW_HEIGHT = 56;
  const GRID_CARD_MIN_W = 150; // px — matches grid-template-columns minmax
  const GRID_GAP = 20; // px — matches .item-grid gap
  // Chrome beyond the square artwork per grid card: the 8px gap between
  // artwork and title, title line (~16px), optional subtitle line
  // (~14px), plus 8px top+bottom padding on .grid-card. Tuned by
  // inspection so the dynamic row height lines up with real cards; the
  // old fixed 240 constant drifted on wide containers where cards grow
  // past 170 and the unaccounted height piled up per row until the
  // scrollbar couldn't reach the last row.
  const GRID_CARD_CHROME_H = 62;
  const WINDOW_BUFFER = 4; // extra rows above/below viewport

  let scrollEl = $state<HTMLDivElement | null>(null);
  let listAnchorEl = $state<HTMLDivElement | null>(null);
  // Seed viewportHeight with something reasonable so the very first
  // virtualWindow computation returns a small slice instead of the
  // `viewportHeight <= 0` fallback (which returned all items and
  // caused every card's cachedSrc action to fire a backend invoke on
  // mount — the main cause of "slow to open" on large collections).
  // bind:clientHeight replaces this with the real value a tick later.
  let viewportHeight = $state(typeof window !== 'undefined' ? window.innerHeight : 800);
  let listScrollTop = $state(0); // scrollTop of scrollEl, 0 when hero in view
  let listAnchorOffsetTop = $state(0); // distance from scroller top to list start
  let listContainerWidth = $state(0);

  function onDetailScroll() {
    if (!scrollEl) return;
    listScrollTop = scrollEl.scrollTop;
  }

  // Re-measure listAnchorOffsetTop when the *scroller itself* resizes. We do
  // NOT observe the list anchor — its height changes every frame as the
  // virtual spacers update, and a height change there does not affect its
  // own offsetTop (that's determined by the hero + sticky-toolbar above it).
  // Observing the anchor caused RO to fire on every scroll tick, which in
  // turn ticked an unrelated scroll-reset effect and fought the user's
  // scroll. One measurement on mount + re-measures on scroller resize is
  // enough.
  $effect(() => {
    if (!listAnchorEl || !scrollEl) return;
    const measure = () => {
      if (!listAnchorEl || !scrollEl) return;
      const lRect = listAnchorEl.getBoundingClientRect();
      const sRect = scrollEl.getBoundingClientRect();
      listAnchorOffsetTop = lRect.top - sRect.top + scrollEl.scrollTop;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(scrollEl);
    return () => ro.disconnect();
  });

  // Reset scroll when the active filter / view mode changes — otherwise the
  // user would land mid-list after switching from grid → list and see blank
  // spacer rows. Read scrollEl/listAnchorOffsetTop via untrack so this effect
  // only re-runs for the explicit deps (viewMode / search / filters). Without
  // untrack, any RO-driven change to listAnchorOffsetTop during scroll would
  // re-fire the effect and snap scroll back — visible as scroll stutter.
  $effect(() => {
    viewMode; // track
    normalizedSearch;
    typeFilter;
    sourceFilter;
    untrack(() => {
      if (scrollEl && listAnchorOffsetTop > 0 && scrollEl.scrollTop > listAnchorOffsetTop) {
        scrollEl.scrollTop = listAnchorOffsetTop;
      }
    });
  });

  const gridColumns = $derived.by(() => {
    if (viewMode !== 'grid' || listContainerWidth <= 0) return 1;
    return Math.max(
      1,
      Math.floor((listContainerWidth + GRID_GAP) / (GRID_CARD_MIN_W + GRID_GAP)),
    );
  });

  // Actual card width given current column count, accounting for gaps.
  // Cards are 1:1 aspect-ratio for artwork, so card_width also drives
  // artwork height. Row height = artwork (= card_width) + chrome + gap.
  // Tracked as a single $derived so the virtualWindow math picks up
  // container-resize changes automatically.
  const gridRowHeight = $derived.by(() => {
    if (viewMode !== 'grid' || listContainerWidth <= 0 || gridColumns <= 0) {
      return GRID_CARD_MIN_W + GRID_CARD_CHROME_H + GRID_GAP;
    }
    const cardWidth = Math.floor(
      (listContainerWidth - GRID_GAP * (gridColumns - 1)) / gridColumns,
    );
    return cardWidth + GRID_CARD_CHROME_H + GRID_GAP;
  });

  // Compute [firstIdx, lastIdx) of items to render + top/bottom spacer heights.
  const virtualWindow = $derived.by(() => {
    const total = visibleItems.length;
    const empty = { firstIdx: 0, lastIdx: total, topSpacer: 0, bottomSpacer: 0 };
    if (total === 0 || viewportHeight <= 0) return empty;

    const localScroll = Math.max(0, listScrollTop - listAnchorOffsetTop);

    if (viewMode === 'grid') {
      const cols = Math.max(1, gridColumns);
      const rowH = gridRowHeight;
      const totalRows = Math.ceil(total / cols);
      const firstRow = Math.max(0, Math.floor(localScroll / rowH) - WINDOW_BUFFER);
      const lastRow = Math.min(
        totalRows,
        Math.ceil((localScroll + viewportHeight) / rowH) + WINDOW_BUFFER,
      );
      return {
        firstIdx: firstRow * cols,
        lastIdx: Math.min(total, lastRow * cols),
        topSpacer: firstRow * rowH,
        bottomSpacer: Math.max(0, (totalRows - lastRow) * rowH),
      };
    }

    // list / expanded — expanded rows vary in height once tracks mount, but
    // using the base row height still correctly windows the top-level items;
    // the loaded tracks just overflow the reserved spacer. Good enough for
    // the 60+ item case this is actually trying to fix.
    const firstIdx = Math.max(0, Math.floor(localScroll / LIST_ROW_HEIGHT) - WINDOW_BUFFER);
    const lastIdx = Math.min(
      total,
      Math.ceil((localScroll + viewportHeight) / LIST_ROW_HEIGHT) + WINDOW_BUFFER,
    );
    return {
      firstIdx,
      lastIdx,
      topSpacer: firstIdx * LIST_ROW_HEIGHT,
      bottomSpacer: Math.max(0, (total - lastIdx) * LIST_ROW_HEIGHT),
    };
  });

  // Expanded mode has variable row heights (base ~56px for the item row
  // plus whatever height the loaded TrackRows occupy below it). The
  // fixed-height virtualization math can't express that, so spacers
  // come out shorter than the real content and the scrollbar tops out
  // before the last row — that's the "scroll snaps back / can't reach
  // the end" regression. In expanded mode we render every item and
  // skip spacer clipping entirely; the auto-fetch gate still uses
  // virtualWindow.{firstIdx,lastIdx} to cap backend calls to the
  // visible viewport, so the only DOM cost of rendering all items is
  // the item-row shell (the 56px row) — tracks still mount on demand.
  const windowedItems = $derived(
    viewMode === 'expanded'
      ? visibleItems
      : visibleItems.slice(virtualWindow.firstIdx, virtualWindow.lastIdx),
  );

  const activeTopSpacer = $derived(
    viewMode === 'expanded' ? 0 : virtualWindow.topSpacer,
  );
  const activeBottomSpacer = $derived(
    viewMode === 'expanded' ? 0 : virtualWindow.bottomSpacer,
  );

  // Qobuz artwork URLs follow `<cdn>/.../<hash>_<size>.jpg` where size is
  // one of 50/100/150/230/300/600. In list/grid listings we never need the
  // 600px original — downsize to whatever actually fits the rendered <img>
  // so the WebView doesn't waste memory decoding huge JPEGs. Non-Qobuz URLs
  // (local file:// / tauri asset URLs / Plex) are returned as-is.
  function smallQobuzArtwork(
    url: string | null | undefined,
    target: 50 | 150 | 230 = 50,
  ): string | null {
    if (!url) return null;
    return url.replace(/_(50|100|150|230|300|600|max|org)\.jpg/i, `_${target}.jpg`);
  }

  function toggleSelect(position: number, event?: MouseEvent | KeyboardEvent) {
    if (event?.shiftKey && lastSelectedPosition !== null && collection) {
      const positions = collection.items.map((it) => it.position);
      selectedPositions = applyShiftRange({
        current: selectedPositions,
        ids: positions,
        lastIndex: positions.indexOf(lastSelectedPosition),
        currentIndex: positions.indexOf(position),
      });
      lastSelectedPosition = position;
      return;
    }
    const next = new Set(selectedPositions);
    if (next.has(position)) next.delete(position);
    else next.add(position);
    selectedPositions = next;
    lastSelectedPosition = position;
  }

  function clearSelection() {
    selectedPositions = new Set();
    lastSelectedPosition = null;
  }

  function selectedItems(): MixtapeCollectionItem[] {
    if (!collection) return [];
    return collection.items.filter((it) => selectedPositions.has(it.position));
  }

  function toAddToMixtapeItem(it: MixtapeCollectionItem) {
    return {
      item_type: it.item_type,
      source: it.source,
      source_item_id: it.source_item_id,
      title: it.title,
      subtitle: it.subtitle ?? undefined,
      artwork_url: it.artwork_url ?? undefined,
      year: it.year ?? undefined,
      track_count: it.track_count ?? undefined,
    };
  }

  async function handleBulkRemove() {
    const items = selectedItems();
    if (items.length === 0) return;
    try {
      // Sort descending so earlier positions don't shift as we remove.
      const sorted = [...items].sort((a, b) => b.position - a.position);
      for (const it of sorted) {
        await removeCollectionItem(collectionId, it.position);
      }
      await loadCollection();
      clearSelection();
      showToast(
        $t('toast.removedFromQueue', { values: { count: items.length } }) ||
          `Removed ${items.length}`,
        'info',
      );
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] bulk remove failed:', err);
      showToast('Failed to remove items', 'error');
    }
  }

  function handleBulkAddToMixtape() {
    const items = selectedItems();
    if (items.length === 0) return;
    openAddToMixtape(items.map(toAddToMixtapeItem));
  }

  let collection = $state<MixtapeCollection | null>(null);
  let loading = $state(true);
  let overflowOpen = $state(false);
  let renameModalOpen = $state(false);
  let descriptionModalOpen = $state(false);
  let confirmDeleteOpen = $state(false);
  let mixModalOpen = $state(false);

  // Edit drafts
  let draftName = $state('');
  let draftDescription = $state('');

  // Item overflow menus (track which item's ⋯ is open)
  let openItemMenu = $state<number | null>(null);
  // When non-null, the "Change release type" submenu is expanded for this
  // position inside the row's ⋯ menu.
  let openTypeSubmenu = $state<number | null>(null);

  function currentReleaseTypeFor(item: MixtapeCollectionItem): ReleaseType | null {
    const override = $releaseTypeOverrides[`${item.source}|${item.source_item_id}`];
    return (override as ReleaseType | undefined) ?? null;
  }

  async function loadCollection() {
    loading = true;
    try {
      collection = await getCollection(collectionId);
      if (collection) {
        void resolveItems(collection.items);
        // Prime the image cache for every item up front. preloadImages
        // fires getCachedImageUrl in the background for each URL so by
        // the time the user scrolls to a card, its resolved asset://
        // URL is already in the in-memory map and cachedSrc can set
        // src without awaiting a backend round trip — that round trip
        // was the visible "dark placeholder flash" per card during
        // scroll on large collections.
        // Request the 150px variant (matches the grid card display
        // size) so the backend caches the right asset.
        const urls = collection.items
          .map((it) => {
            const raw = it.artwork_url;
            if (!raw) return null;
            return smallQobuzArtwork(raw, 150) ?? raw;
          })
          .filter((u): u is string => !!u);
        preloadImages(urls);
      }
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] load failed:', err);
      collection = null;
    } finally {
      loading = false;
    }
  }

  // Re-load whenever collectionId changes. The detail view owns its own
  // `collection` state — every mutation handler below calls loadCollection()
  // explicitly after success, so we don't ALSO sync from $collectionsStore
  // here. Reading and writing `collection` inside the same $effect produces
  // an infinite effect-update loop (Svelte 5 effect_update_depth_exceeded).
  $effect(() => {
    void collectionId;
    loadCollection();
  });

  onMount(() => {
    loadReleaseTypeOverrides();
  });

  // ── Per-collection view preferences ───────────────────────────────────────
  // Remember viewMode / sort / filters per collection id so opening a
  // specific Mixtape or Collection keeps its user-chosen layout. Stored in
  // per-user localStorage under one key per collection so a collection
  // deletion can trivially clean up (see onDelete below). searchQuery and
  // selectMode intentionally stay transient — they're per-session UI state.
  interface ViewPrefs {
    viewMode?: ViewMode;
    sortBy?: SortBy;
    sortDir?: SortDir;
    typeFilter?: TypeFilter;
    sourceFilter?: SourceKind[]; // Set serialized as array
  }
  function prefsKey(id: string): string {
    return `collection-view-prefs.${id}`;
  }
  function loadPrefs(id: string): ViewPrefs | null {
    try {
      const raw = getUserItem(prefsKey(id));
      if (!raw) return null;
      return JSON.parse(raw) as ViewPrefs;
    } catch {
      return null;
    }
  }
  function savePrefs(id: string, prefs: ViewPrefs): void {
    try {
      setUserItem(prefsKey(id), JSON.stringify(prefs));
    } catch {
      // localStorage quota / disabled — silently skip, prefs are non-critical.
    }
  }

  // Hydration gate: the load effect applies stored prefs to the state vars,
  // which would immediately re-trigger the persist effect below and save
  // those same values back (fine) OR save defaults before load had a chance
  // (NOT fine — overwrites stored prefs). prefsHydrated flips true only
  // AFTER load has run, so the persist effect does nothing until then.
  let prefsHydrated = $state(false);

  $effect(() => {
    const id = collectionId;
    if (!id) return;
    prefsHydrated = false;
    const stored = loadPrefs(id);
    if (stored) {
      if (stored.viewMode) viewMode = stored.viewMode;
      if (stored.sortBy) sortBy = stored.sortBy;
      if (stored.sortDir) sortDir = stored.sortDir;
      if (stored.typeFilter) typeFilter = stored.typeFilter;
      if (stored.sourceFilter) sourceFilter = new Set(stored.sourceFilter);
    } else {
      // Reset to defaults when the collectionId changes to a collection with
      // no stored prefs — otherwise state leaks from the previous collection.
      viewMode = 'list';
      sortBy = 'position';
      sortDir = 'asc';
      typeFilter = 'all';
      sourceFilter = new Set();
    }
    prefsHydrated = true;
  });

  $effect(() => {
    // Read the prefs so Svelte tracks them as deps, then persist.
    const payload: ViewPrefs = {
      viewMode,
      sortBy,
      sortDir,
      typeFilter,
      sourceFilter: Array.from(sourceFilter),
    };
    if (!prefsHydrated || !collectionId) return;
    savePrefs(collectionId, payload);
  });

  // ──────── helpers ────────

  function formatItemCountSummary(items: MixtapeCollectionItem[]): string {
    return $t('mixtapes.albumCount', { values: { count: items.length } });
  }

  function kindLabel(kind: CollectionKind | undefined): string {
    if (!kind) return '';
    if (kind === 'mixtape') return $t('mixtapes.label');
    if (kind === 'artist_collection') return $t('collections.artistLabel');
    return $t('collections.label');
  }

  function itemTypeLabel(type: ItemType): string {
    if (type === 'album') return $t('itemType.album');
    if (type === 'track') return $t('itemType.track');
    return $t('itemType.playlist');
  }

  /**
   * Displayed label in the Type column. Tracks and playlists keep their
   * item-type label unchanged; album items surface the effective release
   * type (either the user's override or 'album' as the default) so the
   * column visibly reacts when the user picks EP / Single / Live / etc.
   * from the row ⋯ menu.
   */
  function displayedTypeLabel(item: MixtapeCollectionItem): string {
    if (item.item_type !== 'album') return itemTypeLabel(item.item_type);
    const override = currentReleaseTypeFor(item);
    const effective: ReleaseType = override ?? 'album';
    return $t(`discographyBuilder.releaseType.${effective}`);
  }

  function itemTracks(item: MixtapeCollectionItem): string {
    if (item.item_type === 'track') return '1';
    return item.track_count == null ? '—' : String(item.track_count);
  }

  function itemYear(item: MixtapeCollectionItem): string {
    return item.year == null ? '' : String(item.year);
  }

  // ──────── live source/artwork/quality resolution ────────
  // Items only store `source: 'qobuz' | 'local'` at persistence time. The
  // real story — plex vs local file, offline-cached vs streaming vs
  // purchased — has to be resolved at render time by cross-referencing
  // the local library + plex cache. We do this once on mount and cache
  // the result per item id.

  type SourceKind = 'qobuz' | 'plex' | 'local';

  interface ResolvedItem {
    kind: SourceKind;
    artworkUrl: string | null;
    bitDepth: number | null;
    /** kHz (not Hz) — matches QualityBadge's samplingRate prop */
    sampleRateKhz: number | null;
    format: string | null;
  }

  let resolvedById = $state<Record<string, ResolvedItem>>({});

  function buildPlexArtworkUrl(path: string): string | null {
    const baseUrl = (getUserItem('qbz-plex-poc-base-url') || '').trim();
    const token = (getUserItem('qbz-plex-poc-token') || '').trim();
    if (!baseUrl || !token) return null;
    const base = baseUrl.replace(/\/+$/, '');
    const separator = path.includes('?') ? '&' : '?';
    return `${base}${path}${separator}X-Plex-Token=${encodeURIComponent(token)}`;
  }

  async function resolveItems(items: MixtapeCollectionItem[]) {
    if (items.length === 0) {
      resolvedById = {};
      return;
    }

    const plexEnabled = getUserItem('qbz-plex-enabled') === 'true';

    const [localAlbumsRaw, plexAlbumsRaw] = await Promise.all([
      invoke<Array<{
        id: string;
        source?: string;
        format?: string;
        bit_depth?: number;
        sample_rate?: number;
        artwork_path?: string;
      }>>('v2_library_get_albums', {
        includeHidden: false,
        excludeNetworkFolders: false,
      }).catch(() => []),
      plexEnabled
        ? invoke<Array<{
            id: string;
            format?: string;
            bitDepth?: number;
            sampleRate?: number;
            artworkPath?: string;
          }>>('v2_plex_cache_get_albums').catch(() => [])
        : Promise.resolve([]),
    ]);

    const localMap = new Map(localAlbumsRaw.map((a) => [a.id, a]));
    const plexMap = new Map(plexAlbumsRaw.map((a) => [a.id, a]));

    // Qobuz items: fire-and-forget v2_get_album in parallel to get audio_info.
    // The Qobuz API returns maximum_bit_depth + maximum_sampling_rate (kHz).
    const qobuzAlbumFetches = items
      .filter((it) => it.source === 'qobuz' && it.item_type === 'album')
      .map(async (item) => {
        try {
          const album = await invoke<{
            maximum_bit_depth?: number;
            maximum_sampling_rate?: number;
          }>('v2_get_album', { albumId: item.source_item_id });
          return { item, album };
        } catch {
          return { item, album: null as { maximum_bit_depth?: number; maximum_sampling_rate?: number } | null };
        }
      });
    const qobuzResults = await Promise.all(qobuzAlbumFetches);
    const qobuzMap = new Map(
      qobuzResults
        .filter((r) => r.album)
        .map((r) => [r.item.source_item_id, r.album!]),
    );

    const next: Record<string, ResolvedItem> = {};
    for (const item of items) {
      const key = `${item.source}|${item.source_item_id}`;

      const plexHit = plexMap.get(item.source_item_id);
      if (plexHit) {
        next[key] = {
          kind: 'plex',
          artworkUrl: plexHit.artworkPath
            ? buildPlexArtworkUrl(plexHit.artworkPath)
            : null,
          bitDepth: plexHit.bitDepth ?? null,
          sampleRateKhz: plexHit.sampleRate ? plexHit.sampleRate / 1000 : null,
          format: plexHit.format ?? null,
        };
        continue;
      }

      const localHit = localMap.get(item.source_item_id);
      if (localHit) {
        const src = localHit.source ?? 'user';
        // qobuz_download / qobuz_purchase are still *Qobuz-origin* albums,
        // just stored locally. Surface them as 'qobuz' — matches the
        // DiscographyBuilder's 3-kind model (qobuz | plex | local).
        const kind: SourceKind =
          src === 'plex'
            ? 'plex'
            : src === 'qobuz_download' || src === 'qobuz_purchase'
              ? 'qobuz'
              : 'local';
        next[key] = {
          kind,
          artworkUrl: null,
          bitDepth: localHit.bit_depth ?? null,
          sampleRateKhz: localHit.sample_rate
            ? localHit.sample_rate / 1000
            : null,
          format: localHit.format ?? null,
        };
        continue;
      }

      const qobuzHit = qobuzMap.get(item.source_item_id);
      if (qobuzHit) {
        next[key] = {
          kind: 'qobuz',
          artworkUrl: null,
          bitDepth: qobuzHit.maximum_bit_depth ?? null,
          // v2_get_album returns sampling_rate in kHz already.
          sampleRateKhz: qobuzHit.maximum_sampling_rate ?? null,
          format: 'FLAC',
        };
        continue;
      }

      next[key] = {
        kind: item.source === 'qobuz' ? 'qobuz' : 'local',
        artworkUrl: null,
        bitDepth: null,
        sampleRateKhz: null,
        format: null,
      };
    }

    resolvedById = next;
  }

  function resolvedFor(item: MixtapeCollectionItem): ResolvedItem {
    const key = `${item.source}|${item.source_item_id}`;
    return (
      resolvedById[key] ?? {
        kind: item.source === 'qobuz' ? 'qobuz' : 'local',
        artworkUrl: null,
        bitDepth: null,
        sampleRateKhz: null,
        format: null,
      }
    );
  }

  // ──────── actions ────────

  // True while v2_enqueue_collection is resolving + initial track is loading.
  // Used to disable Play/Shuffle buttons and swap their icons for a spinner.
  let playLoading = $state(false);

  /**
   * v2_enqueue_collection sets the queue and calls bridge.play_index(0),
   * but play_index only updates queue state — it does NOT start audio.
   * The authoritative audio-start happens when the frontend calls playTrack().
   * Mirror the pattern used by ArtistDetailView for radio playback:
   *   1. enqueueCollection('replace') — populate queue
   *   2. playQueueIndex(0) — reads the first queue track back from the core
   *   3. playTrack(trackData) — actually starts the audio pipeline
   */
  async function startPlaybackFromQueue() {
    const firstTrack = await playQueueIndex(0);
    if (!firstTrack) {
      throw new Error('Queue empty after enqueue');
    }
    const quality = firstTrack.hires && firstTrack.bit_depth && firstTrack.sample_rate
      ? `${firstTrack.bit_depth}bit/${firstTrack.sample_rate}kHz`
      : firstTrack.hires
        ? 'Hi-Res'
        : '-';
    await playTrack({
      id: firstTrack.id,
      title: firstTrack.title,
      artist: firstTrack.artist,
      album: firstTrack.album ?? '',
      artwork: firstTrack.artwork_url ?? '',
      duration: firstTrack.duration_secs,
      quality,
      bitDepth: firstTrack.bit_depth ?? undefined,
      samplingRate: firstTrack.sample_rate ?? undefined,
      albumId: firstTrack.album_id ?? undefined,
      artistId: firstTrack.artist_id ?? undefined,
    });
  }

  async function handlePlay() {
    if (playLoading) return;
    playLoading = true;
    try {
      await enqueueCollection(collectionId, 'replace');
      await startPlaybackFromQueue();
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] enqueue (play) failed:', err);
      showToast($t('toast.failedStartPlayback') || 'Failed to start playback', 'error');
    } finally {
      playLoading = false;
    }
  }

  async function handleShuffle() {
    if (!collection || playLoading) return;
    playLoading = true;
    try {
      if (collection.play_mode !== 'album_shuffle') {
        await setPlayMode(collectionId, 'album_shuffle');
      }
      await enqueueCollection(collectionId, 'replace');
      await startPlaybackFromQueue();
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] enqueue (shuffle) failed:', err);
      showToast($t('toast.failedStartPlayback') || 'Failed to start playback', 'error');
    } finally {
      playLoading = false;
    }
  }

  function openRenameModal() {
    if (!collection) return;
    draftName = collection.name;
    renameModalOpen = true;
    overflowOpen = false;
  }

  async function submitRename() {
    const name = draftName.trim();
    if (!name || !collection) { renameModalOpen = false; return; }
    await renameCollection(collectionId, name);
    await loadCollection();
    renameModalOpen = false;
  }

  function openDescriptionModal() {
    if (!collection) return;
    draftDescription = collection.description ?? '';
    descriptionModalOpen = true;
    overflowOpen = false;
  }

  async function submitDescription() {
    if (!collection) return;
    const desc = draftDescription.trim() === '' ? null : draftDescription.trim();
    await setDescription(collectionId, desc);
    await loadCollection();
    descriptionModalOpen = false;
  }

  async function togglePlayMode() {
    if (!collection) return;
    const next: CollectionPlayMode =
      collection.play_mode === 'in_order' ? 'album_shuffle' : 'in_order';
    await setPlayMode(collectionId, next);
    await loadCollection();
    overflowOpen = false;
  }

  async function handleUploadCover() {
    overflowOpen = false;
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
      });
      if (!selected || typeof selected !== 'string') return;
      await uploadCustomCover(collectionId, selected);
      await loadCollection();
      showToast($t('collectionDetail.coverUploaded'), 'success');
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] uploadCover failed:', err);
      showToast($t('collectionDetail.coverUploadFailed'), 'error');
    }
  }

  async function handleRemoveCover() {
    overflowOpen = false;
    try {
      await removeCustomCover(collectionId);
      await loadCollection();
      showToast($t('collectionDetail.coverRemoved'), 'success');
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] removeCover failed:', err);
      showToast($t('collectionDetail.coverRemoveFailed'), 'error');
    }
  }

  async function convertKind() {
    if (!collection) return;
    const next: CollectionKind =
      collection.kind === 'mixtape' ? 'collection' : 'mixtape';
    try {
      await setKind(collectionId, next);
      await loadCollection();
      showToast('Converted', 'success');
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] convertKind failed:', err);
      showToast('Cannot convert this kind', 'error');
    }
    overflowOpen = false;
  }

  async function handleDelete() {
    if (!collection) return;
    try {
      await deleteCollection(collectionId);
      // Drop the persisted view prefs for this collection — no point
      // leaving orphaned keys in localStorage when the collection is gone.
      removeUserItem(prefsKey(collectionId));
      onBack?.();
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] delete failed:', err);
      showToast('Failed to delete', 'error');
    } finally {
      confirmDeleteOpen = false;
    }
  }

  async function handleRemoveItem(position: number) {
    try {
      await removeCollectionItem(collectionId, position);
      await loadCollection();
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] remove item failed:', err);
      showToast('Failed to remove', 'error');
    } finally {
      openItemMenu = null;
    }
  }

  // Close overflow menus on ESC
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (confirmDeleteOpen) confirmDeleteOpen = false;
      else if (renameModalOpen) renameModalOpen = false;
      else if (descriptionModalOpen) descriptionModalOpen = false;
      else if (overflowOpen) overflowOpen = false;
      else if (openItemMenu !== null) openItemMenu = null;
    }
  }

  async function handleConfirmMix(sampleSize: number) {
    if (!collection || playLoading) return;
    mixModalOpen = false;
    playLoading = true;
    try {
      const result = await invoke<{ requestedCount: number; actualCount: number }>(
        'v2_collection_shuffle_tracks',
        { collectionId, sampleSize },
      );
      await startPlaybackFromQueue();
      if (result.actualCount < result.requestedCount) {
        showToast(
          $t('toast.mixTrimmed', {
            values: { actual: result.actualCount, requested: result.requestedCount },
          }),
          'info',
        );
      }
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] shuffle tracks failed:', err);
      showToast($t('toast.failedStartPlayback') || 'Failed to start playback', 'error');
    } finally {
      playLoading = false;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="detail-view"
  bind:this={scrollEl}
  bind:clientHeight={viewportHeight}
  onscroll={onDetailScroll}
>
  {#if loading}
    <div class="loading">{$t('actions.loading')}</div>
  {:else if !collection}
    <div class="not-found">
      <button class="back-btn" onclick={() => onBack?.()}>
        <ArrowLeft size={16} />
        <span>{$t('actions.back')}</span>
      </button>
      <p>{$t('errors.notFound')}</p>
    </div>
  {:else}
    <!-- Header -->
    <header class="detail-header">
      {#if onBack}
        <button class="back-btn" onclick={() => onBack()}>
          <ArrowLeft size={16} />
          <span>{$t('actions.back')}</span>
        </button>
      {/if}

      <div class="header-content">
        <div class="header-cover">
          <CollectionMosaic
            items={collection.items}
            size={186}
            kind={collection.kind}
            customCoverUrl={collection.custom_artwork_path
              ? convertFileSrc(collection.custom_artwork_path)
              : null}
          />
        </div>

        <div class="header-info">
          <div class="eyebrow">
            <span class="kind-tag">{kindLabel(collection.kind)}</span>
          </div>
          <h1 class="title">{collection.name}</h1>
          {#if collection.description}
            <p class="description">{collection.description}</p>
          {/if}
          <div class="meta">
            {formatItemCountSummary(collection.items)}
          </div>

          <div class="header-actions">
            <button
              class="action-btn-circle primary"
              onclick={handlePlay}
              disabled={collection.items.length === 0 || playLoading}
              title={$t('common.playAllInOrder')}
              aria-label={$t('common.playAllInOrder')}
            >
              {#if playLoading}
                <LoaderCircle size={20} class="spin" />
              {:else}
                <Play size={20} fill="currentColor" color="currentColor" />
              {/if}
            </button>
            <button
              class="action-btn-circle"
              onclick={handleShuffle}
              disabled={collection.items.length === 0 || playLoading}
              title={$t('common.shuffleAlbums')}
              aria-label={$t('common.shuffleAlbums')}
            >
              {#if playLoading}
                <LoaderCircle size={18} class="spin" />
              {:else}
                <Shuffle size={18} />
              {/if}
            </button>
            <button
              class="action-btn-circle"
              onclick={() => (mixModalOpen = true)}
              disabled={collection.items.length === 0 || playLoading}
              title={$t('common.shuffleTracksMix')}
              aria-label={$t('common.shuffleTracksMix')}
            >
              {#if playLoading}
                <LoaderCircle size={18} class="spin" />
              {:else}
                <DjIcon size={18} />
              {/if}
            </button>
            <div class="overflow-wrap">
              <button
                class="action-btn-circle"
                onclick={() => (overflowOpen = !overflowOpen)}
                aria-label={$t('actions.more')}
                title={$t('actions.more')}
              >
                <MoreHorizontal size={18} />
              </button>
              {#if overflowOpen}
                <div
                  class="overflow-backdrop"
                  onclick={() => (overflowOpen = false)}
                  role="presentation"
                ></div>
                <div class="overflow-menu" role="menu">
                  <button class="overflow-item" onclick={openRenameModal}>
                    {$t('collectionDetail.rename')}
                  </button>
                  <button class="overflow-item" onclick={openDescriptionModal}>
                    {$t('collectionDetail.editDescription')}
                  </button>
                  <button class="overflow-item" onclick={handleUploadCover}>
                    {$t('collectionDetail.uploadCover')}
                  </button>
                  {#if collection.custom_artwork_path}
                    <button class="overflow-item" onclick={handleRemoveCover}>
                      {$t('collectionDetail.clearCustomCover')}
                    </button>
                  {/if}
                  <button class="overflow-item" onclick={togglePlayMode}>
                    {collection.play_mode === 'in_order'
                      ? $t('common.playModeAlbumShuffle')
                      : $t('common.playModeInOrder')}
                  </button>
                  {#if collection.kind !== 'artist_collection'}
                    <button class="overflow-item" onclick={convertKind}>
                      {collection.kind === 'mixtape'
                        ? $t('collectionDetail.convertToCollection')
                        : $t('collectionDetail.convertToMixtape')}
                    </button>
                  {/if}
                  <button
                    class="overflow-item destructive"
                    onclick={() => { confirmDeleteOpen = true; overflowOpen = false; }}
                  >
                    {$t('collectionDetail.delete')}
                  </button>
                </div>
              {/if}
            </div>
          </div>
        </div>
      </div>
    </header>

    <!-- Item list -->
    {#if collection.items.length === 0}
      <div class="empty-list">
        <p>No items yet. Add albums, tracks, or playlists from their detail pages.</p>
      </div>
    {:else}
      <div class="sticky-toolbar">
      <div class="list-controls">
        <div class="search-box" class:has-query={normalizedSearch.length > 0}>
          <Search size={14} />
          <input
            type="text"
            class="search-input"
            placeholder={$t('collectionDetail.searchPlaceholder') || 'Search albums & tracks…'}
            bind:value={searchQuery}
            aria-label={$t('collectionDetail.searchPlaceholder') || 'Search'}
          />
          {#if normalizedSearch}
            <button
              type="button"
              class="search-clear"
              onclick={() => (searchQuery = '')}
              aria-label={$t('actions.clearSearch') || 'Clear search'}
              title={$t('actions.clearSearch') || 'Clear'}
            >
              <X size={12} />
            </button>
          {/if}
        </div>
        <div class="dropdown-container">
          <button
            type="button"
            class="control-btn"
            onclick={() => { showSortMenu = !showSortMenu; showFilterMenu = false; }}
            title={$t('collectionDetail.sort') || 'Sort'}
          >
            <ArrowUpDown size={14} />
            <span>{sortOptions.find((o) => o.value === sortBy)?.label}</span>
            <span class="sort-indicator">{sortDir === 'asc' ? '↑' : '↓'}</span>
          </button>
          {#if showSortMenu}
            <div class="control-backdrop" onclick={() => (showSortMenu = false)} role="presentation"></div>
            <div class="control-menu">
              {#each sortOptions as option}
                <button
                  type="button"
                  class="control-menu-item"
                  class:selected={sortBy === option.value}
                  onclick={() => selectSort(option.value)}
                >
                  <span>{option.label}</span>
                  {#if sortBy === option.value}
                    <span class="sort-indicator">{sortDir === 'asc' ? '↑' : '↓'}</span>
                  {/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>

        <div class="dropdown-container">
          <button
            type="button"
            class="control-btn"
            class:active={typeFilter !== 'all' || sourceFilter.size > 0}
            onclick={() => { showFilterMenu = !showFilterMenu; showSortMenu = false; }}
            title={$t('collectionDetail.filter') || 'Filter'}
          >
            <Filter size={14} />
            <span>{$t('collectionDetail.filter') || 'Filter'}</span>
            {#if sourceFilter.size > 0 || typeFilter !== 'all'}
              <span class="filter-count">{(sourceFilter.size) + (typeFilter !== 'all' ? 1 : 0)}</span>
            {/if}
          </button>
          {#if showFilterMenu}
            <div class="control-backdrop" onclick={() => (showFilterMenu = false)} role="presentation"></div>
            <div class="control-menu wide">
              <div class="filter-section-label">{$t('collectionDetail.colType') || $t('discographyBuilder.colType')}</div>
              {#each typeFilterOptions as option}
                <button
                  type="button"
                  class="control-menu-item"
                  class:selected={typeFilter === option.value}
                  onclick={() => { typeFilter = option.value; }}
                >
                  <span>{option.label}</span>
                  {#if typeFilter === option.value}
                    <Check size={12} />
                  {/if}
                </button>
              {/each}

              <div class="filter-section-divider"></div>

              <div class="filter-section-label">{$t('library.source')}</div>
              <button
                type="button"
                class="control-menu-item"
                class:selected={sourceFilter.has('qobuz')}
                onclick={() => toggleSourceFilter('qobuz')}
              >
                <span>{$t('library.qobuzTrackIndicator')}</span>
                {#if sourceFilter.has('qobuz')}<Check size={12} />{/if}
              </button>
              <button
                type="button"
                class="control-menu-item"
                class:selected={sourceFilter.has('plex')}
                onclick={() => toggleSourceFilter('plex')}
              >
                <span>{$t('library.plexTrackIndicator')}</span>
                {#if sourceFilter.has('plex')}<Check size={12} />{/if}
              </button>
              <button
                type="button"
                class="control-menu-item"
                class:selected={sourceFilter.has('local')}
                onclick={() => toggleSourceFilter('local')}
              >
                <span>{$t('library.localTrackIndicator')}</span>
                {#if sourceFilter.has('local')}<Check size={12} />{/if}
              </button>
            </div>
          {/if}
        </div>

        {#if hasAnyFilter}
          <button
            type="button"
            class="control-btn subtle"
            onclick={() => { typeFilter = 'all'; sourceFilter = new Set(); sortBy = 'position'; sortDir = 'asc'; }}
            title={$t('discographyBuilder.typeOverrideReset') || 'Reset'}
          >
            <RotateCcw size={12} />
          </button>
        {/if}

        <button
          type="button"
          class="control-btn"
          class:active={selectMode}
          onclick={toggleSelectMode}
          disabled={collection.items.length === 0}
          title={selectMode ? ($t('actions.cancelSelection') || 'Cancel selection') : ($t('actions.select') || 'Select')}
          aria-label={selectMode ? ($t('actions.cancelSelection') || 'Cancel selection') : ($t('actions.select') || 'Select')}
        >
          <SquareCheckBig size={14} />
          <span>{selectMode ? ($t('actions.cancelSelection') || 'Cancel') : ($t('actions.select') || 'Select')}</span>
        </button>

        <!-- View-mode segmented control: list / grid / expanded. -->
        <div class="view-mode-group" role="radiogroup" aria-label={$t('collectionDetail.viewMode') || 'View mode'}>
          <button
            type="button"
            class="control-btn seg"
            class:active={viewMode === 'list'}
            onclick={() => (viewMode = 'list')}
            title={$t('collectionDetail.viewList') || 'List'}
            aria-label={$t('collectionDetail.viewList') || 'List'}
            aria-pressed={viewMode === 'list'}
          >
            <List size={14} />
          </button>
          <button
            type="button"
            class="control-btn seg"
            class:active={viewMode === 'grid'}
            onclick={() => (viewMode = 'grid')}
            title={$t('collectionDetail.viewGrid') || 'Grid'}
            aria-label={$t('collectionDetail.viewGrid') || 'Grid'}
            aria-pressed={viewMode === 'grid'}
          >
            <LayoutGrid size={14} />
          </button>
          <button
            type="button"
            class="control-btn seg"
            class:active={viewMode === 'expanded'}
            onclick={() => (viewMode = 'expanded')}
            title={$t('collectionDetail.viewExpanded') || 'Expanded'}
            aria-label={$t('collectionDetail.viewExpanded') || 'Expanded'}
            aria-pressed={viewMode === 'expanded'}
          >
            <AlignJustify size={14} />
          </button>
        </div>
      </div>
      </div>
      <div
        class="list-anchor"
        bind:this={listAnchorEl}
        bind:clientWidth={listContainerWidth}
      >
      {#if viewMode === 'grid'}
        <div class="virtual-spacer" style:height="{activeTopSpacer}px"></div>
        <div class="item-grid">
          {#each windowedItems as item (item.position)}
            {@const resolved = resolvedFor(item)}
            {@const artworkSrc = item.artwork_url || resolved.artworkUrl}
            {@const isSelected = selectedPositions.has(item.position)}
            <div
              class="grid-card"
              class:is-selected={isSelected}
              role="button"
              tabindex="0"
              onclick={(e) => {
                if (selectMode) {
                  toggleSelect(item.position, e);
                } else {
                  onOpenItem?.(item.source, item.item_type, item.source_item_id);
                }
              }}
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  if (selectMode) toggleSelect(item.position, e);
                  else onOpenItem?.(item.source, item.item_type, item.source_item_id);
                }
              }}
            >
              <div class="grid-artwork-wrap">
                {#if artworkSrc}
                  <img
                    class="grid-artwork"
                    use:cachedSrc={smallQobuzArtwork(artworkSrc, 150) ?? artworkSrc}
                    alt=""
                    loading="lazy"
                    decoding="async"
                  />
                {:else}
                  <div class="grid-artwork grid-artwork-placeholder">
                    {#if item.item_type === 'track'}
                      <Music2 size={28} />
                    {:else if item.item_type === 'playlist'}
                      <ListMusic size={28} />
                    {:else}
                      <Disc size={28} />
                    {/if}
                  </div>
                {/if}
                {#if selectMode}
                  <input
                    type="checkbox"
                    class="grid-checkbox"
                    checked={isSelected}
                    onclick={(e) => { e.stopPropagation(); toggleSelect(item.position); }}
                    onchange={() => {}}
                    aria-label={$t('collectionDetail.selectItem') || 'Select item'}
                  />
                {:else}
                  <button
                    type="button"
                    class="grid-play-overlay"
                    onclick={(e) => { e.stopPropagation(); onPlayItem?.(item); }}
                    aria-label={$t('actions.play')}
                  >
                    <Play size={24} fill="currentColor" />
                  </button>
                {/if}
              </div>
              <div class="grid-title" title={item.title}>{item.title}</div>
              {#if item.subtitle}
                <div class="grid-subtitle" title={item.subtitle}>{item.subtitle}</div>
              {/if}
            </div>
          {/each}
        </div>
        <div class="virtual-spacer" style:height="{activeBottomSpacer}px"></div>
      {:else}
      <div class="item-list">
        <div class="item-list-header">
          <div class="col-idx">#</div>
          <div class="col-item">Item</div>
          <div class="col-type">Type</div>
          <div class="col-source">Source</div>
          <div class="col-quality">Quality</div>
          <div class="col-tracks">Tracks</div>
          <div class="col-year">Year</div>
          <div class="col-menu"></div>
        </div>

        <div class="virtual-spacer" style:height="{activeTopSpacer}px"></div>
        {#each windowedItems as item (item.position)}
          {@const resolved = resolvedFor(item)}
          {@const artworkSrc = item.artwork_url || resolved.artworkUrl}
          {@const isSelected = selectedPositions.has(item.position)}
          {@const key = itemKey(item)}
          {@const isExpandLoading = expandLoading.has(key)}
          {@const showTracks = viewMode === 'expanded' && canExpand(item)}
          <!-- .item-block wraps the row and its (optional) expanded track
               list so `content-visibility: auto` can skip rendering the
               whole unit when it's scrolled off-screen. With virtualization
               disabled in expanded mode the item-row count is the full
               visibleItems count, so this is how we keep layout cost
               bounded to the viewport. -->
          <div class="item-block">
          <div class="item-row" class:is-selected={isSelected} class:is-expanded={showTracks}>
            <div class="col-idx">
              {#if selectMode}
                <input
                  type="checkbox"
                  class="row-checkbox"
                  checked={isSelected}
                  onclick={(e) => {
                    e.stopPropagation();
                    e.preventDefault();
                    toggleSelect(item.position, e);
                  }}
                  aria-label={$t('collectionDetail.selectItem') || 'Select item'}
                />
              {:else}
                <span class="row-index">{item.position + 1}</span>
              {/if}
            </div>

            <div class="col-item">
              <div class="artwork-wrap">
                {#if artworkSrc}
                  <img
                    class="artwork"
                    use:cachedSrc={smallQobuzArtwork(artworkSrc, 50) ?? artworkSrc}
                    alt=""
                    loading="lazy"
                    decoding="async"
                  />
                {:else}
                  <div class="artwork artwork-placeholder">
                    {#if item.item_type === 'track'}
                      <Music2 size={18} />
                    {:else if item.item_type === 'playlist'}
                      <ListMusic size={18} />
                    {:else}
                      <Disc size={18} />
                    {/if}
                  </div>
                {/if}
                <button
                  type="button"
                  class="row-play-overlay"
                  onclick={(e) => { e.stopPropagation(); onPlayItem?.(item); }}
                  aria-label={$t('actions.play')}
                  title={$t('actions.play')}
                >
                  <Play size={16} fill="currentColor" />
                </button>
              </div>
              <div class="item-meta">
                <button
                  type="button"
                  class="item-title item-link"
                  onclick={() => onOpenItem?.(item.source, item.item_type, item.source_item_id)}
                  title={item.title}
                >
                  {item.title}
                </button>
                {#if item.subtitle}
                  {#if item.source === 'qobuz'}
                    <button
                      type="button"
                      class="item-subtitle item-link"
                      onclick={() => onOpenArtist?.(item.source, item.subtitle ?? '')}
                      title={item.subtitle}
                    >
                      {item.subtitle}
                    </button>
                  {:else}
                    <div class="item-subtitle">{item.subtitle}</div>
                  {/if}
                {/if}
              </div>
            </div>

            <div class="col-type">
              <span class="type-cell">
                {#if item.item_type === 'album'}
                  <Disc size={13} />
                {:else if item.item_type === 'track'}
                  <Music2 size={13} />
                {:else}
                  <ListMusic size={13} />
                {/if}
                <span class="type-label">{displayedTypeLabel(item)}</span>
              </span>
            </div>

            <div class="col-source">
              <div
                class="source-indicator"
                title={resolved.kind === 'plex'
                  ? $t('library.plexTrackIndicator')
                  : resolved.kind === 'qobuz'
                    ? $t('library.qobuzTrackIndicator')
                    : $t('library.localTrackIndicator')}
              >
                {#if resolved.kind === 'plex'}
                  <span class="plex-indicator-icon" aria-hidden="true"></span>
                {:else if resolved.kind === 'qobuz'}
                  <span class="qobuz-indicator-icon" aria-hidden="true"></span>
                {:else}
                  <span class="local-indicator-icon" aria-hidden="true"></span>
                {/if}
              </div>
            </div>

            <div class="col-quality">
              <QualityBadgeStatic
                bitDepth={resolved.bitDepth ?? undefined}
                samplingRate={resolved.sampleRateKhz ?? undefined}
                format={resolved.format ?? undefined}
              />
            </div>

            <div class="col-tracks">{itemTracks(item)}</div>
            <div class="col-year">{itemYear(item)}</div>

            <div class="col-menu">
              <button
                class="icon-action small"
                onclick={() => {
                  if (openItemMenu === item.position) {
                    openItemMenu = null;
                    openTypeSubmenu = null;
                  } else {
                    openItemMenu = item.position;
                    openTypeSubmenu = null;
                  }
                }}
                aria-label={$t('collectionDetail.itemActions')}
              >
                <MoreHorizontal size={14} />
              </button>
              {#if openItemMenu === item.position}
                <div
                  class="overflow-backdrop"
                  onclick={() => { openItemMenu = null; openTypeSubmenu = null; }}
                  role="presentation"
                ></div>
                <div class="overflow-menu item-menu" role="menu">
                  <button
                    class="overflow-item"
                    onclick={() => { onPlayItem?.(item); openItemMenu = null; openTypeSubmenu = null; }}
                  >
                    <Play size={13} />
                    <span>{$t('actions.play')}</span>
                  </button>
                  <button
                    class="overflow-item"
                    onclick={() => { onPlayItemNext?.(item); openItemMenu = null; openTypeSubmenu = null; }}
                  >
                    <span>{$t('actions.playNext')}</span>
                  </button>
                  <button
                    class="overflow-item"
                    onclick={() => { onAddItemToQueueLater?.(item); openItemMenu = null; openTypeSubmenu = null; }}
                  >
                    <span>{$t('actions.addToQueue')}</span>
                  </button>
                  <div class="overflow-divider"></div>
                  {#if item.item_type === 'album'}
                    <button
                      class="overflow-item with-trailing"
                      onclick={() => (openTypeSubmenu = openTypeSubmenu === item.position ? null : item.position)}
                    >
                      <span>{$t('collectionDetail.changeReleaseType')}</span>
                      <ChevronRight size={13} />
                    </button>
                    {#if openTypeSubmenu === item.position}
                      {@const currentType = currentReleaseTypeFor(item)}
                      <div class="submenu">
                        {#each RELEASE_TYPE_CHOICES as choice}
                          <button
                            class="overflow-item with-leading"
                            class:selected={currentType === choice}
                            onclick={() => {
                              setReleaseTypeOverride(item.source, item.source_item_id, choice);
                              openTypeSubmenu = null;
                              openItemMenu = null;
                            }}
                          >
                            {#if currentType === choice}
                              <Check size={12} />
                            {:else}
                              <span class="check-placeholder"></span>
                            {/if}
                            <span>{$t(`discographyBuilder.releaseType.${choice}`)}</span>
                          </button>
                        {/each}
                        {#if hasReleaseTypeOverride(item.source, item.source_item_id)}
                          <div class="submenu-divider"></div>
                          <button
                            class="overflow-item with-leading reset"
                            onclick={() => {
                              clearReleaseTypeOverride(item.source, item.source_item_id);
                              openTypeSubmenu = null;
                              openItemMenu = null;
                            }}
                          >
                            <RotateCcw size={12} />
                            <span>{$t('discographyBuilder.typeOverrideReset')}</span>
                          </button>
                        {/if}
                      </div>
                    {/if}
                    <div class="overflow-divider"></div>
                  {/if}
                  <button
                    class="overflow-item destructive"
                    onclick={() => handleRemoveItem(item.position)}
                  >
                    <Trash2 size={13} />
                    <span>{$t('collectionDetail.removeItem')}</span>
                  </button>
                </div>
              {/if}
            </div>
          </div>

          {#if showTracks}
            {@const tracksForRow = filteredTracksFor(item)}
            <div class="expanded-tracks">
              {#if isExpandLoading && !expandedTracks[key]}
                <div class="expanded-empty">
                  <LoaderCircle size={14} class="spin" />
                </div>
              {:else if tracksForRow.length === 0}
                <div class="expanded-empty">{$t('search.noResults')}</div>
              {:else}
                {#each tracksForRow as et}
                  <TrackRow
                    trackId={et.id}
                    number={et.number}
                    title={et.title}
                    artist={et.artist}
                    duration={et.durationStr}
                    quality={et.quality}
                    explicit={et.parental_warning === true}
                    isLocal={et.isLocal === true}
                    localSource={et.localSource ?? 'local'}
                    hideDownload={true}
                    hideFavorite={et.isLocal === true}
                    onPlay={() => onPlayTrackFromItem?.(item, et.id)}
                    menuActions={{
                      onPlayNow: () => onPlayTrackFromItem?.(item, et.id),
                      onPlayNext: () => onPlayTrackNext?.(et.id),
                      onPlayLater: () => onPlayTrackLater?.(et.id),
                      onGoToAlbum: () => onOpenItem?.(item.source, item.item_type, item.source_item_id),
                    }}
                  />
                {/each}
              {/if}
            </div>
          {/if}
          </div>
        {/each}
        <div class="virtual-spacer" style:height="{activeBottomSpacer}px"></div>
      </div>
      {/if}
      </div>

      <BulkActionBar
        count={selectedPositions.size}
        onPlayNext={() => {
          onBulkPlayNext?.(selectedItems());
        }}
        onPlayLater={() => {
          onBulkPlayLater?.(selectedItems());
        }}
        onAddToPlaylist={() => {
          onBulkAddToPlaylist?.(selectedItems());
        }}
        onAddToMixtape={handleBulkAddToMixtape}
        onRemoveFromCollection={handleBulkRemove}
        onClearSelection={clearSelection}
      />
    {/if}

    <!-- Rename modal -->
    {#if renameModalOpen}
      <div class="m-backdrop" onclick={() => (renameModalOpen = false)} role="presentation"></div>
      <div class="m-modal" role="dialog">
        <h2>{$t('collectionDetail.rename')}</h2>
        <input type="text" bind:value={draftName} maxlength="80" class="m-input" />
        <div class="m-footer">
          <button class="m-btn-secondary" onclick={() => (renameModalOpen = false)}>Cancel</button>
          <button
            class="m-btn-primary"
            onclick={submitRename}
            disabled={!draftName.trim()}
          >Save</button>
        </div>
      </div>
    {/if}

    <!-- Description modal -->
    {#if descriptionModalOpen}
      <div
        class="m-backdrop"
        onclick={() => (descriptionModalOpen = false)}
        role="presentation"
      ></div>
      <div class="m-modal" role="dialog">
        <h2>{$t('collectionDetail.editDescription')}</h2>
        <textarea
          bind:value={draftDescription}
          maxlength="400"
          class="m-input"
          rows="4"
        ></textarea>
        <div class="m-footer">
          <button class="m-btn-secondary" onclick={() => (descriptionModalOpen = false)}>
            Cancel
          </button>
          <button class="m-btn-primary" onclick={submitDescription}>Save</button>
        </div>
      </div>
    {/if}

    <!-- Delete confirm -->
    {#if confirmDeleteOpen}
      <div
        class="m-backdrop"
        onclick={() => (confirmDeleteOpen = false)}
        role="presentation"
      ></div>
      <div class="m-modal" role="dialog">
        <h2>{$t('collectionDetail.delete')}</h2>
        <p>
          {$t('collectionDetail.deleteConfirm', { values: { name: collection.name } })}
        </p>
        <div class="m-footer">
          <button class="m-btn-secondary" onclick={() => (confirmDeleteOpen = false)}>
            Cancel
          </button>
          <button class="m-btn-destructive" onclick={handleDelete}>
            {$t('collectionDetail.delete')}
          </button>
        </div>
      </div>
    {/if}

    <!-- DJ-Mix random-track sampler modal -->
    <TrackMixModal
      open={mixModalOpen}
      collectionId={collectionId}
      totalRawTracks={collection.items.length}
      onClose={() => (mixModalOpen = false)}
      onConfirm={handleConfirmMix}
    />
  {/if}
</div>

<style>
  /* Canonical scroll pattern + root-view padding that matches AlbumDetailView
     so the Back button + hero line up with the rest of the app. */
  .detail-view {
    width: 100%;
    height: 100%;
    padding: 8px 8px 100px 18px;
    color: var(--text-primary);
    box-sizing: border-box;
    overflow-y: auto;
    position: relative;
  }

  /* Mirror of AlbumDetailView's .back-btn — borderless, text + icon, muted. */
  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    font-family: inherit;
    padding: 0;
    margin-top: 8px;
    margin-bottom: 24px;
    transition: color 150ms ease;
  }
  .back-btn:hover {
    color: var(--text-secondary);
  }

  .loading,
  .empty-list,
  .not-found {
    padding: 40px;
    color: var(--text-muted);
    text-align: center;
  }

  /* ── Header ── */
  .detail-header {
    margin-bottom: 32px;
  }

  /* Hero sizing homologated to PlaylistDetailView so Collection / Mixtape
     detail views share the same visual weight as the other detail
     views (playlist, album). Cover 186×186, title 24px, uppercase tag
     12px, 32px gap between cover and metadata. */
  .header-content {
    display: flex;
    gap: 32px;
    align-items: flex-end;
  }

  .header-cover {
    flex-shrink: 0;
  }

  .header-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 8px;
  }

  .eyebrow {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .kind-tag {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .title {
    margin: 0;
    font-size: 24px;
    font-weight: 700;
    line-height: 1.2;
    word-wrap: break-word;
  }

  .description {
    margin: 0;
    color: var(--text-secondary);
    font-size: 14px;
    line-height: 1.4;
    max-width: 720px;
  }

  .meta {
    font-size: 13px;
    color: var(--text-muted);
    margin-top: 4px;
  }

  .header-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-top: 12px;
  }

  /* Play/Shuffle/More in the hero use the global .action-btn-circle family
     (see src/app.css). Canonical pattern — same treatment as AlbumDetailView. */

  /* Source indicator — same monochrome masked-svg treatment as
     DiscographyBuilder (and TrackRow's .local-indicator in LocalLibrary).
     Live-resolved kind is one of: qobuz | plex | local. */
  /* Spin keyframe for the Play/Shuffle loading spinner (LoaderCircle icon).
     Scoped-global so the svg class from lucide-svelte picks it up. */
  :global(.spin) {
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Sort + filter + search controls — same dropdown pattern as
     LocalLibraryView / PlaylistDetailView. */
  /* Toolbar is pinned to the top of the scroller so Search / Filter / Sort /
     view-mode / Select stay reachable while the hero scrolls away. The
     hero (cover + name + description + CTAs) remains in normal flow above.
     z-index sits under the overflow menu backdrops (which use inline fixed
     positioning) but above row hover states. */
  .sticky-toolbar {
    position: sticky;
    top: 0;
    z-index: 20;
    background: var(--bg-primary, #0b0b0b);
    /* box-shadow extends the opaque --bg-primary 8px above and 8px below the
       toolbar's border-box:
         •  -8px up: covers the scroller's padding-top: 8px zone where rows
            scrolling above the sticky used to peek through.
         •  +8px down: cushions any sub-pixel / next-element margin seam. */
    box-shadow:
      0 -8px 0 0 var(--bg-primary, #0b0b0b),
      0 8px 0 0 var(--bg-primary, #0b0b0b);
    padding-top: 4px;
    padding-bottom: 4px;
    margin: 0 -18px 0 -18px; /* negative margin so the sticky band extends
                               past .detail-view's left/right padding */
    padding-left: 18px;
    padding-right: 8px;
  }

  /* Item list anchor — measures its own offsetTop inside the scroller so the
     virtual window can convert scrollTop → local offset even when the hero
     has variable height. */
  .list-anchor {
    position: relative;
  }

  /* Virtual spacer — reserves scroll height for items that are unmounted.
     Using `content-visibility: auto` lets the browser skip painting this
     empty region entirely. */
  .virtual-spacer {
    flex-shrink: 0;
    content-visibility: auto;
  }

  .list-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 10px;
    padding: 0 12px;
    flex-wrap: wrap;
  }

  /* Search box matches the canonical `.search-container` look used across
     the app — input embedded with a leading magnifying glass and a trailing
     clear button that appears when the query is non-empty. */
  .search-box {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-muted);
    min-width: 220px;
    max-width: 320px;
    flex: 0 1 260px;
  }
  .search-box.has-query {
    border-color: var(--accent-primary);
    color: var(--accent-primary);
  }
  .search-input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 12px;
    padding: 0;
  }
  .search-input::placeholder {
    color: var(--text-muted);
  }
  .search-clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 3px;
  }
  .search-clear:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .dropdown-container {
    position: relative;
  }
  .control-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 12px;
    cursor: pointer;
    transition: background 120ms ease, color 120ms ease;
  }
  .control-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .control-btn.active {
    color: var(--accent-primary);
    border-color: var(--accent-primary);
  }
  .control-btn.subtle {
    background: transparent;
    border-color: transparent;
    padding: 6px;
  }
  .sort-indicator {
    color: var(--text-muted);
    font-size: 11px;
  }
  .control-backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
  }
  .control-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 101;
    min-width: 160px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 4px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.35);
    display: flex;
    flex-direction: column;
  }
  .control-menu-item {
    display: inline-flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 8px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    border-radius: 4px;
  }
  .control-menu-item:hover {
    background: var(--bg-hover);
  }
  .control-menu-item.selected {
    color: var(--accent-primary);
  }
  .control-menu.wide {
    min-width: 200px;
  }
  .filter-section-label {
    padding: 6px 8px 2px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.8px;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .filter-section-divider {
    height: 1px;
    background: var(--bg-tertiary);
    margin: 4px 0;
  }
  .filter-count {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    font-size: 10px;
    font-weight: 600;
    background: var(--accent-primary);
    color: #fff;
    border-radius: 10px;
    margin-left: 2px;
  }

  /* View-mode segmented group — three buttons packed tight with shared borders. */
  .view-mode-group {
    display: inline-flex;
    margin-left: auto;
  }
  .view-mode-group .control-btn.seg {
    padding: 6px 8px;
    border-radius: 0;
    border-right-width: 0;
  }
  .view-mode-group .control-btn.seg:first-child {
    border-top-left-radius: 6px;
    border-bottom-left-radius: 6px;
  }
  .view-mode-group .control-btn.seg:last-child {
    border-top-right-radius: 6px;
    border-bottom-right-radius: 6px;
    border-right-width: 1px;
  }
  .view-mode-group .control-btn.seg.active {
    background: var(--accent-primary);
    color: #fff;
    border-color: var(--accent-primary);
  }

  /* Grid view — auto-fill tiles mirroring AlbumCard proportions. */
  .item-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 20px;
    padding: 0 12px;
  }
  .grid-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border-radius: 8px;
    cursor: pointer;
    transition: background 120ms ease;
    background: transparent;
    /* Virtualization already clips the rendered slice to viewport +
       buffer; doubling up with content-visibility:auto caused cards
       to re-eject and re-paint on scroll-back, which read as a black
       flash over already-seen content. */
  }
  .grid-card:hover {
    background: var(--bg-hover);
  }
  .grid-card.is-selected {
    background: var(--bg-hover);
    outline: 2px solid var(--accent-primary);
  }
  .grid-artwork-wrap {
    position: relative;
    width: 100%;
    aspect-ratio: 1 / 1;
    border-radius: 6px;
    overflow: hidden;
    background: var(--bg-tertiary);
  }
  /* Loading spinner behind the <img>. The wrap has overflow:hidden +
     dark background; while the img is still fetching (src set, no
     bytes yet) the img is transparent and the spinner shows through.
     As soon as the image paints, its opaque pixels cover the pseudo-
     element — no JS state, no onload handler, no per-card re-render. */
  .grid-artwork-wrap::after {
    content: '';
    position: absolute;
    top: 50%;
    left: 50%;
    width: 24px;
    height: 24px;
    margin: -12px 0 0 -12px;
    border: 2px solid var(--border-subtle);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: grid-artwork-spin 0.8s linear infinite;
    pointer-events: none;
    z-index: 0;
  }
  .grid-artwork,
  .grid-artwork-placeholder {
    position: relative;
    z-index: 1;
  }
  .grid-artwork {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .grid-artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    background: var(--bg-tertiary);
  }

  @keyframes grid-artwork-spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
  .grid-play-overlay {
    position: absolute;
    inset: 0;
    display: none;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
    color: #fff;
    border: none;
    cursor: pointer;
  }
  .grid-card:hover .grid-play-overlay {
    display: flex;
  }
  .grid-checkbox {
    position: absolute;
    top: 8px;
    left: 8px;
    width: 18px;
    height: 18px;
    accent-color: var(--accent-primary);
    background: rgba(0, 0, 0, 0.55);
    border-radius: 3px;
    cursor: pointer;
  }
  .grid-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grid-subtitle {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .source-indicator {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    opacity: 0.9;
  }
  .plex-indicator-icon,
  .local-indicator-icon {
    width: 14px;
    height: 14px;
    background-color: var(--accent-primary);
  }
  .plex-indicator-icon {
    -webkit-mask: url('/plex-mono.svg') center / contain no-repeat;
    mask: url('/plex-mono.svg') center / contain no-repeat;
  }
  .local-indicator-icon {
    -webkit-mask: url('/hdd.svg') center / contain no-repeat;
    mask: url('/hdd.svg') center / contain no-repeat;
  }
  .qobuz-indicator-icon {
    width: 16px;
    height: 16px;
    background-color: var(--accent-primary);
    -webkit-mask: url('/qobuz-logo.svg') center / contain no-repeat;
    mask: url('/qobuz-logo.svg') center / contain no-repeat;
  }

  /* Static quality badge — visually identical to QualityBadge's full mode but
     not reactive to now-playing. List rows show the stored quality of each
     item, not the currently-decoded/hardware state. */
  .col-quality {
    display: inline-flex;
    align-items: center;
    justify-content: flex-start;
    min-width: 0;
  }

  /* Small icon button (used on row ⋯ menu trigger). */
  .icon-action.small {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    color: var(--text-muted);
    border: none;
    border-radius: 4px;
    cursor: pointer;
  }
  .icon-action.small:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* ── Overflow menu (shared by header ⋯ and row ⋯) ── */
  .overflow-wrap {
    position: relative;
  }

  .overflow-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
  }

  .overflow-menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    min-width: 200px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    z-index: 51;
  }

  .overflow-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    cursor: pointer;
    text-align: left;
    border-radius: 6px;
  }
  .overflow-item:hover {
    background: var(--bg-hover);
  }
  .overflow-item.destructive {
    color: var(--error, #e57373);
  }

  .item-menu {
    min-width: 180px;
  }

  /* "Change release type ▸" row layout — label left, chevron right. */
  .overflow-item.with-trailing {
    justify-content: space-between;
  }
  .overflow-item.with-leading .check-placeholder {
    display: inline-block;
    width: 12px;
    height: 12px;
    flex-shrink: 0;
  }
  .overflow-item.with-leading.selected {
    color: var(--accent-primary);
  }
  .overflow-item.with-leading.reset {
    color: var(--text-muted);
  }

  .submenu {
    display: flex;
    flex-direction: column;
    margin: 4px 8px 4px 16px;
    padding: 4px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
  }
  .submenu .overflow-item {
    font-size: 12px;
    padding: 6px 8px;
  }
  .submenu-divider {
    height: 1px;
    background: var(--bg-tertiary);
    margin: 4px 0;
  }
  .overflow-divider {
    height: 1px;
    background: var(--bg-tertiary);
    margin: 4px 0;
  }

  /* ── Item list — matches existing track-list vocabulary ── */
  .item-list {
    border-top: 1px solid var(--bg-tertiary);
  }

  /* Wrapper around each item-row + expanded-tracks block.
     content-visibility: auto tells the engine to skip rendering the
     whole unit when it's off-screen. For expanded mode (where we
     render every item so the scrollbar reaches the real end) this
     substitutes manual virtualization with native skipping, so even
     a 50-item collection fully expanded stays fluid.
     The intrinsic hint is the collapsed row height (56px) — when
     a row becomes visible and gets expanded, the browser measures
     the real height and the scrollbar adjusts. */
  .item-block {
    content-visibility: auto;
    contain-intrinsic-size: 100% 56px;
  }

  .item-list-header,
  .item-row {
    display: grid;
    grid-template-columns: 40px 1fr 140px 80px 160px 72px 60px 40px;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
  }

  .item-list-header {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--text-muted);
    border-bottom: 1px solid var(--bg-tertiary);
    padding-top: 12px;
    padding-bottom: 10px;
  }

  /* Fixed row height — MUST match LIST_ROW_HEIGHT in the script (56). The
     virtualization math uses that constant to compute top/bottom spacer
     heights; any mismatch between the constant and the actual rendered
     row height compounds over the list and causes the scrollbar to report
     a scrollHeight that doesn't line up with the real end of the content,
     which shows up as stuttering / inability to reach the last row.
     Use explicit height (not min-height) so rows can't bulge when a title
     or subtitle wraps. Overflow hidden clips the rare long string. */
  .item-row {
    position: relative;
    height: 56px;
    box-sizing: border-box;
    overflow: hidden;
  }
  .item-row:not(:last-child) {
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  }
  .item-row:hover {
    background: var(--bg-hover);
  }

  .col-idx {
    color: var(--text-muted);
    font-size: 13px;
    text-align: center;
    position: relative;
  }

  /* Selection checkbox — only rendered when selectMode is ON (hero toggle).
     Mirrors the ArtistDetailView multi-select pattern: default state shows
     index + expand chevron; entering select mode swaps that for a checkbox. */
  .row-checkbox {
    margin: 0;
    width: 15px;
    height: 15px;
    accent-color: var(--accent-primary);
    cursor: pointer;
  }
  .item-row.is-selected {
    background: var(--bg-hover);
  }

  /* Expanded-mode tracks: render real TrackRow components. Only the
     container + empty/loading placeholder stays local here — TrackRow
     handles its own internal layout, quality badge, play overlay, menu. */
  .expanded-tracks {
    display: flex;
    flex-direction: column;
    margin: 4px 12px 8px 52px;
    padding: 4px 0;
  }
  .expanded-empty {
    padding: 12px;
    color: var(--text-muted);
    font-size: 12px;
    text-align: center;
  }

  .col-item {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  /* Artwork wrapper hosts the play overlay (same pattern as ArtistDetailView
     popular tracks .track-play-overlay). */
  .artwork-wrap {
    position: relative;
    width: 36px;
    height: 36px;
    flex-shrink: 0;
    border-radius: 4px;
    overflow: hidden;
  }

  .artwork {
    width: 36px;
    height: 36px;
    object-fit: cover;
    border-radius: 4px;
    display: block;
  }

  .artwork-placeholder {
    background: var(--bg-tertiary);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .row-play-overlay {
    position: absolute;
    inset: 0;
    display: none;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
    border: none;
    cursor: pointer;
    transition: background 150ms ease;
    z-index: 2;
  }
  .item-row:hover .row-play-overlay {
    display: flex;
  }
  .row-play-overlay:hover {
    background: rgba(0, 0, 0, 0.78);
  }

  .item-meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 2px;
  }

  .item-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .item-subtitle {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Clickable title / artist buttons inside item-meta. No visual weight of
     their own; the underline on hover signals the link affordance. */
  .item-link {
    background: none;
    border: none;
    padding: 0;
    margin: 0;
    font-family: inherit;
    font-size: inherit;
    font-weight: inherit;
    color: inherit;
    text-align: left;
    cursor: pointer;
    display: block;
    max-width: 100%;
  }
  .item-link:hover {
    color: var(--accent-primary);
    text-decoration: underline;
  }

  .type-cell {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-muted);
    font-size: 11px;
  }

  .type-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .col-tracks,
  .col-year {
    text-align: right;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .col-menu {
    display: flex;
    justify-content: flex-end;
    position: relative;
  }

  /* ── Inline modals (rename / description / delete confirm) ── */
  .m-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    z-index: 9998;
  }

  .m-modal {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 420px;
    max-width: 90vw;
    padding: 24px;
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    z-index: 9999;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .m-modal h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 700;
  }

  .m-modal p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 14px;
  }

  .m-input {
    width: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    font-family: inherit;
  }

  textarea.m-input {
    resize: vertical;
    min-height: 80px;
  }

  .m-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .m-btn-primary,
  .m-btn-secondary,
  .m-btn-destructive {
    padding: 10px 20px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }

  .m-btn-primary {
    background: var(--accent-primary);
    color: #fff;
    border: none;
  }
  .m-btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .m-btn-secondary {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
  }

  .m-btn-destructive {
    background: var(--error, #e57373);
    color: #fff;
    border: none;
  }
</style>
