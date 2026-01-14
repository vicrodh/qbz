<script lang="ts">
  import { tick } from 'svelte';
  import {
    ChevronRight,
    MoreHorizontal,
    ListPlus,
    ListEnd,
    Share2,
    Download,
    Link
  } from 'lucide-svelte';

  interface Props {
    onPlayNext?: () => void;
    onPlayLater?: () => void;
    onShareQobuz?: () => void;
    onShareSonglink?: () => void;
    onDownload?: () => void;
  }

  let {
    onPlayNext,
    onPlayLater,
    onShareQobuz,
    onShareSonglink,
    onDownload
  }: Props = $props();

  let isOpen = $state(false);
  let shareOpen = $state(false);
  let menuRef: HTMLDivElement | null = null;
  let triggerRef: HTMLButtonElement | null = null;
  let menuEl: HTMLDivElement | null = null;
  let shareTriggerRef: HTMLDivElement | null = null;
  let submenuEl: HTMLDivElement | null = null;
  let menuStyle = $state('');
  let submenuStyle = $state('');

  const hasQueue = $derived(!!(onPlayNext || onPlayLater));
  const hasShare = $derived(!!(onShareQobuz || onShareSonglink));
  const hasDownload = $derived(!!onDownload);
  const hasMenu = $derived(hasQueue || hasShare || hasDownload);

  function closeMenu() {
    isOpen = false;
    shareOpen = false;
  }

  function handleClickOutside(event: MouseEvent) {
    if (menuRef && !menuRef.contains(event.target as Node)) {
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

  function handleAction(action?: () => void) {
    if (!action) return;
    action();
    closeMenu();
  }

  $effect(() => {
    if (isOpen && menuEl && triggerRef) {
      setMenuPosition();
    }
  });

  $effect(() => {
    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);
      const handleResize = () => setMenuPosition();
      const handleScroll = () => {
        setMenuPosition();
        if (shareOpen) setSubmenuPosition();
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
        isOpen = !isOpen;
        shareOpen = false;
        if (isOpen) setMenuPosition();
      }}
      aria-label="Album actions"
    >
      <MoreHorizontal size={20} color="white" />
    </button>

    {#if isOpen}
      <div class="menu" bind:this={menuEl} style={menuStyle}>
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

        {#if hasShare && hasDownload}
          <div class="separator"></div>
        {/if}

        {#if hasDownload}
          <button class="menu-item" onclick={() => handleAction(onDownload)}>
            <Download size={14} />
            <span>Download album</span>
          </button>
        {/if}
      </div>
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
    min-width: 180px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10000;
  }

  .menu-item {
    width: 100%;
    padding: 10px 14px;
    background: none;
    border: none;
    color: var(--text-secondary);
    text-align: left;
    font-size: 13px;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 10px;
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
    min-width: 160px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10001;
  }
</style>
