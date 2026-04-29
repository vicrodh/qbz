<script lang="ts">
  import { ListPlus, ListEnd, ListMusic, Heart, HeartOff, Trash2, X, ChevronDown, CloudDownload, CassetteTape } from 'lucide-svelte';
  import { t } from 'svelte-i18n';

  interface Props {
    count: number;
    onPlayNext: () => void;
    onPlayLater: () => void;
    onAddToPlaylist: () => void;
    onAddFavorites?: () => void;
    onRemoveFavorites?: () => void;
    onRemoveFromPlaylist?: () => void;
    onAddToMixtape?: () => void;
    onRemoveFromCollection?: () => void;
    onMakeOffline?: () => void;
    onClearSelection: () => void;
    placement?: 'top' | 'bottom';
  }

  let {
    count,
    onPlayNext,
    onPlayLater,
    onAddToPlaylist,
    onAddFavorites,
    onRemoveFavorites,
    onRemoveFromPlaylist,
    onAddToMixtape,
    onRemoveFromCollection,
    onMakeOffline,
    onClearSelection,
    placement = 'bottom',
  }: Props = $props();

  let queueMenuOpen = $state(false);

  // Dock/float mechanism: a zero-height sentinel sits in the DOM where the
  // bar would naturally live (end of the selectable list). When the sentinel
  // is inside the viewport we "dock" the bar in the flow; otherwise the bar
  // floats as position:fixed just above the player bar, sized to the
  // sentinel's width so it never overlaps adjacent layout columns.
  let sentinelEl: HTMLDivElement | undefined = $state();
  let isDocked = $state(true);
  let sentinelLeft = $state(0);
  let sentinelWidth = $state(0);

  $effect(() => {
    if (!sentinelEl) return;

    const updateRect = () => {
      if (!sentinelEl) return;
      const rect = sentinelEl.getBoundingClientRect();
      sentinelLeft = rect.left;
      sentinelWidth = rect.width;
    };
    updateRect();

    const io = new IntersectionObserver(
      ([entry]) => {
        isDocked = entry.isIntersecting;
      },
      { threshold: 0 }
    );
    io.observe(sentinelEl);

    const ro = new ResizeObserver(updateRect);
    ro.observe(sentinelEl);
    window.addEventListener('resize', updateRect);

    return () => {
      io.disconnect();
      ro.disconnect();
      window.removeEventListener('resize', updateRect);
    };
  });

  function handleQueueMenuToggle(e: MouseEvent) {
    e.stopPropagation();
    queueMenuOpen = !queueMenuOpen;
  }

  function handlePlayNext() {
    queueMenuOpen = false;
    onPlayNext();
  }

  function handlePlayLater() {
    queueMenuOpen = false;
    onPlayLater();
  }

  function handleClickOutside() {
    queueMenuOpen = false;
  }
</script>

{#if count > 0}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="bulk-bar-backdrop" class:queue-open={queueMenuOpen} onclick={handleClickOutside} role="presentation">
  </div>
  <div class="bulk-bar-sentinel" bind:this={sentinelEl} aria-hidden="true"></div>
  <div
    class="bulk-bar"
    class:placement-top={placement === 'top'}
    class:floating={!isDocked}
    style:left={!isDocked ? `${sentinelLeft}px` : undefined}
    style:width={!isDocked ? `${sentinelWidth}px` : undefined}
  >
    <span class="count-label">
      {$t('actions.selectedTracks', { values: { count } })}
    </span>

    <div class="actions">
      <!-- Queue split button -->
      <div class="queue-btn-group">
        <button class="action-btn primary" onclick={handlePlayLater} title={$t('actions.addToQueue')}>
          <ListEnd size={15} />
          <span>{$t('actions.addToQueue')}</span>
        </button>
        <button
          class="action-btn primary queue-arrow"
          class:open={queueMenuOpen}
          onclick={handleQueueMenuToggle}
          title="More queue options"
        >
          <ChevronDown size={14} />
        </button>

        {#if queueMenuOpen}
          <div class="queue-dropdown">
            <button class="dropdown-item" onclick={handlePlayNext}>
              <ListPlus size={14} />
              <span>{$t('actions.playNext')}</span>
            </button>
            <button class="dropdown-item" onclick={handlePlayLater}>
              <ListEnd size={14} />
              <span>{$t('actions.addToQueue')}</span>
            </button>
          </div>
        {/if}
      </div>

      <button class="action-btn" onclick={onAddToPlaylist} title={$t('actions.addToPlaylist')}>
        <ListMusic size={15} />
        <span>{$t('actions.addToPlaylist')}</span>
      </button>

      {#if onAddToMixtape}
        <button class="action-btn" onclick={onAddToMixtape} title={$t('common.addToMixtapeOrCollection')}>
          <CassetteTape size={15} />
          <span>{$t('common.addToMixtapeOrCollection')}</span>
        </button>
      {/if}

      {#if onMakeOffline}
        <button class="action-btn" onclick={onMakeOffline} title={$t('actions.makeAvailableOffline')}>
          <CloudDownload size={15} />
          <span>{$t('actions.makeAvailableOffline')}</span>
        </button>
      {/if}

      {#if onAddFavorites}
        <button class="action-btn" onclick={onAddFavorites} title={$t('actions.addToFavorites')}>
          <Heart size={15} />
          <span>{$t('actions.addToFavorites')}</span>
        </button>
      {/if}

      {#if onRemoveFavorites}
        <button class="action-btn danger" onclick={onRemoveFavorites} title={$t('actions.removeFromFavorites')}>
          <HeartOff size={15} />
          <span>{$t('actions.removeFromFavorites')}</span>
        </button>
      {/if}

      {#if onRemoveFromPlaylist}
        <button class="action-btn danger" onclick={onRemoveFromPlaylist} title={$t('actions.removeFromPlaylist')}>
          <Trash2 size={15} />
          <span>{$t('actions.removeFromPlaylist')}</span>
        </button>
      {/if}

      {#if onRemoveFromCollection}
        <button class="action-btn danger" onclick={onRemoveFromCollection} title={$t('collectionDetail.removeItem')}>
          <Trash2 size={15} />
          <span>{$t('collectionDetail.removeItem')}</span>
        </button>
      {/if}

      <button class="clear-btn" onclick={onClearSelection} title={ $t('actions.clearSelection') }>
        <X size={16} />
      </button>
    </div>
  </div>
{/if}

<style>
  .bulk-bar-backdrop {
    display: none;
  }

  .bulk-bar-backdrop.queue-open {
    display: block;
    position: fixed;
    inset: 0;
    z-index: 99;
  }

  .bulk-bar-sentinel {
    display: block;
    width: 100%;
    height: 0;
    pointer-events: none;
  }

  .bulk-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 16px;
    background: var(--bg-secondary);
    border-top: 1px solid var(--bg-tertiary);
    border-radius: 8px 8px 0 0;
    box-shadow: 0 -4px 16px rgba(0, 0, 0, 0.25);
    z-index: 40;
    animation: slideUp 180ms ease;
  }

  .bulk-bar.floating {
    position: fixed;
    bottom: var(--player-bar-height, 104px);
  }

  .bulk-bar.placement-top {
    border-radius: 0 0 8px 8px;
    border-top: none;
    border-bottom: 1px solid var(--bg-tertiary);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    animation: slideDown 180ms ease;
  }

  .bulk-bar.placement-top.floating {
    top: 0;
    bottom: unset;
  }

  @keyframes slideUp {
    from { opacity: 0; transform: translateY(8px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  @keyframes slideDown {
    from { opacity: 0; transform: translateY(-8px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  .count-label {
    font-size: 13px;
    font-weight: 600;
    color: var(--accent-primary);
    white-space: nowrap;
    min-width: 90px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .queue-btn-group {
    position: relative;
    display: flex;
  }

  .action-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background 120ms ease;
    white-space: nowrap;
  }

  .action-btn:hover {
    background: var(--bg-hover);
  }

  .action-btn.primary {
    background: var(--accent-primary);
    color: white;
  }

  .action-btn.primary:hover {
    filter: brightness(1.1);
  }

  /* The arrow button attaches to the right of the primary queue button */
  .queue-btn-group .action-btn:first-child {
    border-radius: 6px 0 0 6px;
    padding-right: 10px;
  }

  .queue-arrow {
    border-radius: 0 6px 6px 0 !important;
    padding: 6px 7px !important;
    border-left: 1px solid rgba(255, 255, 255, 0.15) !important;
  }

  .queue-arrow.open :global(svg) {
    transform: rotate(180deg);
  }

  .queue-dropdown {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    min-width: 160px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    z-index: 100;
    overflow: hidden;
    animation: fadeIn 120ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(4px); }
    to   { opacity: 1; transform: translateY(0); }
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 9px 14px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
    transition: background 100ms;
  }

  .dropdown-item:hover {
    background: var(--bg-hover);
  }

  .action-btn.danger {
    color: var(--error, #e05454);
  }

  .action-btn.danger:hover {
    background: rgba(224, 84, 84, 0.12);
  }

  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    background: none;
    border: none;
    border-radius: 50%;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 120ms ease, background-color 120ms ease, border-color 120ms ease, opacity 120ms ease;
    margin-left: 4px;
  }

  .clear-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
</style>
