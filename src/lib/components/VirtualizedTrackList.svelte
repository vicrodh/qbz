<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import TrackRow from './TrackRow.svelte';
  import type { OfflineCacheStatus } from '$lib/stores/offlineCacheState';

  // Use generic types to match whatever caller passes
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  type Track = any;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  type TrackSection = any;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  type TrackGroup = any;

  type VirtualItem =
    | { type: 'group-header'; group: TrackGroup; height: number }
    | { type: 'disc-header'; label: string; height: number }
    | { type: 'track'; track: Track; index: number; height: number };

  interface Props {
    groups: TrackGroup[];
    groupingEnabled: boolean;
    groupMode: 'album' | 'artist' | 'name';
    activeTrackId?: number | null;
    isPlaybackActive: boolean;
    formatDuration: (secs: number) => string;
    getQualityBadge: (track: Track) => string;
    buildAlbumSections: (tracks: Track[]) => TrackSection[];
    onTrackPlay: (track: Track) => void | Promise<void>;
    onArtistClick?: (artist: string) => void;
    onAlbumClick?: (track: Track) => void;
    onTrackPlayNext?: (track: Track) => void;
    onTrackPlayLater?: (track: Track) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    // Optional accessors for different track data structures (e.g., FavoriteTrack vs LocalTrack)
    getTrackId?: (track: Track) => number;
    getTrackNumber?: (track: Track, index: number) => number;
    getTrackTitle?: (track: Track) => string;
    getTrackArtist?: (track: Track) => string | undefined;
    getTrackDuration?: (track: Track) => number;
    getTrackAlbumKey?: (track: Track) => string | undefined;
    getTrackAlbum?: (track: Track) => string | undefined;
    // Whether to show the album column (false when grouping by album to avoid redundancy)
    showAlbum?: boolean;
    // Optional props for non-local tracks (Favorites, Search, etc.)
    isLocal?: boolean;
    hideDownload?: boolean;
    hideFavorite?: boolean;
    isFavoriteOverride?: boolean;
    // Download status support for Favorites/Search
    getOfflineCacheStatus?: (trackId: number) => { status: OfflineCacheStatus; progress: number };
    onDownload?: (track: Track) => void;
    onRemoveDownload?: (trackId: number) => void;
    // Additional menu actions for Favorites
    onShareQobuz?: (trackId: number) => void;
    onShareSonglink?: (track: Track) => void;
    onGoToAlbum?: (albumId: string) => void;
    onGoToArtist?: (artistId: number) => void;
    onShowInfo?: (trackId: number) => void;
    onReDownload?: (track: Track) => void;
    // For getting artist/album IDs from non-local tracks
    getArtistId?: (track: Track) => number | undefined;
    getAlbumId?: (track: Track) => string | undefined;
  }

  let {
    groups,
    groupingEnabled,
    groupMode,
    activeTrackId,
    isPlaybackActive,
    formatDuration,
    getQualityBadge,
    buildAlbumSections,
    onTrackPlay,
    onArtistClick,
    onAlbumClick,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddToPlaylist,
    // Optional accessors with defaults for LocalTrack structure
    getTrackId = (t: Track) => t.id,
    getTrackNumber = (t: Track, idx: number) => t.track_number ?? idx + 1,
    getTrackTitle = (t: Track) => t.title,
    getTrackArtist = (t: Track) => t.artist,
    getTrackDuration = (t: Track) => t.duration_secs,
    getTrackAlbumKey = (t: Track) => t.album_group_key,
    getTrackAlbum = (t: Track) => t.album,
    showAlbum = false,
    // Non-local defaults
    isLocal = true,
    hideDownload = true,
    hideFavorite = true,
    isFavoriteOverride,
    getOfflineCacheStatus,
    onDownload,
    onRemoveDownload,
    onShareQobuz,
    onShareSonglink,
    onGoToAlbum,
    onGoToArtist,
    onShowInfo,
    onReDownload,
    getArtistId,
    getAlbumId,
  }: Props = $props();

  // Constants
  const GROUP_HEADER_HEIGHT = 56; // px
  const DISC_HEADER_HEIGHT = 32; // px
  const TRACK_ROW_HEIGHT = 56; // px
  const BUFFER_ITEMS = 5; // Consistent with VirtualizedAlbumList and VirtualizedArtistGrid

  // State
  let containerEl: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let containerHeight = $state(0);

  // Computed: flatten groups into virtual items with cumulative positions
  let virtualItems = $derived.by(() => {
    const items: (VirtualItem & { top: number; groupId?: string })[] = [];
    let currentTop = 0;

    for (const group of groups) {
      // Add group header if grouping is enabled
      if (groupingEnabled && group.title) {
        items.push({
          type: 'group-header',
          group,
          height: GROUP_HEADER_HEIGHT,
          top: currentTop,
          groupId: group.id,
        });
        currentTop += GROUP_HEADER_HEIGHT;
      }

      // For album grouping, we need disc headers
      if (groupingEnabled && groupMode === 'album') {
        const sections = buildAlbumSections(group.tracks);
        const showDiscHeaders = sections.length > 1;

        for (const section of sections) {
          if (showDiscHeaders) {
            items.push({
              type: 'disc-header',
              label: section.label,
              height: DISC_HEADER_HEIGHT,
              top: currentTop,
            });
            currentTop += DISC_HEADER_HEIGHT;
          }

          for (let i = 0; i < section.tracks.length; i++) {
            items.push({
              type: 'track',
              track: section.tracks[i],
              index: i,
              height: TRACK_ROW_HEIGHT,
              top: currentTop,
            });
            currentTop += TRACK_ROW_HEIGHT;
          }
        }
      } else {
        // Simple case: just tracks
        for (let i = 0; i < group.tracks.length; i++) {
          items.push({
            type: 'track',
            track: group.tracks[i],
            index: i,
            height: TRACK_ROW_HEIGHT,
            top: currentTop,
          });
          currentTop += TRACK_ROW_HEIGHT;
        }
      }
    }

    return items;
  });

  // Computed: total height
  let totalHeight = $derived(
    virtualItems.length > 0
      ? virtualItems[virtualItems.length - 1].top + virtualItems[virtualItems.length - 1].height
      : 0
  );

  // Binary search for first visible item
  function binarySearchStart(items: typeof virtualItems, targetTop: number): number {
    let low = 0;
    let high = items.length - 1;
    let result = 0;

    while (low <= high) {
      const mid = Math.floor((low + high) / 2);
      const item = items[mid];
      if (item.top + item.height > targetTop) {
        result = mid;
        high = mid - 1;
      } else {
        low = mid + 1;
      }
    }
    return result;
  }

  // Binary search for last visible item
  function binarySearchEnd(items: typeof virtualItems, targetBottom: number, startFrom: number): number {
    let low = startFrom;
    let high = items.length - 1;
    let result = high;

    while (low <= high) {
      const mid = Math.floor((low + high) / 2);
      const item = items[mid];
      if (item.top > targetBottom) {
        result = mid;
        high = mid - 1;
      } else {
        low = mid + 1;
      }
    }
    return result;
  }

  // Computed: visible items
  let visibleItems = $derived.by(() => {
    if (virtualItems.length === 0) return [];

    const viewportTop = scrollTop;
    const viewportBottom = scrollTop + containerHeight;

    const firstVisible = binarySearchStart(virtualItems, viewportTop);
    const lastVisible = binarySearchEnd(virtualItems, viewportBottom, firstVisible);

    const startIdx = Math.max(0, firstVisible - BUFFER_ITEMS);
    const endIdx = Math.min(virtualItems.length - 1, lastVisible + BUFFER_ITEMS);

    return virtualItems.slice(startIdx, endIdx + 1);
  });

  // Group ID to scroll position map
  let groupPositions = $derived.by(() => {
    const map = new Map<string, number>();
    for (const item of virtualItems) {
      if (item.groupId) {
        map.set(item.groupId, item.top);
      }
    }
    return map;
  });

  function handleScroll(e: Event) {
    scrollTop = (e.target as HTMLDivElement).scrollTop;
  }

  let resizeObserver: ResizeObserver | null = null;

  onMount(() => {
    if (containerEl) {
      containerHeight = containerEl.clientHeight;

      resizeObserver = new ResizeObserver((entries) => {
        for (const entry of entries) {
          containerHeight = entry.contentRect.height;
        }
      });
      resizeObserver.observe(containerEl);
    }
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
  });

  // Public method to scroll to a group
  export function scrollToGroup(groupId: string) {
    const position = groupPositions.get(groupId);
    if (position !== undefined && containerEl) {
      containerEl.scrollTo({ top: position, behavior: 'smooth' });
    }
  }

  // Unique key generator for items
  function getItemKey(item: typeof virtualItems[0]): string {
    if (item.type === 'group-header') return `group-${item.group.id}`;
    if (item.type === 'disc-header') return `disc-${item.label}-${item.top}`;
    return `track-${item.track.id}`;
  }
</script>

<div class="virtual-container" bind:this={containerEl} onscroll={handleScroll}>
  <div class="virtual-content" style="height: {totalHeight}px;">
    {#each visibleItems as item (getItemKey(item))}
      <div
        class="virtual-item"
        style="transform: translateY({item.top}px); height: {item.height}px;"
      >
        {#if item.type === 'group-header'}
          <div class="track-group-header">
            <div class="track-group-title">{item.group.title}</div>
            {#if item.group.subtitle}
              <div class="track-group-subtitle">{item.group.subtitle}</div>
            {/if}
            <div class="track-group-count">{item.group.tracks.length} tracks</div>
          </div>
        {:else if item.type === 'disc-header'}
          <div class="disc-header">{item.label}</div>
        {:else if item.type === 'track'}
          {@const trackId = getTrackId(item.track)}
          {@const trackArtist = getTrackArtist(item.track)}
          {@const albumKey = getTrackAlbumKey(item.track)}
          {@const albumName = showAlbum ? getTrackAlbum(item.track) : undefined}
          {@const artistId = getArtistId?.(item.track)}
          {@const albumId = getAlbumId?.(item.track)}
          {@const downloadInfo = getOfflineCacheStatus?.(trackId) ?? { status: 'none' as const, progress: 0 }}
          {@const isTrackDownloaded = downloadInfo.status === 'ready'}
          <TrackRow
            trackId={trackId}
            number={getTrackNumber(item.track, item.index)}
            title={getTrackTitle(item.track)}
            artist={trackArtist}
            album={albumName}
            duration={formatDuration(getTrackDuration(item.track))}
            quality={getQualityBadge(item.track)}
            isPlaying={isPlaybackActive && activeTrackId === trackId}
            {isLocal}
            {hideDownload}
            {hideFavorite}
            isFavoriteOverride={isFavoriteOverride}
            downloadStatus={downloadInfo.status}
            downloadProgress={downloadInfo.progress}
            onArtistClick={trackArtist && onArtistClick ? () => onArtistClick(trackArtist) : undefined}
            onAlbumClick={albumKey && onAlbumClick ? () => onAlbumClick(item.track) : undefined}
            onPlay={() => onTrackPlay(item.track)}
            onDownload={onDownload ? () => onDownload(item.track) : undefined}
            onRemoveDownload={onRemoveDownload ? () => onRemoveDownload(trackId) : undefined}
            menuActions={{
              onPlayNow: () => onTrackPlay(item.track),
              onPlayNext: onTrackPlayNext ? () => onTrackPlayNext(item.track) : undefined,
              onPlayLater: onTrackPlayLater ? () => onTrackPlayLater(item.track) : undefined,
              onAddToPlaylist: onTrackAddToPlaylist ? () => onTrackAddToPlaylist(trackId) : undefined,
              onShareQobuz: onShareQobuz ? () => onShareQobuz(trackId) : undefined,
              onShareSonglink: onShareSonglink ? () => onShareSonglink(item.track) : undefined,
              onGoToAlbum: albumId && onGoToAlbum ? () => onGoToAlbum(albumId) : undefined,
              onGoToArtist: artistId && onGoToArtist ? () => onGoToArtist(artistId) : undefined,
              onShowInfo: !isLocal && onShowInfo ? () => onShowInfo(trackId) : undefined,
              onDownload: onDownload ? () => onDownload(item.track) : undefined,
              isTrackDownloaded,
              onReDownload: isTrackDownloaded && onReDownload ? () => onReDownload(item.track) : undefined,
              onRemoveDownload: isTrackDownloaded && onRemoveDownload ? () => onRemoveDownload(trackId) : undefined
            }}
          />
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .virtual-container {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
  }

  .virtual-content {
    position: relative;
    width: 100%;
  }

  .virtual-item {
    position: absolute;
    left: 0;
    right: 0;
    will-change: transform;
  }

  .track-group-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px 0 8px 0;
  }

  .track-group-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .track-group-subtitle {
    font-size: 13px;
    color: var(--text-muted);
  }

  .track-group-count {
    font-size: 12px;
    color: var(--text-muted);
    margin-left: auto;
  }

  .disc-header {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 8px 0;
    border-bottom: 1px solid var(--border-primary);
  }
</style>
