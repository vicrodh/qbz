<script lang="ts">
  // Compact album-detail variant rendered inside the LocalLibrary Folders
  // tab tree-mode right pane. Mirrors the data layout of the full-page
  // local-library album-detail (header + track list) but strips backdrop,
  // sidebar sections, "more by this artist" carousel, and any chrome that
  // would compete with the tree rail on the left. The full-page view
  // (line ~4573 in LocalLibraryView.svelte) remains unchanged for flat
  // mode and direct navigation to a library album.
  //
  // Helpers (formatDuration, getQualityBadge, etc.) are received as props
  // following the same idiom as VirtualizedTrackList — keeps the component
  // free of tight coupling to LocalLibraryView's local helpers without
  // duplicating them.

  import { Disc3, Play, Shuffle, CircleAlert, Search, X, SquareCheckBig, CassetteTape, ListPlus, ListEnd, ListMusic } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { openAddToMixtape } from '$lib/stores/addToMixtapeModalStore';
  import TrackRow from './TrackRow.svelte';
  import ImageLightbox from './ImageLightbox.svelte';
  import { formatTrackTitle } from '$lib/utils/trackTitle';
  import { applyShiftRange, isSelectAllShortcut } from '$lib/utils/multiSelect';

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
    source?: string;
  }

  interface LocalAlbum {
    id: string;
    title: string;
    artist: string;
    all_artists?: string;
    year?: number;
    genre?: string;
    catalog_number?: string;
    artwork_path?: string;
    track_count: number;
    total_duration_secs: number;
    format: string;
    bit_depth?: number;
    sample_rate: number;
    directory_path: string;
    source_folders?: string | null;
    source?: string;
    likely_single_file_album?: boolean;
  }

  interface AlbumSection {
    disc: number;
    label: string;
    tracks: LocalTrack[];
    useIndexNumbering: boolean;
  }

  interface Props {
    album: LocalAlbum;
    tracks: LocalTrack[];
    activeTrackId?: number | null;
    isPlaybackActive?: boolean;
    onPlayAll: () => void;
    onShuffleAll: () => void;
    onTrackPlay: (track: LocalTrack) => void;
    onTrackPlayNext?: (track: LocalTrack) => void;
    onTrackPlayLater?: (track: LocalTrack) => void;
    onTrackAddToPlaylist?: (trackId: number) => void;
    onTrackAddPlexToPlaylist?: (filePath: string) => void;
    onArtistClick?: (name: string) => void;
    /**
     * Bulk handlers — when present, the compact view exposes per-track
     * selection (checkboxes) plus a Play Next / Add to Queue / Add to
     * Playlist row above the track list. Local + Plex split mirrors the
     * full-page album-detail view so each path stays in its own backend
     * namespace. Mixtape add for selection routes through the standard
     * `openAddToMixtape` modal directly (no parent callback needed).
     */
    onBulkPlayNext?: (trackIds: number[]) => void;
    onBulkPlayLater?: (trackIds: number[]) => void;
    onBulkAddToPlaylist?: (trackIds: number[]) => void;
    onBulkAddPlexToPlaylist?: (ratingKeys: string[]) => void;
    /**
     * Right-click on the artwork opens the album-edit modal — same target
     * as the pencil button in the page-level album view. Wired by the
     * parent so the modal state (which lives in `LocalLibraryView`) stays
     * out of this compact view. Single-action context menu: no popover,
     * direct open. Plex write-protection is enforced inside the parent's
     * handler.
     */
    onEditAlbum?: () => void;
    formatDuration: (seconds: number) => string;
    formatTotalDuration: (seconds: number) => string;
    formatBitDepth: (bits?: number) => string;
    formatSampleRate: (hz: number) => string;
    getQualityBadge: (track: LocalTrack) => string;
    isHiRes: (track: LocalTrack) => boolean;
    getFullArtworkUrl: (path?: string) => string;
    buildAlbumSections: (tracks: LocalTrack[]) => AlbumSection[];
    normalizeArtistName: (name: string) => string;
    /**
     * One of the parent's selection contexts (tracks tab, albums tab,
     * folder tree) is currently active. When this flips to `true` while
     * this view has its own local selection mode on, the local
     * selection is cleared — light-switch mutex across the parent/child
     * boundary so two selections can never be live at the same time.
     */
    parentSelectionActive?: boolean;
    /**
     * Called when the user toggles ON this view's local selection mode.
     * Parent uses it to clear its own selection contexts so the
     * footer's BulkActionBar reflects a single source of truth.
     */
    onSelectionEntered?: () => void;
  }

  let {
    album,
    tracks,
    activeTrackId = null,
    isPlaybackActive = false,
    onPlayAll,
    onShuffleAll,
    onTrackPlay,
    onTrackPlayNext,
    onTrackPlayLater,
    onTrackAddToPlaylist,
    onTrackAddPlexToPlaylist,
    onArtistClick,
    onBulkPlayNext,
    onBulkPlayLater,
    onBulkAddToPlaylist,
    onBulkAddPlexToPlaylist,
    onEditAlbum,
    formatDuration,
    formatTotalDuration,
    formatBitDepth,
    formatSampleRate,
    getQualityBadge,
    isHiRes,
    getFullArtworkUrl,
    buildAlbumSections,
    normalizeArtistName,
    parentSelectionActive = false,
    onSelectionEntered,
  }: Props = $props();

  let trackSearchQuery = $state('');

  // Cover context menu — uses the canonical .cover-context-* classes from
  // AlbumDetailView so the menu visually matches the rest of the app.
  // Local-library artwork is a file path, not a remote URL, and cover
  // changes belong to the metadata editor — so this menu only exposes the
  // two actions that apply here: View album art (opens lightbox) and
  // Edit metadata (opens the parent's edit-album modal).
  let lightboxOpen = $state(false);
  let showCoverMenu = $state(false);
  let coverMenuPos = $state({ x: 0, y: 0 });

  // Local selection state — independent from the LocalLibraryView's global
  // tracks-tab `selectedTrackIds` so navigating between albums in the tree
  // doesn't bleed selection across albums. Reset by the album-change
  // $effect below.
  let selectionMode = $state(false);
  let selectedTrackIds = $state(new Set<number>());

  // Reset selection whenever the user navigates to a different album in the
  // tree. We track album.id explicitly (rather than the `tracks` reference)
  // so re-renders that don't change the album leave selection untouched.
  let lastAlbumId: string | null = null;
  $effect(() => {
    if (album.id === lastAlbumId) return;
    lastAlbumId = album.id;
    selectionMode = false;
    selectedTrackIds = new Set();
  });

  function handleViewArtwork() {
    showCoverMenu = false;
    if (album.artwork_path) lightboxOpen = true;
  }

  function handleEditMetadataFromMenu() {
    showCoverMenu = false;
    onEditAlbum?.();
  }

  function toggleSelectionMode() {
    selectionMode = !selectionMode;
    if (!selectionMode) {
      selectedTrackIds = new Set();
      selectionAnchorId = null;
    } else {
      // Notify parent so it can clear its own selection contexts —
      // light-switch mutex with the parent (tracks-tab / albums-tab /
      // folder-tree). The parent's `clearOtherSelectionContexts`
      // handles the cross-context teardown.
      onSelectionEntered?.();
    }
  }

  // Inbound half of the cross-component mutex: when the parent enters
  // any of its own selection contexts, our local selection turns off.
  // No infinite loop because the toggle above only fires
  // `onSelectionEntered` on the ENTRY transition (false -> true).
  $effect(() => {
    if (parentSelectionActive && selectionMode) {
      selectionMode = false;
      selectedTrackIds = new Set();
      selectionAnchorId = null;
    }
  });

  // Anchor for shift-click range selection. Set by every plain
  // (non-shift) click and consumed by the next shift-click. Reset
  // whenever selection mode is exited so a future re-entry starts
  // from a fresh anchor.
  let selectionAnchorId: number | null = null;

  /**
   * Toggle a single track in the selection, with optional shift-range
   * extension. When `shift` is true and an anchor exists, every track
   * between the anchor and `trackId` (inclusive, in current visual
   * order) is added to the selection — matches the tracks-tab behaviour
   * in `LocalLibraryView`. Without shift, this is a plain toggle and
   * the clicked track becomes the new anchor.
   */
  function toggleTrackSelection(trackId: number, shift: boolean = false) {
    if (shift && selectionAnchorId !== null && selectionAnchorId !== trackId) {
      const ids = tracks.map((trk) => trk.id);
      const lastIndex = ids.indexOf(selectionAnchorId);
      const currentIndex = ids.indexOf(trackId);
      if (lastIndex !== -1 && currentIndex !== -1) {
        selectedTrackIds = applyShiftRange({
          current: selectedTrackIds,
          ids,
          lastIndex,
          currentIndex,
        });
        return;
      }
    }
    const next = new Set(selectedTrackIds);
    if (next.has(trackId)) next.delete(trackId);
    else next.add(trackId);
    selectedTrackIds = next;
    selectionAnchorId = trackId;
  }

  function clearSelection() {
    selectedTrackIds = new Set();
    selectionAnchorId = null;
  }

  // Tri-state for the "Select All" checkbox at the top of the track
  // list — 'none' / 'partial' / 'all' based on how many of the
  // currently visible album tracks are in the selection set.
  const selectAllState = $derived(
    tracks.length === 0 ? 'none' as const
    : selectedTrackIds.size === 0 ? 'none' as const
    : tracks.every((trk) => selectedTrackIds.has(trk.id)) ? 'all' as const
    : 'partial' as const
  );

  function toggleSelectAll() {
    if (selectAllState === 'all') {
      selectedTrackIds = new Set();
    } else {
      selectedTrackIds = new Set(tracks.map((trk) => trk.id));
    }
  }

  // Ctrl/Cmd+A while selection mode is active selects every visible
  // track. No-op when focus is in a text input.
  $effect(() => {
    if (!selectionMode) return;
    const handler = (e: KeyboardEvent) => {
      if (!isSelectAllShortcut(e)) return;
      e.preventDefault();
      selectedTrackIds = new Set(tracks.map((trk) => trk.id));
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  });

  /** Resolve the selected IDs back to the full LocalTrack objects so we
   *  can split local vs Plex rows for the playlist add path. */
  function selectedTracks(): LocalTrack[] {
    return tracks.filter((trk) => selectedTrackIds.has(trk.id));
  }

  function handleBulkPlayNext() {
    const ids = Array.from(selectedTrackIds);
    if (ids.length === 0) return;
    onBulkPlayNext?.(ids);
    selectionMode = false;
    selectedTrackIds = new Set();
  }

  function handleBulkPlayLater() {
    const ids = Array.from(selectedTrackIds);
    if (ids.length === 0) return;
    onBulkPlayLater?.(ids);
    selectionMode = false;
    selectedTrackIds = new Set();
  }

  function handleBulkAddToPlaylist() {
    const picked = selectedTracks();
    if (picked.length === 0) return;
    const localIds = picked.filter((trk) => trk.source !== 'plex').map((trk) => trk.id);
    const plexRatingKeys = picked
      .filter((trk) => trk.source === 'plex')
      .map((trk) => trk.file_path);
    if (localIds.length > 0) onBulkAddToPlaylist?.(localIds);
    if (plexRatingKeys.length > 0) onBulkAddPlexToPlaylist?.(plexRatingKeys);
    selectionMode = false;
    selectedTrackIds = new Set();
  }

  function handleAlbumAddToMixtape() {
    openAddToMixtape({
      item_type: 'album',
      source: 'local',
      source_item_id: album.id,
      title: album.title,
      subtitle: album.artist,
      year: album.year,
      track_count: album.track_count,
    });
  }

  function handleBulkAddToMixtape() {
    const picked = selectedTracks();
    if (picked.length === 0) return;
    // Mixtape items are limited to source 'qobuz' | 'local' (see
    // AddToMixtapeItem). All compact-view rows live in `local_tracks` (Plex
    // rows included, since they're tracked there too), so 'local' is correct
    // for both. The numeric track id is the canonical key — same as the
    // single-track TrackRow path in AlbumDetailView.
    openAddToMixtape(
      picked.map((trk) => ({
        item_type: 'track' as const,
        source: 'local' as const,
        source_item_id: String(trk.id),
        title: trk.title,
        subtitle: [trk.artist, trk.album].filter(Boolean).join(' · '),
      }))
    );
    selectionMode = false;
    selectedTrackIds = new Set();
  }

  const filteredTracks = $derived.by(() => {
    const q = trackSearchQuery.trim().toLowerCase();
    if (q === '') return tracks;
    return tracks.filter((track) => {
      const title = track.title?.toLowerCase() ?? '';
      const artist = track.artist?.toLowerCase() ?? '';
      return title.includes(q) || artist.includes(q);
    });
  });

  const albumSections = $derived(buildAlbumSections(filteredTracks));
  const showDiscHeaders = $derived(albumSections.length > 1);
  const isVariousArtists = $derived(
    normalizeArtistName(album.artist) === 'various artists'
  );
  const isSearching = $derived(trackSearchQuery.trim() !== '');
  const noResults = $derived(isSearching && filteredTracks.length === 0);
</script>

<div class="folder-album-view">
  <!-- Compact header: artwork on the left, metadata + actions on the right.
       No artwork backdrop, no back button (the tree on the left is the
       navigation), no edit / search / select chrome — those belong to the
       full-page album-detail view. -->
  <div class="folder-album-header">
    <div
      class="folder-album-artwork"
      onclick={() => { if (album.artwork_path) lightboxOpen = true; }}
      onkeydown={(e) => { if (e.key === 'Enter' && album.artwork_path) lightboxOpen = true; }}
      oncontextmenu={(e) => { e.preventDefault(); coverMenuPos = { x: e.clientX, y: e.clientY }; showCoverMenu = true; }}
      role="button"
      tabindex="0"
    >
      {#if album.artwork_path}
        <img src={getFullArtworkUrl(album.artwork_path)} alt={album.title} loading="lazy" />
      {:else}
        <div class="folder-album-artwork-placeholder">
          <Disc3 size={48} />
        </div>
      {/if}
    </div>
    <div class="folder-album-info">
      <h2 class="folder-album-title" title={album.title}>{album.title}</h2>
      {#if !isVariousArtists && onArtistClick}
        <button
          type="button"
          class="folder-album-artist artist-link"
          onclick={() => onArtistClick?.(album.artist)}
        >
          {album.artist}
        </button>
      {:else}
        <p class="folder-album-artist">{album.artist}</p>
      {/if}
      <p class="folder-album-meta">
        {#if album.year}{album.year} &bull; {/if}
        {album.track_count} {$t('library.tracks').toLowerCase()} &bull;
        {formatTotalDuration(album.total_duration_secs)}
      </p>
      {#if tracks.length > 0}
        {@const firstTrack = tracks[0]}
        <div class="folder-album-specs">
          <span class="folder-album-spec-badge" class:hires={isHiRes(firstTrack)}>
            {firstTrack.format.toUpperCase()}
          </span>
          <span class="folder-album-spec-item">{formatBitDepth(firstTrack.bit_depth)}</span>
          <span class="folder-album-spec-item">{formatSampleRate(firstTrack.sample_rate)}</span>
        </div>
      {/if}
      {#if album.likely_single_file_album}
        <div class="folder-album-single-file-notice">
          <CircleAlert size={12} />
          <span>{$t('library.singleFileAlbumNotice')}</span>
        </div>
      {/if}
      <div class="folder-album-actions">
        <button
          type="button"
          class="action-btn-circle primary"
          onclick={onPlayAll}
          disabled={tracks.length === 0}
          title={$t('actions.playAll')}
          aria-label={$t('actions.playAll')}
        >
          <Play size={20} fill="currentColor" color="currentColor" />
        </button>
        <button
          type="button"
          class="action-btn-circle"
          onclick={onShuffleAll}
          disabled={tracks.length === 0}
          title={$t('actions.shuffle')}
          aria-label={$t('actions.shuffle')}
        >
          <Shuffle size={18} />
        </button>
        <button
          type="button"
          class="action-btn-circle"
          class:is-active={selectionMode}
          onclick={toggleSelectionMode}
          disabled={tracks.length === 0}
          title={selectionMode ? $t('actions.cancelSelection') : $t('actions.select')}
          aria-label={selectionMode ? $t('actions.cancelSelection') : $t('actions.select')}
          aria-pressed={selectionMode}
        >
          <SquareCheckBig size={18} />
        </button>
        {#if selectionMode}
          <label class="folder-album-select-all" title={$t('actions.selectAll')}>
            <input
              type="checkbox"
              checked={selectAllState === 'all'}
              indeterminate={selectAllState === 'partial'}
              onchange={toggleSelectAll}
            />
            <span>{$t('actions.selectAll')}</span>
          </label>
        {/if}
        <button
          type="button"
          class="action-btn-circle"
          onclick={selectionMode && selectedTrackIds.size > 0 ? handleBulkAddToMixtape : handleAlbumAddToMixtape}
          disabled={tracks.length === 0}
          title={$t('common.addToMixtapeOrCollection')}
          aria-label={$t('common.addToMixtapeOrCollection')}
        >
          <CassetteTape size={18} />
        </button>
        {#if selectionMode}
          <button
            type="button"
            class="action-btn-circle"
            onclick={handleBulkPlayNext}
            disabled={selectedTrackIds.size === 0}
            title={$t('actions.playNext')}
            aria-label={$t('actions.playNext')}
          >
            <ListPlus size={18} />
          </button>
          <button
            type="button"
            class="action-btn-circle"
            onclick={handleBulkPlayLater}
            disabled={selectedTrackIds.size === 0}
            title={$t('actions.addToQueue')}
            aria-label={$t('actions.addToQueue')}
          >
            <ListEnd size={18} />
          </button>
          <button
            type="button"
            class="action-btn-circle"
            onclick={handleBulkAddToPlaylist}
            disabled={selectedTrackIds.size === 0}
            title={$t('actions.addToPlaylist')}
            aria-label={$t('actions.addToPlaylist')}
          >
            <ListMusic size={18} />
          </button>
          {#if selectedTrackIds.size > 0}
            <span class="folder-album-selected-count">
              {$t('actions.selectedTracks', { values: { count: selectedTrackIds.size } })}
            </span>
            <button
              type="button"
              class="folder-album-selection-clear"
              onclick={clearSelection}
              title={$t('actions.clearSelection')}
              aria-label={$t('actions.clearSelection')}
            >
              <X size={14} />
            </button>
          {/if}
        {/if}
        <div
          class="folder-album-track-search"
          role="search"
          aria-label={$t('library.foldersTree.searchTracksLabel')}
        >
          <Search size={14} />
          <input
            type="search"
            class="folder-album-track-search-input"
            placeholder={$t('library.foldersTree.searchTracksPlaceholder')}
            bind:value={trackSearchQuery}
            aria-label={$t('library.foldersTree.searchTracksLabel')}
          />
          {#if isSearching}
            <button
              type="button"
              class="folder-album-track-search-clear"
              onclick={() => (trackSearchQuery = '')}
              title={$t('actions.close')}
              aria-label={$t('actions.close')}
            >
              <X size={12} />
            </button>
          {/if}
        </div>
      </div>
    </div>
  </div>

  <!-- Track list. Reuses TrackRow with the same callback wiring as the
       full-page album-detail view so the queue/play/context-menu actions
       behave identically. Selection state is local to this component
       (resets when the user navigates to a different album in the tree)
       so it doesn't bleed into the LocalLibraryView's tracks-tab global
       selection. -->
  <div class="folder-album-tracks">
    <div class="folder-album-tracks-header">
      {#if selectionMode}
        <div class="col-select"></div>
      {/if}
      <div class="col-number">#</div>
      <div class="col-title">{$t('tracklist.title')}</div>
      <div class="col-duration">{$t('tracklist.duration')}</div>
      <div class="col-quality">{$t('tracklist.quality')}</div>
      <div class="col-spacer"></div>
      <div class="col-spacer"></div>
      <div class="col-spacer"></div>
    </div>
    {#each albumSections as section (section.disc)}
      {#if showDiscHeaders}
        <div class="folder-album-disc-header">{section.label}</div>
      {/if}
      {#each section.tracks as track, index (track.id)}
        <TrackRow
          number={section.useIndexNumbering ? index + 1 : (track.track_number || index + 1)}
          title={formatTrackTitle(track)}
          artist={track.artist !== album.artist ? track.artist : undefined}
          duration={formatDuration(track.duration_secs)}
          quality={getQualityBadge(track)}
          isPlaying={isPlaybackActive && activeTrackId === track.id}
          isActiveTrack={activeTrackId === track.id}
          isLocal={true}
          localSource={track.source === 'plex' ? 'plex' : 'local'}
          hideDownload={true}
          hideFavorite={true}
          selectable={selectionMode}
          selected={selectedTrackIds.has(track.id)}
          onToggleSelect={(e) => toggleTrackSelection(track.id, e.shiftKey)}
          onArtistClick={track.artist && track.artist !== album.artist && onArtistClick
            ? () => onArtistClick?.(track.artist)
            : undefined}
          onPlay={() => onTrackPlay(track)}
          menuActions={{
            onPlayNow: () => onTrackPlay(track),
            onPlayNext: onTrackPlayNext ? () => onTrackPlayNext!(track) : undefined,
            onPlayLater: onTrackPlayLater ? () => onTrackPlayLater!(track) : undefined,
            onAddToPlaylist: track.source === 'plex'
              ? (onTrackAddPlexToPlaylist ? () => onTrackAddPlexToPlaylist!(track.file_path) : undefined)
              : (onTrackAddToPlaylist ? () => onTrackAddToPlaylist!(track.id) : undefined)
          }}
        />
      {/each}
    {/each}
    {#if tracks.length === 0}
      <div class="folder-album-empty">
        {$t('library.foldersTree.treeLoading')}
      </div>
    {:else if noResults}
      <div class="folder-album-empty">
        {$t('library.foldersTree.searchNoResults')}
      </div>
    {/if}
  </div>
</div>

<ImageLightbox
  isOpen={lightboxOpen}
  onClose={() => lightboxOpen = false}
  src={album.artwork_path ? getFullArtworkUrl(album.artwork_path) : ''}
  alt={album.title ?? ''}
/>

{#if showCoverMenu}
  <div
    class="cover-context-backdrop"
    onclick={() => showCoverMenu = false}
    onkeydown={(e) => { if (e.key === 'Escape') showCoverMenu = false; }}
    role="button"
    tabindex="-1"
  ></div>
  <div
    class="cover-context-menu"
    style="left: {coverMenuPos.x}px; top: {coverMenuPos.y}px;"
  >
    <button
      class="cover-context-item"
      onclick={handleViewArtwork}
      disabled={!album.artwork_path}
    >
      {$t('library.foldersTree.viewArtwork')}
    </button>
    {#if onEditAlbum}
      <button class="cover-context-item" onclick={handleEditMetadataFromMenu}>
        {$t('metadata.editMetadata')}
      </button>
    {/if}
  </div>
{/if}

<style>
  .folder-album-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow-y: auto;
    padding: 4px 4px 24px 4px;
  }

  /* Compact header: smaller artwork (160px vs 200px in full view), no
     artwork backdrop, action row sits inline with metadata. */
  .folder-album-header {
    display: flex;
    gap: 16px;
    padding: 8px 4px 16px 4px;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 12px;
  }

  .folder-album-artwork {
    width: 160px;
    height: 160px;
    border-radius: 6px;
    overflow: hidden;
    flex-shrink: 0;
    background: var(--bg-secondary);
  }

  .folder-album-artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .folder-album-artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
  }

  .folder-album-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 4px;
  }

  .folder-album-title {
    font-size: 18px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
    line-height: 1.25;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    line-clamp: 2;
  }

  .folder-album-artist {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    margin: 0;
  }

  .folder-album-artist.artist-link {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
    color: var(--accent-primary);
    font-family: inherit;
  }

  .folder-album-artist.artist-link:hover {
    color: var(--text-primary);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .folder-album-meta {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0;
  }

  .folder-album-specs {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
    flex-wrap: wrap;
  }

  .folder-album-spec-badge {
    padding: 2px 8px;
    background: var(--bg-tertiary);
    border-radius: 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .folder-album-spec-badge.hires {
    background: linear-gradient(135deg, #f59e0b 0%, #d97706 100%);
    color: white;
  }

  .folder-album-spec-item {
    font-size: 11px;
    color: var(--text-secondary);
    padding: 2px 6px;
    background: var(--bg-secondary);
    border-radius: 4px;
  }

  .folder-album-single-file-notice {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-radius: 6px;
    padding: 4px 8px;
    max-width: fit-content;
    margin-top: 4px;
  }

  .folder-album-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    flex-wrap: wrap;
  }

  /* Use the global .action-btn-circle / .action-btn-circle.primary classes
     defined in app.css — same circular buttons used in AlbumDetailView,
     TopQView, FavQView, FavoritesView, LabelView. No pills. */

  .folder-album-selected-count {
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-primary);
    white-space: nowrap;
    margin-left: 4px;
  }

  .folder-album-selection-clear {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 50%;
    transition: background 120ms ease, color 120ms ease;
  }

  .folder-album-selection-clear:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  /* Search lives at the far right of the action row regardless of how many
     bulk-action buttons are visible. `margin-left: auto` consumes whatever
     remaining space exists between the last action button and the row's
     right edge so the search input stays pinned right. */
  .folder-album-track-search {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-muted);
    margin-left: auto;
    min-width: 200px;
    max-width: 100%;
    transition: border-color 150ms ease;
  }

  .folder-album-track-search:focus-within {
    border-color: var(--accent-primary);
    color: var(--text-primary);
  }

  .folder-album-track-search-input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 12px;
  }

  .folder-album-track-search-input::placeholder {
    color: var(--text-muted);
  }

  .folder-album-track-search-input::-webkit-search-cancel-button {
    display: none;
  }

  .folder-album-track-search-clear {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 50%;
    transition: background 150ms ease, color 150ms ease;
  }

  .folder-album-track-search-clear:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  /* Track list — reuse TrackRow's grid columns. The header here mirrors
     the full-page album-detail track-list-header so the TrackRow row
     widths line up. */
  .folder-album-tracks {
    display: flex;
    flex-direction: column;
  }

  .folder-album-tracks-header {
    display: grid;
    grid-template-columns: 50px 1fr 80px 100px 32px 32px 32px;
    gap: 8px;
    padding: 6px 12px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    border-bottom: 1px solid var(--bg-tertiary);
    margin-bottom: 4px;
  }

  /* In selection mode the header gains a leading checkbox-width column
     so it visually aligns with the TrackRow's checkbox column. The
     column itself is empty (no select-all affordance in the compact
     view — keeps the header lean). */
  .folder-album-tracks-header:has(.col-select) {
    grid-template-columns: 24px 50px 1fr 80px 100px 32px 32px 32px;
  }

  .folder-album-tracks-header .col-number {
    text-align: center;
  }

  .folder-album-disc-header {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 12px 12px 4px 12px;
  }

  .folder-album-empty {
    padding: 24px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }

  /* Cover context menu — uses the canonical .cover-context-* classes
     from AlbumDetailView so the menu visually matches the rest of the
     app. Artwork is interactive (click opens lightbox, right-click
     opens menu) — signal that with a pointer cursor. */
  .folder-album-artwork {
    cursor: pointer;
  }

  .cover-context-backdrop {
    position: fixed;
    inset: 0;
    z-index: 2999;
  }

  .cover-context-menu {
    position: fixed;
    z-index: 3000;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px;
    min-width: 200px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    animation: coverMenuIn 100ms ease;
  }

  @keyframes coverMenuIn {
    from { opacity: 0; transform: scale(0.95); }
    to { opacity: 1; transform: scale(1); }
  }

  .cover-context-item {
    display: block;
    width: 100%;
    padding: 8px 12px;
    font-size: 13px;
    color: var(--text-primary);
    background: none;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    transition: background 100ms ease;
  }

  .cover-context-item:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .cover-context-item:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Select-all checkbox shown next to the selection-mode toggle in the
     folder album header. Matches the visual rhythm of the other inline
     toolbar items — small, sits beside the round action buttons rather
     than competing with them. */
  .folder-album-select-all {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    user-select: none;
  }

  .folder-album-select-all input[type='checkbox'] {
    width: 14px;
    height: 14px;
    accent-color: var(--accent-primary);
    cursor: pointer;
  }
</style>
