<script lang="ts">
  import { Minus, Maximize2, Minimize2, X, Search } from 'lucide-svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';

  interface TraySettings {
    enable_tray: boolean;
    minimize_to_tray: boolean;
    close_to_tray: boolean;
  }

  interface Props {
    searchInTitlebar?: boolean;
    searchQuery?: string;
    onSearchInput?: (query: string) => void;
    onSearchClear?: () => void;
  }

  let {
    searchInTitlebar = false,
    searchQuery = '',
    onSearchInput,
    onSearchClear
  }: Props = $props();

  let isMaximized = $state(false);
  let minimizeToTray = $state(false);
  let appWindow: ReturnType<typeof getCurrentWindow>;
  let searchInputEl = $state<HTMLInputElement | null>(null);

  onMount(() => {
    let unlisten: (() => void) | undefined;

    (async () => {
      appWindow = getCurrentWindow();

      // Check initial maximized state
      isMaximized = await appWindow.isMaximized();

      // Load tray settings
      try {
        const settings = await invoke<TraySettings>('v2_get_tray_settings');
        minimizeToTray = settings.minimize_to_tray;
      } catch (e) {
        console.debug('Failed to load tray settings:', e);
      }

      // Listen for window state changes
      unlisten = await appWindow.onResized(async () => {
        isMaximized = await appWindow.isMaximized();
      });
    })();

    return () => {
      unlisten?.();
    };
  });

  async function handleMinimize() {
    // Re-read setting to pick up per-user value after login
    try {
      const settings = await invoke<TraySettings>('v2_get_tray_settings');
      minimizeToTray = settings.minimize_to_tray;
    } catch {}

    if (minimizeToTray) {
      await appWindow?.hide();
    } else {
      await appWindow?.minimize();
    }
  }

  async function handleMaximize() {
    await appWindow?.toggleMaximize();
  }

  async function handleClose() {
    await appWindow?.close();
  }

  async function handleDoubleClick(e: MouseEvent) {
    // Don't toggle maximize when double-clicking the search input
    if ((e.target as HTMLElement)?.closest('.titlebar-search')) return;
    await appWindow?.toggleMaximize();
  }

  function handleInput(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    onSearchInput?.(value);
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && searchQuery) {
      onSearchClear?.();
      e.preventDefault();
    }
  }

  export function focusSearch() {
    searchInputEl?.focus();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="titlebar" class:has-search={searchInTitlebar} ondblclick={handleDoubleClick}>
  <!-- Left drag region -->
  <div class="drag-region" data-tauri-drag-region></div>

  <!-- Search Bar (only when active) -->
  {#if searchInTitlebar}
    <div
      class="titlebar-search"
      class:has-text={searchQuery.trim().length > 0}
      data-tauri-drag-region="false"
    >
      <Search size={14} />
      <input
        type="text"
        class="titlebar-search-input"
        placeholder={$t('nav.search')}
        value={searchQuery}
        oninput={handleInput}
        onkeydown={handleSearchKeydown}
        bind:this={searchInputEl}
        data-tauri-drag-region="false"
      />
      {#if searchQuery.trim()}
        <button
          type="button"
          class="titlebar-search-clear"
          onclick={onSearchClear}
          data-tauri-drag-region="false"
        >
          <X size={12} />
        </button>
      {/if}
    </div>
    <!-- Right drag region after search -->
    <div class="drag-region" data-tauri-drag-region></div>
  {/if}

  <!-- Window Controls -->
  <div class="window-controls" data-tauri-drag-region="false">
    <button
      class="control-btn minimize"
      onclick={handleMinimize}
      title="Minimize"
      aria-label="Minimize window"
      data-tauri-drag-region="false"
    >
      <Minus size={16} strokeWidth={1.5} />
    </button>
    <button
      class="control-btn maximize"
      onclick={handleMaximize}
      title={isMaximized ? "Restore" : "Maximize"}
      aria-label={isMaximized ? "Restore window" : "Maximize window"}
      data-tauri-drag-region="false"
    >
      {#if isMaximized}
        <Minimize2 size={14} strokeWidth={1.5} />
      {:else}
        <Maximize2 size={14} strokeWidth={1.5} />
      {/if}
    </button>
    <button
      class="control-btn close"
      onclick={handleClose}
      title="Close"
      aria-label="Close window"
      data-tauri-drag-region="false"
    >
      <X size={16} strokeWidth={1.5} />
    </button>
  </div>
</header>

<style>
  .titlebar {
    height: 36px;
    min-height: 36px;
    background: linear-gradient(180deg, rgba(255,255,255,0.03) 0%, transparent 100%);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0;
    user-select: none;
    -webkit-user-select: none;
    -webkit-app-region: drag;
    app-region: drag;
  }

  .drag-region {
    flex: 1;
    height: 100%;
    cursor: default;
  }

  /* Search bar in titlebar */
  .titlebar-search {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    max-width: 400px;
    height: 24px;
    background-color: var(--bg-tertiary);
    border-radius: 6px;
    padding: 0 8px;
    border: 1px solid transparent;
    transition: background-color 150ms ease, border-color 150ms ease;
    flex-shrink: 0;
    -webkit-app-region: no-drag;
    app-region: no-drag;
    color: var(--text-muted);
  }

  .titlebar-search:hover {
    background-color: var(--bg-hover);
  }

  .titlebar-search:focus-within {
    border-color: var(--accent-primary);
    background-color: var(--bg-tertiary);
  }

  .titlebar-search :global(svg) {
    flex-shrink: 0;
  }

  .titlebar-search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    font-size: 12px;
    color: var(--text-primary);
    padding: 0;
    min-width: 0;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  .titlebar-search-input::placeholder {
    color: var(--text-muted);
  }

  .titlebar-search-clear {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    padding: 0;
    background: var(--alpha-10);
    border: none;
    border-radius: 50%;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    transition: background-color 150ms ease, color 150ms ease;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  .titlebar-search-clear:hover {
    background: var(--alpha-20);
    color: var(--text-primary);
  }

  .window-controls {
    display: flex;
    align-items: stretch;
    height: 100%;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  .control-btn {
    width: 46px;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  .control-btn:hover {
    color: var(--text-primary);
  }

  .control-btn.minimize:hover,
  .control-btn.maximize:hover {
    background-color: var(--alpha-10);
  }

  .control-btn.close:hover {
    background-color: #e81123;
    color: white;
  }

  .control-btn:active {
    opacity: 0.8;
  }

  .control-btn :global(svg) {
    pointer-events: none;
  }
</style>
