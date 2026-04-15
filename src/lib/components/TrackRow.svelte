<script lang="ts">
  import { onMount } from 'svelte';
  import { Play, Pause, Heart, HardDrive, CircleAlert, Ban, Music } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import TrackMenu from './TrackMenu.svelte';
  import DownloadButton from './DownloadButton.svelte';
  import {
    subscribe as subscribeFavorites,
    isTrackFavorite,
    isTrackToggling,
    toggleTrackFavorite
  } from '$lib/stores/favoritesStore';
  import { togglePlay } from '$lib/stores/playerStore';

  // Offline cache status for tracks
  type OfflineCacheStatus = 'none' | 'queued' | 'downloading' | 'ready' | 'failed';

  interface Props {
    trackId?: number; // Optional - required for favorites functionality unless hideFavorite=true
    number: number;
    title: string;
    artist?: string;
    album?: string;
    duration: string;
    quality?: string;
    isPlaying?: boolean;
    isActiveTrack?: boolean;
    isLocal?: boolean; // Whether this is a local library track
    localSource?: 'local' | 'plex';
    isUnavailable?: boolean; // Track removed from Qobuz or otherwise unavailable
    unavailableTooltip?: string; // Tooltip for unavailable indicator
    isBlacklisted?: boolean; // Artist is blacklisted
    isFavoriteOverride?: boolean; // Optional override for favorite state
    downloadStatus?: OfflineCacheStatus;
    downloadProgress?: number;
    hideDownload?: boolean;
    hideFavorite?: boolean;
    compact?: boolean; // Compact mode: smaller height, artist as column
    showArtwork?: boolean; // Optional artwork column (e.g., playlist detail)
    artworkUrl?: string;
    explicit?: boolean; // Parental advisory / explicit content
    selectable?: boolean; // Multi-select mode: show checkbox
    selected?: boolean;
    dragTrackIds?: number[]; // When multi-selected, all selected IDs for drag
    onToggleSelect?: (e: MouseEvent) => void;
    onPlay?: () => void;
    onArtistClick?: () => void;
    onAlbumClick?: () => void;
    onDownload?: () => void;
    onRemoveDownload?: () => void;
    menuActions?: TrackMenuActions;
  }

  interface TrackMenuActions {
    onPlayNow?: () => void;
    onPlayTrackOnly?: () => void;
    onPlayFromHere?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onCreateQbzRadio?: () => void;
    onCreateQobuzRadio?: () => void;
    onAddToPlaylist?: () => void;
    onRemoveFromPlaylist?: () => void;
    onFindReplacement?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onGoToAlbum?: () => void;
    onGoToArtist?: () => void;
    onShowInfo?: () => void;
    onDownload?: () => void;
    isTrackDownloaded?: boolean;
    onReDownload?: () => void;
    onRemoveDownload?: () => void;
  }

  let {
    trackId,
    number,
    title,
    artist,
    album,
    duration,
    quality,
    isPlaying = false,
    isActiveTrack = false,
    isLocal = false,
    localSource = 'local',
    isUnavailable = false,
    unavailableTooltip,
    isFavoriteOverride,
    isBlacklisted = false,
    downloadStatus = 'none',
    downloadProgress = 0,
    hideDownload = false,
    hideFavorite = false,
    compact = false,
    explicit = false,
    showArtwork = false,
    artworkUrl,
    selectable = false,
    selected = false,
    dragTrackIds,
    onToggleSelect,
    onPlay,
    onArtistClick,
    onAlbumClick,
    onDownload,
    onRemoveDownload,
    menuActions
  }: Props = $props();

  let isHovered = $state(false);
  let contextMenuPos = $state<{ x: number; y: number } | null>(null);
  let favoriteFromStore = $state(false);
  let isToggling = $state(false);

  // Use override if provided, otherwise use store
  const isFavorite = $derived(isFavoriteOverride ?? favoriteFromStore);
  const playNowAction = $derived(menuActions?.onPlayNow ?? onPlay);
  const artistClickAction = $derived(onArtistClick ?? menuActions?.onGoToArtist);
  const albumClickAction = $derived(onAlbumClick ?? menuActions?.onGoToAlbum);

  // Subscribe to favorites store (only if trackId is provided)
  onMount(() => {
    if (trackId !== undefined) {
      favoriteFromStore = isTrackFavorite(trackId);
      isToggling = isTrackToggling(trackId);
      const unsubscribe = subscribeFavorites(() => {
        favoriteFromStore = isTrackFavorite(trackId);
        isToggling = isTrackToggling(trackId);
      }, trackId);
      return unsubscribe;
    }
  });

  // Handle favorite toggle internally
  async function handleToggleFavorite(e: MouseEvent) {
    e.stopPropagation();
    if (trackId !== undefined) {
      await toggleTrackFavorite(trackId);
    }
  }

  function handleArtistClick(e: MouseEvent) {
    e.stopPropagation();
    artistClickAction?.();
  }

  function handleAlbumClick(e: MouseEvent) {
    e.stopPropagation();
    albumClickAction?.();
  }

  function handlePauseClick(e: MouseEvent) {
    e.stopPropagation();
    void togglePlay();
  }

  function handleDragStart(e: DragEvent) {
    if (!e.dataTransfer || !trackId || isBlacklisted) return;
    e.dataTransfer.effectAllowed = 'copy';
    const ids = dragTrackIds?.length ? dragTrackIds : [trackId];
    e.dataTransfer.setData('application/x-qbz-tracks', JSON.stringify(ids));
    e.dataTransfer.setData('text/plain', title);

    // Custom drag ghost: compact, semi-transparent pill
    const ghost = document.createElement('div');
    const count = ids.length;
    Object.assign(ghost.style, {
      position: 'fixed',
      top: '-1000px',
      padding: '8px 14px',
      maxWidth: '260px',
      borderRadius: '8px',
      background: 'rgba(30, 30, 40, 0.85)',
      color: '#fff',
      fontSize: '12px',
      lineHeight: '1.4',
      boxShadow: '0 4px 12px rgba(0,0,0,0.3)',
      border: '1px solid rgba(255,255,255,0.1)',
      opacity: '0.9',
    });
    if (count > 1) {
      ghost.textContent = `${count} tracks`;
      ghost.style.fontWeight = '500';
    } else {
      const titleEl = document.createElement('div');
      titleEl.textContent = title;
      Object.assign(titleEl.style, { fontWeight: '600', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' });
      ghost.appendChild(titleEl);
      const sub = [artist, album].filter(Boolean).join(' · ');
      if (sub) {
        const subEl = document.createElement('div');
        subEl.textContent = sub;
        Object.assign(subEl.style, { fontSize: '10px', color: 'rgba(255,255,255,0.55)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', marginTop: '1px' });
        ghost.appendChild(subEl);
      }
    }
    document.body.appendChild(ghost);
    e.dataTransfer.setDragImage(ghost, 0, 20);
    requestAnimationFrame(() => ghost.remove());
  }
</script>

<div
  class="track-row"
  class:playing={isActiveTrack || isPlaying}
  class:hovered={isHovered && !isActiveTrack && !isPlaying && !isBlacklisted}
  class:compact
  class:blacklisted={isBlacklisted}
  class:selected
  data-track-id={trackId ?? undefined}
  draggable={!!trackId && !isBlacklisted}
  ondragstart={handleDragStart}
  onmouseenter={() => (isHovered = true)}
  onmouseleave={() => (isHovered = false)}
  onclick={selectable ? onToggleSelect : (isBlacklisted ? undefined : onPlay)}
  oncontextmenu={(e) => {
    if (isBlacklisted || selectable) return;
    e.preventDefault();
    contextMenuPos = { x: e.clientX, y: e.clientY };
  }}
  role="button"
  tabindex={isBlacklisted ? -1 : 0}
  onkeydown={(e) => e.key === 'Enter' && !isBlacklisted && (selectable ? onToggleSelect?.(e as unknown as MouseEvent) : onPlay?.())}
>
  <!-- Checkbox (select mode) -->
  {#if selectable}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="track-checkbox" onclick={(e) => e.stopPropagation()}>
      <input
        type="checkbox"
        checked={selected}
        onchange={onToggleSelect as unknown as (e: Event) => void}
        aria-label="Select track"
      />
    </div>
  {/if}

  <!-- Track Number / Play Button / Unavailable Indicator -->
  <div class="track-number" class:unavailable={isUnavailable} class:blacklisted={isBlacklisted}>
    {#if isBlacklisted}
      <span class="blacklisted-icon" title="Artist is blacklisted">
        <Ban size={14} />
      </span>
    {:else if isUnavailable}
      <span class="unavailable-icon" title={unavailableTooltip}>
        <CircleAlert size={16} />
      </span>
    {:else if isActiveTrack || isPlaying}
      {#if isHovered}
        {#if isPlaying}
          <button class="pause-btn" type="button" onclick={handlePauseClick} aria-label="Pause">
            <Pause size={16} class="pause-icon" />
          </button>
        {:else}
          <button class="pause-btn" type="button" onclick={handlePauseClick} aria-label="Resume">
            <Play size={16} class="play-icon" fill="white" />
          </button>
        {/if}
      {:else if isPlaying}
        <div class="playing-indicator">
          <div class="bar"></div>
          <div class="bar"></div>
          <div class="bar"></div>
        </div>
      {:else}
        <span>{number}</span>
      {/if}
    {:else if isHovered}
      <Play size={16} class="play-icon" fill="white" />
    {:else}
      <span>{number}</span>
    {/if}
  </div>

  <!-- Track Info -->
  {#if showArtwork}
    <div class="track-artwork">
      <div class="track-artwork-placeholder">
        <Music size={14} />
      </div>
      {#if artworkUrl}
        <img use:cachedSrc={artworkUrl} alt={title} loading="lazy" decoding="async" />
      {/if}
    </div>
  {/if}

  <!-- Track Info -->
  <div class="track-info">
    <div class="track-title-row">
      <span class="track-title" class:active={isActiveTrack || isPlaying}>{title}</span>
      {#if explicit}
        <span class="explicit-badge" title={ $t('library.explicit') }></span>
      {/if}
    </div>
    {#if artist && !compact}
      {#if artistClickAction}
        <button class="track-artist track-link" type="button" onclick={handleArtistClick}>
          {artist}
        </button>
      {:else}
        <div class="track-artist">{artist}</div>
      {/if}
    {/if}
  </div>

  <!-- Artist Column (compact mode) -->
  {#if artist && compact}
    {#if artistClickAction}
      <button class="track-artist-column track-link" type="button" onclick={handleArtistClick}>
        {artist}
      </button>
    {:else}
      <div class="track-artist-column">{artist}</div>
    {/if}
  {/if}

  <!-- Album Column -->
  {#if album}
    {#if albumClickAction}
      <button class="track-album track-link" type="button" onclick={handleAlbumClick}>
        {album}
      </button>
    {:else}
      <div class="track-album">{album}</div>
    {/if}
  {/if}

  <!-- Duration -->
  <div class="track-duration">{duration}</div>

  <!-- Quality (always render to maintain column alignment) -->
  <div class="track-quality">{quality ?? ''}</div>

  <!-- Favorite Button (placeholder for local tracks or hidden to maintain column width) -->
  {#if isLocal || hideFavorite}
    <div class="favorite-placeholder"></div>
  {:else}
    <button
      type="button"
      class="favorite-btn"
      class:is-favorite={isFavorite}
      class:is-toggling={isToggling}
      onclick={handleToggleFavorite}
      title={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
      disabled={isToggling}
    >
      {#if isFavorite}
        <Heart size={14} fill="var(--accent-primary)" color="var(--accent-primary)" />
      {:else}
        <Heart size={14} color="var(--text-muted)" />
      {/if}
    </button>
  {/if}

  <!-- Download Indicator / Local Indicator (placeholder when hidden to maintain column width) -->
  {#if isLocal}
    <div
      class="local-indicator"
      class:plex-source={localSource === 'plex'}
      title={localSource === 'plex' ? $t('library.plexTrackIndicator') : $t('library.localTrackIndicator')}
    >
      {#if localSource === 'plex'}
        <span class="plex-indicator-icon" aria-hidden="true"></span>
      {:else}
        <HardDrive size={14} />
      {/if}
    </div>
  {:else if hideDownload}
    <div class="download-placeholder"></div>
  {:else}
    <div class="download-indicator" class:has-download={downloadStatus !== 'none'}>
      <DownloadButton
        status={downloadStatus}
        progress={downloadProgress}
        size={14}
        onDownload={onDownload}
        onRemove={onRemoveDownload}
      />
    </div>
  {/if}

  <div class="track-actions">
    <TrackMenu
      onPlayNow={playNowAction}
      onPlayTrackOnly={menuActions?.onPlayTrackOnly}
      onPlayFromHere={menuActions?.onPlayFromHere}
      onPlayNext={menuActions?.onPlayNext}
      onPlayLater={menuActions?.onPlayLater}
      onCreateQbzRadio={menuActions?.onCreateQbzRadio}
      onCreateQobuzRadio={menuActions?.onCreateQobuzRadio}
      onAddFavorite={trackId !== undefined ? () => toggleTrackFavorite(trackId) : undefined}
      onAddToPlaylist={menuActions?.onAddToPlaylist}
      onRemoveFromPlaylist={menuActions?.onRemoveFromPlaylist}
      onFindReplacement={menuActions?.onFindReplacement}
      onShareQobuz={menuActions?.onShareQobuz}
      onShareSonglink={menuActions?.onShareSonglink}
      onGoToAlbum={menuActions?.onGoToAlbum}
      onGoToArtist={menuActions?.onGoToArtist}
      onShowInfo={menuActions?.onShowInfo}
      onDownload={menuActions?.onDownload}
      isTrackDownloaded={menuActions?.isTrackDownloaded}
      onReDownload={menuActions?.onReDownload}
      onRemoveDownload={menuActions?.onRemoveDownload ?? onRemoveDownload}
      contextMenuPosition={contextMenuPos}
      onContextMenuClosed={() => { contextMenuPos = null; }}
    />
  </div>
</div>

<style>
  .track-row {
    width: 100%;
    height: 56px;
    padding: 0 16px;
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 16px;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
    box-sizing: border-box;
  }

  .track-row.hovered {
    background-color: var(--bg-hover);
  }

  .track-row.playing {
    background-color: var(--bg-secondary);
  }

  .track-row.compact {
    height: 44px;
    padding: 0 12px;
    gap: 12px;
  }

  .track-row.compact .track-number {
    width: 32px;
  }

  .track-row.selected {
    background-color: color-mix(in srgb, var(--accent-primary) 12%, transparent);
  }

  .track-row.selected.hovered {
    background-color: color-mix(in srgb, var(--accent-primary) 20%, transparent);
  }

  .track-checkbox {
    width: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .track-checkbox input[type='checkbox'] {
    width: 15px;
    height: 15px;
    cursor: pointer;
    accent-color: var(--accent-primary);
  }

  .track-number {
    width: 48px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .track-number span {
    font-size: 14px;
    color: #666666;
  }

  .track-number.unavailable {
    color: var(--error-color, #ef4444);
  }

  .unavailable-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--error-color, #ef4444);
    cursor: help;
  }

  /* Blacklisted track styles */
  .track-row.blacklisted {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .track-row.blacklisted:hover {
    background: transparent;
  }

  .track-number.blacklisted {
    color: var(--text-muted);
  }

  .blacklisted-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .track-number :global(.play-icon) {
    color: white;
  }

  :global([data-theme="light"]) .track-number :global(.play-icon) {
    color: rgba(40, 42, 54, 0.85);
  }

  .pause-btn {
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .pause-btn :global(.pause-icon) {
    color: white;
  }

  :global([data-theme="light"]) .pause-btn :global(.pause-icon) {
    color: rgba(40, 42, 54, 0.85);
  }

  .playing-indicator {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .playing-indicator .bar {
    width: 3px;
    background-color: var(--accent-primary);
    border-radius: 9999px;
    transform-origin: bottom;
    animation: equalize 1s ease-in-out infinite;
    animation-play-state: running;
  }

  .playing-indicator .bar:nth-child(1) {
    height: 12px;
  }

  .playing-indicator .bar:nth-child(2) {
    height: 16px;
    animation-delay: 0.15s;
  }

  .playing-indicator .bar:nth-child(3) {
    height: 10px;
    animation-delay: 0.3s;
  }

  @keyframes equalize {
    0%, 100% {
      transform: scaleY(0.5);
      opacity: 0.7;
    }
    50% {
      transform: scaleY(1);
      opacity: 1;
    }
  }

  .track-info {
    flex: 1;
    min-width: 0;
  }

  .track-artwork {
    width: 36px;
    height: 36px;
    border-radius: 4px;
    overflow: hidden;
    flex-shrink: 0;
    position: relative;
    background-color: var(--bg-tertiary);
  }

  .track-artwork img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    z-index: 1;
  }

  .track-artwork-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    opacity: 0.7;
  }

  .track-title-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .track-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-title.active {
    color: var(--accent-primary);
  }

  .explicit-badge {
    display: inline-block;
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    opacity: 0.45;
    background-color: var(--text-secondary);
    -webkit-mask: url('/explicit.svg') center / contain no-repeat;
    mask: url('/explicit.svg') center / contain no-repeat;
  }

  .track-artist {
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-artist-column {
    width: 180px;
    flex-shrink: 0;
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-album {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .track-link {
    background: none;
    border: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
  }

  .track-link:hover {
    color: var(--text-primary);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .track-duration {
    font-size: 14px;
    color: var(--text-muted);
    font-family: var(--font-sans);
    font-variant-numeric: tabular-nums;
    width: 80px;
    text-align: center;
  }

  .track-quality {
    font-size: 12px;
    color: #666666;
    width: 80px;
    text-align: center;
  }

  .favorite-placeholder {
    width: 28px;
    height: 28px;
  }

  .local-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    color: var(--text-muted);
    opacity: 0.6;
  }

  .local-indicator.plex-source {
    opacity: 0.9;
  }

  .plex-indicator-icon {
    width: 14px;
    height: 14px;
    background-color: var(--accent-primary);
    -webkit-mask: url('/plex-mono.svg') center / contain no-repeat;
    mask: url('/plex-mono.svg') center / contain no-repeat;
  }

  .download-placeholder {
    width: 28px;
    height: 28px;
  }

  .favorite-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    opacity: 0.3;
    transition: opacity 150ms ease, background-color 150ms ease;
  }

  .favorite-btn.is-favorite {
    opacity: 1;
  }

  .favorite-btn:hover {
    opacity: 1;
    background-color: var(--bg-tertiary);
  }

  .track-row:hover .favorite-btn {
    opacity: 0.6;
  }

  .track-row:hover .favorite-btn.is-favorite,
  .track-row:hover .favorite-btn:hover {
    opacity: 1;
  }

  .favorite-btn.is-toggling {
    opacity: 1;
    cursor: wait;
    animation: favorite-pulse 0.8s ease-in-out infinite;
  }

  @keyframes favorite-pulse {
    0%, 100% {
      opacity: 0.4;
    }
    50% {
      opacity: 1;
    }
  }

  .download-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    opacity: 0;
    transition: opacity 150ms ease;
    pointer-events: none;
  }

  .download-indicator.has-download {
    opacity: 1;
    pointer-events: auto;
  }

  .track-row:hover .download-indicator {
    opacity: 0.6;
    pointer-events: auto;
  }

  .track-row:hover .download-indicator.has-download,
  .track-row:hover .download-indicator:hover {
    opacity: 1;
  }

  .track-actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    opacity: 0.7;
    transition: opacity 150ms ease;
  }

  .track-row:hover .track-actions,
  .track-row.playing .track-actions {
    opacity: 1;
  }
</style>
