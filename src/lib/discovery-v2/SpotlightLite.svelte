<script lang="ts">
  import { Play, User } from 'lucide-svelte';
  import { onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { ChevronLeft, ChevronRight } from 'lucide-svelte';
  import RadioCardLite from './RadioCardLite.svelte';
  import AlbumCardLite from './AlbumCardLite.svelte';
  import type { SpotlightSection, SpotlightTopTrack, DiscoveryAlbumCard, DiscoveryPlaylistCard } from './data';

  interface Props {
    spotlight: SpotlightSection;
    onArtistClick?: (artistId: number) => void;
    onPlayTopTracks?: () => void;
    onPlayTrack?: (track: SpotlightTopTrack) => void;
    onAlbumClick?: (albumId: string) => void;
    onAlbumPlay?: (albumId: string) => void;
    onAlbumPlayNext?: (albumId: string) => void;
    onAlbumPlayLater?: (albumId: string) => void;
    onAlbumAddToPlaylist?: (albumId: string) => void;
    onAlbumShareQobuz?: (albumId: string) => void;
    onAlbumShareSonglink?: (albumId: string) => void;
    onAlbumDownload?: (albumId: string) => void;
    onPlaylistClick?: (playlistId: number) => void;
    onStartRadio?: (artistId: number, artistName: string) => void;
  }

  let {
    spotlight,
    onArtistClick,
    onPlayTopTracks,
    onAlbumClick,
    onAlbumPlay,
    onAlbumPlayNext,
    onAlbumPlayLater,
    onAlbumAddToPlaylist,
    onAlbumShareQobuz,
    onAlbumShareSonglink,
    onAlbumDownload,
    onPlaylistClick,
    onStartRadio,
  }: Props = $props();

  /**
   * Artist Spotlight — faithful rewrite of the legacy ForYouTab spotlight
   * (see `src/lib/components/views/ForYouTab.svelte:1397+`). Two-part
   * layout: a hero (portrait + name + circle action buttons) and a single
   * paginated row that mixes a TOP TRACKS card, a RADIO card, the artist's
   * Qobuz playlists (as radio-styled cards labelled PLAYLIST), and the
   * artist's albums (as regular AlbumCardLite tiles).
   *
   * The legacy version used `HorizontalScrollRow` with overflow-x. We use
   * DiscoverySection's pagination-by-slice pattern instead (no overflow,
   * no transform) to keep paint cost flat under software compositing.
   */

  type SpotlightItem =
    | { kind: 'topTracks' }
    | { kind: 'radio' }
    | { kind: 'playlist'; playlist: DiscoveryPlaylistCard }
    | { kind: 'album'; album: DiscoveryAlbumCard };

  const items = $derived<SpotlightItem[]>([
    ...(spotlight.topTracks.length > 0 && onPlayTopTracks
      ? [{ kind: 'topTracks' as const }]
      : []),
    ...(onStartRadio ? [{ kind: 'radio' as const }] : []),
    ...spotlight.playlists.map((p) => ({ kind: 'playlist' as const, playlist: p })),
    ...spotlight.albums.map((a) => ({ kind: 'album' as const, album: a })),
  ]);

  // Pagination — mirrors DiscoverySection so the heterogeneous mix of card
  // types lives in the same visual rhythm as the rest of Discovery V2.
  const CARD_WIDTH = 220;
  const GAP = 32;
  let containerEl: HTMLDivElement | undefined = $state();
  let page = $state(0);
  let itemsPerPage = $state(1);

  function recompute() {
    if (!containerEl) return;
    const width = containerEl.clientWidth;
    if (width <= 0) return;
    itemsPerPage = Math.max(1, Math.floor((width + GAP) / (CARD_WIDTH + GAP)));
    const maxPage = Math.max(0, Math.ceil(items.length / itemsPerPage) - 1);
    if (page > maxPage) page = maxPage;
  }

  onMount(() => {
    recompute();
    if (!containerEl) return;
    const ro = new ResizeObserver(recompute);
    ro.observe(containerEl);
    return () => ro.disconnect();
  });

  $effect(() => {
    void items.length;
    recompute();
  });

  const totalPages = $derived(Math.max(1, Math.ceil(items.length / itemsPerPage)));
  const canPrev = $derived(page > 0);
  const canNext = $derived(page < totalPages - 1);
  const visibleItems = $derived(items.slice(page * itemsPerPage, (page + 1) * itemsPerPage));
</script>

<section class="spotlight">
  <header class="header">
    <h2 class="title">{$t('home.spotlight')}</h2>
    <p class="subtitle">{$t('home.spotlightDesc')}</p>
  </header>

  <!-- Hero: portrait + ARTIST label + name + circle action buttons. -->
  <div class="hero">
    <button
      class="hero-portrait"
      type="button"
      onclick={() => onArtistClick?.(spotlight.artistId)}
      aria-label={spotlight.artistName}
    >
      {#if spotlight.artistImage}
        <img
          use:cachedSrc={spotlight.artistImage}
          alt={spotlight.artistName}
          loading="lazy"
          decoding="async"
        />
      {:else}
        <div class="hero-placeholder"><User size={64} /></div>
      {/if}
    </button>
    <div class="hero-info">
      {#if spotlight.category}
        <span class="hero-category">{$t('home.spotlightArtist')}</span>
      {/if}
      <h3 class="hero-name">{spotlight.artistName}</h3>
      <div class="hero-actions">
        {#if onPlayTopTracks}
          <button
            class="circle-btn primary"
            type="button"
            aria-label={$t('home.topTracks')}
            onclick={onPlayTopTracks}
          >
            <Play size={20} fill="currentColor" />
          </button>
        {/if}
        <button
          class="circle-btn"
          type="button"
          aria-label={spotlight.artistName}
          onclick={() => onArtistClick?.(spotlight.artistId)}
        >
          <User size={18} />
        </button>
      </div>
    </div>
  </div>

  <!-- Single paginated row mixing TopTracks card, Radio card, playlists,
       and albums. Matches the legacy ForYouTab spotlight content row. -->
  {#if items.length > 0}
    <div class="row-controls">
      <button
        class="nav-btn"
        type="button"
        aria-label="Previous page"
        disabled={!canPrev}
        onclick={() => { if (canPrev) page = page - 1; }}
      >
        <ChevronLeft size={18} />
      </button>
      <button
        class="nav-btn"
        type="button"
        aria-label="Next page"
        disabled={!canNext}
        onclick={() => { if (canNext) page = page + 1; }}
      >
        <ChevronRight size={18} />
      </button>
    </div>
    <div class="row-outer" bind:this={containerEl}>
      {#key page}
        <div class="row" in:fade={{ duration: 120 }}>
          {#each visibleItems as item, idx (idx)}
            {#if item.kind === 'topTracks'}
              <RadioCardLite
                seedTitle={$t('home.topTracks')}
                seedSubtitle={$t('home.topTracksBy', { values: { artist: spotlight.artistName } })}
                artwork={spotlight.artistImage}
                label={$t('home.topTracksLabel')}
                onPlay={onPlayTopTracks}
                onClick={onPlayTopTracks}
              />
            {:else if item.kind === 'radio'}
              <RadioCardLite
                seedTitle={spotlight.artistName}
                seedSubtitle={$t('home.qobuzRadioStation')}
                artwork={spotlight.artistImage}
                label={$t('home.radioLabel')}
                onPlay={() => onStartRadio?.(spotlight.artistId, spotlight.artistName)}
                onClick={() => onStartRadio?.(spotlight.artistId, spotlight.artistName)}
              />
            {:else if item.kind === 'playlist'}
              <RadioCardLite
                seedTitle={item.playlist.name}
                artwork={item.playlist.image}
                label={$t('home.playlistLabel')}
                onClick={() => onPlaylistClick?.(item.playlist.playlistId)}
              />
            {:else}
              <AlbumCardLite
                albumId={item.album.albumId}
                title={item.album.title}
                artist={item.album.artist}
                artwork={item.album.artwork}
                quality={item.album.quality}
                isHiRes={item.album.isHiRes}
                bitDepth={item.album.bitDepth}
                samplingRate={item.album.samplingRate}
                ribbon={item.album.ribbon}
                genre={item.album.genre}
                releaseYear={item.album.releaseYear}
                releaseDate={item.album.releaseDate}
                onClick={() => onAlbumClick?.(item.album.albumId)}
                onArtistClick={item.album.artistId ? () => onArtistClick?.(item.album.artistId!) : undefined}
                onPlay={onAlbumPlay ? () => onAlbumPlay(item.album.albumId) : undefined}
                onPlayNext={onAlbumPlayNext ? () => onAlbumPlayNext(item.album.albumId) : undefined}
                onPlayLater={onAlbumPlayLater ? () => onAlbumPlayLater(item.album.albumId) : undefined}
                onAddToPlaylist={onAlbumAddToPlaylist ? () => onAlbumAddToPlaylist(item.album.albumId) : undefined}
                onShareQobuz={onAlbumShareQobuz ? () => onAlbumShareQobuz(item.album.albumId) : undefined}
                onShareSonglink={onAlbumShareSonglink ? () => onAlbumShareSonglink(item.album.albumId) : undefined}
                onDownload={onAlbumDownload ? () => onAlbumDownload(item.album.albumId) : undefined}
              />
            {/if}
          {/each}
        </div>
      {/key}
    </div>
  {/if}
</section>

<style>
  .spotlight {
    margin-bottom: 48px;
  }

  .header {
    margin-bottom: 16px;
  }

  .title {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 4px;
  }

  .subtitle {
    margin: 0;
    font-size: 13px;
    color: var(--text-muted);
  }

  /* Hero — portrait next to name + circle actions. Legacy proportions:
     a sizable circular portrait (140px) flanked by an info column that
     stacks category / name / action buttons. */
  .hero {
    display: flex;
    align-items: center;
    gap: 24px;
    margin-bottom: 24px;
  }

  .hero-portrait {
    width: 140px;
    height: 140px;
    flex: 0 0 140px;
    border-radius: 50%;
    overflow: hidden;
    background: var(--bg-tertiary);
    border: none;
    padding: 0;
    cursor: pointer;
  }

  .hero-portrait img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .hero-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }

  .hero-info {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .hero-category {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
  }

  .hero-name {
    margin: 0;
    font-size: 32px;
    font-weight: 700;
    color: var(--text-primary);
    line-height: 1.15;
  }

  .hero-actions {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }

  .circle-btn {
    width: 44px;
    height: 44px;
    border-radius: 50%;
    border: none;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .circle-btn:hover {
    background: var(--bg-hover, var(--bg-secondary));
  }

  .circle-btn.primary {
    background: var(--accent-primary);
    color: #fff;
  }

  /* Paginated row, mirroring DiscoverySection. */
  .row-controls {
    display: flex;
    justify-content: flex-end;
    gap: 4px;
    margin-bottom: 12px;
  }

  .nav-btn {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: none;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .nav-btn:disabled {
    opacity: 0.4;
    cursor: default;
    color: var(--text-muted);
  }

  .row-outer {
    position: relative;
    width: 100%;
  }

  .row {
    display: flex;
    gap: 32px;
  }
</style>
