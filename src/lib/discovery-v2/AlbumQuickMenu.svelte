<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { t } from '$lib/i18n';
  import {
    ListPlus,
    ListEnd,
    Disc,
    User,
    Share2,
    Download,
    Plus,
    Link as LinkIcon,
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
    onGoToAlbum?: () => void;
    onGoToArtist?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onDownload?: () => void;
  }

  let {
    isOpen,
    anchor,
    onClose,
    onPlayNext,
    onPlayLater,
    onAddToPlaylist,
    onGoToAlbum,
    onGoToArtist,
    onShareQobuz,
    onShareSonglink,
    onDownload,
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

  /** Each menu item only renders if its callback is provided. */
  const items = $derived([
    onPlayNext ? { labelKey: 'actions.playNext', icon: ListPlus, run: () => fire(onPlayNext) } : null,
    onPlayLater ? { labelKey: 'actions.playLater', icon: ListEnd, run: () => fire(onPlayLater) } : null,
    onAddToPlaylist ? { labelKey: 'actions.addToPlaylist', icon: Plus, run: () => fire(onAddToPlaylist) } : null,
    onGoToAlbum ? { labelKey: 'actions.goToAlbum', icon: Disc, run: () => fire(onGoToAlbum) } : null,
    onGoToArtist ? { labelKey: 'actions.goToArtist', icon: User, run: () => fire(onGoToArtist) } : null,
    onShareQobuz ? { labelKey: 'actions.shareQobuz', icon: Share2, run: () => fire(onShareQobuz) } : null,
    onShareSonglink ? { labelKey: 'actions.shareSonglink', icon: LinkIcon, run: () => fire(onShareSonglink) } : null,
    onDownload ? { labelKey: 'actions.download', icon: Download, run: () => fire(onDownload) } : null,
  ].filter((item): item is NonNullable<typeof item> => item !== null));
</script>

{#if isOpen}
  <div class="menu" style={menuStyle} bind:this={menuEl} role="menu">
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
