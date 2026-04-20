<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { Play, Disc3, Heart, Check } from 'lucide-svelte';
  import { t, locale } from 'svelte-i18n';
  import AlbumMenu from './AlbumMenu.svelte';
  import { openAddToMixtape } from '$lib/stores/addToMixtapeModalStore';
  import {
    subscribe as subscribeAlbumFavorites,
    isAlbumFavorite,
    isAlbumToggling,
    loadAlbumFavorites,
    toggleAlbumFavorite
  } from '$lib/stores/albumFavoritesStore';
  import { resolveAlbumCover } from '$lib/stores/customAlbumCoverStore';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import SourceBadge from './SourceBadge.svelte';

  interface Props {
    albumId?: string;
    artwork: string;
    title: string;
    artist: string;
    genre?: string;
    releaseDate?: string;
    quality?: string;
    size?: 'standard' | 'large';
    searchId?: string;
    onclick?: () => void;
    onPlay?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onAddAlbumToPlaylist?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onDownload?: () => void;
    showFavorite?: boolean;
    favoriteEnabled?: boolean;
    showGenre?: boolean;
    isAlbumFullyDownloaded?: boolean;
    onOpenContainingFolder?: () => void;
    onReDownloadAlbum?: () => void;
    downloadStateVersion?: number;
    /** Source badge for local library: 'user' | 'qobuz_download' | 'qobuz_purchase' | 'plex' */
    sourceBadge?: 'user' | 'qobuz_download' | 'qobuz_purchase' | 'plex';
    artistId?: number;
    onArtistClick?: (artistId: number) => void;
    /** Editorial ribbon: Qobuz award (id 88 = Qobuzissime,
     *  id 151 = Album of the Week) or a press accolade
     *  (Pitchfork BNM, Rolling Stone 5★, etc.). `label` is the
     *  display string the card renders. */
    ribbon?: { kind: 'qobuzissime' | 'albumOfTheWeek' | 'press'; label: string };
    /** Whether this album is from the local library (determines source field in AddToMixtapeItem) */
    isLocal?: boolean;
    /** Year for AddToMixtapeItem (optional, parsed from releaseDate if not provided) */
    year?: number;
    /** Track count for AddToMixtapeItem */
    trackCount?: number;
    /** Multi-select mode: shows a persistent corner checkbox and routes
     *  clicks through onToggleSelect instead of onclick. */
    selectable?: boolean;
    selected?: boolean;
    onToggleSelect?: () => void;
  }

  let {
    albumId,
    artwork,
    title,
    artist,
    genre,
    releaseDate,
    quality,
    size = 'standard',
    searchId,
    onclick,
    onPlay,
    onPlayNext,
    onPlayLater,
    onAddAlbumToPlaylist,
    onShareQobuz,
    onShareSonglink,
    onDownload,
    showFavorite,
    favoriteEnabled,
    showGenre = true,
    isAlbumFullyDownloaded = false,
    onOpenContainingFolder,
    onReDownloadAlbum,
    downloadStateVersion,
    sourceBadge,
    artistId,
    onArtistClick,
    ribbon,
    isLocal,
    year,
    trackCount,
    selectable = false,
    selected = false,
    onToggleSelect
  }: Props = $props();
  
  const isDownloaded = $derived.by(() => {
    void downloadStateVersion;
    return isAlbumFullyDownloaded;
  });

  let imageError = $state(false);
  const cardSize = $derived(size === 'large' ? 210 : 190);
  let titleRef: HTMLDivElement | null = $state(null);
  let titleTextRef: HTMLSpanElement | null = $state(null);
  let titleOverflow = $state(0);
  let artistRef: HTMLElement | null = $state(null);
  let artistTextRef: HTMLSpanElement | null = $state(null);
  let artistOverflow = $state(0);
  const titleOffset = $derived(titleOverflow > 0 ? `-${titleOverflow + 16}px` : '0px');
  const artistOffset = $derived(artistOverflow > 0 ? `-${artistOverflow + 16}px` : '0px');
  const tickerSpeed = 40;
  const titleDuration = $derived(titleOverflow > 0 ? `${(titleOverflow + 16) / tickerSpeed}s` : '0s');
  const artistDuration = $derived(artistOverflow > 0 ? `${(artistOverflow + 16) / tickerSpeed}s` : '0s');

  let favoriteFromStore = $state(false);
  let isToggling = $state(false);
  const isFavorite = $derived(albumId ? favoriteFromStore : false);
  const hasMenu = $derived(!!(onPlayNext || onPlayLater || onShareQobuz || onShareSonglink || onDownload || albumId));
  const showFavoriteButton = $derived(showFavorite ?? !!albumId);
  const favoriteAvailable = $derived(favoriteEnabled ?? !!albumId);
  const hasOverlay = $derived(!!(showFavoriteButton || onPlay || hasMenu));
  let menuOpen = $state(false);
  let contextMenuOpen = $state(false);
  let contextMenuPos = $state<{ x: number; y: number } | null>(null);

  function handleImageError() {
    imageError = true;
  }

  function formatReleaseDate(dateStr: string | undefined): string {
    if (!dateStr) return '';
    const date = new Date(dateStr);
    if (isNaN(date.getTime())) return '';
    return date.toLocaleDateString($locale ? $locale : 'en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    });
  }

  const formattedDate = $derived(formatReleaseDate(releaseDate));

  async function handleToggleFavorite(event: MouseEvent) {
    event.stopPropagation();
    if (!albumId || !favoriteAvailable) return;
    await toggleAlbumFavorite(albumId);
  }

  function handlePlay(event: MouseEvent) {
    event.stopPropagation();
    onPlay?.();
  }

  function handleArtistClick(event: MouseEvent) {
    event.stopPropagation();
    if (artistId && onArtistClick) {
      onArtistClick(artistId);
    }
  }

  const artistClickable = $derived(!!(artistId && onArtistClick));

  function handleAddToMixtape() {
    if (!albumId) return;
    const source = isLocal || !!sourceBadge ? 'local' : 'qobuz';
    const parsedYear = year ?? (releaseDate ? new Date(releaseDate).getFullYear() : undefined);
    openAddToMixtape({
      item_type: 'album',
      source,
      source_item_id: albumId,
      title,
      subtitle: artist,
      artwork_url: artwork,
      year: parsedYear && !isNaN(parsedYear) ? parsedYear : undefined,
      track_count: trackCount,
    });
  }

  function isOverlayAction(target: EventTarget | null) {
    if (!(target instanceof HTMLElement)) return false;
    return !!target.closest('.action-buttons') || !!target.closest('.artist-link');
  }

  function handleCardClick(event: MouseEvent) {
    if (isOverlayAction(event.target)) return;
    // Multi-select mode: click toggles selection instead of opening the card.
    if (selectable) {
      onToggleSelect?.();
      return;
    }
    onclick?.();
  }

  function updateOverflow() {
    if (titleRef && titleTextRef) {
      const overflow = titleTextRef.scrollWidth - titleRef.clientWidth;
      titleOverflow = overflow > 0 ? overflow : 0;
    }
    if (artistRef && artistTextRef) {
      const overflow = artistTextRef.scrollWidth - artistRef.clientWidth;
      artistOverflow = overflow > 0 ? overflow : 0;
    }
  }

  // Track if overflow has been measured (only measure on first hover for performance)
  let overflowMeasured = false;

  function measureOverflowOnce() {
    if (!overflowMeasured) {
      updateOverflow();
      overflowMeasured = true;
    }
  }

  onMount(() => {
    // Don't create ResizeObserver per-card for performance in large libraries
    // Overflow is measured on first hover instead

    if (albumId) {
      void loadAlbumFavorites();
      favoriteFromStore = isAlbumFavorite(albumId);
      isToggling = isAlbumToggling(albumId);
      const unsubscribe = subscribeAlbumFavorites(() => {
        favoriteFromStore = isAlbumFavorite(albumId);
        isToggling = isAlbumToggling(albumId);
      });
      return () => unsubscribe();
    }
  });

  // Reset measurement when title/artist changes
  $effect(() => {
    title;
    artist;
    overflowMeasured = false;
    titleOverflow = 0;
    artistOverflow = 0;
  });
</script>

<div
  class="album-card"
  class:is-selectable={selectable}
  class:is-selected={selectable && selected}
  style="width: {cardSize}px"
  data-search-id={searchId}
  onclick={handleCardClick}
  oncontextmenu={(e) => { e.preventDefault(); e.stopPropagation(); menuOpen = true; contextMenuOpen = true; contextMenuPos = { x: e.clientX, y: e.clientY }; }}
  onmouseenter={measureOverflowOnce}
  onfocus={measureOverflowOnce}
  role="button"
  tabindex="0"
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      if (selectable) onToggleSelect?.();
      else onclick?.();
    }
  }}
>
  <!-- Artwork Container -->
  <div
    class="artwork-container"
    style="width: {cardSize}px; height: {cardSize}px"
  >
    <!-- Placeholder always rendered as background layer (visible while image loads) -->
    <div class="artwork-placeholder">
      <Disc3 size={48} />
    </div>

    <!-- Image overlays placeholder when loaded -->
    {#if !imageError && artwork}
      <img class="artwork-image" use:cachedSrc={albumId ? resolveAlbumCover(albumId, artwork) : artwork} alt={title} loading="lazy" decoding="async" onerror={handleImageError} />
    {/if}

    {#if selectable}
      <div class="select-checkbox" aria-hidden="true">
        {#if selected}<Check size={14} />{/if}
      </div>
    {/if}

    <!-- Action Overlay -->
    {#if hasOverlay}
      <div class="action-overlay" class:menu-open={menuOpen}>
        {#if showGenre && (genre || formattedDate)}
          <div class="overlay-info">
            {#if genre}<span class="overlay-genre">{genre}</span>{/if}
            {#if formattedDate}
              <span class="overlay-date">{formattedDate}</span>
            {/if}
          </div>
        {/if}
        <div class="action-buttons">
          {#if showFavoriteButton}
            <button
              class="overlay-btn overlay-btn--minor"
              class:is-active={isFavorite}
              class:is-toggling={isToggling}
              class:disabled={!favoriteAvailable}
              type="button"
              aria-disabled={!favoriteAvailable || isToggling}
              disabled={!favoriteAvailable || isToggling}
              onclick={handleToggleFavorite}
              title={isFavorite ? $t('actions.removeFromFavorites') : $t('actions.addToFavorites')}
            >
              <Heart size={18} fill={isFavorite ? 'white' : 'none'} color="white" />
            </button>
          {:else}
            <!-- Spacer to keep Play button centered -->
            <div class="overlay-btn--spacer"></div>
          {/if}
          {#if onPlay}
            <button class="overlay-btn" type="button" onclick={handlePlay} title={$t('actions.play')}>
              <Play size={18} fill="white" color="white" />
            </button>
          {/if}
          {#if hasMenu}
            <div class="overlay-menu">
              <AlbumMenu
                onPlayNext={onPlayNext}
                onPlayLater={onPlayLater}
                onAddToPlaylist={onAddAlbumToPlaylist}
                onAddToMixtape={albumId ? handleAddToMixtape : undefined}
                onShareQobuz={onShareQobuz}
                onShareSonglink={onShareSonglink}
                onDownload={onDownload}
                isAlbumFullyDownloaded={isDownloaded}
                onOpenContainingFolder={onOpenContainingFolder}
                onReDownloadAlbum={onReDownloadAlbum}
                externalOpen={contextMenuOpen}
                contextMenuPosition={contextMenuPos}
                onOpenChange={(open) => { menuOpen = open; if (!open) { contextMenuOpen = false; contextMenuPos = null; } }}
              />
            </div>
          {/if}
        </div>
      </div>
    {/if}

    <!-- Editorial ribbon (Album of the Week / Qobuzissime / Press).
         Only the last award in album.awards is shown on the card; the
         full stack lives in AlbumView's sidebar. -->
    {#if ribbon}
      <div class="editorial-ribbon"
           class:ribbon-aotw={ribbon.kind === 'albumOfTheWeek'}
           class:ribbon-qbz={ribbon.kind === 'qobuzissime'}
           class:ribbon-press={ribbon.kind === 'press'}
           title={ribbon.label}>
        {ribbon.label}
      </div>
    {/if}

    <!-- Source Badge (Local Library only) -->
    {#if sourceBadge}
      <div class="source-badge-slot">
        <SourceBadge value={sourceBadge} />
      </div>
    {/if}
  </div>

  <!-- Text Info -->
  <div class="info">
    <div
      class="title"
      class:scrollable={titleOverflow > 0}
      style="--ticker-offset: {titleOffset}; --ticker-duration: {titleDuration};"
      bind:this={titleRef}
    >
      <span class="title-text" bind:this={titleTextRef}>{title}</span>
    </div>
    {#if artistClickable}
      <button
        class="artist artist-link"
        class:scrollable={artistOverflow > 0}
        style="--ticker-offset: {artistOffset}; --ticker-duration: {artistDuration};"
        bind:this={artistRef}
        onclick={handleArtistClick}
        type="button"
      >
        <span class="artist-text" bind:this={artistTextRef}>{artist}</span>
      </button>
    {:else}
      <div
        class="artist"
        class:scrollable={artistOverflow > 0}
        style="--ticker-offset: {artistOffset}; --ticker-duration: {artistDuration};"
        bind:this={artistRef}
      >
        <span class="artist-text" bind:this={artistTextRef}>{artist}</span>
      </div>
    {/if}
    {#if quality}
      <div class="quality-badge">{quality}</div>
    {/if}
  </div>
</div>

<style>
  .album-card {
    flex-shrink: 0;
    cursor: pointer;
    transition: transform 150ms ease;
  }

  .artwork-container {
    position: relative;
    margin-bottom: 8px;
    border-radius: 8px;
    overflow: hidden;
  }

  /* Multi-select corner checkbox (top-left of the artwork). Always rendered
     when selectable=true; filled accent when selected, outlined-only when not. */
  .select-checkbox {
    position: absolute;
    top: 8px;
    left: 8px;
    z-index: 3;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.85);
    background: rgba(0, 0, 0, 0.45);
    color: #fff;
    pointer-events: none;
    backdrop-filter: blur(2px);
    transition: background 120ms ease, border-color 120ms ease;
  }
  .album-card.is-selected .select-checkbox {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }
  .album-card.is-selected {
    outline: 2px solid var(--accent-primary);
    outline-offset: 2px;
    border-radius: 10px;
  }
  /* Disable the drag-cursor feel when in select-mode — tap = toggle. */
  .album-card.is-selectable {
    cursor: pointer;
  }

  .artwork-container .artwork-image {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: inherit;
    z-index: 1;
    transition: opacity 0.15s ease-in;
  }

  .artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
    border-radius: inherit;
  }

  .quality-badge {
    display: inline-block;
    margin-top: 4px;
    font-family: 'LINE Seed JP', var(--font-sans);
    font-size: 10px;
    font-weight: 100;
    color: var(--alpha-85);
    background: var(--alpha-10);
    border: 1px solid var(--alpha-15);
    border-radius: 4px;
    padding: 4px 6px;
    min-width: 90px;
    text-align: center;
    box-sizing: border-box;
  }
  
  :global([data-theme="light"]) .quality-badge {
    color: rgba(40, 42, 54, 0.85) !important;
    background: #ffffff !important;
    border: 1px solid rgba(40, 42, 54, 0.95) !important;
  }

  /* Source badge for Local Library albums */
  /* Editorial ribbon — sits at bottom-left of the artwork. z-index
     below the action-overlay (z-index 2) so it hides on hover, as
     requested. */
  .editorial-ribbon {
    position: absolute;
    bottom: 8px;
    left: 0;
    z-index: 1;
    padding: 3px 10px 3px 8px;
    font-family: var(--font-sans);
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: #fff;
    background: rgba(0, 0, 0, 0.75);
    border-left: 3px solid var(--accent-primary);
    border-top-right-radius: 3px;
    border-bottom-right-radius: 3px;
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
    pointer-events: none;
    white-space: nowrap;
    max-width: calc(100% - 12px);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .editorial-ribbon.ribbon-aotw {
    border-left-color: #eab308;
  }

  .editorial-ribbon.ribbon-qbz {
    border-left-color: #8b5cf6;
  }

  /* Press accolades get a solid gold ribbon with dark, readable text —
     distinct from the Qobuz-branded variants (which stay dark with a
     coloured left border). */
  .editorial-ribbon.ribbon-press {
    background: linear-gradient(135deg, #f5c042 0%, #d49511 100%);
    color: #1f1407;
    border-left: none;
    padding-left: 10px;
    text-shadow: 0 1px 0 rgba(255, 255, 255, 0.15);
  }

  .source-badge-slot {
    position: absolute;
    bottom: 6px;
    right: 6px;
    z-index: 3;
  }

  .action-overlay {
    position: absolute;
    inset: -1px;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    opacity: 0;
    transition: opacity 150ms ease;
    background: rgba(10, 10, 10, 0.75);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    pointer-events: auto;
    border-radius: inherit;
    z-index: 2;
  }

  .album-card:hover .action-overlay,
  .action-overlay:focus-within,
  .action-overlay.menu-open {
    opacity: 1;
  }

  .action-buttons {
    display: flex;
    align-items: center;
    gap: 12px;
    pointer-events: auto;
    position: absolute;
    left: 50%;
    top: 75%;
    transform: translate(-50%, -50%);
    opacity: 0;
  }

  .album-card:hover .action-buttons,
  .action-overlay:focus-within .action-buttons,
  .action-overlay.menu-open .action-buttons {
    animation: slide-in-down 0.4s ease-out forwards;
  }

  @keyframes slide-in-down {
    0% {
      opacity: 0;
      transform: translate(-50%, calc(-50% - 12px));
    }
    100% {
      opacity: 1;
      transform: translate(-50%, -50%);
    }
  }

  .overlay-btn {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    /* Use box-shadow instead of border for smoother anti-aliasing */
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.85), 0 0 1px rgba(0, 0, 0, 0.3);
    transition: transform 150ms ease, background-color 150ms ease, box-shadow 150ms ease;
  }

  .overlay-btn:hover {
    background-color: rgba(0, 0, 0, 0.3);
    box-shadow: inset 0 0 0 1px var(--accent-primary), 0 0 4px rgba(0, 0, 0, 0.5);
  }

  .overlay-btn.is-active {
    background-color: rgba(255, 255, 255, 0.2);
  }

  .overlay-btn.disabled,
  .overlay-btn:disabled {
    opacity: 0.5;
    cursor: default;
    transform: none;
  }

  .overlay-btn.is-toggling {
    cursor: wait;
    animation: favorite-pulse 0.8s ease-in-out infinite;
  }

  @keyframes favorite-pulse {
    0%, 100% {
      opacity: 0.5;
    }
    50% {
      opacity: 1;
    }
  }

  .overlay-menu {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .overlay-btn--minor {
    width: 30px;
    height: 30px;
  }

  .overlay-btn--spacer {
    width: 30px;
    height: 30px;
    visibility: hidden;
  }

  .overlay-info {
    align-self: flex-start;
    width: 100%;
    text-align: left;
    padding: 14px 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    opacity: 0;
    transform: translateY(12px);
  }

  .album-card:hover .overlay-info,
  .action-overlay:focus-within .overlay-info,
  .action-overlay.menu-open .overlay-info {
    animation: slide-in-up 0.4s ease-out forwards;
  }

  @keyframes slide-in-up {
    0% {
      opacity: 0;
      transform: translateY(12px);
    }
    100% {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .overlay-genre {
    font-size: 14px;
    font-weight: 600;
    color: white;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.8);
    word-wrap: break-word;
    overflow-wrap: break-word;
  }

  .overlay-date {
    font-size: 12px;
    font-weight: 400;
    color: rgba(255, 255, 255, 0.85);
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.8);
  }

  :global(.album-card .album-menu) {
    display: flex;
    align-items: center;
  }

  :global(.album-card .album-menu .menu-trigger) {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    /* Use box-shadow instead of border for smoother anti-aliasing */
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.85), 0 0 1px rgba(0, 0, 0, 0.3);
    transition: background-color 150ms ease, box-shadow 150ms ease;
  }

  :global(.album-card .album-menu .menu-trigger:hover) {
    background-color: rgba(0, 0, 0, 0.3);
    box-shadow: inset 0 0 0 1px var(--accent-primary), 0 0 4px rgba(0, 0, 0, 0.5);
  }

  .info {
    width: 100%;
  }

  .title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.4;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-bottom: 2px;
  }

  .title.scrollable {
    text-overflow: clip;
  }

  .title-text {
    display: inline-block;
    white-space: nowrap;
  }

  .album-card:hover .title.scrollable .title-text {
    animation: title-ticker var(--ticker-duration) linear infinite;
    will-change: transform;
  }

  @keyframes title-ticker {
    0%, 20% { transform: translateX(0); }
    70%, 80% { transform: translateX(var(--ticker-offset)); }
    90%, 100% { transform: translateX(0); }
  }

  .artist {
    font-size: 13px;
    font-weight: 400;
    color: var(--text-muted);
    line-height: 1.4;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist.artist-link {
    display: block;
    appearance: none;
    -webkit-appearance: none;
    background: none;
    border: none;
    padding: 0;
    margin: 0;
    font: inherit;
    text-align: left;
    width: 100%;
    cursor: pointer;
    color: var(--text-muted);
  }

  .artist.artist-link:hover {
    color: var(--text-primary);
  }

  .artist.scrollable {
    text-overflow: clip;
  }

  .artist-text {
    display: inline-block;
    white-space: nowrap;
  }

  .album-card:hover .artist.scrollable .artist-text,
  .artist.artist-link.scrollable:hover .artist-text {
    animation: title-ticker var(--ticker-duration) linear infinite;
    will-change: transform;
  }
</style>
