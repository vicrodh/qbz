<script lang="ts">
  import { Plus, Search, X, CassetteTape, LibraryBig, User } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    collectionsStore,
    loadCollections,
    addItem,
    itemExists,
    createCollection,
    type MixtapeCollection,
    type CollectionKind,
  } from '$lib/stores/mixtapeCollectionsStore';
  import { showToast } from '$lib/stores/toastStore';

  export interface AddToMixtapeItem {
    item_type: 'album' | 'track' | 'playlist';
    source: 'qobuz' | 'local';
    source_item_id: string;
    title: string;
    subtitle?: string;
    artwork_url?: string;
    year?: number;
    track_count?: number;
  }

  interface Props {
    open: boolean;
    /** One or more items to add. Single-item use cases pass a 1-element array. */
    items: AddToMixtapeItem[];
    onClose: () => void;
  }
  let { open, items, onClose }: Props = $props();

  const firstItem = $derived(items[0] ?? null);
  const bulkMode = $derived(items.length > 1);

  // Kind restriction: Collections hold whole albums only. If any incoming
  // item is a track or playlist, restrict the target list to Mixtapes and
  // ArtistCollections are also excluded (they're auto-generated from an
  // artist's discography — not a user target).
  const restrictToMixtape = $derived(
    items.some((it) => it.item_type !== 'album'),
  );

  let searchQuery = $state('');
  let busyCollectionId = $state<string | null>(null);

  // Create-new sub-panel state
  let creating = $state(false);
  let createName = $state('');
  let createKind = $state<CollectionKind>('mixtape');
  let createBusy = $state(false);

  // Load collections on first open (if store is empty)
  $effect(() => {
    if (open && $collectionsStore.length === 0) {
      loadCollections().catch((err) => {
        console.error('[AddToMixtapeModal] loadCollections failed:', err);
      });
    }
  });

  // Reset state whenever modal opens
  $effect(() => {
    if (open) {
      searchQuery = '';
      busyCollectionId = null;
      creating = false;
      createName = '';
      createKind = 'mixtape';
      createBusy = false;
    }
  });

  // Ordering: most-recently-played, then most-recently-updated
  const sortedCollections = $derived(
    [...$collectionsStore].sort((col_a, col_b) => {
      const aRecent = col_a.last_played_at ?? col_a.updated_at;
      const bRecent = col_b.last_played_at ?? col_b.updated_at;
      return bRecent - aRecent;
    }),
  );

  // Filter by kind restriction + search query (case-insensitive).
  const kindFilteredCollections = $derived(
    restrictToMixtape
      ? sortedCollections.filter((col) => col.kind === 'mixtape')
      : sortedCollections,
  );
  const filteredCollections = $derived(
    searchQuery.trim() === ''
      ? kindFilteredCollections
      : kindFilteredCollections.filter((col) =>
          col.name.toLowerCase().includes(searchQuery.toLowerCase()),
        ),
  );

  function kindLabelFor(col: MixtapeCollection): string {
    if (col.kind === 'artist_collection') return $t('collections.artistLabel');
    if (col.kind === 'collection') return $t('collections.label');
    return $t('mixtapes.label');
  }

  // Duplicate-confirmation state. When handlePick detects existing items in
  // the target, the modal pauses on this screen instead of silently skipping.
  let pendingCollection = $state<MixtapeCollection | null>(null);
  let pendingDuplicates = $state<AddToMixtapeItem[]>([]);
  let pendingFresh = $state<AddToMixtapeItem[]>([]);

  async function findDuplicates(
    collectionId: string,
    list: AddToMixtapeItem[],
  ): Promise<{ fresh: AddToMixtapeItem[]; dups: AddToMixtapeItem[] }> {
    const fresh: AddToMixtapeItem[] = [];
    const dups: AddToMixtapeItem[] = [];
    for (const it of list) {
      try {
        const exists = await itemExists(collectionId, it.source, it.source_item_id);
        if (exists) dups.push(it);
        else fresh.push(it);
      } catch (err) {
        // Fail-open: treat as fresh; the backend's own dedup will catch it if
        // we're wrong. Logged for visibility.
        console.warn('[AddToMixtapeModal] itemExists check failed:', err);
        fresh.push(it);
      }
    }
    return { fresh, dups };
  }

  async function insertBatch(
    collectionId: string,
    list: AddToMixtapeItem[],
    allowDuplicate: boolean,
  ): Promise<number> {
    let added = 0;
    for (const it of list) {
      try {
        const ok = await addItem(collectionId, {
          item_type: it.item_type,
          source: it.source,
          source_item_id: it.source_item_id,
          title: it.title,
          subtitle: it.subtitle,
          artwork_url: it.artwork_url,
          year: it.year,
          track_count: it.track_count,
        }, { allowDuplicate });
        if (ok) added += 1;
      } catch (err) {
        console.warn('[AddToMixtapeModal] addItem failed for one item:', err);
      }
    }
    return added;
  }

  function toastBatchResult(collectionName: string, added: number) {
    if (added === 0) return;
    if (bulkMode) {
      showToast(
        $t('mixtapes.bulkAdded', { values: { count: added, name: collectionName } }) ||
          `Added ${added} to ${collectionName}`,
        'success',
      );
    } else {
      showToast(
        $t('common.addedToMixtapeOrCollection', { values: { name: collectionName } }),
        'success',
      );
    }
  }

  async function handlePick(collection: MixtapeCollection) {
    if (items.length === 0 || busyCollectionId) return;
    busyCollectionId = collection.id;
    try {
      const { fresh, dups } = await findDuplicates(collection.id, items);

      if (dups.length > 0) {
        // Pause here; show the confirmation sub-panel. Fresh items are
        // held aside so they can be added immediately regardless of what
        // the user chooses on the duplicates.
        pendingCollection = collection;
        pendingDuplicates = dups;
        pendingFresh = fresh;
        busyCollectionId = null;
        return;
      }

      const added = await insertBatch(collection.id, fresh, false);
      toastBatchResult(collection.name, added);
      if (added > 0) await loadCollections();
      onClose();
    } catch (err) {
      console.error('[AddToMixtapeModal] addItem failed:', err);
      showToast('Failed to add item', 'error');
    } finally {
      busyCollectionId = null;
    }
  }

  /** User confirmed the duplicates dialog: insert duplicates with force + fresh. */
  async function handleConfirmDuplicates() {
    if (!pendingCollection) return;
    const collection = pendingCollection;
    busyCollectionId = collection.id;
    try {
      const freshAdded = await insertBatch(collection.id, pendingFresh, false);
      const dupAdded = await insertBatch(collection.id, pendingDuplicates, true);
      const total = freshAdded + dupAdded;
      toastBatchResult(collection.name, total);
      if (total > 0) await loadCollections();
      onClose();
    } catch (err) {
      console.error('[AddToMixtapeModal] confirmDuplicates failed:', err);
      showToast('Failed to add items', 'error');
    } finally {
      pendingCollection = null;
      pendingDuplicates = [];
      pendingFresh = [];
      busyCollectionId = null;
    }
  }

  /** User chose "Skip duplicates": only insert the fresh items, drop duplicates. */
  async function handleSkipDuplicates() {
    if (!pendingCollection) return;
    const collection = pendingCollection;
    busyCollectionId = collection.id;
    try {
      const added = await insertBatch(collection.id, pendingFresh, false);
      if (added > 0) {
        toastBatchResult(collection.name, added);
        await loadCollections();
      } else {
        showToast(
          $t('common.alreadyInToast', { values: { name: collection.name } }),
          'info',
        );
      }
      onClose();
    } finally {
      pendingCollection = null;
      pendingDuplicates = [];
      pendingFresh = [];
      busyCollectionId = null;
    }
  }

  function handleCancelDuplicates() {
    pendingCollection = null;
    pendingDuplicates = [];
    pendingFresh = [];
    busyCollectionId = null;
  }

  async function handleCreateAndAdd() {
    if (items.length === 0 || !createName.trim() || createBusy) return;
    createBusy = true;
    try {
      const created = await createCollection(createKind, createName.trim());
      // Fresh collection — no duplicates possible.
      const added = await insertBatch(created.id, items, false);
      toastBatchResult(created.name, added);
      await loadCollections();
      onClose();
    } catch (err) {
      console.error('[AddToMixtapeModal] createAndAdd failed:', err);
      showToast('Failed to create', 'error');
    } finally {
      createBusy = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (creating) {
        creating = false;
      } else {
        onClose();
      }
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open && firstItem}
  <div
    class="backdrop"
    role="presentation"
    onclick={onClose}
  ></div>

  <div class="modal" role="dialog" aria-label={$t('common.addToMixtapeOrCollection')}>
    <header class="modal-header">
      <div class="header-text">
        <span class="eyebrow">{$t('common.addToMixtapeOrCollection')}</span>
        {#if bulkMode}
          <h2 class="title">
            {$t('mixtapes.bulkAddTitle', { values: { count: items.length } }) ||
              `${items.length} items`}
          </h2>
          <span class="subtitle">{firstItem.title}{items.length > 1 ? ` + ${items.length - 1} more` : ''}</span>
        {:else}
          <h2 class="title">{firstItem.title}</h2>
          {#if firstItem.subtitle}
            <span class="subtitle">{firstItem.subtitle}</span>
          {/if}
        {/if}
      </div>
      <button type="button" class="close-btn" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </header>

    {#if pendingCollection}
      <!-- Duplicate-confirmation sub-panel: one or more items already live in
           the target collection. User can add anyway (force), skip the
           duplicates, or cancel back to the picker. -->
      <div class="dup-panel">
        <p class="dup-summary">
          {$t('mixtapes.dupSummary', {
            values: {
              count: pendingDuplicates.length,
              name: pendingCollection.name,
            },
          }) || `${pendingDuplicates.length} already in ${pendingCollection.name}`}
        </p>
        <ul class="dup-list">
          {#each pendingDuplicates as dupItem (dupItem.source_item_id)}
            <li class="dup-list-item">
              <span class="dup-title">{dupItem.title}</span>
              {#if dupItem.subtitle}
                <span class="dup-subtitle">{dupItem.subtitle}</span>
              {/if}
            </li>
          {/each}
        </ul>
        {#if pendingFresh.length > 0}
          <p class="dup-hint">
            {$t('mixtapes.dupFreshHint', { values: { count: pendingFresh.length } }) ||
              `${pendingFresh.length} other item(s) will be added regardless.`}
          </p>
        {/if}
        <footer class="modal-footer">
          <button
            type="button"
            class="secondary-btn"
            onclick={handleCancelDuplicates}
            disabled={busyCollectionId !== null}
          >
            {$t('actions.cancel') || 'Cancel'}
          </button>
          {#if pendingFresh.length > 0}
            <button
              type="button"
              class="secondary-btn"
              onclick={handleSkipDuplicates}
              disabled={busyCollectionId !== null}
            >
              {$t('mixtapes.dupSkip') || 'Skip duplicates'}
            </button>
          {/if}
          <button
            type="button"
            class="primary-btn"
            onclick={handleConfirmDuplicates}
            disabled={busyCollectionId !== null}
          >
            {$t('mixtapes.dupAddAnyway') || 'Add anyway'}
          </button>
        </footer>
      </div>
    {:else if !creating}
      <div class="search-row">
        <Search size={14} />
        <input
          type="text"
          class="search-input"
          placeholder="Search…"
          bind:value={searchQuery}
        />
      </div>

      <div class="list">
        {#if filteredCollections.length === 0}
          <div class="empty">
            {#if $collectionsStore.length === 0}
              <p>No mixtapes or collections yet.</p>
            {:else}
              <p>No matches.</p>
            {/if}
          </div>
        {:else}
          {#each filteredCollections as col (col.id)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <div
              class="row"
              class:busy={busyCollectionId === col.id}
              role="button"
              tabindex="0"
              onclick={() => handlePick(col)}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') handlePick(col); }}
            >
              <div class="row-icon">
                {#if col.kind === 'mixtape'}<CassetteTape size={18} />
                {:else if col.kind === 'artist_collection'}<User size={18} />
                {:else}<LibraryBig size={18} />
                {/if}
              </div>
              <div class="row-body">
                <div class="row-name">{col.name}</div>
                <div class="row-meta">
                  {$t('mixtapes.albumCount', { values: { count: col.items.length } })}
                </div>
              </div>
              <div class="row-kind">{kindLabelFor(col)}</div>
            </div>
          {/each}
        {/if}
      </div>

      <footer class="modal-footer">
        <button
          type="button"
          class="create-btn"
          onclick={() => { createKind = 'mixtape'; creating = true; }}
        >
          <Plus size={14} /> {$t('mixtapes.nav')}
        </button>
        {#if !restrictToMixtape}
          <!-- Collections hold whole albums only. Hide this entry point when
               the picker is restricted (track / playlist targets). -->
          <button
            type="button"
            class="create-btn"
            onclick={() => { createKind = 'collection'; creating = true; }}
          >
            <Plus size={14} /> {$t('collections.nav')}
          </button>
        {/if}
      </footer>
    {:else}
      <!-- Create-new sub-panel -->
      <div class="create-panel">
        <label class="field">
          <span class="field-label">Name</span>
          <input
            type="text"
            bind:value={createName}
            maxlength="80"
            disabled={createBusy}
            placeholder={createKind === 'mixtape' ? '90s Cassettes' : 'Jazz Library'}
          />
        </label>

        <div class="field">
          <span class="field-label">Kind</span>
          <div class="kind-toggle">
            <label>
              <input
                type="radio"
                value="mixtape"
                bind:group={createKind}
                disabled={createBusy}
              />
              <span>{$t('mixtapes.nav')}</span>
            </label>
            <label>
              <input
                type="radio"
                value="collection"
                bind:group={createKind}
                disabled={createBusy}
              />
              <span>{$t('collections.nav')}</span>
            </label>
          </div>
        </div>

        <footer class="modal-footer">
          <button
            type="button"
            class="secondary-btn"
            onclick={() => (creating = false)}
            disabled={createBusy}
          >
            Back
          </button>
          <button
            type="button"
            class="primary-btn"
            onclick={handleCreateAndAdd}
            disabled={createBusy || !createName.trim()}
          >
            Create & Add
          </button>
        </footer>
      </div>
    {/if}
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    z-index: 9998;
  }
  .modal {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 480px;
    max-width: 90vw;
    max-height: 80vh;
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    z-index: 9999;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: 20px 20px 16px;
    border-bottom: 1px solid var(--bg-tertiary);
  }
  .header-text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .eyebrow {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.5px;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .title {
    margin: 0;
    font-size: 18px;
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .subtitle {
    font-size: 12px;
    color: var(--text-muted);
  }
  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: 6px;
  }
  .close-btn:hover { background: var(--bg-hover); color: var(--text-primary); }

  .search-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--bg-tertiary);
    color: var(--text-muted);
  }
  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 13px;
  }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 8px 12px;
  }

  .empty {
    padding: 32px 8px;
    text-align: center;
    color: var(--text-muted);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-radius: 8px;
    cursor: pointer;
    transition: background 150ms ease;
  }
  .row:hover { background: var(--bg-hover); }
  .row.busy { opacity: 0.5; pointer-events: none; }

  .row-icon {
    width: 32px;
    height: 32px;
    display: grid;
    place-items: center;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .row-body { flex: 1; min-width: 0; }
  .row-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row-meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  .row-kind {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .modal-footer {
    display: flex;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--bg-tertiary);
    background: var(--bg-secondary);
  }
  .create-btn {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 9px 12px;
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }
  .create-btn:hover { background: var(--bg-hover); }

  .create-panel { padding: 20px; display: flex; flex-direction: column; gap: 16px; }

  /* Duplicate-confirmation sub-panel. Keeps the modal chrome (backdrop,
     header, footer) so the user's mental model stays intact. */
  .dup-panel {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .dup-summary {
    margin: 0;
    font-size: 14px;
    color: var(--text-primary);
  }
  .dup-list {
    margin: 0;
    padding: 8px 12px;
    max-height: 160px;
    overflow-y: auto;
    background: var(--alpha-6);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .dup-list-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
  }
  .dup-title {
    color: var(--text-primary);
    font-weight: 500;
  }
  .dup-subtitle {
    color: var(--text-muted);
  }
  .dup-hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }
  .field { display: flex; flex-direction: column; gap: 8px; }
  .field-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.5px;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .field input[type="text"] {
    padding: 10px 12px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    font-family: inherit;
  }
  .kind-toggle { display: inline-flex; gap: 16px; }
  .kind-toggle label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    color: var(--text-primary);
    font-size: 14px;
  }

  .create-panel .modal-footer {
    margin: 16px -20px -20px;
  }
  .primary-btn {
    display: inline-flex;
    align-items: center;
    padding: 10px 20px;
    background: var(--accent-primary);
    color: #fff;
    border: none;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }
  .primary-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .secondary-btn {
    padding: 10px 16px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }
</style>
