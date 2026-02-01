<script lang="ts">
  import { Disc3, LayoutGrid, Mic2, ListMusic, Music2, Info, Radio, Maximize, Minimize, ChevronDown, X } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { getCurrentWindow } from '@tauri-apps/api/window';

  export type ImmersiveTab = 'lyrics' | 'trackInfo' | 'suggestions' | 'queue';
  export type DisplayMode = 'coverflow' | 'split' | 'lyrics-focus' | 'queue-focus';

  interface Props {
    activeTab: ImmersiveTab;
    displayMode: DisplayMode;
    onTabChange: (tab: ImmersiveTab) => void;
    onDisplayModeChange: (mode: DisplayMode) => void;
    onClose: () => void;
    onCloseApp?: () => void;
    visible?: boolean;
    hasLyrics?: boolean;
    hasTrackInfo?: boolean;
    hasSuggestions?: boolean;
    isFullscreen?: boolean;
    onToggleFullscreen?: () => void;
  }

  let {
    activeTab,
    displayMode,
    onTabChange,
    onDisplayModeChange,
    onClose,
    onCloseApp,
    visible = true,
    hasLyrics = true,
    hasTrackInfo = true,
    hasSuggestions = true,
    isFullscreen = false,
    onToggleFullscreen
  }: Props = $props();

  // Expandable window controls state
  let isWindowControlsExpanded = $state(false);
  let collapseTimeout: ReturnType<typeof setTimeout> | null = null;

  function handleWindowControlsEnter() {
    if (collapseTimeout) {
      clearTimeout(collapseTimeout);
      collapseTimeout = null;
    }
    isWindowControlsExpanded = true;
  }

  function handleWindowControlsLeave() {
    collapseTimeout = setTimeout(() => {
      isWindowControlsExpanded = false;
    }, 300);
  }

  async function handleCloseApp() {
    if (onCloseApp) {
      onCloseApp();
    } else {
      const window = getCurrentWindow();
      await window.close();
    }
  }

  const tabs = $derived([
    { id: 'lyrics' as const, label: $t('player.lyrics'), icon: Music2, enabled: hasLyrics },
    { id: 'trackInfo' as const, label: $t('player.trackInfo') || 'Track Info', icon: Info, enabled: hasTrackInfo },
    { id: 'suggestions' as const, label: $t('player.suggestions') || 'Suggestions', icon: Radio, enabled: hasSuggestions },
    { id: 'queue' as const, label: $t('player.queue') || 'Queue', icon: ListMusic, enabled: true },
  ].filter(tab => tab.enabled));

  const displayModes: { id: DisplayMode; icon: typeof Disc3; title: string }[] = [
    { id: 'coverflow', icon: Disc3, title: 'Coverflow (1)' },
    { id: 'split', icon: LayoutGrid, title: 'Split View (2)' },
    { id: 'lyrics-focus', icon: Mic2, title: 'Lyrics Focus (3)' },
    { id: 'queue-focus', icon: ListMusic, title: 'Queue Focus (4)' },
  ];

  // Only show tabs in split mode
  const showTabs = $derived(displayMode === 'split');
</script>

<header class="immersive-header" class:visible>
  <!-- Left: Display mode switcher -->
  <div class="header-left">
    <div class="mode-switcher">
      {#each displayModes as mode (mode.id)}
        <button
          class="mode-btn"
          class:active={displayMode === mode.id}
          onclick={() => onDisplayModeChange(mode.id)}
          title={mode.title}
        >
          <mode.icon size={18} />
        </button>
      {/each}
    </div>
  </div>

  <!-- Center: Tabs (only in split mode) -->
  {#if showTabs}
    <nav class="tabs">
      {#each tabs as tab (tab.id)}
        <button
          class="tab"
          class:active={activeTab === tab.id}
          onclick={() => onTabChange(tab.id)}
        >
          <tab.icon size={16} />
          <span class="tab-label">{tab.label}</span>
        </button>
      {/each}
    </nav>
  {:else}
    <div class="tabs-placeholder"></div>
  {/if}

  <!-- Right: Expandable Window Controls -->
  <div class="header-actions">
    <div
      class="window-controls"
      class:expanded={isWindowControlsExpanded}
      onmouseenter={handleWindowControlsEnter}
      onmouseleave={handleWindowControlsLeave}
      role="group"
      aria-label="Window controls"
    >
      <!-- Expanded buttons (appear on hover) -->
      <div class="expanded-buttons">
        <button
          class="window-btn"
          onclick={onToggleFullscreen}
          title={isFullscreen ? 'Exit Fullscreen (F11)' : 'Fullscreen (F11)'}
        >
          {#if isFullscreen}
            <Minimize size={16} />
          {:else}
            <Maximize size={16} />
          {/if}
        </button>
        <button
          class="window-btn"
          onclick={onClose}
          title="Exit Immersive (Esc)"
        >
          <ChevronDown size={16} />
        </button>
        <button
          class="window-btn close"
          onclick={handleCloseApp}
          title="Close App"
        >
          <X size={16} />
        </button>
      </div>

      <!-- Default icon (window) -->
      <button class="window-trigger" title="Window Controls">
        <img src="/window.svg" alt="Window" class="window-icon" />
      </button>
    </div>
  </div>
</header>

<style>
  .immersive-header {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 20;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    opacity: 0;
    transform: translateY(-8px);
    transition: opacity 250ms ease, transform 250ms ease;
    pointer-events: none;
  }

  .immersive-header.visible {
    opacity: 1;
    transform: translateY(0);
    pointer-events: auto;
  }

  .header-left {
    flex: 1;
    min-width: 100px;
  }

  .mode-switcher {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    background: rgba(0, 0, 0, 0.6);
    border: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    border-radius: 10px;
  }

  .mode-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    background: none;
    border: none;
    border-radius: 8px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    cursor: pointer;
    transition: all 150ms ease;
  }

  .mode-btn:hover {
    color: var(--alpha-80, rgba(255, 255, 255, 0.8));
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .mode-btn.active {
    color: var(--text-primary, white);
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
  }

  .tabs-placeholder {
    flex: 1;
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    background: rgba(0, 0, 0, 0.6);
    border: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    border-radius: 12px;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: none;
    border: none;
    border-radius: 8px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .tab:hover {
    color: var(--alpha-90, rgba(255, 255, 255, 0.9));
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .tab.active {
    color: var(--text-primary, white);
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
  }

  .header-actions {
    flex: 1;
    min-width: 100px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  /* Expandable Window Controls */
  .window-controls {
    position: relative;
    display: flex;
    align-items: center;
    background: rgba(0, 0, 0, 0.6);
    border: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    border-radius: 20px;
    padding: 4px;
    overflow: hidden;
    transition: all 250ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  .window-trigger {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: none;
    border: none;
    border-radius: 50%;
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .window-trigger:hover {
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .window-icon {
    width: 18px;
    height: 18px;
    filter: invert(1) opacity(0.7);
    transition: filter 150ms ease;
  }

  .window-trigger:hover .window-icon {
    filter: invert(1) opacity(1);
  }

  .expanded-buttons {
    display: flex;
    align-items: center;
    gap: 2px;
    max-width: 0;
    opacity: 0;
    overflow: hidden;
    transition: all 250ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  .window-controls.expanded .expanded-buttons {
    max-width: 120px;
    opacity: 1;
    margin-right: 4px;
  }

  .window-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: none;
    border: none;
    border-radius: 50%;
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .window-btn:hover {
    color: var(--text-primary, white);
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
  }

  .window-btn.close:hover {
    color: white;
    background: rgba(239, 68, 68, 0.8);
  }

  /* Responsive */
  @media (max-width: 900px) {
    .tabs {
      padding: 3px;
    }

    .tab {
      padding: 8px 12px;
    }

    .tab-label {
      display: none;
    }
  }

  @media (max-width: 600px) {
    .immersive-header {
      padding: 12px 16px;
    }

    .mode-switcher {
      padding: 3px;
    }

    .mode-btn {
      width: 30px;
      height: 30px;
    }

    .action-btn {
      width: 36px;
      height: 36px;
    }
  }
</style>
