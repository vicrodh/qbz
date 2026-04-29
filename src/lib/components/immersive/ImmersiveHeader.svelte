<script lang="ts">
  import { Disc3, Disc, MicVocal, ListMusic, Music2, Info, Radio, Maximize, Minimize, ChevronDown, X, Square, Copy, Minus, Image, Activity, AudioWaveform, CircleDot, Crosshair, Zap, HeartPulse, Move } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { getCurrentWindow } from '@tauri-apps/api/window';

  export type ImmersiveTab = 'lyrics' | 'trackInfo' | 'suggestions' | 'queue';
  export type FocusTab = 'coverflow' | 'static' | 'visualizer' | 'neon-flow' | 'tunnel-flow' | 'comet-flow' | 'oscilloscope' | 'spectral-ribbon' | 'energy-bands' | 'lissajous' | 'transient-pulse' | 'album-reactive' | 'linebed' | 'lyrics-focus' | 'queue-focus';
  export type ViewMode = 'focus' | 'split';

  const VISUALIZER_TABS: FocusTab[] = ['visualizer', 'neon-flow', 'tunnel-flow', 'comet-flow', 'oscilloscope', 'spectral-ribbon', 'energy-bands', 'lissajous', 'transient-pulse', 'album-reactive', 'linebed'];

  interface Props {
    viewMode: ViewMode;
    activeTab: ImmersiveTab;
    activeFocusTab: FocusTab;
    onViewModeChange: (mode: ViewMode) => void;
    onTabChange: (tab: ImmersiveTab) => void;
    onFocusTabChange: (tab: FocusTab) => void;
    onClose: () => void;
    onCloseApp?: () => void;
    visible?: boolean;
    hasLyrics?: boolean;
    hasTrackInfo?: boolean;
    hasSuggestions?: boolean;
    isFullscreen?: boolean;
    isMaximized?: boolean;
    onToggleFullscreen?: () => void;
    onToggleMaximize?: () => void;
    onMinimize?: () => void;
  }

  let {
    viewMode,
    activeTab,
    activeFocusTab,
    onViewModeChange,
    onTabChange,
    onFocusTabChange,
    onClose,
    onCloseApp,
    visible = true,
    hasLyrics = true,
    hasTrackInfo = true,
    hasSuggestions = true,
    isFullscreen = false,
    isMaximized = false,
    onToggleFullscreen,
    onToggleMaximize,
    onMinimize
  }: Props = $props();

  // Expandable window controls state
  let isWindowControlsExpanded = $state(false);
  let collapseTimeout: ReturnType<typeof setTimeout> | null = null;

  // Visualizer dropdown state
  let isVizDropdownOpen = $state(false);
  let vizDropdownTimeout: ReturnType<typeof setTimeout> | null = null;
  let isNeonSubmenuOpen = $state(false);

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

  function handleVizDropdownEnter() {
    if (vizDropdownTimeout) {
      clearTimeout(vizDropdownTimeout);
      vizDropdownTimeout = null;
    }
    isVizDropdownOpen = true;
  }

  function handleVizDropdownLeave() {
    vizDropdownTimeout = setTimeout(() => {
      isVizDropdownOpen = false;
      isNeonSubmenuOpen = false;
    }, 250);
  }

  function selectVisualizerTab(tab: FocusTab) {
    onFocusTabChange(tab);
    isVizDropdownOpen = false;
    isNeonSubmenuOpen = false;
  }

  async function handleCloseApp() {
    if (onCloseApp) {
      onCloseApp();
    } else {
      const window = getCurrentWindow();
      await window.close();
    }
  }

  async function handleDragStart() {
    try {
      const window = getCurrentWindow();
      await window.startDragging();
    } catch {
      // Ignore drag errors (e.g., fullscreen mode)
    }
  }

  // Split mode tabs
  // Use labelKey pattern — never call $t() inside $derived()
  const splitTabs = $derived([
    { id: 'lyrics' as const, labelKey: 'player.lyrics', icon: Music2, enabled: hasLyrics },
    { id: 'trackInfo' as const, labelKey: 'player.trackInfo', icon: Info, enabled: hasTrackInfo },
    { id: 'suggestions' as const, labelKey: 'player.suggestions', icon: Radio, enabled: hasSuggestions },
    { id: 'queue' as const, labelKey: 'player.queue', icon: ListMusic, enabled: true },
  ].filter(tab => tab.enabled));

  // Top-level focus tabs (non-visualizer)
  const focusTabsTop: { id: FocusTab; labelKey: string; icon: typeof Disc3 }[] = [
    { id: 'coverflow', labelKey: 'settings.appearance.immersiveViews.coverflow', icon: Disc3 },
    { id: 'static', labelKey: 'settings.appearance.immersiveViews.static', icon: Image },
  ];

  const focusTabsBottom: { id: FocusTab; labelKey: string; icon: typeof Disc3 }[] = [
    { id: 'lyrics-focus', labelKey: 'settings.appearance.immersiveViews.lyrics-focus', icon: MicVocal },
    { id: 'queue-focus', labelKey: 'settings.appearance.immersiveViews.queue-focus', icon: ListMusic },
  ];

  // Visualizer sub-options
  const vizOptions: { id: FocusTab; labelKey: string; icon: typeof Activity }[] = [
    { id: 'visualizer', labelKey: 'settings.appearance.immersiveFps.panels.visualizer', icon: Activity },
    { id: 'oscilloscope', labelKey: 'settings.appearance.immersiveFps.panels.oscilloscope', icon: AudioWaveform },
    { id: 'spectral-ribbon', labelKey: 'settings.appearance.immersiveFps.panels.spectral-ribbon', icon: AudioWaveform },
    { id: 'energy-bands', labelKey: 'settings.appearance.immersiveFps.panels.energy-bands', icon: CircleDot },
    { id: 'lissajous', labelKey: 'settings.appearance.immersiveFps.panels.lissajous', icon: Crosshair },
    { id: 'transient-pulse', labelKey: 'settings.appearance.immersiveFps.panels.transient-pulse', icon: Zap },
    { id: 'album-reactive', labelKey: 'settings.appearance.immersiveFps.panels.album-reactive', icon: HeartPulse },
    { id: 'linebed', labelKey: 'settings.appearance.immersiveFps.panels.linebed', icon: AudioWaveform },
  ];

  const isVisualizerActive = $derived(VISUALIZER_TABS.includes(activeFocusTab));
  const activeVizOption = $derived(vizOptions.find(opt => opt.id === activeFocusTab));
  const activeVizLabelKey = $derived(
    activeFocusTab === 'neon-flow'
      ? 'settings.appearance.immersiveFps.panels.neon-laser'
      : activeFocusTab === 'tunnel-flow'
        ? 'settings.appearance.immersiveFps.panels.tunnel-flow'
        : activeFocusTab === 'comet-flow'
          ? 'settings.appearance.immersiveFps.panels.comet-flow'
        : (activeVizOption?.labelKey ?? 'settings.appearance.immersiveViews.visualizer')
  );

  const isFocusMode = $derived(viewMode === 'focus');
</script>

<header class="immersive-header" class:visible>
  <!-- Left: Spacer for balance -->
  <div class="header-left" data-tauri-drag-region></div>

  <!-- Center: Mode toggle + Tabs -->
  <nav class="tabs">
    <!-- Mode toggle button (icon shows target mode) -->
    <button
      class="mode-toggle"
      onclick={() => onViewModeChange(isFocusMode ? 'split' : 'focus')}
      title={isFocusMode ? $t('actions.switchToSplitView') : $t('actions.switchToImmersiveView')}
    >
      <img
        src={isFocusMode ? '/split-view.svg' : '/lotus.svg'}
        alt={isFocusMode ? 'Split Mode' : 'Immersive Mode'}
        class="mode-icon"
      />
    </button>

    <div class="tab-divider"></div>
    {#if isFocusMode}
      <!-- Top-level tabs before visualizer -->
      {#each focusTabsTop as tab (tab.id)}
        <button
          class="tab"
          class:active={activeFocusTab === tab.id}
          onclick={() => onFocusTabChange(tab.id)}
        >
          <tab.icon size={16} />
          <span class="tab-label">{$t(tab.labelKey)}</span>
        </button>
      {/each}

      <!-- Visualizer dropdown -->
      <div
        class="viz-dropdown-wrapper"
        onmouseenter={handleVizDropdownEnter}
        onmouseleave={handleVizDropdownLeave}
        role="group"
      >
        <button
          class="tab"
          class:active={isVisualizerActive}
          onclick={() => {
            if (!isVisualizerActive) {
              onFocusTabChange('spectral-ribbon');
            } else {
              isVizDropdownOpen = !isVizDropdownOpen;
            }
          }}
        >
          {#if activeVizOption}
            {@const VizIcon = activeVizOption.icon}
            <VizIcon size={16} />
          {:else if activeFocusTab === 'neon-flow'}
            <img src="/laser.svg" alt="" class="viz-img-icon" aria-hidden="true" />
          {:else if activeFocusTab === 'tunnel-flow'}
            <img src="/cube-svgrepo-com.svg" alt="" class="viz-img-icon" aria-hidden="true" />
          {:else if activeFocusTab === 'comet-flow'}
            <Disc size={16} />
          {:else}
            <Activity size={16} />
          {/if}
          <span class="tab-label">
            {isVisualizerActive ? $t(activeVizLabelKey) : $t('settings.appearance.immersiveViews.visualizer')}
          </span>
          <ChevronDown size={12} class="viz-chevron" />
        </button>

        {#if isVizDropdownOpen}
          <div class="viz-dropdown">
            {#each vizOptions as opt (opt.id)}
              <button
                class="viz-dropdown-item"
                class:active={activeFocusTab === opt.id}
                onclick={() => selectVisualizerTab(opt.id)}
              >
                <opt.icon size={14} />
                <span>{$t(opt.labelKey)}</span>
              </button>
            {/each}

            <div
              class="viz-dropdown-submenu-wrapper"
              onmouseenter={() => (isNeonSubmenuOpen = true)}
              onmouseleave={() => (isNeonSubmenuOpen = false)}
              role="group"
            >
              <button
                class="viz-dropdown-item"
                class:active={activeFocusTab === 'neon-flow' || activeFocusTab === 'tunnel-flow' || activeFocusTab === 'comet-flow'}
                onclick={() => (isNeonSubmenuOpen = !isNeonSubmenuOpen)}
              >
                <img src="/bulb.svg" alt="" class="viz-img-icon" aria-hidden="true" />
                <span>{$t('settings.appearance.immersiveViews.neon')}</span>
                <ChevronDown size={12} class="submenu-chevron" />
              </button>

              {#if isNeonSubmenuOpen}
                <div class="viz-submenu">
                  <button
                    class="viz-dropdown-item"
                    class:active={activeFocusTab === 'neon-flow'}
                    onclick={() => selectVisualizerTab('neon-flow')}
                  >
                    <img src="/laser.svg" alt="" class="viz-img-icon" aria-hidden="true" />
                    <span>{$t('settings.appearance.immersiveFps.panels.neon-laser')}</span>
                  </button>
                  <button
                    class="viz-dropdown-item"
                    class:active={activeFocusTab === 'tunnel-flow'}
                    onclick={() => selectVisualizerTab('tunnel-flow')}
                  >
                    <img src="/cube-svgrepo-com.svg" alt="" class="viz-img-icon" aria-hidden="true" />
                    <span>{$t('settings.appearance.immersiveFps.panels.tunnel-flow')}</span>
                  </button>
                  <button
                    class="viz-dropdown-item"
                    class:active={activeFocusTab === 'comet-flow'}
                    onclick={() => selectVisualizerTab('comet-flow')}
                  >
                    <Disc size={14} />
                    <span>{$t('settings.appearance.immersiveFps.panels.comet-flow')}</span>
                  </button>
                </div>
              {/if}
            </div>
          </div>
        {/if}
      </div>

      <!-- Top-level tabs after visualizer -->
      {#each focusTabsBottom as tab (tab.id)}
        <button
          class="tab"
          class:active={activeFocusTab === tab.id}
          onclick={() => onFocusTabChange(tab.id)}
        >
          <tab.icon size={16} />
          <span class="tab-label">{$t(tab.labelKey)}</span>
        </button>
      {/each}
    {:else}
      {#each splitTabs as tab (tab.id)}
        <button
          class="tab"
          class:active={activeTab === tab.id}
          onclick={() => onTabChange(tab.id)}
        >
          <tab.icon size={16} />
          <span class="tab-label">{$t(tab.labelKey)}</span>
        </button>
      {/each}
    {/if}
  </nav>

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
          class="window-btn drag-btn"
          onmousedown={handleDragStart}
          title={$t('player.dragWindow')}
        >
          <Move size={16} />
        </button>
        <button
          class="window-btn"
          onclick={onToggleFullscreen}
          title={isFullscreen ? $t('player.exitFullscreenWithKey', { values: { key: "F11" } }) : $t('player.fullscreenWithKey', { values: { key: "F11" } })}
        >
          {#if isFullscreen}
            <Minimize size={16} />
          {:else}
            <Maximize size={16} />
          {/if}
        </button>
        <button
          class="window-btn"
          onclick={onToggleMaximize}
          title={isMaximized ? $t('player.restoreWindow') : $t('player.maximizeWindow')}
        >
          {#if isMaximized}
            <Copy size={14} />
          {:else}
            <Square size={14} />
          {/if}
        </button>
        <button
          class="window-btn"
          onclick={onMinimize}
          title={$t('player.minimize')}
        >
          <Minus size={16} />
        </button>
        <button
          class="window-btn"
          onclick={onClose}
          title={$t('player.exitImmersiveWithKey', { values: { key: $t('keys.esc') } })}
        >
          <ChevronDown size={16} />
        </button>
        <button
          class="window-btn close"
          onclick={handleCloseApp}
          title={$t('player.closeApp')}
        >
          <X size={16} />
        </button>
      </div>

      <!-- Default icon (window) -->
      <button class="window-trigger" title="{ $t('player.windowControls') }">
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
    height: 100%;
    cursor: grab;
    -webkit-app-region: drag;
    app-region: drag;
  }

  .mode-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 6px;
    background: none;
    border: none;
    border-radius: 8px;
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
  }

  .mode-toggle:hover {
    background: rgba(255, 255, 255, 0.12);
  }

  .mode-icon {
    width: 20px;
    height: 20px;
    filter: invert(1) opacity(0.85);
    transition: filter 150ms ease;
  }

  .mode-toggle:hover .mode-icon {
    filter: invert(1) opacity(1);
  }

  .tab-divider {
    width: 1px;
    height: 20px;
    background: rgba(255, 255, 255, 0.2);
    margin: 0 4px;
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    background: rgba(0, 0, 0, 0.5);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 12px;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: none;
    border: none;
    border-radius: 8px;
    color: rgba(255, 255, 255, 0.7);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
  }

  .tab:hover {
    color: rgba(255, 255, 255, 0.95);
    background: rgba(255, 255, 255, 0.12);
  }

  .tab.active {
    color: var(--text-primary, white);
    background: rgba(255, 255, 255, 0.2);
  }

  /* Visualizer dropdown */
  .viz-dropdown-wrapper {
    position: relative;
  }

  :global(.viz-chevron) {
    opacity: 0.5;
    transition: transform 150ms ease;
  }

  .viz-dropdown,
  .viz-submenu {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px;
    background: rgba(0, 0, 0, 0.85);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 10px;
    backdrop-filter: blur(16px);
    min-width: 140px;
  }

  .viz-dropdown {
    position: absolute;
    top: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    z-index: 30;
    animation: dropdownFadeIn 150ms ease;
  }

  @keyframes dropdownFadeIn {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  .viz-dropdown-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 14px;
    background: none;
    border: none;
    border-radius: 6px;
    color: rgba(255, 255, 255, 0.7);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: color 120ms ease, background-color 120ms ease, border-color 120ms ease, opacity 120ms ease;
    white-space: nowrap;
    text-align: left;
  }

  .viz-dropdown-submenu-wrapper {
    position: relative;
    width: 100%;
  }

  .viz-submenu {
    position: absolute;
    bottom: 0;
    left: calc(100% + 2px);
    z-index: 31;
    width: 100%;
    min-width: unset;
    animation: submenuFadeIn 120ms ease;
  }

  /* Invisible bridge to prevent mouseout gap between Neon item and submenu */
  .viz-dropdown-submenu-wrapper::after {
    content: '';
    position: absolute;
    top: 0;
    right: -8px;
    width: 8px;
    height: 100%;
    display: none;
  }

  .viz-dropdown-submenu-wrapper:hover::after {
    display: block;
  }

  @keyframes submenuFadeIn {
    from {
      opacity: 0;
      transform: translateX(-4px);
    }
    to {
      opacity: 1;
      transform: translateX(0);
    }
  }

  :global(.submenu-chevron) {
    margin-left: auto;
    transform: rotate(-90deg);
    opacity: 0.5;
    transition: transform 150ms ease;
  }

  .viz-img-icon {
    width: 14px;
    height: 14px;
    filter: var(--icon-filter, brightness(0) invert(1));
    opacity: 0.9;
  }

  .viz-dropdown-item:hover {
    color: rgba(255, 255, 255, 0.95);
    background: rgba(255, 255, 255, 0.12);
  }

  .viz-dropdown-item.active {
    color: var(--text-primary, white);
    background: rgba(255, 255, 255, 0.18);
  }

  .header-actions {
    flex: 1;
    min-width: 100px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    -webkit-app-region: no-drag;
    app-region: no-drag;
  }

  /* Expandable Window Controls */
  .window-controls {
    position: relative;
    display: flex;
    align-items: center;
    background: rgba(0, 0, 0, 0.5);
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 20px;
    padding: 4px;
    overflow: hidden;
    transition: opacity 250ms cubic-bezier(0.4, 0, 0.2, 1), background-color 250ms cubic-bezier(0.4, 0, 0.2, 1);
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
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
    flex-shrink: 0;
  }

  .window-trigger:hover {
    background: rgba(255, 255, 255, 0.12);
  }

  .window-icon {
    width: 18px;
    height: 18px;
    filter: invert(1) opacity(0.85);
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
    transition: max-width 250ms cubic-bezier(0.4, 0, 0.2, 1), opacity 250ms cubic-bezier(0.4, 0, 0.2, 1);
  }

  .window-controls.expanded .expanded-buttons {
    max-width: 240px;
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
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
    flex-shrink: 0;
  }

  .window-btn:hover {
    color: var(--text-primary, white);
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
  }

  .window-btn.drag-btn {
    cursor: grab;
  }

  .window-btn.drag-btn:active {
    cursor: grabbing;
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

  }
</style>
