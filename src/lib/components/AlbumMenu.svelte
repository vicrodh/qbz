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


  function handleAction(action?: () => void) {
    if (!action) return;
    action();
    closeMenu();
  }

  $effect(() => {
    if (isOpen) {
      document.addEventListener('mousedown', handleClickOutside);
      return () => {
        document.removeEventListener('mousedown', handleClickOutside);
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
      }}
      aria-label="Album actions"
    >
      <MoreHorizontal size={20} color="white" />
    </button>

    {#if isOpen}
      <div class="menu">
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
            onmouseenter={() => {
              shareOpen = true;
            }}
            onclick={() => {
              shareOpen = !shareOpen;
            }}
          >
            <Share2 size={14} />
            <span>Share</span>
            <ChevronRight size={14} class="chevron" />
            {#if shareOpen}
              <div class="submenu">
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
    position: absolute;
    right: 0;
    top: 44px;
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
    position: absolute;
    top: 0;
    right: calc(100% + 6px);
    min-width: 160px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px 0;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 10001;
  }
</style>
