<script lang="ts">
  import { onMount } from 'svelte';
  import { Play, Disc3, MoreHorizontal, Check } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { resolveAlbumCover } from '$lib/stores/customAlbumCoverStore';
  import { openAddToMixtape } from '$lib/stores/addToMixtapeModalStore';
  import SourceBadge from '$lib/components/SourceBadge.svelte';
  import AlbumQuickMenu from './AlbumQuickMenu.svelte';

  interface Props {
    albumId: string;
    title: string;
    artist: string;
    artwork?: string;
    quality?: string;
    /** Local-library / purchases source identifier; mounts SourceBadge
     *  bottom-right of the cover. */
    sourceBadge?: 'user' | 'qobuz_download' | 'qobuz_purchase' | 'plex';
    /** Year and track count are forwarded to AddToMixtape's payload so
     *  the destination mixtape can pre-populate its metadata. */
    year?: number;
    trackCount?: number;
    /** Multi-select state. When `selectable` is true the card renders a
     *  top-left checkbox; the regular `onClick` is bypassed and the click
     *  flows through `onToggleSelect` instead (the parent owns selection
     *  state). Shift-click range support belongs to the parent — we just
     *  forward the event. */
    selectable?: boolean;
    selected?: boolean;
    onClick?: () => void;
    onPlay?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onToggleSelect?: (e: MouseEvent | KeyboardEvent) => void;
  }

  let {
    albumId,
    title,
    artist,
    artwork,
    quality,
    sourceBadge,
    year,
    trackCount,
    selectable = false,
    selected = false,
    onClick,
    onPlay,
    onPlayNext,
    onPlayLater,
    onToggleSelect,
  }: Props = $props();

  /**
   * Local-library / Purchases card variant of AlbumCardLite.
   *
   * Differences from the Discovery V2 base AlbumCardLite:
   *   - No favorite button overlay (Library cards run `showFavorite=false`).
   *   - No genre / release-date overlay on hover (Library cards run
   *     `showGenre=false` — the user is already inside their library
   *     view and doesn't need editorial copy on every card).
   *   - SourceBadge anchored bottom-right of the cover.
   *   - Optional multi-select checkbox top-left of the cover.
   *   - Quality string rendered as a full pill (not gated on Hi-Res-only
   *     like Discovery's variant). Library users care about "16bit/44.1kHz"
   *     as much as "24bit/192kHz".
   *   - 220px wide to match Discovery V2 `AlbumCardLite` so Home and
   *     Library share the same visual rhythm (per 2026-05-12 user call).
   *     The legacy `VirtualizedAlbumList` grid math is updated in lock-
   *     step (`GRID_MIN_CARD_WIDTH` 210 → 220).
   *
   * Inherited from AlbumCardLite philosophy:
   *   - Cero efectos: opacity-only transitions, no marquee, no
   *     ResizeObserver per card, no will-change, no backdrop-filter.
   *   - `content-visibility: auto` so offscreen cards skip paint entirely
   *     — important for libraries with thousands of albums.
   */

  let imageError = $state(false);
  let menuOpen = $state(false);
  let menuAnchor = $state<{ x: number; y: number } | null>(null);

  /**
   * Reset transient internal state when the slot recycles to a
   * different album. The pool reuses this component instance as scroll
   * advances; without this reset, a menu that was open on the previous
   * album would stay open on the new one, and a failed-image flag
   * would stick. The image-cache action's own `update` handles the
   * cover-URL swap; this reset covers state we own here. */
  $effect(() => {
    void albumId;
    menuOpen = false;
    menuAnchor = null;
    imageError = false;
  });

  function handleImageError() {
    imageError = true;
  }

  function isOverlayAction(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    return !!target.closest('.action-buttons');
  }

  function handleCardClick(e: MouseEvent) {
    if (isOverlayAction(e.target)) return;
    if (selectable) {
      onToggleSelect?.(e);
      return;
    }
    onClick?.();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key !== 'Enter' && e.key !== ' ') return;
    e.preventDefault();
    if (selectable) onToggleSelect?.(e);
    else onClick?.();
  }

  function handlePlay(e: MouseEvent) {
    e.stopPropagation();
    onPlay?.();
  }

  function handleKebab(e: MouseEvent) {
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuAnchor = { x: rect.left, y: rect.bottom + 4 };
    menuOpen = true;
  }

  function handleContextMenu(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    menuAnchor = { x: e.clientX, y: e.clientY };
    menuOpen = true;
  }

  function handleAddToMixtape() {
    openAddToMixtape({
      item_type: 'album',
      // Library cards are always local-side; sourceBadge presence
      // confirms it. Discovery V2 'safe' Library callers never mount
      // this card for a Qobuz-only album.
      source: 'local',
      source_item_id: albumId,
      title,
      subtitle: artist,
      artwork_url: artwork ?? '',
      year,
      track_count: trackCount,
    });
  }

  // The quick-menu mounts only when the album has at least one
  // applicable action. With just `albumId`, AddToMixtape is enough to
  // justify a menu trigger; play-next/play-later optionally extend it.
  const hasMenu = $derived(!!(onPlayNext || onPlayLater || albumId));

  const coverSrc = $derived(
    artwork ? resolveAlbumCover(albumId, artwork) : undefined
  );
</script>

<div
  class="card"
  class:is-selectable={selectable}
  class:is-selected={selectable && selected}
  data-album-id={albumId}
  role="button"
  tabindex="0"
  onclick={handleCardClick}
  oncontextmenu={hasMenu ? handleContextMenu : undefined}
  onkeydown={handleKeydown}
>
  <div class="cover-wrap">
    <!-- Always-rendered placeholder behind the image so a broken / loading
         image never shows a blank rectangle. -->
    <div class="cover-placeholder"><Disc3 size={48} /></div>

    {#if !imageError && coverSrc}
      <!-- src is intentionally not bound here: cachedSrc owns the
           attribute. Setting both `src=` and `use:cachedSrc=` makes the
           WebView fire a fetch on the raw URL in parallel with the
           cache check, double-loading every cover during a big library
           scroll. Legacy AlbumCard followed the same pattern. -->
      <img
        class="cover"
        use:cachedSrc={coverSrc}
        alt={title}
        loading="lazy"
        decoding="async"
        onerror={handleImageError}
      />
    {/if}

    {#if selectable}
      <div class="select-checkbox" aria-hidden="true">
        {#if selected}<Check size={14} />{/if}
      </div>
    {/if}

    <!-- Hover overlay: opacity 0→1, dark scrim + centered action buttons.
         No genre/date row (Library cards intentionally don't surface that). -->
    <div class="overlay">
      <div class="action-buttons">
        {#if onPlay}
          <button
            class="overlay-btn primary"
            type="button"
            aria-label={$t('actions.play')}
            onclick={handlePlay}
          >
            <Play size={18} fill="currentColor" />
          </button>
        {/if}
        {#if hasMenu}
          <button
            class="overlay-btn"
            type="button"
            aria-label={$t('actions.moreActions')}
            onclick={handleKebab}
          >
            <MoreHorizontal size={18} />
          </button>
        {/if}
      </div>
    </div>

    {#if sourceBadge}
      <div class="source-badge-slot">
        <SourceBadge value={sourceBadge} />
      </div>
    {/if}
  </div>

  <div class="info">
    <div class="title" title={title}>{title}</div>
    <div class="artist" title={artist}>{artist}</div>
    {#if quality}
      <div class="quality-pill" title={quality}>{quality}</div>
    {/if}
  </div>
</div>

<AlbumQuickMenu
  isOpen={menuOpen}
  anchor={menuAnchor}
  onClose={() => (menuOpen = false)}
  onPlayNext={onPlayNext ? () => onPlayNext?.() : undefined}
  onPlayLater={onPlayLater ? () => onPlayLater?.() : undefined}
  onAddToMixtape={albumId ? handleAddToMixtape : undefined}
/>

<style>
  /* 220px fixed width to match Discovery V2 `AlbumCardLite` so Home
     and Library share the same column step. The cover-wrap is 1:1
     (220x220) so total card height is ~220 + ~110 info = ~330. */
  .card {
    width: 220px;
    flex-shrink: 0;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: transparent;
    border: none;
    padding: 0;
    text-align: left;
    /* The card is the one hit target the cover-wrap lets through to.
       Click here = navigate to album / toggle select. The decorative
       children inside `.cover-wrap` are `pointer-events: none`; only
       the action buttons re-enable themselves. */
    pointer-events: auto;
  }

  .cover-wrap {
    position: relative;
    width: 220px;
    height: 220px;
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-tertiary);
    /* Hit-test reduction under SW compositing: the cover-wrap and its
       decorative children (placeholder, image, source badge) all opt
       out of pointer events. Mouse moves over them fall through to
       `.card` (which carries `role="button"` and the click handler).
       The action buttons re-enable pointer events on themselves so
       their click still works. Reason: every pointermove during a
       mouse-wheel scroll triggers hit-testing across every visible
       hit target, and with ~50 cards × 5 nested elements per card
       the cascade was visible as hover-delay + scroll lag. With this
       opt-out the hit surface drops to just `.card` outers + the
       buttons themselves. */
    pointer-events: none;
  }

  /* Hover scrim. Restored 2026-05-12 after the Plex thumbnail-size
     fix (commit 9f417999) — that change cut Plex artwork from
     1000x1000 raw JPEGs to 220x220 server-side transcoded thumbnails,
     ~30x less decode + memory per card. With that win, the scrim's
     opacity transition is no longer the dominant per-frame cost it
     was when 50 cards × 4MB raw rasters were sitting in memory. */
  .cover-wrap::after {
    content: '';
    position: absolute;
    inset: 0;
    background: rgba(10, 10, 10, 0.85);
    opacity: 0;
    transition: opacity 150ms ease;
    pointer-events: none;
    /* Cover img sits at z-index: 1, so the scrim needs z-index: 2 to
       paint above the artwork and below the action buttons (which
       are inside `.overlay` at z-index: 2 already; siblings stack
       in DOM order so buttons declared after stay on top). */
    z-index: 2;
  }

  .card:hover .cover-wrap::after {
    opacity: 1;
  }

  .cover-placeholder {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(
      135deg,
      var(--bg-tertiary) 0%,
      var(--bg-secondary) 100%
    );
    color: var(--text-muted);
  }

  .cover {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    z-index: 1;
  }

  /* Multi-select checkbox in the top-left corner of the cover. Same
     geometry as the legacy AlbumCard so the visual is familiar to users
     coming over from the old library views. */
  .select-checkbox {
    position: absolute;
    top: 8px;
    left: 8px;
    z-index: 4;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.85);
    background: rgba(0, 0, 0, 0.45);
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }

  .card.is-selected .select-checkbox {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .card.is-selected {
    outline: 2px solid var(--accent-primary);
    outline-offset: 2px;
    border-radius: 10px;
  }

  /* Always-visible action buttons. Earlier versions used opacity 0 → 1
     on `.card:hover` so the artwork stayed clean when idle; under
     software compositing the hit-testing required for `:hover`
     evaluation became the dominant frame cost during mouse-wheel
     scroll (the cursor traverses cards as they pass under it,
     triggering pointermove on every card boundary). Removing the
     hover state takes the entire `:hover` cascade out of the loop —
     buttons are statically painted on each card mount and never
     touched again until clicked.
     Theme-aware colours guarantee contrast on any cover art (dark or
     light) without needing the artwork-pixel sample work the legacy
     AlbumCard did. */
  /* Buttons sit close to the bottom edge of the cover (12px clearance).
     Flex `flex-end` + `padding-bottom` rather than absolute positioning
     so no transform is needed on the child. */
  /* z-index layering inside `.cover-wrap` (a stacking context due to
     position: relative + children with z-index):
       1  cover img
       2  scrim (cover-wrap::after)
       3  overlay (action buttons)
       4  select-checkbox + source-badge-slot
     The scrim and overlay sharing z-index 2 with the scrim being the
     pseudo (always last in DOM order) made the scrim paint OVER the
     buttons. Bumping the overlay to 3 puts buttons cleanly above. */
  .overlay {
    position: absolute;
    inset: 0;
    z-index: 3;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    padding-bottom: 12px;
    pointer-events: none;
  }

  /* Buttons hidden by default, shown when the scrim fades in. */
  .action-buttons {
    display: flex;
    align-items: center;
    gap: 12px;
    opacity: 0;
    transition: opacity 150ms ease;
    pointer-events: none;
  }

  .card:hover .action-buttons {
    opacity: 1;
    pointer-events: auto;
  }

  /* White-outline circles, transparent fill — the scrim provides
     contrast. Same visual language as AlbumCardLite on Home. */
  .overlay-btn {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    border: 1.5px solid rgba(255, 255, 255, 0.95);
    background: transparent;
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .overlay-btn:hover {
    background: rgba(255, 255, 255, 0.15);
  }

  .overlay-btn.primary {
    width: 44px;
    height: 44px;
    background: #fff;
    color: #000;
    border-color: #fff;
  }

  .source-badge-slot {
    position: absolute;
    bottom: 6px;
    right: 6px;
    z-index: 4;
  }

  .info {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist {
    font-size: 13px;
    color: var(--text-muted);
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Quality pill: always shown when caller passes a quality string.
     Library users care about format/bit-depth on every album, not just
     Hi-Res ones — different from the Discovery V2 variant which gates
     the pill on `isHiRes`. */
  .quality-pill {
    align-self: flex-start;
    margin-top: 2px;
    font-family: 'LINE Seed JP', var(--font-sans);
    font-size: 10px;
    font-weight: 100;
    color: var(--alpha-85);
    background: var(--alpha-10);
    border: 1px solid var(--alpha-15);
    border-radius: 4px;
    padding: 3px 6px;
    min-width: 90px;
    text-align: center;
    box-sizing: border-box;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  :global([data-theme='light']) .quality-pill {
    color: rgba(40, 42, 54, 0.85) !important;
    background: #ffffff !important;
    border: 1px solid rgba(40, 42, 54, 0.95) !important;
  }
</style>
