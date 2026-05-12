<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { t } from '$lib/i18n';
  import {
    ListPlus,
    ListEnd,
    Disc,
    User,
    Share2,
    CloudOff,
    Plus,
    Link as LinkIcon,
    Library,
    CassetteTape,
    RefreshCw,
  } from 'lucide-svelte';
  import {
    openMenu as openGlobalMenu,
    closeMenu as closeGlobalMenu,
    subscribe as subscribeGlobal,
    getActiveMenuId,
  } from '$lib/stores/floatingMenuStore';

  interface Props {
    isOpen: boolean;
    /** Click position in viewport coordinates (from the kebab onclick event). */
    anchor: { x: number; y: number } | null;
    onClose: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onAddToPlaylist?: () => void;
    onAddToMixtape?: () => void;
    onCopyToLibrary?: () => void;
    onGoToAlbum?: () => void;
    onGoToArtist?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onDownload?: () => void;
    /** Local-side actions surfaced only when the album is already cached
     *  offline. Both are optional; passing them without `isAlbumFullyDownloaded=true`
     *  hides the items. */
    onOpenContainingFolder?: () => void;
    onReDownloadAlbum?: () => void;
    /** Toggles the offline-related action surface:
     *  - true  → render "Open containing folder" + "Refresh offline copy"
     *  - false → render "Make available offline" (the onDownload action)
     *  Pass the live download state so the menu always matches reality. */
    isAlbumFullyDownloaded?: boolean;
  }

  let {
    isOpen,
    anchor,
    onClose,
    onPlayNext,
    onPlayLater,
    onAddToPlaylist,
    onAddToMixtape,
    onCopyToLibrary,
    onGoToAlbum,
    onGoToArtist,
    onShareQobuz,
    onShareSonglink,
    onDownload,
    onOpenContainingFolder,
    onReDownloadAlbum,
    isAlbumFullyDownloaded = false,
  }: Props = $props();

  /**
   * Minimal action popover for album cards. Renders a vertical list of
   * the six actions Discovery V2 surfaces today; portals to body so it
   * escapes the card's stacking context and can position absolutely
   * against viewport coordinates. Uses the shared `floatingMenuStore`
   * so opening it auto-closes any other open menu in the app and vice
   * versa.
   */

  // Generate a stable ID per mount. Tracks the floatingMenuStore.
  const menuId = `discovery-quickmenu-${Math.random().toString(36).slice(2, 10)}`;

  let menuEl = $state<HTMLDivElement | null>(null);
  let menuStyle = $state('');

  async function positionMenu() {
    if (!anchor || !menuEl) return;
    await tick();
    const rect = menuEl.getBoundingClientRect();
    const padding = 8;
    let left = anchor.x;
    let top = anchor.y;
    if (left + rect.width + padding > window.innerWidth) {
      left = window.innerWidth - rect.width - padding;
    }
    if (top + rect.height + padding > window.innerHeight) {
      top = anchor.y - rect.height;
    }
    left = Math.max(padding, left);
    top = Math.max(padding, top);
    menuStyle = `left: ${left}px; top: ${top}px;`;
  }

  function handleOutside(e: MouseEvent) {
    if (!isOpen) return;
    if (menuEl && menuEl.contains(e.target as Node)) return;
    onClose();
  }

  $effect(() => {
    if (!isOpen) return;
    openGlobalMenu(menuId);
    void positionMenu();
    document.addEventListener('mousedown', handleOutside);
    const onResize = () => void positionMenu();
    window.addEventListener('resize', onResize);
    window.addEventListener('scroll', onResize, true);
    return () => {
      document.removeEventListener('mousedown', handleOutside);
      window.removeEventListener('resize', onResize);
      window.removeEventListener('scroll', onResize, true);
      closeGlobalMenu(menuId);
    };
  });

  // Close if another menu opens elsewhere in the app.
  onMount(() => {
    return subscribeGlobal(() => {
      const active = getActiveMenuId();
      if (active !== null && active !== menuId && isOpen) onClose();
    });
  });

  function fire(action: (() => void) | undefined) {
    if (action) action();
    onClose();
  }

  /** Portal the menu to body so it's not constrained by ancestors
   *  that create a containing block (`transform`, `filter`,
   *  `contain`, etc.). The SearchView Albums virtual scroller wraps
   *  every row in a `transform: translateY(...)` div, which makes
   *  `position: fixed` resolve relative to that transformed row
   *  instead of the viewport — the menu would open way off where
   *  the user clicked. Mounting at body level keeps the menu in the
   *  viewport's coordinate system. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        if (node.parentNode === document.body) {
          document.body.removeChild(node);
        }
      },
    };
  }

  /** Each menu item only renders if its callback is provided.
   *  Order is intentional: queue actions first (the most common use),
   *  then library/navigation, then sharing, then offline. The
   *  `onPlayLater` callback is wired but labelled "Add to queue" — the
   *  internal name kept for backwards compat with the parent's existing
   *  per-album / per-playlist handlers; the user-facing copy is what
   *  changes. Same idea for `onDownload` → "Make available offline":
   *  the qbz product doesn't speak in terms of "Download", everything
   *  user-facing is "offline available". */
  const items = $derived([
    onPlayLater ? { labelKey: 'actions.addToQueue', icon: ListEnd, run: () => fire(onPlayLater) } : null,
    onPlayNext ? { labelKey: 'actions.playNext', icon: ListPlus, run: () => fire(onPlayNext) } : null,
    onAddToPlaylist ? { labelKey: 'actions.addToPlaylist', icon: Plus, run: () => fire(onAddToPlaylist) } : null,
    onAddToMixtape ? { labelKey: 'common.addToMixtapeOrCollection', icon: CassetteTape, run: () => fire(onAddToMixtape) } : null,
    onCopyToLibrary ? { labelKey: 'playlist.copyToLibrary', icon: Library, run: () => fire(onCopyToLibrary) } : null,
    onGoToAlbum ? { labelKey: 'actions.goToAlbum', icon: Disc, run: () => fire(onGoToAlbum) } : null,
    onGoToArtist ? { labelKey: 'actions.goToArtist', icon: User, run: () => fire(onGoToArtist) } : null,
    onShareQobuz ? { labelKey: 'actions.shareQobuz', icon: Share2, run: () => fire(onShareQobuz) } : null,
    onShareSonglink ? { labelKey: 'actions.shareSonglink', icon: LinkIcon, run: () => fire(onShareSonglink) } : null,
    // Offline / download surface — matches legacy AlbumMenu behavior:
    //  - downloaded + has onReDownloadAlbum → "Refresh offline copy"
    //  - not downloaded + has onDownload    → "Make available offline"
    // (Legacy declares `onOpenContainingFolder` for forward-compat but
    //  never renders an item for it; we mirror that.)
    isAlbumFullyDownloaded && onReDownloadAlbum
      ? { labelKey: 'actions.refreshOfflineCopy', icon: RefreshCw, run: () => fire(onReDownloadAlbum) }
      : null,
    !isAlbumFullyDownloaded && onDownload
      ? { labelKey: 'actions.makeAvailableOffline', icon: CloudOff, run: () => fire(onDownload) }
      : null,
  ].filter((item): item is NonNullable<typeof item> => item !== null));
</script>

{#if isOpen}
  <div class="menu" style={menuStyle} bind:this={menuEl} use:portal role="menu">
    {#each items as item}
      <button class="menu-item" type="button" role="menuitem" onclick={item.run}>
        <item.icon size={14} />
        <span>{$t(item.labelKey)}</span>
      </button>
    {/each}
  </div>
{/if}

<style>
  /* Minimal action popover. Solid background, no backdrop-filter (the
     P1 lesson from Modal.svelte applies here too). z-index above the
     scrollable home view but below modals. */
  .menu {
    position: fixed;
    z-index: 1000;
    min-width: 200px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    border-radius: 4px;
    font-family: inherit;
    text-align: left;
  }

  .menu-item:hover {
    background: var(--bg-tertiary);
  }
</style>
