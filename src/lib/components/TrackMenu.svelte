<script lang="ts">
  import { tick } from 'svelte';
  import {
    ChevronRight,
    MoreHorizontal,
    Play,
    ListPlus,
    ListEnd,
    Heart,
    ListMusic,
    Share2,
    Disc3,
    User,
    Link,
    Trash2,
    Download,
    FolderOpen,
    RefreshCw
  } from 'lucide-svelte';

  interface Props {
    onPlayNow?: () => void;
    onPlayTrackOnly?: () => void;
    onPlayFromHere?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onAddFavorite?: () => void;
    onAddToPlaylist?: () => void;
    onRemoveFromPlaylist?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onGoToAlbum?: () => void;
    onGoToArtist?: () => void;
    onDownload?: () => void;
    isTrackDownloaded?: boolean;
    onOpenFolder?: () => void;
    onReDownload?: () => void;
  }

  let {
    onPlayNow,
    onPlayTrackOnly,
    onPlayFromHere,
    onPlayNext,
    onPlayLater,
    onAddFavorite,
    onAddToPlaylist,
    onRemoveFromPlaylist,
    onShareQobuz,
    onShareSonglink,
    onGoToAlbum,
    onGoToArtist,
    onDownload,
    isTrackDownloaded = false,
    onOpenFolder,
    onReDownload
  }: Props = $props();

  let isOpen = $state(false);
  let shareOpen = $state(false);
  let downloadOpen = $state(false);
  let menuRef: HTMLDivElement | null = null;
  let triggerRef: HTMLButtonElement | null = null;
  let menuEl: HTMLDivElement | null = null;
  let shareTriggerRef: HTMLDivElement | null = null;
  let downloadTriggerRef: HTMLDivElement | null = null;
  let submenuEl: HTMLDivElement | null = null;
  let downloadSubmenuEl: HTMLDivElement | null = null;
  let menuStyle = $state('');
  let submenuStyle = $state('');
  let downloadSubmenuStyle = $state('');

  // Portal action - moves element to body to escape stacking context
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        if (node.parentNode) {
          node.parentNode.removeChild(node);
        }
      }
    };
  }

  const hasPlayback = $derived(!!(onPlayNow || onPlayTrackOnly || onPlayFromHere || onPlayNext || onPlayLater));
  const hasLibrary = $derived(!!(onAddFavorite || onAddToPlaylist || onRemoveFromPlaylist));
  const hasShare = $derived(!!(onShareQobuz || onShareSonglink));
  const hasDownload = $derived(!!onDownload || isTrackDownloaded);
  const hasNav = $derived(!!(onGoToAlbum || onGoToArtist));
  const hasMenu = $derived(hasPlayback || hasLibrary || hasShare || hasDownload || hasNav);

  function closeMenu() {
    isOpen = false;
    shareOpen = false;
    downloadOpen = false;
  }

  function handleClickOutside(event: MouseEvent) {
    const target = event.target as Node;
    // Check if click is outside both the trigger container and the menu (which is in portal)
    const isOutsideTrigger = menuRef && !menuRef.contains(target);
    const isOutsideMenu = menuEl && !menuEl.contains(target);
    const isOutsideSubmenu = submenuEl && !submenuEl.contains(target);
    const isOutsideDownloadSubmenu = downloadSubmenuEl && !downloadSubmenuEl.contains(target);
    if (isOutsideTrigger && isOutsideMenu && isOutsideSubmenu && isOutsideDownloadSubmenu) {
      closeMenu();
    }
  }

  async function setMenuPosition() {
    await tick();
    if (!triggerRef || !menuEl) return;

    const triggerRect = triggerRef.getBoundingClientRect();
    const menuRect = menuEl.getBoundingClientRect();
    const padding = 8;

    let left = triggerRect.right - menuRect.width;
    let top = triggerRect.bottom + 6;

    if (left < padding) left = padding;
    if (left + menuRect.width > window.innerWidth - padding) {
      left = Math.max(padding, window.innerWidth - menuRect.width - padding);
    }

    if (top + menuRect.height > window.innerHeight - padding) {
      top = triggerRect.top - menuRect.height - 6;
      if (top < padding) top = padding;
    }

    menuStyle = `left: ${left}px; top: ${top}px;`;
  }

  async function setSubmenuPosition() {
    await tick();
    if (!shareTriggerRef || !submenuEl) return;

    const triggerRect = shareTriggerRef.getBoundingClientRect();
    const submenuRect = submenuEl.getBoundingClientRect();
    const padding = 8;

    const spaceRight = window.innerWidth - triggerRect.right;
    const openRight = spaceRight >= submenuRect.width + padding;

    let left = openRight
      ? triggerRect.right + 6
      : triggerRect.left - submenuRect.width - 6;
    let top = triggerRect.top - 6;

    if (left < padding) left = padding;
    if (left + submenuRect.width > window.innerWidth - padding) {
      left = Math.max(padding, window.innerWidth - submenuRect.width - padding);
    }

    if (top + submenuRect.height > window.innerHeight - padding) {
      top = window.innerHeight - submenuRect.height - padding;
    }
    if (top < padding) top = padding;

    submenuStyle = `left: ${left}px; top: ${top}px;`;
  }

  async function setDownloadSubmenuPosition() {
    await tick();
    if (!downloadTriggerRef || !downloadSubmenuEl) return;

    const triggerRect = downloadTriggerRef.getBoundingClientRect();
    const submenuRect = downloadSubmenuEl.getBoundingClientRect();
    const padding = 8;

    const spaceRight = window.innerWidth - triggerRect.right;
    const openRight = spaceRight >= submenuRect.width + padding;

    let left = openRight
      ? triggerRect.right + 6
      : triggerRect.left - submenuRect.width - 6;
    let top = triggerRect.top - 6;

    if (left < padding) left = padding;
    if (left + submenuRect.width > window.innerWidth - padding) {
      left = Math.max(padding, window.innerWidth - submenuRect.width - padding);
    }

    if (top + submenuRect.height > window.innerHeight - padding) {
      top = window.innerHeight - submenuRect.height - padding;
    }
    if (top < padding) top = padding;

    downloadSubmenuStyle = `left: ${left}px; top: ${top}px;`;
  }

  function handleAction(action?: () => void) {
    if (!action) return;
    action();
    closeMenu();
  }

  $effect(() => {
    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);
      const handleResize = () => setMenuPosition();
      const handleScroll = () => {
        setMenuPosition();
        if (shareOpen) setSubmenuPosition();
        if (downloadOpen) setDownloadSubmenuPosition();
      };

      window.addEventListener('resize', handleResize);
      window.addEventListener('scroll', handleScroll, true);
      return () => {
        document.removeEventListener('mousedown', handleClickOutside);
        window.removeEventListener('resize', handleResize);
        window.removeEventListener('scroll', handleScroll, true);
      };
    }
  });
</script>

{#if hasMenu}
  <div
    class="track-menu"
    bind:this={menuRef}
    onmousedown={(e) => e.stopPropagation()}
    onclick={(e) => e.stopPropagation()}
  >
    <button
      class="menu-trigger"
      bind:this={triggerRef}
      onclick={(e) => {
        e.stopPropagation();
        isOpen = !isOpen;
        shareOpen = false;
        if (isOpen) setMenuPosition();
      }}
      aria-label="Track actions"
    >
      <MoreHorizontal size={18} />
    </button>

    {#if isOpen}
      <div class="menu" bind:this={menuEl} style={menuStyle} use:portal>
        {#if hasPlayback}
          {#if onPlayNow}
            <button class="menu-item" onclick={() => handleAction(onPlayNow)}>
              <Play size={14} />
              <span>Play now</span>
            </button>
          {/if}
          {#if onPlayTrackOnly}
            <button class="menu-item" onclick={() => handleAction(onPlayTrackOnly)}>
              <Play size={14} />
              <span>Play track only</span>
            </button>
          {/if}
          {#if onPlayFromHere}
            <button class="menu-item" onclick={() => handleAction(onPlayFromHere)}>
              <Play size={14} />
              <span>Play from here</span>
            </button>
          {/if}
          {#if onPlayNext}
            <button class="menu-item" onclick={() => handleAction(onPlayNext)}>
              <ListPlus size={14} />
              <span>Play next</span>
            </button>
          {/if}
          {#if onPlayLater}
            <button class="menu-item" onclick={() => handleAction(onPlayLater)}>
              <ListEnd size={14} />
              <span>Play later</span>
            </button>
          {/if}
        {/if}

        {#if hasPlayback && (hasLibrary || hasShare || hasNav)}
          <div class="separator"></div>
        {/if}

        {#if hasLibrary}
          {#if onAddFavorite}
            <button class="menu-item" onclick={() => handleAction(onAddFavorite)}>
              <Heart size={14} />
              <span>Add to favorites</span>
            </button>
          {/if}
          {#if onAddToPlaylist}
            <button class="menu-item" onclick={() => handleAction(onAddToPlaylist)}>
              <ListMusic size={14} />
              <span>Add to playlist</span>
            </button>
          {/if}
          {#if onRemoveFromPlaylist}
            <button class="menu-item danger" onclick={() => handleAction(onRemoveFromPlaylist)}>
              <Trash2 size={14} />
              <span>Remove from playlist</span>
            </button>
          {/if}
        {/if}

        {#if hasLibrary && (hasShare || hasNav)}
          <div class="separator"></div>
        {/if}

        {#if hasShare}
          <div
            class="menu-item submenu-trigger"
            bind:this={shareTriggerRef}
            onmouseenter={() => {
              shareOpen = true;
              setSubmenuPosition();
            }}
            onclick={() => {
              shareOpen = !shareOpen;
              if (shareOpen) setSubmenuPosition();
            }}
          >
            <Share2 size={14} />
            <span>Share</span>
            <ChevronRight size={14} class="chevron" />
            {#if shareOpen}
              <div class="submenu" bind:this={submenuEl} style={submenuStyle}>
                {#if onShareQobuz}
                  <button class="menu-item" onclick={() => handleAction(onShareQobuz)}>
                    <Link size={14} />
                    <span>Qobuz link</span>
                  </button>
                {/if}
                {#if onShareSonglink}
                  <button class="menu-item" onclick={() => handleAction(onShareSonglink)}>
                    <Link size={14} />
                    <span>Song.link</span>
                  </button>
                {/if}
              </div>
            {/if}
          </div>
        {/if}

        {#if (hasShare || hasLibrary) && hasDownload}
          <div class="separator"></div>
        {/if}

        {#if hasDownload}
          {#if isTrackDownloaded}
            <div
              class="menu-item submenu-trigger"
              bind:this={downloadTriggerRef}
              onmouseenter={() => {
                downloadOpen = true;
                shareOpen = false;
                setDownloadSubmenuPosition();
              }}
              onclick={() => {
                downloadOpen = !downloadOpen;
                shareOpen = false;
                if (downloadOpen) setDownloadSubmenuPosition();
              }}
            >
              <Download size={14} />
              <span>Download</span>
              <ChevronRight size={14} class="chevron" />
              {#if downloadOpen}
                <div class="submenu" bind:this={downloadSubmenuEl} style={downloadSubmenuStyle}>
                  {#if onOpenFolder}
                    <button class="menu-item" onclick={() => handleAction(onOpenFolder)}>
                      <FolderOpen size={14} />
                      <span>Open containing folder</span>
                    </button>
                  {/if}
                  {#if onReDownload}
                    <button class="menu-item" onclick={() => handleAction(onReDownload)}>
                      <RefreshCw size={14} />
                      <span>Re-download</span>
                    </button>
                  {/if}
                </div>
              {/if}
            </div>
          {:else}
            <button class="menu-item" onclick={() => handleAction(onDownload)}>
              <Download size={14} />
              <span>Download</span>
            </button>
          {/if}
        {/if}

        {#if hasDownload && hasNav}
          <div class="separator"></div>
        {/if}

        {#if hasNav}
          {#if onGoToAlbum}
            <button class="menu-item" onclick={() => handleAction(onGoToAlbum)}>
              <Disc3 size={14} />
              <span>Go to album</span>
            </button>
          {/if}
          {#if onGoToArtist}
            <button class="menu-item" onclick={() => handleAction(onGoToArtist)}>
              <User size={14} />
              <span>Go to artist</span>
            </button>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .track-menu {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .menu-trigger {
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 6px;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .menu-trigger:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .menu {
    position: fixed;
    min-width: 160px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 2px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 99999;
  }

  .menu-item {
    width: 100%;
    padding: 8px 12px;
    background: none;
    border: none;
    color: var(--text-secondary);
    text-align: left;
    font-size: 12px;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 8px;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .menu-item span {
    flex: 1;
  }

  .menu-item :global(.chevron) {
    margin-left: auto;
  }

  .menu-item:hover {
    background-color: var(--bg-hover);
    color: var(--text-primary);
  }

  .menu-item.danger {
    color: #ef4444;
  }

  .menu-item.danger:hover {
    background-color: rgba(239, 68, 68, 0.1);
    color: #ef4444;
  }

  .separator {
    height: 1px;
    background-color: var(--bg-hover);
    margin: 4px 0;
  }

  .submenu-trigger {
    position: relative;
  }

  .submenu {
    position: fixed;
    min-width: 150px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 2px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 100000;
  }
</style>
