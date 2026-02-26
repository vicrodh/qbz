<script lang="ts">
  import { Minus, Maximize2, Minimize2, X, Search } from 'lucide-svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import type { ButtonColorSet } from '$lib/stores/windowControlsStore';

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
    controlsPosition?: 'right' | 'left';
    controlsShape?: 'rectangular' | 'circular' | 'square';
    controlsSize?: 'small' | 'normal' | 'large';
    controlsColors?: {
      minimize: ButtonColorSet;
      maximize: ButtonColorSet;
      close: ButtonColorSet;
    };
  }

  let {
    searchInTitlebar = false,
    searchQuery = '',
    onSearchInput,
    onSearchClear,
    controlsPosition = 'right',
    controlsShape = 'rectangular',
    controlsSize = 'normal',
    controlsColors
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

  function btnStyle(colors: ButtonColorSet | undefined): string {
    if (!colors) return '';
    return `--wc-bg:${colors.bg};--wc-bg-hover:${colors.bgHover};--wc-bg-active:${colors.bgActive};--wc-fg:${colors.fg};--wc-fg-hover:${colors.fgHover};--wc-fg-active:${colors.fgActive}`;
  }

  export function focusSearch() {
    searchInputEl?.focus();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header
  class="titlebar"
  class:has-search={searchInTitlebar}
  class:controls-left={controlsPosition === 'left'}
  ondblclick={handleDoubleClick}
>
  {#if controlsPosition === 'left'}
    <!-- Window Controls (left position - macOS order: close, maximize, minimize) -->
    <div
      class="window-controls shape-{controlsShape} size-{controlsSize}"
      class:has-custom-colors={!!controlsColors}
      data-tauri-drag-region="false"
    >
      <button
        class="control-btn close"
        onclick={handleClose}
        title="Close"
        aria-label="Close window"
        style={btnStyle(controlsColors?.close)}
        data-tauri-drag-region="false"
      >
        <X size={controlsShape === 'circular' ? 10 : controlsShape === 'square' ? 12 : 16} strokeWidth={1.5} />
      </button>
      <button
        class="control-btn maximize"
        onclick={handleMaximize}
        title={isMaximized ? "Restore" : "Maximize"}
        aria-label={isMaximized ? "Restore window" : "Maximize window"}
        style={btnStyle(controlsColors?.maximize)}
        data-tauri-drag-region="false"
      >
        {#if isMaximized}
          <Minimize2 size={controlsShape === 'circular' ? 8 : controlsShape === 'square' ? 10 : 14} strokeWidth={1.5} />
        {:else}
          <Maximize2 size={controlsShape === 'circular' ? 8 : controlsShape === 'square' ? 10 : 14} strokeWidth={1.5} />
        {/if}
      </button>
      <button
        class="control-btn minimize"
        onclick={handleMinimize}
        title="Minimize"
        aria-label="Minimize window"
        style={btnStyle(controlsColors?.minimize)}
        data-tauri-drag-region="false"
      >
        <Minus size={controlsShape === 'circular' ? 10 : controlsShape === 'square' ? 12 : 16} strokeWidth={1.5} />
      </button>
    </div>
  {/if}

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

  {#if controlsPosition === 'right'}
    <!-- Window Controls (right position - standard order: minimize, maximize, close) -->
    <div
      class="window-controls shape-{controlsShape} size-{controlsSize}"
      class:has-custom-colors={!!controlsColors}
      data-tauri-drag-region="false"
    >
      <button
        class="control-btn minimize"
        onclick={handleMinimize}
        title="Minimize"
        aria-label="Minimize window"
        style={btnStyle(controlsColors?.minimize)}
        data-tauri-drag-region="false"
      >
        <Minus size={controlsShape === 'circular' ? 10 : controlsShape === 'square' ? 12 : 16} strokeWidth={1.5} />
      </button>
      <button
        class="control-btn maximize"
        onclick={handleMaximize}
        title={isMaximized ? "Restore" : "Maximize"}
        aria-label={isMaximized ? "Restore window" : "Maximize window"}
        style={btnStyle(controlsColors?.maximize)}
        data-tauri-drag-region="false"
      >
        {#if isMaximized}
          <Minimize2 size={controlsShape === 'circular' ? 8 : controlsShape === 'square' ? 10 : 14} strokeWidth={1.5} />
        {:else}
          <Maximize2 size={controlsShape === 'circular' ? 8 : controlsShape === 'square' ? 10 : 14} strokeWidth={1.5} />
        {/if}
      </button>
      <button
        class="control-btn close"
        onclick={handleClose}
        title="Close"
        aria-label="Close window"
        style={btnStyle(controlsColors?.close)}
        data-tauri-drag-region="false"
      >
        <X size={controlsShape === 'circular' ? 10 : controlsShape === 'square' ? 12 : 16} strokeWidth={1.5} />
      </button>
    </div>
  {/if}
</header>

<style>
  .titlebar {
    height: 36px;
    min-height: 36px;
    background-color: var(--bg-secondary);
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

  /* === Window Controls Container === */

  .window-controls {
    display: flex;
    align-items: stretch;
    height: 100%;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  /* Left position: add padding */
  .controls-left .window-controls {
    padding-left: 4px;
  }

  /* Circular shape: center buttons vertically with gap */
  .window-controls.shape-circular {
    align-items: center;
    height: auto;
    gap: 6px;
    padding: 0 8px;
  }
  .window-controls.shape-circular.size-normal {
    gap: 12px;
  }
  .window-controls.shape-circular.size-large {
    gap: 15px;
  }

  /* Square shape: center buttons vertically with gap */
  .window-controls.shape-square {
    align-items: center;
    height: auto;
    gap: 4px;
    padding: 0 6px;
  }
  .window-controls.shape-square.size-normal {
    gap: 8px;
  }
  .window-controls.shape-square.size-large {
    gap: 12px;
  }

  /* === Control Buttons === */

  .control-btn {
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

  /* --- Rectangular --- */
  .shape-rectangular .control-btn {
    width: 46px;
    height: 100%;
  }
  .shape-rectangular.size-small .control-btn {
    width: 36px;
  }
  .shape-rectangular.size-large .control-btn {
    width: 56px;
  }

  /* --- Circular --- */
  .shape-circular .control-btn {
    width: 14px;
    height: 14px;
    border-radius: 50%;
  }
  .shape-circular.size-small .control-btn {
    width: 10px;
    height: 10px;
  }
  .shape-circular.size-large .control-btn {
    width: 18px;
    height: 18px;
  }

  /* --- Square --- */
  .shape-square .control-btn {
    width: 24px;
    height: 24px;
    border-radius: 4px;
  }
  .shape-square.size-small .control-btn {
    width: 18px;
    height: 18px;
  }
  .shape-square.size-large .control-btn {
    width: 30px;
    height: 30px;
  }

  /* === Default Colors (no custom colors) === */

  .window-controls:not(.has-custom-colors) .control-btn:hover {
    color: var(--text-primary);
  }

  .window-controls:not(.has-custom-colors) .control-btn.minimize:hover,
  .window-controls:not(.has-custom-colors) .control-btn.maximize:hover {
    background-color: var(--alpha-10);
  }

  .window-controls:not(.has-custom-colors) .control-btn.close:hover {
    background-color: #e81123;
    color: white;
  }

  /* Default active/clicked states */
  .window-controls:not(.has-custom-colors) .control-btn.minimize:active,
  .window-controls:not(.has-custom-colors) .control-btn.maximize:active {
    background-color: rgba(255,255,255,0.06);
    color: var(--text-muted);
  }

  .window-controls:not(.has-custom-colors) .control-btn.close:active {
    background-color: #b20f1c;
    color: white;
  }

  .control-btn :global(svg) {
    pointer-events: none;
  }

  /* === Custom Colors (CSS variable driven) === */

  .has-custom-colors .control-btn {
    background: var(--wc-bg, transparent);
    color: var(--wc-fg, var(--text-muted));
  }

  .has-custom-colors .control-btn:hover {
    background: var(--wc-bg-hover, var(--alpha-10));
    color: var(--wc-fg-hover, var(--text-primary));
  }

  .has-custom-colors .control-btn:active {
    background: var(--wc-bg-active, rgba(255,255,255,0.06));
    color: var(--wc-fg-active, var(--text-muted));
  }
</style>
