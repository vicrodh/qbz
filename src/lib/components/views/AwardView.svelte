<script lang="ts">
  /**
   * AwardView — ports the Qobuz iOS/mobile "Premio" detail screen
   * (laurel hero + award-winning releases). Pulls /award/page for the
   * hero info and uses the embedded releases arrays for the album
   * grids. Matches LabelView's overall structure.
   */
  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { ArrowLeft, Heart, LoaderCircle, ArrowRight } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import type { AwardPageData, QobuzAlbum } from '$lib/types';
  import { formatQuality, getQobuzImage } from '$lib/adapters/qobuzAdapters';
  import {
    subscribe as subscribeAwardFavorites,
    isAwardFavorite,
    isAwardToggling,
    toggleAwardFavorite,
  } from '$lib/stores/awardFavoritesStore';

  interface AlbumCardData {
    id: string;
    artwork: string;
    title: string;
    artist: string;
    artistId?: number;
    genre: string;
    quality?: string;
    releaseDate?: string;
  }

  interface Props {
    awardId: string;
    awardName?: string;
    onBack: () => void;
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onOpenAlbumFolder?: (albumId: string) => void;
    onReDownloadAlbum?: (albumId: string) => void;
    onAddAlbumToPlaylist?: (albumId: string) => void;
    isAlbumDownloaded?: (albumId: string) => boolean;
    downloadStateVersion?: number;
    onArtistClick?: (artistId: number) => void;
    /** Navigate to the full paginated listing of this award's albums. */
    onNavigateAwardAlbums?: (awardId: string, awardName: string) => void;
    /** Navigate into another award from the 'Other awards' carousel. */
    onAwardClick?: (awardId: string, awardName: string) => void;
  }

  let {
    awardId,
    awardName,
    onBack,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onOpenAlbumFolder,
    onReDownloadAlbum,
    onAddAlbumToPlaylist,
    isAlbumDownloaded,
    downloadStateVersion,
    onArtistClick,
    onNavigateAwardAlbums,
    onAwardClick,
  }: Props = $props();

  interface OtherAward {
    id: string;
    name: string;
    image?: string;
    magazine?: string;
  }

  const PAGE_SIZE = 20;

  let page = $state<AwardPageData | null>(null);
  let albums = $state<AlbumCardData[]>([]);
  let totalEstimate = $state<number | null>(null);
  let hasMore = $state(false);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let heroImageFailed = $state(false);

  let otherAwards = $state<OtherAward[]>([]);
  let failedOtherImages = $state<Set<string>>(new Set());

  /** Adapts a QobuzAlbum (shape returned by /award/getAlbums) into
   * what AlbumCard consumes. */
  function qobuzToCard(album: QobuzAlbum): AlbumCardData {
    const hires = (album.maximum_bit_depth ?? 16) > 16;
    return {
      id: album.id,
      artwork: getQobuzImage(album.image),
      title: album.title,
      artist: album.artist?.name ?? 'Unknown Artist',
      artistId: album.artist?.id,
      genre: album.genre?.name ?? '',
      quality: formatQuality(hires, album.maximum_bit_depth, album.maximum_sampling_rate),
      releaseDate: album.release_date_original,
    };
  }

  async function loadHero() {
    try {
      // /award/page is user-scoped (only returns releases from awards
      // the user has favorited) so we only use it to hydrate the hero
      // — name, image, magazine. The album grid below fills from
      // /award/getAlbums which is the public catalog listing.
      const data = await invoke<AwardPageData>('v2_get_award_page', { awardId });
      page = data;
    } catch (err) {
      console.warn('[AwardView] /award/page failed, falling back to passed name:', err);
    }
  }

  async function loadAlbums() {
    loading = true;
    error = null;
    try {
      const result = await invoke<{ items: QobuzAlbum[]; total: number; offset: number; limit: number }>(
        'v2_get_award_albums',
        { awardId, limit: PAGE_SIZE, offset: 0 }
      );
      albums = (result.items ?? []).map(qobuzToCard);
      totalEstimate = result.total;
      hasMore = albums.length >= PAGE_SIZE && albums.length < result.total;
    } catch (err) {
      console.error('[AwardView] failed to load albums:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  function handleSeeAll() {
    onNavigateAwardAlbums?.(awardId, displayName);
  }

  /** Populate the 'Other awards' carousel from /award/explore. Filter
   *  out the current award so we never show "go to the page you're
   *  already on". */
  async function loadOtherAwards() {
    try {
      const result = await invoke<{ items?: Array<{ id?: string | number; name?: string; image?: string; magazine?: { name?: string } }> }>(
        'v2_get_award_explore',
        { limit: 30, offset: 0 }
      );
      const items = result.items ?? [];
      otherAwards = items
        .filter(it => it?.id != null && it?.name && String(it.id) !== String(awardId))
        .map(it => ({
          id: String(it.id),
          name: it!.name!,
          image: it!.image ?? undefined,
          magazine: it!.magazine?.name ?? undefined,
        }));
    } catch (err) {
      console.warn('[AwardView] failed to load other awards:', err);
    }
  }

  function handleOtherImageError(id: string) {
    failedOtherImages = new Set(failedOtherImages).add(id);
  }

  // Subscribe to the favorites store so the Follow button's visual
  // state updates in real time (and from other views that may mutate it).
  let awardFavoritesVersion = $state(0);
  const unsubAwardFavorites = subscribeAwardFavorites(() => {
    awardFavoritesVersion += 1;
  });
  onDestroy(() => unsubAwardFavorites());

  const awardIsFavorite = $derived.by(() => {
    void awardFavoritesVersion;
    return isAwardFavorite(awardId);
  });
  const awardIsToggling = $derived.by(() => {
    void awardFavoritesVersion;
    return isAwardToggling(awardId);
  });

  async function handleToggleAwardFavorite() {
    await toggleAwardFavorite(awardId);
  }

  // Re-fetch whenever the awardId prop changes. Happens when the user
  // jumps between awards via the "Other awards" carousel — the view
  // stays mounted (same ViewType) so onMount wouldn't fire again.
  let lastLoadedId = '';
  $effect(() => {
    if (awardId && awardId !== lastLoadedId) {
      lastLoadedId = awardId;
      page = null;
      albums = [];
      totalEstimate = null;
      hasMore = false;
      error = null;
      heroImageFailed = false;
      loading = true;
      loadHero();
      loadAlbums();
      loadOtherAwards();
    }
  });

  const displayName = $derived(page?.name ?? awardName ?? '');
  const magazineName = $derived(page?.magazine?.name ?? '');
  const heroImage = $derived(page?.image || page?.magazine?.image || '');
</script>

<div class="award-detail-view">
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>{$t('actions.back')}</span>
  </button>

  <header class="award-header">
    <div class="award-image-wrapper">
      {#if heroImage && !heroImageFailed}
        <img
          src={heroImage}
          alt={displayName}
          class="award-image"
          loading="lazy"
          decoding="async"
          onerror={() => (heroImageFailed = true)}
        />
      {:else}
        <div class="award-image-placeholder">
          <img src="/laurels.svg" alt="" class="laurel-icon" />
        </div>
      {/if}
    </div>
    <div class="award-header-info">
      <div class="award-subtitle">{$t('award.kicker')}</div>
      <h1 class="award-name">{displayName}</h1>
      {#if magazineName}
        <div class="award-magazine">{magazineName}</div>
      {/if}

      <div class="award-actions">
        <button
          class="favorite-btn"
          class:is-favorite={awardIsFavorite}
          onclick={handleToggleAwardFavorite}
          disabled={awardIsToggling}
          title={awardIsFavorite ? $t('award.unfollow') : $t('award.follow')}
          aria-label={awardIsFavorite ? $t('award.unfollow') : $t('award.follow')}
        >
          {#if awardIsFavorite}
            <Heart size={24} fill="var(--accent-primary)" color="var(--accent-primary)" />
          {:else}
            <Heart size={24} />
          {/if}
        </button>
      </div>
    </div>
  </header>

  <main class="content">
    <div class="section-header">
      <div class="section-title-group">
        <h2 class="section-title">{$t('award.section.releases')}</h2>
        {#if totalEstimate}
          <span class="section-count">{totalEstimate}</span>
        {/if}
      </div>
      {#if hasMore && onNavigateAwardAlbums}
        <button class="see-all-link" onclick={handleSeeAll}>
          {$t('home.seeAll')}
          <ArrowRight size={14} />
        </button>
      {/if}
    </div>

    {#if loading}
      <div class="loading">
        <LoaderCircle size={28} class="spinner" />
        <p>{$t('album.loading')}</p>
      </div>
    {:else if error}
      <div class="error">
        <p>{$t('favorites.failedLoadFavorites')}</p>
        <p class="error-detail">{error}</p>
        <button class="retry-btn" onclick={loadAlbums}>{$t('actions.retry')}</button>
      </div>
    {:else if albums.length === 0}
      <div class="empty">
        <p>{$t('award.empty')}</p>
      </div>
    {:else}
      <div class="album-grid">
        {#each albums as album (album.id)}
          <AlbumCard
            albumId={album.id}
            artwork={album.artwork}
            title={album.title}
            artist={album.artist}
            artistId={album.artistId}
            onArtistClick={onArtistClick}
            genre={album.genre}
            releaseDate={album.releaseDate}
            size="large"
            quality={album.quality}
            onPlay={onAlbumPlay ? () => onAlbumPlay(album.id) : undefined}
            onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(album.id) : undefined}
            onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(album.id) : undefined}
            onAddAlbumToPlaylist={onAddAlbumToPlaylist ? () => onAddAlbumToPlaylist(album.id) : undefined}
            onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(album.id) : undefined}
            onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(album.id) : undefined}
            onDownload={onAlbumDownload ? () => onAlbumDownload(album.id) : undefined}
            isAlbumFullyDownloaded={isAlbumDownloaded?.(album.id) ?? false}
            onOpenContainingFolder={onOpenAlbumFolder ? () => onOpenAlbumFolder(album.id) : undefined}
            onReDownloadAlbum={onReDownloadAlbum ? () => onReDownloadAlbum(album.id) : undefined}
            {downloadStateVersion}
            onclick={() => onAlbumClick?.(album.id)}
          />
        {/each}
      </div>
    {/if}

    <!-- Other awards carousel (mirrors mobile's "Más premios" rail). -->
    {#if otherAwards.length > 0}
      <section class="other-awards-section">
        <HorizontalScrollRow title={$t('award.otherAwards')}>
          {#snippet children()}
            {#each otherAwards as other (other.id)}
              {@const imgBroken = failedOtherImages.has(other.id)}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="award-mini-card"
                role="button"
                tabindex="0"
                onclick={() => onAwardClick?.(other.id, other.name)}
              >
                <div class="award-mini-image-wrapper">
                  {#if other.image && !imgBroken}
                    <img
                      src={other.image}
                      alt={other.name}
                      class="award-mini-image"
                      loading="lazy"
                      decoding="async"
                      onerror={() => handleOtherImageError(other.id)}
                    />
                  {:else}
                    <div class="award-mini-placeholder">
                      <img src="/laurels.svg" alt="" class="laurel-icon-sm" />
                    </div>
                  {/if}
                </div>
                <div class="award-mini-name">{other.name}</div>
                {#if other.magazine}
                  <div class="award-mini-magazine">{other.magazine}</div>
                {/if}
              </div>
            {/each}
            <div class="spacer"></div>
          {/snippet}
        </HorizontalScrollRow>
      </section>
    {/if}
  </main>
</div>

<style>
  /* Mirrors LabelView / ArtistDetailView outer container — same
     paddings, scroll behavior, scrollbar treatment. */
  .award-detail-view {
    padding: 24px;
    padding-top: 0;
    padding-left: 18px;
    padding-right: 8px;
    padding-bottom: 100px;
    overflow-y: auto;
    height: 100%;
  }
  .award-detail-view::-webkit-scrollbar { width: 6px; }
  .award-detail-view::-webkit-scrollbar-track { background: transparent; }
  .award-detail-view::-webkit-scrollbar-thumb { background: var(--bg-tertiary); border-radius: 3px; }
  .award-detail-view::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }

  /* Back button — identical to LabelView */
  .back-btn {
    display: flex; align-items: center; gap: 8px;
    font-size: 14px; color: var(--text-muted);
    background: none; border: none; cursor: pointer;
    margin-top: 8px; margin-bottom: 24px; transition: color 150ms ease;
  }
  .back-btn:hover { color: var(--text-secondary); }

  /* Header — identical layout to LabelView (image + info, gap 24,
     mb 40, 180px circular avatar, same typography scale). */
  .award-header { display: flex; gap: 24px; margin-bottom: 40px; }
  .award-image-wrapper {
    width: 180px; height: 180px; border-radius: 50%;
    overflow: hidden; flex-shrink: 0; background: var(--bg-tertiary);
  }
  .award-image { width: 100%; height: 100%; object-fit: cover; }
  .award-image-placeholder {
    width: 100%; height: 100%;
    display: flex; align-items: center; justify-content: center;
    background: linear-gradient(135deg, #b45309 0%, #eab308 100%); color: white;
  }
  /* Laurel wreath — black-fill SVG filtered to white so it reads on the
     gold placeholder gradient. */
  .laurel-icon {
    width: 68%; height: 68%;
    filter: brightness(0) invert(1);
    pointer-events: none;
  }
  .award-header-info {
    flex: 1; min-width: 0; display: flex; flex-direction: column; justify-content: center;
  }
  .award-subtitle {
    font-size: 12px; font-weight: 600; color: var(--text-muted);
    text-transform: uppercase; letter-spacing: 0.1em; margin-bottom: 4px;
  }
  .award-name {
    font-size: 32px; font-weight: 700; color: var(--text-primary);
    margin: 0 0 8px 0; line-height: 1.2;
  }
  .award-magazine {
    font-size: 14px; color: var(--text-secondary); line-height: 1.4;
  }

  /* Follow button — matches LabelView / ArtistDetailView visual */
  .award-actions {
    display: flex;
    gap: 12px;
    margin-top: 20px;
  }
  .favorite-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 50%;
    cursor: pointer;
    color: var(--text-muted);
    transition: color 150ms ease, background-color 150ms ease, opacity 150ms ease;
    flex-shrink: 0;
  }
  .favorite-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--accent-primary);
  }
  .favorite-btn.is-favorite {
    background: rgba(var(--accent-primary-rgb, 139, 92, 246), 0.15);
  }
  .favorite-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .content { display: flex; flex-direction: column; gap: 20px; }
  .section-header {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
  }
  .section-title-group {
    display: flex; align-items: baseline; gap: 12px;
  }
  .section-title {
    font-size: 20px; font-weight: 600; color: var(--text-primary); margin: 0;
  }
  .section-count {
    font-size: 12px; color: var(--text-muted);
  }
  .see-all-link {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 6px 10px;
    background: transparent; border: none; border-radius: 6px;
    color: var(--text-secondary);
    font-size: 13px; font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }
  .see-all-link:hover {
    color: var(--text-primary);
    background: var(--bg-tertiary);
  }
  .album-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 24px 16px;
  }

  .loading,
  .error,
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px 24px;
    color: var(--text-secondary);
    text-align: center;
  }
  .error-detail { font-size: 12px; color: var(--text-muted); }
  .retry-btn {
    margin-top: 8px;
    padding: 8px 16px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }
  .retry-btn:hover { background: var(--bg-secondary); }
  .spacer { width: 8px; flex-shrink: 0; }

  /* Other awards carousel */
  .other-awards-section {
    margin-top: 48px;
  }
  .award-mini-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 140px;
    flex-shrink: 0;
    background: none;
    border: none;
    cursor: pointer;
    padding: 8px;
    border-radius: 8px;
  }
  .award-mini-image-wrapper {
    width: 120px;
    height: 120px;
    border-radius: 50%;
    overflow: hidden;
    background: var(--bg-tertiary);
    position: relative;
  }
  .award-mini-image { width: 100%; height: 100%; object-fit: cover; }
  .award-mini-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, #b45309 0%, #eab308 100%);
    color: #fff;
  }
  .laurel-icon-sm {
    width: 56%;
    height: 56%;
    filter: brightness(0) invert(1);
    pointer-events: none;
  }
  .award-mini-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    line-height: 1.3;
    width: 100%;
  }
  .award-mini-magazine {
    font-size: 11px;
    color: var(--text-muted);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }
</style>
