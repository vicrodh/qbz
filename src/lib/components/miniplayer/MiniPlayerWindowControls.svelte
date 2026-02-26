<script lang="ts">
  import { Disc3, Image, ListMusic, AlignLeft, Maximize2, Pin, Move, X, Square } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { MiniPlayerSurface } from './types';

  interface Props {
    activeSurface: MiniPlayerSurface;
    isPinned: boolean;
    onSurfaceChange: (surface: MiniPlayerSurface) => void;
    onTogglePin: () => void;
    onExpand: () => void;
    onClose: () => void;
    onStartDrag: (event: MouseEvent) => void;
  }

  let { activeSurface, isPinned, onSurfaceChange, onTogglePin, onExpand, onClose, onStartDrag }: Props = $props();

  let isExpanded = $state(false);
  let collapseTimer: ReturnType<typeof setTimeout> | null = null;

  const surfaceTabs: { id: MiniPlayerSurface; icon: typeof Disc3; labelKey: string }[] = [
    { id: 'compact', icon: Disc3, labelKey: 'player.miniSurfaceCompact' },
    { id: 'artwork', icon: Image, labelKey: 'player.miniSurfaceArtwork' },
    { id: 'queue', icon: ListMusic, labelKey: 'player.miniSurfaceQueue' },
    { id: 'lyrics', icon: AlignLeft, labelKey: 'player.miniSurfaceLyrics' }
  ];

  function handleMouseEnter(): void {
    if (collapseTimer) {
      clearTimeout(collapseTimer);
      collapseTimer = null;
    }
    isExpanded = true;
  }

  function handleMouseLeave(): void {
    collapseTimer = setTimeout(() => {
      isExpanded = false;
      collapseTimer = null;
    }, 250);
  }
</script>

<div
  class="window-controls"
  class:expanded={isExpanded}
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
  role="group"
  aria-label={$t('player.miniWindowControls')}
>
  <div class="expanded-buttons">
    <div class="surface-tabs">
      {#each surfaceTabs as surfaceTab (surfaceTab.id)}
        <button
          class="surface-tab"
          class:active={activeSurface === surfaceTab.id}
          onclick={() => onSurfaceChange(surfaceTab.id)}
          title={$t(surfaceTab.labelKey)}
          aria-label={$t(surfaceTab.labelKey)}
        >
          <surfaceTab.icon size={13} />
        </button>
      {/each}
    </div>

    <div class="separator"></div>

    <div class="window-actions">
      <button
        class="window-btn"
        onclick={onExpand}
        title={$t('player.miniExpand')}
        aria-label={$t('player.miniExpand')}
      >
        <Maximize2 size={13} />
      </button>

      <button
        class="window-btn"
        class:active={isPinned}
        onclick={onTogglePin}
        title={isPinned ? $t('player.miniAlwaysOnTopDisable') : $t('player.miniAlwaysOnTopEnable')}
        aria-label={isPinned ? $t('player.miniAlwaysOnTopDisable') : $t('player.miniAlwaysOnTopEnable')}
      >
        <Pin size={13} />
      </button>

      <button
        class="window-btn drag-btn"
        onmousedown={(event) => {
          event.preventDefault();
          onStartDrag(event);
        }}
        title={$t('player.dragWindow')}
        aria-label={$t('player.dragWindow')}
      >
        <Move size={12} />
      </button>

      <button
        class="window-btn close"
        onclick={onClose}
        title={$t('player.miniClose')}
        aria-label={$t('player.miniClose')}
      >
        <X size={13} />
      </button>
    </div>
  </div>

  <button class="window-trigger" title={$t('player.miniWindowControls')} aria-label={$t('player.miniWindowControls')}>
    <Square size={12} />
  </button>
</div>

<style>
  .window-controls {
    position: relative;
    display: flex;
    align-items: center;
    background: rgba(6, 7, 10, 0.9);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: 999px;
    padding: 2px;
    overflow: hidden;
    -webkit-app-region: no-drag;
    app-region: no-drag;
    transition: box-shadow 180ms ease, opacity 140ms ease, transform 140ms ease;
  }

  .window-controls.expanded {
    box-shadow: 0 8px 20px rgba(0, 0, 0, 0.4);
  }

  .window-trigger {
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--alpha-75);
    cursor: pointer;
    transition: background 120ms ease, color 120ms ease;
    flex-shrink: 0;
  }

  .window-trigger:hover {
    background: var(--alpha-12);
    color: var(--text-primary);
  }

  .expanded-buttons {
    display: flex;
    align-items: center;
    gap: 3px;
    max-width: 0;
    opacity: 0;
    overflow: hidden;
    transition: max-width 220ms cubic-bezier(0.4, 0, 0.2, 1), opacity 160ms ease;
  }

  .window-controls.expanded .expanded-buttons {
    max-width: 280px;
    opacity: 1;
    margin-right: 3px;
  }

  .surface-tabs,
  .window-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .surface-tab,
  .window-btn {
    width: 22px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--alpha-65);
    cursor: pointer;
    transition: background 120ms ease, color 120ms ease;
  }

  .surface-tab:hover,
  .window-btn:hover {
    background: var(--alpha-12);
    color: var(--text-primary);
  }

  .surface-tab.active,
  .window-btn.active {
    color: var(--accent-primary);
  }

  .window-btn.close:hover {
    background: rgba(239, 68, 68, 0.8);
    color: white;
  }

  .window-btn.drag-btn {
    cursor: grab;
  }

  .window-btn.drag-btn:active {
    cursor: grabbing;
  }

  .separator {
    width: 1px;
    height: 12px;
    background: rgba(255, 255, 255, 0.16);
    margin: 0 1px;
  }
</style>
