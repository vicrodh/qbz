<script lang="ts">
  import { Minus, Maximize2, Minimize2, X } from 'lucide-svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount } from 'svelte';

  let isMaximized = $state(false);
  let appWindow: ReturnType<typeof getCurrentWindow>;

  onMount(async () => {
    appWindow = getCurrentWindow();

    // Check initial maximized state
    isMaximized = await appWindow.isMaximized();

    // Listen for window state changes
    const unlisten = await appWindow.onResized(async () => {
      isMaximized = await appWindow.isMaximized();
    });

    return () => {
      unlisten();
    };
  });

  async function handleMinimize() {
    await appWindow?.minimize();
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
  <!-- Drag region - uses CSS app-region: drag -->
  <div class="drag-region"></div>

  <!-- Window Controls -->
  <div class="window-controls">
    <button
      class="control-btn minimize"
      onclick={handleMinimize}
      title="Minimize"
      aria-label="Minimize window"
    >
      <Minus size={16} strokeWidth={1.5} />
    </button>
    <button
      class="control-btn maximize"
      onclick={handleMaximize}
      title={isMaximized ? "Restore" : "Maximize"}
      aria-label={isMaximized ? "Restore window" : "Maximize window"}
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
    background-color: rgba(255, 255, 255, 0.1);
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
