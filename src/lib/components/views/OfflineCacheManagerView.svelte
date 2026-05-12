<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import { ArrowLeft, Pencil } from 'lucide-svelte';
  import { ask } from '@tauri-apps/plugin-dialog';
  import { invoke } from '@tauri-apps/api/core';
  import Dropdown from '$lib/components/Dropdown.svelte';
  import Modal from '$lib/discovery-v2/ModalLite.svelte';
  import { offlineCacheManagerStore } from '$lib/stores/offlineCacheManagerStore.svelte';
  import { showToast } from '$lib/stores/toastStore';

  type Props = {
    onBack: () => void;
    onGoToAlbum: (albumId: string) => void;
    onGoToFavorites: () => void;
  };
  const { onBack, onGoToAlbum, onGoToFavorites }: Props = $props();

  const store = offlineCacheManagerStore;

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  type SortKey = 'alpha' | 'recent' | 'largest' | 'smallest';

  function sortKeyToLabel(key: SortKey): string {
    switch (key) {
      case 'alpha': return $t('offlineManager.sort.alpha');
      case 'recent': return $t('offlineManager.sort.recent');
      case 'largest': return $t('offlineManager.sort.largest');
      case 'smallest': return $t('offlineManager.sort.smallest');
    }
  }

  function labelToSortKey(label: string): SortKey {
    if (label === $t('offlineManager.sort.alpha')) return 'alpha';
    if (label === $t('offlineManager.sort.recent')) return 'recent';
    if (label === $t('offlineManager.sort.largest')) return 'largest';
    return 'smallest';
  }

  function getSortOptions(): string[] {
    return [
      $t('offlineManager.sort.alpha'),
      $t('offlineManager.sort.recent'),
      $t('offlineManager.sort.largest'),
      $t('offlineManager.sort.smallest'),
    ];
  }

  onMount(async () => {
    store.setSinglesLabel($t('offlineManager.singlesPseudoAlbum'));
    await store.loadAll();
    await store.subscribeToProgress();
  });

  onDestroy(() => {
    store.unsubscribe();
  });

  const alphabetLetters = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ#'.split('');

  const groupedByLetter = $derived.by(() => {
    const map = new Map<string, typeof store.artists>();
    for (const artist of store.artists) {
      const first = artist.artistName.charAt(0).toUpperCase();
      const letter = /[A-Z]/.test(first) ? first : '#';
      if (!map.has(letter)) map.set(letter, []);
      map.get(letter)!.push(artist);
    }
    return alphabetLetters
      .filter(l => map.has(l))
      .map(l => ({ letter: l, artists: map.get(l)! }));
  });

  const presentLetters = $derived(new Set(groupedByLetter.map(g => g.letter)));

  let railEl: HTMLDivElement | null = $state(null);

  function jumpToLetter(letter: string) {
    const target = railEl?.querySelector<HTMLElement>(`[data-letter="${letter}"]`);
    target?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  const selectedArtist = $derived(store.selectedArtist());
  const visibleAlbums = $derived(store.visibleAlbumGroups());

  type AlbumActionTarget = {
    albumId: string | null;
    title: string;
    totalSizeBytes: number;
    cachedTracks: Array<{ trackId: number }>;
  };

  async function onAlbumAction(album: AlbumActionTarget, action: 'redownload' | 'retryFailed' | 'remove' | 'goToAlbum') {
    switch (action) {
      case 'redownload': {
        if (!album.albumId) return;
        const { queuedTrackIds } = await store.redownloadAlbum(album.albumId, false);
        showToast(
          $t('offlineManager.toast.redownloadingAlbum', { values: { count: queuedTrackIds.length } }),
          'info',
        );
        break;
      }
      case 'retryFailed': {
        if (!album.albumId) return;
        const { queuedTrackIds } = await store.redownloadAlbum(album.albumId, true);
        showToast(
          $t('offlineManager.toast.redownloadingAlbum', { values: { count: queuedTrackIds.length } }),
          'info',
        );
        break;
      }
      case 'remove': {
        const confirmed = await ask(
          $t('offlineManager.confirmRemoveAlbum.body', {
            values: { album: album.title, size: formatBytes(album.totalSizeBytes) },
          }),
          { title: $t('offlineManager.confirmRemoveAlbum.title'), kind: 'warning' },
        );
        if (!confirmed) return;
        if (album.albumId) {
          const result = await store.removeAlbum(album.albumId);
          showToast(
            $t('offlineManager.toast.removedAlbum', {
              values: { album: album.title, size: formatBytes(result.freedBytes) },
            }),
            'success',
          );
        } else {
          // Singles bucket: iterate per-track removal.
          for (const track of album.cachedTracks) {
            await store.removeTrack(track.trackId);
          }
          showToast(
            $t('offlineManager.toast.removedAlbum', {
              values: { album: album.title, size: formatBytes(album.totalSizeBytes) },
            }),
            'success',
          );
        }
        break;
      }
      case 'goToAlbum': {
        if (album.albumId) onGoToAlbum(album.albumId);
        break;
      }
    }
  }

  // ── Limit modal (Fix #5b) ────────────────────────────────────────────────
  let isLimitModalOpen = $state(false);
  let limitGbInput = $state(5);
  let savingLimit = $state(false);

  function openLimitModal() {
    const limitBytes = store.stats?.limitBytes ?? 5 * 1024 * 1024 * 1024;
    // Round to nearest GB so the number input shows a clean integer.
    limitGbInput = Math.max(5, Math.round(limitBytes / (1024 * 1024 * 1024)));
    isLimitModalOpen = true;
  }

  function closeLimitModal() {
    if (savingLimit) return;
    isLimitModalOpen = false;
  }

  async function saveLimit() {
    const gb = Number(limitGbInput);
    if (!Number.isFinite(gb) || gb < 5) {
      showToast($t('offlineManager.toast.limitTooLow'), 'error');
      return;
    }
    savingLimit = true;
    try {
      // Backend takes limit in megabytes; 1 GB = 1024 MB.
      await invoke('v2_set_offline_cache_limit', { limitMb: Math.round(gb * 1024) });
      isLimitModalOpen = false;
      await store.loadAll();
      showToast($t('offlineManager.toast.limitSaved'), 'success');
    } catch (err) {
      console.error('[OfflineCacheManager] Failed to save limit:', err);
      showToast(String(err), 'error');
    } finally {
      savingLimit = false;
    }
  }

  async function onTrackAction(track: { trackId: number; title: string }, action: 'redownload' | 'remove') {
    switch (action) {
      case 'redownload':
        await store.redownloadTrack(track.trackId);
        showToast($t('offlineManager.toast.redownloadingTrack'), 'info');
        break;
      case 'remove': {
        const confirmed = await ask(
          $t('offlineManager.confirmRemoveTrack.body', { values: { title: track.title } }),
          { title: $t('offlineManager.confirmRemoveTrack.title'), kind: 'warning' },
        );
        if (!confirmed) return;
        await store.removeTrack(track.trackId);
        showToast($t('offlineManager.toast.removedTrack'), 'success');
        break;
      }
    }
  }
</script>

<div class="offline-cache-manager">
  <header class="ocm-header">
    <button type="button" class="back-btn" onclick={onBack} aria-label={$t('actions.back')}>
      <ArrowLeft size={18} />
    </button>
    <h1>{$t('offlineManager.title')}</h1>
  </header>

  {#if store.loading}
    <div class="ocm-loading">{$t('actions.loading')}</div>
  {:else if store.artists.length === 0}
    <div class="ocm-empty">
      <h2>{$t('offlineManager.empty.title')}</h2>
      <p>{$t('offlineManager.empty.body')}</p>
      <button type="button" class="empty-cta" onclick={onGoToFavorites}>
        {$t('offlineManager.empty.cta')}
      </button>
    </div>
  {:else}
    <section class="ocm-stats">
      {#if store.stats}
        <span class="ocm-stat-totals">
          {$t('offlineManager.stats.totals', {
            values: {
              tracks: store.stats.totalTracks,
              albums: store.artists.reduce((s, a) => s + a.albumGroups.length, 0),
              artists: store.artists.length,
            },
          })}
        </span>
        <span class="ocm-stat-usage">
          {#if store.stats.limitBytes}
            {$t('offlineManager.stats.usage', {
              values: {
                used: formatBytes(store.stats.totalSizeBytes),
                limit: formatBytes(store.stats.limitBytes),
              },
            })}
            <span class="ocm-usage-bar">
              <span
                class="ocm-usage-fill"
                style:width="{Math.min(100, (store.stats.totalSizeBytes / store.stats.limitBytes) * 100)}%"
              ></span>
            </span>
          {:else}
            {$t('offlineManager.stats.unlimited', {
              values: { used: formatBytes(store.stats.totalSizeBytes) },
            })}
          {/if}
          <button
            type="button"
            class="ocm-limit-edit"
            onclick={openLimitModal}
            aria-label={$t('offlineManager.limitModal.editAria')}
            title={$t('offlineManager.limitModal.title')}
          >
            <Pencil size={14} />
          </button>
        </span>
      {/if}

      <div class="ocm-controls">
        <span class="ocm-sort-label">{$t('offlineManager.sort.label')}</span>
        <Dropdown
          value={sortKeyToLabel(store.sort)}
          options={getSortOptions()}
          onchange={(label) => store.setSort(labelToSortKey(label))}
        />
        <label class="ocm-toggle">
          <input
            type="checkbox"
            checked={store.showOnlyFailed}
            onchange={(e) => store.setShowOnlyFailed((e.target as HTMLInputElement).checked)}
          />
          <span>{$t('offlineManager.filter.showOnlyFailed')}</span>
        </label>
      </div>
    </section>

    <div class="ocm-body">
      <aside class="ocm-rail">
        <div class="ocm-alpha-bar">
          {#each alphabetLetters as letter (letter)}
            <button
              type="button"
              class="alpha-letter"
              class:disabled={!presentLetters.has(letter)}
              onclick={() => jumpToLetter(letter)}
            >{letter}</button>
          {/each}
        </div>
        <div class="ocm-rail-list" bind:this={railEl}>
          {#each groupedByLetter as group (group.letter)}
            <div class="rail-letter-section" data-letter={group.letter}>
              <h3 class="rail-letter">{group.letter}</h3>
              {#each group.artists as artist (artist.artistKey)}
                <button
                  type="button"
                  class="rail-artist"
                  class:active={artist.artistKey === store.selectedArtistKey}
                  onclick={() => store.selectArtist(artist.artistKey)}
                >
                  <span class="rail-artist-name" title={artist.artistName}>{artist.artistName}</span>
                  <span class="rail-artist-meta">
                    {artist.albumGroups.length} · {artist.totalTracks}
                  </span>
                </button>
              {/each}
            </div>
          {/each}
        </div>
      </aside>

      <main class="ocm-pane">
        {#if selectedArtist}
          <div class="pane-heading">
            <h2>{$t('offlineManager.discographyHeading')}</h2>
            <span class="pane-subhead">
              {$t('offlineManager.albumsCount', { values: { count: visibleAlbums.length } })}
            </span>
          </div>

          {#each visibleAlbums as album (album.albumId ?? '__singles__')}
            {@const isExpanded = album.albumId ? store.expandedAlbums.has(album.albumId) : false}
            <div class="album-row" class:expanded={isExpanded}>
              <button
                type="button"
                class="album-summary"
                onclick={() => album.albumId && store.toggleExpand(album.albumId)}
              >
                <span class="chevron">{isExpanded ? '⌄' : '▸'}</span>
                {#if album.coverUrl}
                  <img class="album-cover" src={album.coverUrl} alt="" loading="lazy" />
                {:else}
                  <div class="album-cover-placeholder"></div>
                {/if}
                <div class="album-meta">
                  <div class="album-title">{album.title}</div>
                  <div class="album-subtitle">{album.artistLabel}</div>
                </div>
                <span class="album-cached-ratio">
                  {#if album.isFullyCached}
                    {$t('offlineManager.row.completeNTracks', { values: { count: album.cachedTracks.length } })}
                  {:else}
                    {$t('offlineManager.row.partialNCached', { values: { count: album.cachedTracks.length } })}
                  {/if}
                </span>
                <span class="album-quality">{album.dominantQuality}</span>
                <span class="album-size">{formatBytes(album.totalSizeBytes)}</span>
                <span class="album-status status-{album.worstStatus}">
                  {#if album.failedCount > 0}
                    {$t('offlineManager.row.nFailed', { values: { count: album.failedCount } })}
                  {:else if album.worstStatus === 'downloading'}
                    {@const downloadingTracks = album.cachedTracks.filter(track => track.status === 'downloading')}
                    {@const avgProgress = downloadingTracks.length === 0
                      ? 0
                      : downloadingTracks.reduce((s, track) => s + track.progressPercent, 0) / downloadingTracks.length}
                    {@const r = 7}
                    {@const circumference = 2 * Math.PI * r}
                    <svg class="progress-ring" width="16" height="16" viewBox="0 0 16 16">
                      <circle class="ring-bg" cx="8" cy="8" r={r} />
                      <circle
                        class="ring-fill"
                        cx="8"
                        cy="8"
                        r={r}
                        stroke-dasharray={circumference}
                        stroke-dashoffset={circumference * (1 - avgProgress / 100)}
                      />
                    </svg>
                  {:else if album.worstStatus === 'queued'}
                    {@const r = 7}
                    {@const circumference = 2 * Math.PI * r}
                    <svg class="progress-ring indeterminate" width="16" height="16" viewBox="0 0 16 16">
                      <circle class="ring-bg" cx="8" cy="8" r={r} />
                      <circle
                        class="ring-fill"
                        cx="8"
                        cy="8"
                        r={r}
                        stroke-dasharray={circumference}
                        stroke-dashoffset={circumference * 0.25}
                      />
                    </svg>
                  {:else}
                    ✓
                  {/if}
                </span>
              </button>

              <div class="album-menu">
                {#if album.albumId}
                  <button type="button" onclick={() => onAlbumAction(album, 'redownload')}>
                    {$t('offlineManager.row.actions.redownloadAlbum')}
                  </button>
                {/if}
                {#if album.failedCount > 0 && album.albumId}
                  <button type="button" onclick={() => onAlbumAction(album, 'retryFailed')}>
                    {$t('offlineManager.row.actions.retryFailed')}
                  </button>
                {/if}
                <button type="button" onclick={() => onAlbumAction(album, 'remove')}>
                  {$t('offlineManager.row.actions.removeAlbum')}
                </button>
                {#if album.albumId}
                  <button type="button" onclick={() => onAlbumAction(album, 'goToAlbum')}>
                    {$t('offlineManager.row.actions.goToAlbum')}
                  </button>
                {/if}
              </div>

              {#if isExpanded}
                <div class="track-list">
                  {#each album.cachedTracks as track (track.trackId)}
                    <div class="track-row status-{track.status}">
                      <span class="track-title">{track.title}</span>
                      <span class="track-quality">{track.quality}</span>
                      <span class="track-size">{formatBytes(track.fileSizeBytes)}</span>
                      <span class="track-status">
                        {#if track.status === 'failed'}
                          <span title={track.errorMessage ?? ''}>⚠</span>
                        {:else if track.status === 'downloading'}
                          {@const r = 7}
                          {@const circumference = 2 * Math.PI * r}
                          <svg class="progress-ring" width="16" height="16" viewBox="0 0 16 16">
                            <circle class="ring-bg" cx="8" cy="8" r={r} />
                            <circle
                              class="ring-fill"
                              cx="8"
                              cy="8"
                              r={r}
                              stroke-dasharray={circumference}
                              stroke-dashoffset={circumference * (1 - track.progressPercent / 100)}
                            />
                          </svg>
                        {:else if track.status === 'ready'}
                          ✓
                        {:else}
                          {@const r = 7}
                          {@const circumference = 2 * Math.PI * r}
                          <svg class="progress-ring indeterminate" width="16" height="16" viewBox="0 0 16 16">
                            <circle class="ring-bg" cx="8" cy="8" r={r} />
                            <circle
                              class="ring-fill"
                              cx="8"
                              cy="8"
                              r={r}
                              stroke-dasharray={circumference}
                              stroke-dashoffset={circumference * 0.25}
                            />
                          </svg>
                        {/if}
                      </span>
                      <div class="track-menu">
                        <button type="button" onclick={() => onTrackAction(track, 'redownload')}>
                          {$t('offlineManager.track.actions.redownloadTrack')}
                        </button>
                        <button type="button" onclick={() => onTrackAction(track, 'remove')}>
                          {$t('offlineManager.track.actions.removeTrack')}
                        </button>
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        {/if}
      </main>
    </div>
  {/if}
</div>

<Modal
  isOpen={isLimitModalOpen}
  onClose={closeLimitModal}
  title={$t('offlineManager.limitModal.title')}
  maxWidth="420px"
>
  <div class="limit-modal-body">
    <label class="limit-input-row">
      <span class="limit-input-label">{$t('offlineManager.limitModal.inputLabel')}</span>
      <input
        type="number"
        min="5"
        step="1"
        bind:value={limitGbInput}
        disabled={savingLimit}
      />
    </label>
    <p class="limit-helper">{$t('offlineManager.limitModal.helper')}</p>
  </div>
  {#snippet footer()}
    <div class="limit-modal-actions">
      <button type="button" class="limit-btn-secondary" onclick={closeLimitModal} disabled={savingLimit}>
        {$t('actions.cancel')}
      </button>
      <button type="button" class="limit-btn-primary" onclick={saveLimit} disabled={savingLimit}>
        {savingLimit ? $t('actions.saving') : $t('actions.save')}
      </button>
    </div>
  {/snippet}
</Modal>

<style>
  .offline-cache-manager {
    display: flex;
    flex-direction: column;
    height: 100%;
    color: var(--text-primary);
  }
  .ocm-header { display: flex; align-items: center; gap: 12px; padding: 16px 24px; }
  .ocm-header h1 { margin: 0; font-size: 1.5rem; font-weight: 600; }
  .back-btn { background: transparent; border: none; color: var(--text-muted); cursor: pointer; padding: 8px; border-radius: 6px; }
  .back-btn:hover { color: var(--text-primary); background: var(--bg-hover); }
  .ocm-stats {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    padding: 8px 24px 16px;
    align-items: center;
    font-size: 0.875rem;
    color: var(--text-muted);
    border-bottom: 1px solid var(--border-subtle);
  }
  .ocm-stat-totals, .ocm-stat-usage { white-space: nowrap; }
  .ocm-usage-bar { display: inline-block; vertical-align: middle; width: 120px; height: 4px; background: var(--bg-tertiary); border-radius: 2px; margin-left: 8px; overflow: hidden; }
  .ocm-usage-fill { display: block; height: 100%; background: var(--accent-primary); transition: width 0.2s ease; }
  .ocm-limit-edit {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    margin-left: 6px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    vertical-align: middle;
    transition: color 150ms ease, background-color 150ms ease;
  }
  .ocm-limit-edit:hover { color: var(--text-primary); background: var(--bg-hover); }
  .limit-modal-body { display: flex; flex-direction: column; gap: 12px; }
  .limit-input-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .limit-input-label {
    font-size: 0.85rem;
    color: var(--text-muted);
  }
  .limit-input-row input {
    width: 100%;
    padding: 8px 10px;
    background: var(--bg-secondary, var(--bg-primary));
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 1rem;
  }
  .limit-input-row input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }
  .limit-helper {
    margin: 0;
    font-size: 0.8rem;
    color: var(--text-muted);
  }
  .limit-modal-actions {
    display: flex;
    gap: 8px;
    margin-left: auto;
  }
  .limit-btn-secondary,
  .limit-btn-primary {
    padding: 6px 14px;
    border-radius: 6px;
    font-size: 0.875rem;
    cursor: pointer;
    border: 1px solid var(--bg-tertiary);
  }
  .limit-btn-secondary {
    background: transparent;
    color: var(--text-primary);
  }
  .limit-btn-secondary:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .limit-btn-primary {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: white;
  }
  .limit-btn-primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }
  .limit-btn-secondary:disabled,
  .limit-btn-primary:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .ocm-controls { margin-left: auto; display: flex; gap: 12px; align-items: center; }
  .ocm-sort-label { color: var(--text-muted); font-size: 0.85rem; }
  .ocm-toggle { display: flex; gap: 6px; align-items: center; cursor: pointer; }
  .ocm-loading { padding: 32px; text-align: center; color: var(--text-muted); }
  .ocm-empty { padding: 64px 24px; text-align: center; }
  .ocm-empty h2 { margin: 0 0 8px; font-size: 1.25rem; font-weight: 600; }
  .ocm-empty p { margin: 0; color: var(--text-muted); }
  .empty-cta {
    margin-top: 16px;
    padding: 8px 16px;
    background: var(--accent-primary);
    border: none;
    border-radius: 6px;
    color: white;
    font-size: 0.9rem;
    cursor: pointer;
  }
  .empty-cta:hover { filter: brightness(1.1); }
  .ocm-body { display: flex; flex: 1; min-height: 0; }
  .ocm-rail {
    width: 260px;
    border-right: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .ocm-alpha-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }
  .alpha-letter {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 0.7rem;
    padding: 2px 4px;
    cursor: pointer;
  }
  .alpha-letter.disabled { opacity: 0.3; pointer-events: none; }
  .alpha-letter:hover:not(.disabled) { color: var(--text-primary); }
  .ocm-rail-list { flex: 1; overflow-y: auto; }
  .rail-letter-section { padding: 4px 0; }
  .rail-letter {
    margin: 0;
    padding: 4px 12px;
    font-size: 0.75rem;
    color: var(--text-muted);
    font-weight: 600;
    position: sticky;
    top: 0;
    background: var(--bg-secondary, var(--bg-primary));
  }
  .rail-artist {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    padding: 8px 16px;
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }
  .rail-artist:hover { background: var(--bg-hover); }
  .rail-artist.active { background: var(--bg-active, var(--bg-hover)); color: var(--text-primary); }
  .rail-artist-name { font-weight: 500; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rail-artist-meta { font-size: 0.7rem; color: var(--text-muted); }
  .ocm-pane { flex: 1; overflow-y: auto; padding: 16px 24px; min-width: 0; }

  .pane-heading { display: flex; align-items: baseline; gap: 12px; margin-bottom: 16px; }
  .pane-heading h2 { margin: 0; font-size: 1.125rem; font-weight: 600; }
  .pane-subhead { color: var(--text-muted); font-size: 0.85rem; }
  .album-row { display: flex; flex-direction: column; gap: 0; padding: 0; border-bottom: 1px solid var(--border-subtle); }
  .album-summary {
    display: grid;
    grid-template-columns: 24px 40px 1fr 160px 100px 80px 80px;
    align-items: center;
    gap: 12px;
    padding: 12px 8px;
    background: transparent;
    border: none;
    color: inherit;
    cursor: pointer;
    text-align: left;
    width: 100%;
  }
  .album-summary:hover { background: var(--bg-hover); }
  .chevron { width: 16px; color: var(--text-muted); }
  .album-cover-placeholder { width: 40px; height: 40px; background: var(--bg-tertiary); border-radius: 4px; }
  .album-meta { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .album-title { font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .album-subtitle { font-size: 0.8rem; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .album-cached-ratio, .album-quality, .album-size { font-size: 0.85rem; color: var(--text-muted); }
  .album-status { font-size: 0.85rem; }
  .album-status.status-ready { color: var(--success, #4ade80); }
  .album-status.status-failed { color: var(--danger, #f87171); }
  .album-status.status-downloading { color: var(--accent-primary); }
  .album-menu { display: flex; gap: 8px; padding: 0 8px 8px 36px; }
  .album-menu button {
    background: transparent;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    padding: 4px 8px;
    font-size: 0.8rem;
    color: var(--text-primary);
    cursor: pointer;
  }
  .album-menu button:hover { background: var(--bg-hover); }
  .track-list {
    padding: 4px 8px 12px 36px;
    display: flex;
    flex-direction: column;
    gap: 0;
    border-top: 1px solid var(--border-subtle);
  }
  .track-row {
    display: grid;
    grid-template-columns: 1fr 100px 80px 30px auto;
    gap: 12px;
    align-items: center;
    padding: 6px 8px;
    font-size: 0.85rem;
  }
  .track-row:hover { background: var(--bg-hover); }
  .track-status { color: var(--text-muted); }
  .track-row.status-failed .track-status { color: var(--danger, #f87171); }
  .track-menu { display: flex; gap: 4px; }
  .track-menu button {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px 6px;
    font-size: 0.75rem;
    border-radius: 3px;
  }
  .track-menu button:hover { background: var(--bg-hover); color: var(--text-primary); }

  .album-cover {
    width: 40px;
    height: 40px;
    border-radius: 4px;
    object-fit: cover;
    background: var(--bg-tertiary);
  }
  .progress-ring { transform: rotate(-90deg); display: inline-block; vertical-align: middle; }
  .ring-bg { fill: none; stroke: var(--bg-tertiary, #333); stroke-width: 1.5; }
  .ring-fill {
    fill: none;
    stroke: var(--accent-primary);
    stroke-width: 1.5;
    stroke-linecap: round;
    transition: stroke-dashoffset 300ms ease;
  }
  .progress-ring.indeterminate { animation: ocm-spin 1.2s linear infinite; }
  @keyframes ocm-spin { to { transform: rotate(270deg); } }
</style>
