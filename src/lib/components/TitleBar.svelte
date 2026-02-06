<script lang="ts">
  import { Minus, Maximize2, Minimize2, X } from 'lucide-svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  interface TraySettings {
    enable_tray: boolean;
    minimize_to_tray: boolean;
    close_to_tray: boolean;
  }

  let isMaximized = $state(false);
  let minimizeToTray = $state(false);
  let appWindow: ReturnType<typeof getCurrentWindow>;

  onMount(() => {
    let unlisten: (() => void) | undefined;

    (async () => {
      appWindow = getCurrentWindow();

      // Check initial maximized state
      isMaximized = await appWindow.isMaximized();

      // Load tray settings
      try {
        const settings = await invoke<TraySettings>('get_tray_settings');
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

  async function handleDoubleClick() {
    await appWindow?.toggleMaximize();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header class="titlebar" ondblclick={handleDoubleClick}>
  <!-- Drag region only (avoid marking the whole header as draggable so buttons remain clickable) -->
  <div class="drag-region" data-tauri-drag-region></div>

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
    height: 32px;
    min-height: 32px;
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
