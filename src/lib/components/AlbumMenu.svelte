<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Portal from './Portal.svelte';
  import {
    ChevronRight,
    MoreHorizontal,
    ListPlus,
    ListEnd,
    Share2,
    CloudDownload,
    Link,
    RefreshCw
  } from 'lucide-svelte';

  interface Props {
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onDownload?: () => void;
    onOpenChange?: (open: boolean) => void;
    isAlbumFullyDownloaded?: boolean;
    onOpenContainingFolder?: () => void;
    onReDownloadAlbum?: () => void;
  }

  let {
    onPlayNext,
    onPlayLater,
    onShareQobuz,
    onShareSonglink,
    onDownload,
    onOpenChange,
    isAlbumFullyDownloaded = false,
    onOpenContainingFolder: _onOpenContainingFolder,
    onReDownloadAlbum
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
  let portalTarget: HTMLElement | null = null;
  const menuId = Symbol('album-menu');

  const hasQueue = $derived(!!(onPlayNext || onPlayLater));
  const hasShare = $derived(!!(onShareQobuz || onShareSonglink));
  const hasDownload = $derived(!!onDownload || isAlbumFullyDownloaded);
  const hasMenu = $derived(hasQueue || hasShare || hasDownload);

  function closeMenu() {
    isOpen = false;
    shareOpen = false;
    downloadOpen = false;
    onOpenChange?.(false);
  }

  function handleClickOutside(event: MouseEvent) {
    const target = event.target as Node;
    if (menuRef?.contains(target)) return;
    if (menuEl?.contains(target)) return;
    if (submenuEl?.contains(target)) return;
    if (downloadSubmenuEl?.contains(target)) return;
    closeMenu();
  }

  onMount(() => {
    portalTarget = document.body;
    const handleOtherOpen = (event: Event) => {
      const detail = (event as CustomEvent<symbol>).detail;
      if (detail !== menuId && isOpen) {
        closeMenu();
      }
    };
    window.addEventListener('qbz-album-menu-open', handleOtherOpen as EventListener);
    return () => {
      window.removeEventListener('qbz-album-menu-open', handleOtherOpen as EventListener);
    };
  });

  async function setMenuPosition(retries = 2) {
    await tick();
    if (!triggerRef || !menuEl) {
      if (retries > 0) {
        await tick();
        return setMenuPosition(retries - 1);
      }
      return;
    }

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

  async function setSubmenuPosition(retries = 2) {
    await tick();
    if (!shareTriggerRef || !submenuEl) {
      if (retries > 0) {
        await tick();
        return setSubmenuPosition(retries - 1);
      }
      return;
    }

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

  async function setDownloadSubmenuPosition(retries = 2) {
    await tick();
    if (!downloadTriggerRef || !downloadSubmenuEl) {
      if (retries > 0) {
        await tick();
        return setDownloadSubmenuPosition(retries - 1);
      }
      return;
    }

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
      setMenuPosition();
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
    class="album-menu"
    bind:this={menuRef}
    onmousedown={(e) => e.stopPropagation()}
    onclick={(e) => e.stopPropagation()}
  >
    <button
      class="menu-trigger icon-btn"
      bind:this={triggerRef}
      onclick={(e) => {
        e.stopPropagation();
        const nextOpen = !isOpen;
        isOpen = nextOpen;
        shareOpen = false;
        downloadOpen = false;
        onOpenChange?.(nextOpen);
        if (nextOpen) {
          window.dispatchEvent(new CustomEvent('qbz-album-menu-open', { detail: menuId }));
          setMenuPosition();
        }
      }}
      aria-label="Album actions"
    >
      <MoreHorizontal size={20} color="white" />
    </button>

    {#if isOpen && portalTarget}
      <Portal target={portalTarget}>
        <div class="menu" bind:this={menuEl} style={menuStyle} onmousedown={(e) => e.stopPropagation()}>
          {#if hasQueue}
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

          {#if hasQueue && (hasShare || hasDownload)}
            <div class="separator"></div>
          {/if}

          {#if hasShare}
            <div
              class="menu-item submenu-trigger"
              bind:this={shareTriggerRef}
              onmouseenter={() => {
                shareOpen = true;
                downloadOpen = false;
                setSubmenuPosition();
              }}
              onclick={() => {
                shareOpen = !shareOpen;
                downloadOpen = false;
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

          {#if hasShare && hasDownload}
            <div class="separator"></div>
          {/if}

          {#if hasDownload}
            {#if isAlbumFullyDownloaded}
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
                <CloudDownload size={14} />
                <span>Save album for offline</span>
                <ChevronRight size={14} class="chevron" />
                {#if downloadOpen}
                  <div class="submenu" bind:this={downloadSubmenuEl} style={downloadSubmenuStyle}>
                    {#if onReDownloadAlbum}
                      <button class="menu-item" onclick={() => handleAction(onReDownloadAlbum)}>
                        <RefreshCw size={14} />
                        <span>Refresh album offline copy</span>
                      </button>
                    {/if}
                  </div>
                {/if}
              </div>
            {:else}
              <button class="menu-item" onclick={() => handleAction(onDownload)}>
                <CloudDownload size={14} />
                <span>Save album for offline</span>
              </button>
            {/if}
          {/if}
        </div>
      </Portal>
    {/if}
  </div>
{/if}

<style>
  .album-menu {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .menu-trigger {
    width: 40px;
    height: 40px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: white;
    cursor: pointer;
    border-radius: 50%;
    transition: background-color 150ms ease;
  }

  .menu-trigger:hover {
    background-color: rgba(255, 255, 255, 0.1);
  }

  .menu {
    position: fixed;
    min-width: 160px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 2px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 30000;
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
    z-index: 30001;
  }
</style>
