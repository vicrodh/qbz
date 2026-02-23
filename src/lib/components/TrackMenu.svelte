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
    CloudDownload,
    RefreshCw,
    Radio,
    Info,
    Search
  } from 'lucide-svelte';
  import { shouldHidePlaylistFeatures } from '$lib/utils/offlineHelpers';
  import {
    getActiveTrackMenuId,
    allocateTrackMenuId,
    setActiveTrackMenuId,
    subscribeActiveTrackMenuId
  } from '$lib/stores/activeTrackMenu';
  import { MENU_INACTIVITY_TIMEOUT } from '$lib/stores/floatingMenuStore';

  interface Props {
    onPlayNow?: () => void;
    onPlayTrackOnly?: () => void;
    onPlayFromHere?: () => void;
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onCreateQbzRadio?: () => void;
    onCreateQobuzRadio?: () => void;
    onAddFavorite?: () => void;
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
    contextMenuPosition?: { x: number; y: number } | null;
    onContextMenuClosed?: () => void;
  }

  let {
    onPlayNow,
    onPlayTrackOnly,
    onPlayFromHere,
    onPlayNext,
    onPlayLater,
    onCreateQbzRadio,
    onCreateQobuzRadio,
    onAddFavorite,
    onAddToPlaylist,
    onRemoveFromPlaylist,
    onFindReplacement,
    onShareQobuz,
    onShareSonglink,
    onGoToAlbum,
    onGoToArtist,
    onShowInfo,
    onDownload,
    isTrackDownloaded = false,
    onReDownload,
    onRemoveDownload,
    contextMenuPosition = null,
    onContextMenuClosed
  }: Props = $props();

  const menuId = allocateTrackMenuId();

  let isOpen = $state(false);
  let openSide = $state<'left' | 'right'>('left');
  let shareOpen = $state(false);
  let downloadOpen = $state(false);
  let menuRef = $state<HTMLDivElement | null>(null);
  let triggerRef = $state<HTMLButtonElement | null>(null);
  let menuEl = $state<HTMLDivElement | null>(null);
  let shareTriggerRef = $state<HTMLDivElement | null>(null);
  let downloadTriggerRef = $state<HTMLDivElement | null>(null);
  let submenuEl = $state<HTMLDivElement | null>(null);
  let downloadSubmenuEl = $state<HTMLDivElement | null>(null);
  let menuStyle = $state('');
  let submenuStyle = $state('');
  let downloadSubmenuStyle = $state('');
  let radioOpen = $state(false);
  let radioTriggerRef = $state<HTMLDivElement | null>(null);
  let radioSubmenuEl = $state<HTMLDivElement | null>(null);
  let radioSubmenuStyle = $state('');
  let isHoveringAnyMenu = $state(false);
  let isHoveringShareSubmenu = $state(false);
  let isHoveringDownloadSubmenu = $state(false);
  let isHoveringRadioSubmenu = $state(false);
  let contextMode = $state(false);

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

  const hasRadio = $derived(!!(onCreateQbzRadio || onCreateQobuzRadio));
  const hasPlayback = $derived(!!(onPlayNow || onPlayTrackOnly || onPlayFromHere || onPlayNext || onPlayLater || hasRadio));
  const hasLibrary = $derived(!!(onAddFavorite || onAddToPlaylist || onRemoveFromPlaylist || onFindReplacement));
  const hasShare = $derived(!!(onShareQobuz || onShareSonglink));
  const hasDownload = $derived(!!onDownload || isTrackDownloaded);
  const hasNav = $derived(!!(onGoToAlbum || onGoToArtist || onShowInfo));
  const hasMenu = $derived(hasPlayback || hasLibrary || hasShare || hasDownload || hasNav);

  function closeMenu(options?: { clearActive?: boolean }) {
    isOpen = false;
    shareOpen = false;
    downloadOpen = false;
    radioOpen = false;
    isHoveringShareSubmenu = false;
    isHoveringDownloadSubmenu = false;
    isHoveringRadioSubmenu = false;
    if (contextMode) {
      contextMode = false;
      onContextMenuClosed?.();
    }
    if (options?.clearActive !== false && getActiveTrackMenuId() === menuId) {
      setActiveTrackMenuId(null);
    }
  }

  async function openMenu() {
    setActiveTrackMenuId(menuId);
    isOpen = true;
    shareOpen = false;
    downloadOpen = false;
    radioOpen = false;
    if (!contextMode) {
      await setMenuPosition();
    }
  }

  function handleClickOutside(event: Event) {
    const target = event.target as Node | null;
    if (!target) return;

    // In context mode, ANY click anywhere dismisses the menu (standard OS behavior).
    if (contextMode) {
      const isInsideMenu = (menuEl && menuEl.contains(target))
        || (submenuEl && submenuEl.contains(target))
        || (downloadSubmenuEl && downloadSubmenuEl.contains(target))
        || (radioSubmenuEl && radioSubmenuEl.contains(target));
      if (!isInsideMenu) {
        closeMenu();
      }
      return;
    }

    // Trigger-button mode: check if click is outside both the trigger container and the menu (which is in portal)
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

    // Prefer opening to the left of the trigger so the menu doesn't cover the action column
    // (other context menu triggers / buttons in the same vertical column).
    const gap = 10;
    const leftPreferred = triggerRect.left - menuRect.width - gap;
    const rightPreferred = triggerRect.right + gap;

    openSide = 'left';
    let left = leftPreferred;
    if (left < padding && rightPreferred + menuRect.width <= window.innerWidth - padding) {
      openSide = 'right';
      left = rightPreferred;
    }

    // Align menu with the trigger vertically (instead of always below).
    let top = triggerRect.top - 6;

    if (left < padding) left = padding;
    if (left + menuRect.width > window.innerWidth - padding) {
      left = Math.max(padding, window.innerWidth - menuRect.width - padding);
    }

    if (top < padding) top = padding;
    if (top + menuRect.height > window.innerHeight - padding) {
      top = Math.max(padding, window.innerHeight - menuRect.height - padding);
    }

    // Arrow position inside the menu (points to the trigger)
    const arrowPadding = 12;
    const arrowTopUnclamped = triggerRect.top + triggerRect.height / 2 - top;
    const arrowTop = Math.max(arrowPadding, Math.min(menuRect.height - arrowPadding, arrowTopUnclamped));

    menuStyle = `left: ${left}px; top: ${top}px; --arrow-top: ${arrowTop}px;`;
  }

  async function setMenuPositionAtPoint(x: number, y: number) {
    await tick();
    if (!menuEl) return;

    const menuRect = menuEl.getBoundingClientRect();
    const padding = 8;

    let left = x;
    let top = y;

    if (left + menuRect.width > window.innerWidth - padding) {
      left = Math.max(padding, window.innerWidth - menuRect.width - padding);
    }
    if (left < padding) left = padding;

    if (top + menuRect.height > window.innerHeight - padding) {
      top = Math.max(padding, window.innerHeight - menuRect.height - padding);
    }
    if (top < padding) top = padding;

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

  async function setRadioSubmenuPosition() {
    await tick();
    if (!radioTriggerRef || !radioSubmenuEl) return;

    const triggerRect = radioTriggerRef.getBoundingClientRect();
    const submenuRect = radioSubmenuEl.getBoundingClientRect();
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

    radioSubmenuStyle = `left: ${left}px; top: ${top}px;`;
  }

  function handleAction(action?: () => void) {
    if (!action) return;
    action();
    closeMenu();
  }

  $effect(() => {
    const unsubscribe = subscribeActiveTrackMenuId((activeId) => {
      if (activeId !== menuId && isOpen) {
        closeMenu({ clearActive: false });
      }
    });
    return unsubscribe;
  });

  $effect(() => {
    const pos = contextMenuPosition;
    if (pos && hasMenu) {
      contextMode = true;
      void openMenu().then(() => setMenuPositionAtPoint(pos.x, pos.y));
    }
  });

  // Auto-close Share submenu after 1s of inactivity (not hovering it).
  $effect(() => {
    if (!shareOpen) return;
    if (isHoveringShareSubmenu) return;
    const timer = setTimeout(() => { shareOpen = false; }, 1000);
    return () => clearTimeout(timer);
  });

  // Auto-close Download submenu after 1s of inactivity (not hovering it).
  $effect(() => {
    if (!downloadOpen) return;
    if (isHoveringDownloadSubmenu) return;
    const timer = setTimeout(() => { downloadOpen = false; }, 1000);
    return () => clearTimeout(timer);
  });

  // Auto-close Radio submenu after 1s of inactivity (not hovering it).
  $effect(() => {
    if (!radioOpen) return;
    if (isHoveringRadioSubmenu) return;
    const timer = setTimeout(() => { radioOpen = false; }, 1000);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (isOpen) {
      // Ensure position + openSide classes are applied even if the initial openMenu()
      // call returns early due to timing.
      if (!contextMode) {
        setMenuPosition();
      }

      // Use capture phase so stopPropagation in other UI doesn't prevent close.
      // Register multiple event types for better cross-platform reliability.
      const outsideListenerOptions: AddEventListenerOptions = { capture: true };
      document.addEventListener('pointerdown', handleClickOutside, outsideListenerOptions);
      document.addEventListener('mousedown', handleClickOutside, outsideListenerOptions);
      document.addEventListener('touchstart', handleClickOutside, outsideListenerOptions);
      document.addEventListener('contextmenu', handleClickOutside, outsideListenerOptions);

      // Auto-close after inactivity when the cursor is not hovering any menu/submenu.
      // Context menus use half the idle timeout for snappier dismissal.
      let idleTimer: ReturnType<typeof setTimeout> | null = null;
      const idleMs = contextMode ? MENU_INACTIVITY_TIMEOUT / 2 : MENU_INACTIVITY_TIMEOUT;

      const scheduleIdleClose = () => {
        if (idleTimer) clearTimeout(idleTimer);
        idleTimer = setTimeout(() => {
          if (isOpen && !isHoveringAnyMenu) closeMenu();
        }, idleMs);
      };

      const cancelIdleClose = () => {
        if (idleTimer) {
          clearTimeout(idleTimer);
          idleTimer = null;
        }
      };

      // Start the timer if the mouse isn't currently hovering the menu.
      if (!isHoveringAnyMenu) scheduleIdleClose();

      const onAnyActivity = () => {
        if (!isHoveringAnyMenu) scheduleIdleClose();
      };

      window.addEventListener('pointermove', onAnyActivity, true);
      window.addEventListener('wheel', onAnyActivity, true);
      window.addEventListener('keydown', onAnyActivity, true);

      const handleResize = () => {
        if (contextMode) { closeMenu(); return; }
        setMenuPosition();
      };
      const handleScroll = () => {
        if (contextMode) { closeMenu(); return; }
        setMenuPosition();
        if (shareOpen) setSubmenuPosition();
        if (downloadOpen) setDownloadSubmenuPosition();
        if (radioOpen) setRadioSubmenuPosition();
      };

      window.addEventListener('resize', handleResize);
      window.addEventListener('scroll', handleScroll, true);
      return () => {
        document.removeEventListener('pointerdown', handleClickOutside, outsideListenerOptions);
        document.removeEventListener('mousedown', handleClickOutside, outsideListenerOptions);
        document.removeEventListener('touchstart', handleClickOutside, outsideListenerOptions);
        document.removeEventListener('contextmenu', handleClickOutside, outsideListenerOptions);
        window.removeEventListener('pointermove', onAnyActivity, true);
        window.removeEventListener('wheel', onAnyActivity, true);
        window.removeEventListener('keydown', onAnyActivity, true);
        window.removeEventListener('resize', handleResize);
        window.removeEventListener('scroll', handleScroll, true);
        cancelIdleClose();
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
        if (isOpen) {
          closeMenu();
        } else {
          openMenu();
        }
      }}
      aria-label="Track actions"
    >
      <MoreHorizontal size={18} />
    </button>

    {#if isOpen}
      <div
        class="menu"
        class:open-left={openSide === 'left'}
        class:open-right={openSide === 'right'}
        class:context-mode={contextMode}
        bind:this={menuEl}
        style={menuStyle}
        use:portal
        onmouseenter={() => { isHoveringAnyMenu = true; }}
        onmouseleave={() => { isHoveringAnyMenu = false; }}
      >
        <div class="menu-panel">
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
                <span>Add to queue</span>
              </button>
            {/if}
            {#if hasRadio}
              {#if onCreateQbzRadio && onCreateQobuzRadio}
                <div
                  class="menu-item submenu-trigger"
                  bind:this={radioTriggerRef}
                  onmouseenter={() => {
                    radioOpen = true;
                    shareOpen = false;
                    downloadOpen = false;
                    setRadioSubmenuPosition();
                  }}
                  onclick={() => {
                    radioOpen = !radioOpen;
                    if (radioOpen) { shareOpen = false; downloadOpen = false; }
                    if (radioOpen) setRadioSubmenuPosition();
                  }}
                >
                  <Radio size={14} />
                  <span>Create radio</span>
                  <ChevronRight size={14} class="chevron" />
                  {#if radioOpen}
                    <div
                      class="submenu"
                      bind:this={radioSubmenuEl}
                      style={radioSubmenuStyle}
                      onmouseenter={() => { isHoveringRadioSubmenu = true; }}
                      onmouseleave={() => { isHoveringRadioSubmenu = false; }}
                    >
                      <button class="menu-item" onclick={() => handleAction(onCreateQbzRadio)}>
                        <Radio size={14} />
                        <span>QBZ Radio</span>
                      </button>
                      <button class="menu-item" onclick={() => handleAction(onCreateQobuzRadio)}>
                        <Radio size={14} />
                        <span>Qobuz Radio</span>
                      </button>
                    </div>
                  {/if}
                </div>
              {:else}
                <button class="menu-item" onclick={() => handleAction(onCreateQbzRadio || onCreateQobuzRadio)}>
                  <Radio size={14} />
                  <span>Create radio</span>
                </button>
              {/if}
            {/if}
          {/if}

          {#if hasPlayback && (hasLibrary || hasShare || hasNav || hasDownload)}
            <div class="separator"></div>
          {/if}

          {#if hasLibrary}
            {#if onAddFavorite}
              <button class="menu-item" onclick={() => handleAction(onAddFavorite)}>
                <Heart size={14} />
                <span>Add to favorites</span>
              </button>
            {/if}
            {#if onAddToPlaylist && !shouldHidePlaylistFeatures()}
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
            {#if onFindReplacement}
              <button class="menu-item" onclick={() => handleAction(onFindReplacement)}>
                <Search size={14} />
                <span>Find available version</span>
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
                downloadOpen = false;
                radioOpen = false;
                setSubmenuPosition();
              }}
              onclick={() => {
                shareOpen = !shareOpen;
                if (shareOpen) { downloadOpen = false; radioOpen = false; }
                if (shareOpen) setSubmenuPosition();
              }}
            >
              <Share2 size={14} />
              <span>Share</span>
              <ChevronRight size={14} class="chevron" />
              {#if shareOpen}
                <div
                  class="submenu"
                  bind:this={submenuEl}
                  style={submenuStyle}
                  onmouseenter={() => { isHoveringShareSubmenu = true; }}
                  onmouseleave={() => { isHoveringShareSubmenu = false; }}
                >
                  {#if onShareQobuz}
                    <button class="menu-item" onclick={() => handleAction(onShareQobuz)}>
                      <Link size={14} />
                      <span>Qobuz™ link</span>
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
                  radioOpen = false;
                  setDownloadSubmenuPosition();
                }}
                onclick={() => {
                  downloadOpen = !downloadOpen;
                  if (downloadOpen) { shareOpen = false; radioOpen = false; }
                  if (downloadOpen) setDownloadSubmenuPosition();
                }}
              >
                <CloudDownload size={14} />
                <span>Make available offline</span>
                <ChevronRight size={14} class="chevron" />
                {#if downloadOpen}
                  <div
                    class="submenu"
                    bind:this={downloadSubmenuEl}
                    style={downloadSubmenuStyle}
                    onmouseenter={() => { isHoveringDownloadSubmenu = true; }}
                    onmouseleave={() => { isHoveringDownloadSubmenu = false; }}
                  >
                    {#if onReDownload}
                      <button class="menu-item" onclick={() => handleAction(onReDownload)}>
                        <RefreshCw size={14} />
                        <span>Refresh offline copy</span>
                      </button>
                    {/if}
                    {#if onRemoveDownload}
                      <button class="menu-item danger" onclick={() => handleAction(onRemoveDownload)}>
                        <Trash2 size={14} />
                        <span>Remove offline copy</span>
                      </button>
                    {/if}
                  </div>
                {/if}
              </div>
            {:else}
              <button class="menu-item" onclick={() => handleAction(onDownload)}>
                <CloudDownload size={14} />
                <span>Make available offline</span>
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
            {#if onShowInfo}
              <button class="menu-item" onclick={() => handleAction(onShowInfo)}>
                <Info size={14} />
                <span>Track info</span>
              </button>
            {/if}
          {/if}
        </div>
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
    z-index: 99999;
    overflow: visible;
  }

  .menu-panel {
    position: relative;
    z-index: 2;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 2px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  /* Caret outside the menu pointing at the trigger */
  .menu.open-left::after,
  .menu.open-right::after {
    content: '';
    position: absolute;
    top: var(--arrow-top, 16px);
    transform: translateY(-50%);
    width: 0;
    height: 0;
    border-top: 9px solid transparent;
    border-bottom: 9px solid transparent;
    pointer-events: none;
  }

  .menu.open-left::after {
    right: -8px;
    border-left: 9px solid var(--bg-tertiary);
  }

  .menu.open-right::after {
    left: -8px;
    border-right: 9px solid var(--bg-tertiary);
  }

  /* Shadow for caret (only outward, never over the panel) */
  .menu.open-left::before,
  .menu.open-right::before {
    content: '';
    position: absolute;
    top: var(--arrow-top, 16px);
    transform: translateY(-50%);
    width: 0;
    height: 0;
    border-top: 10px solid transparent;
    border-bottom: 10px solid transparent;
    pointer-events: none;
    opacity: 0.9;
    filter: blur(2px);
    z-index: 0;
  }

  .menu.open-left::before {
    right: -5px;
    transform: translate(4px, -50%);
    border-left: 10px solid rgba(0, 0, 0, 0.9);
  }

  .menu.open-right::before {
    left: -5px;
    transform: translate(-4px, -50%);
    border-right: 10px solid rgba(0, 0, 0, 0.9);
  }

  .menu.open-left::after,
  .menu.open-right::after {
    z-index: 1;
  }

  .menu.context-mode::after,
  .menu.context-mode::before {
    display: none;
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
