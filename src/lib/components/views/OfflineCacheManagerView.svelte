<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from 'svelte-i18n';
  import { ArrowLeft } from 'lucide-svelte';
  import Dropdown from '$lib/components/Dropdown.svelte';
  import { offlineCacheManagerStore } from '$lib/stores/offlineCacheManagerStore.svelte';

  type Props = {
    onBack: () => void;
    onGoToAlbum: (albumId: string) => void;
    onGoToFavorites: () => void;
  };
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
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
        <!-- right pane goes here in Task 6.3 -->
      </main>
    </div>
  {/if}
</div>

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
  .ocm-controls { margin-left: auto; display: flex; gap: 12px; align-items: center; }
  .ocm-sort-label { color: var(--text-muted); font-size: 0.85rem; }
  .ocm-toggle { display: flex; gap: 6px; align-items: center; cursor: pointer; }
  .ocm-loading { padding: 32px; text-align: center; color: var(--text-muted); }
  .ocm-empty { padding: 64px 24px; text-align: center; }
  .ocm-empty h2 { margin: 0 0 8px; font-size: 1.25rem; font-weight: 600; }
  .ocm-empty p { margin: 0; color: var(--text-muted); }
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
</style>
