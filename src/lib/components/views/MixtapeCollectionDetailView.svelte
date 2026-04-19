<script lang="ts">
  import {
    Play, Shuffle, MoreHorizontal, ChevronLeft, Disc, Music2, ListMusic, Trash2
  } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    collectionsStore,
    getCollection,
    enqueueCollection,
    removeItem as removeCollectionItem,
    renameCollection,
    setDescription,
    setPlayMode,
    setKind,
    deleteCollection,
    type MixtapeCollection,
    type MixtapeCollectionItem,
    type CollectionKind,
    type CollectionPlayMode,
    type ItemType,
  } from '$lib/stores/mixtapeCollectionsStore';
  import CollectionMosaic from '../CollectionMosaic.svelte';
  import SourceBadge, { type SourceBadgeValue } from '../SourceBadge.svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import { showToast } from '$lib/stores/toastStore';

  interface Props {
    collectionId: string;
    onBack?: () => void;
  }
  let { collectionId, onBack }: Props = $props();

  let collection = $state<MixtapeCollection | null>(null);
  let loading = $state(true);
  let overflowOpen = $state(false);
  let renameModalOpen = $state(false);
  let descriptionModalOpen = $state(false);
  let confirmDeleteOpen = $state(false);

  // Edit drafts
  let draftName = $state('');
  let draftDescription = $state('');

  // Item overflow menus (track which item's ⋯ is open)
  let openItemMenu = $state<number | null>(null);

  async function loadCollection() {
    loading = true;
    try {
      collection = await getCollection(collectionId);
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] load failed:', err);
      collection = null;
    } finally {
      loading = false;
    }
  }

  // Reactive subscribe to the store — pick up metadata changes without a full refetch
  $effect(() => {
    const fromStore = $collectionsStore.find((col) => col.id === collectionId);
    if (fromStore && collection && fromStore.id === collection.id) {
      // Merge — keep our local items since the store doesn't always carry them
      collection = { ...fromStore, items: collection.items };
    }
  });

  $effect(() => {
    // Re-load whenever collectionId changes
    void collectionId;
    loadCollection();
  });

  // ──────── helpers ────────

  function formatItemCountSummary(items: MixtapeCollectionItem[]): string {
    return $t('mixtapes.albumCount', { values: { count: items.length } });
  }

  function kindLabel(kind: CollectionKind | undefined): string {
    if (!kind) return '';
    if (kind === 'mixtape') return $t('mixtapes.label');
    if (kind === 'artist_collection') return $t('collections.artistLabel');
    return $t('collections.label');
  }

  function itemTypeLabel(type: ItemType): string {
    if (type === 'album') return $t('itemType.album');
    if (type === 'track') return $t('itemType.track');
    return $t('itemType.playlist');
  }

  function itemTracks(item: MixtapeCollectionItem): string {
    if (item.item_type === 'track') return '1';
    return item.track_count == null ? '—' : String(item.track_count);
  }

  function itemYear(item: MixtapeCollectionItem): string {
    return item.year == null ? '' : String(item.year);
  }

  /** Resolve the source badge value for an item.
   *  For Qobuz items: 'qobuz_streaming'.
   *  For Local items: fall back to 'user' for MVP.
   *  Live-resolve is a follow-up (spec §2.3). */
  function sourceBadgeFor(item: MixtapeCollectionItem): SourceBadgeValue {
    if (item.source === 'qobuz') return 'qobuz_streaming';
    return 'user';
  }

  // ──────── actions ────────

  async function handlePlay() {
    try {
      await enqueueCollection(collectionId, 'replace');
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] enqueue (play) failed:', err);
      showToast('Failed to start playback', 'error');
    }
  }

  async function handleShuffle() {
    if (!collection) return;
    try {
      if (collection.play_mode !== 'album_shuffle') {
        await setPlayMode(collectionId, 'album_shuffle');
      }
      await enqueueCollection(collectionId, 'replace');
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] enqueue (shuffle) failed:', err);
      showToast('Failed to start playback', 'error');
    }
  }

  function openRenameModal() {
    if (!collection) return;
    draftName = collection.name;
    renameModalOpen = true;
    overflowOpen = false;
  }

  async function submitRename() {
    const name = draftName.trim();
    if (!name || !collection) { renameModalOpen = false; return; }
    await renameCollection(collectionId, name);
    await loadCollection();
    renameModalOpen = false;
  }

  function openDescriptionModal() {
    if (!collection) return;
    draftDescription = collection.description ?? '';
    descriptionModalOpen = true;
    overflowOpen = false;
  }

  async function submitDescription() {
    if (!collection) return;
    const desc = draftDescription.trim() === '' ? null : draftDescription.trim();
    await setDescription(collectionId, desc);
    await loadCollection();
    descriptionModalOpen = false;
  }

  async function togglePlayMode() {
    if (!collection) return;
    const next: CollectionPlayMode =
      collection.play_mode === 'in_order' ? 'album_shuffle' : 'in_order';
    await setPlayMode(collectionId, next);
    await loadCollection();
    overflowOpen = false;
  }

  async function convertKind() {
    if (!collection) return;
    const next: CollectionKind =
      collection.kind === 'mixtape' ? 'collection' : 'mixtape';
    try {
      await setKind(collectionId, next);
      await loadCollection();
      showToast('Converted', 'success');
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] convertKind failed:', err);
      showToast('Cannot convert this kind', 'error');
    }
    overflowOpen = false;
  }

  async function handleDelete() {
    if (!collection) return;
    try {
      await deleteCollection(collectionId);
      onBack?.();
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] delete failed:', err);
      showToast('Failed to delete', 'error');
    } finally {
      confirmDeleteOpen = false;
    }
  }

  async function handleRemoveItem(position: number) {
    try {
      await removeCollectionItem(collectionId, position);
      await loadCollection();
    } catch (err) {
      console.error('[MixtapeCollectionDetailView] remove item failed:', err);
      showToast('Failed to remove', 'error');
    } finally {
      openItemMenu = null;
    }
  }

  // Close overflow menus on ESC
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (confirmDeleteOpen) confirmDeleteOpen = false;
      else if (renameModalOpen) renameModalOpen = false;
      else if (descriptionModalOpen) descriptionModalOpen = false;
      else if (overflowOpen) overflowOpen = false;
      else if (openItemMenu !== null) openItemMenu = null;
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="detail-view">
  {#if loading}
    <div class="loading">{$t('actions.loading')}</div>
  {:else if !collection}
    <div class="not-found">
      <button class="back-btn" onclick={() => onBack?.()}>
        <ChevronLeft size={14} /> Back
      </button>
      <p>{$t('errors.notFound')}</p>
    </div>
  {:else}
    <!-- Header -->
    <header class="detail-header">
      {#if onBack}
        <button class="back-btn" onclick={() => onBack()}>
          <ChevronLeft size={14} /> Back
        </button>
      {/if}

      <div class="header-content">
        <div class="header-cover">
          <CollectionMosaic items={collection.items} size={240} kind={collection.kind} />
        </div>

        <div class="header-info">
          <div class="eyebrow">
            <span class="kind-tag">{kindLabel(collection.kind)}</span>
          </div>
          <h1 class="title">{collection.name}</h1>
          {#if collection.description}
            <p class="description">{collection.description}</p>
          {/if}
          <div class="meta">
            {formatItemCountSummary(collection.items)}
          </div>

          <div class="header-actions">
            <button
              class="primary-action"
              onclick={handlePlay}
              disabled={collection.items.length === 0}
            >
              <Play size={16} fill="currentColor" />
              <span>{$t('common.playAllInOrder')}</span>
            </button>
            <button
              class="secondary-action"
              onclick={handleShuffle}
              disabled={collection.items.length === 0}
            >
              <Shuffle size={16} />
              <span>{$t('common.shuffleAlbums')}</span>
            </button>
            <div class="overflow-wrap">
              <button
                class="icon-action"
                onclick={() => (overflowOpen = !overflowOpen)}
                aria-label="More"
              >
                <MoreHorizontal size={16} />
              </button>
              {#if overflowOpen}
                <div
                  class="overflow-backdrop"
                  onclick={() => (overflowOpen = false)}
                  role="presentation"
                ></div>
                <div class="overflow-menu" role="menu">
                  <button class="overflow-item" onclick={openRenameModal}>
                    {$t('collectionDetail.rename')}
                  </button>
                  <button class="overflow-item" onclick={openDescriptionModal}>
                    {$t('collectionDetail.editDescription')}
                  </button>
                  <button class="overflow-item" onclick={togglePlayMode}>
                    {collection.play_mode === 'in_order'
                      ? $t('common.playModeAlbumShuffle')
                      : $t('common.playModeInOrder')}
                  </button>
                  {#if collection.kind !== 'artist_collection'}
                    <button class="overflow-item" onclick={convertKind}>
                      {collection.kind === 'mixtape'
                        ? $t('collectionDetail.convertToCollection')
                        : $t('collectionDetail.convertToMixtape')}
                    </button>
                  {/if}
                  <button
                    class="overflow-item destructive"
                    onclick={() => { confirmDeleteOpen = true; overflowOpen = false; }}
                  >
                    {$t('collectionDetail.delete')}
                  </button>
                </div>
              {/if}
            </div>
          </div>
        </div>
      </div>
    </header>

    <!-- Item list -->
    {#if collection.items.length === 0}
      <div class="empty-list">
        <p>No items yet. Add albums, tracks, or playlists from their detail pages.</p>
      </div>
    {:else}
      <div class="item-list">
        <div class="item-list-header">
          <div class="col-idx">#</div>
          <div class="col-item">Item</div>
          <div class="col-type">Type</div>
          <div class="col-source">Source</div>
          <div class="col-quality">Quality</div>
          <div class="col-tracks">Tracks</div>
          <div class="col-year">Year</div>
          <div class="col-menu"></div>
        </div>

        {#each collection.items as item (item.position)}
          <div class="item-row">
            <div class="col-idx">{item.position + 1}</div>

            <div class="col-item">
              {#if item.artwork_url}
                <img class="artwork" src={item.artwork_url} alt="" loading="lazy" />
              {:else}
                <div class="artwork artwork-placeholder"></div>
              {/if}
              <div class="item-meta">
                <div class="item-title">{item.title}</div>
                {#if item.subtitle}
                  <div class="item-subtitle">{item.subtitle}</div>
                {/if}
              </div>
            </div>

            <div class="col-type">
              <span class="type-cell">
                {#if item.item_type === 'album'}
                  <Disc size={13} />
                {:else if item.item_type === 'track'}
                  <Music2 size={13} />
                {:else}
                  <ListMusic size={13} />
                {/if}
                <span class="type-label">{itemTypeLabel(item.item_type)}</span>
              </span>
            </div>

            <div class="col-source">
              <SourceBadge value={sourceBadgeFor(item)} />
            </div>

            <div class="col-quality">
              <QualityBadge compact />
            </div>

            <div class="col-tracks">{itemTracks(item)}</div>
            <div class="col-year">{itemYear(item)}</div>

            <div class="col-menu">
              <button
                class="icon-action small"
                onclick={() => (openItemMenu = openItemMenu === item.position ? null : item.position)}
                aria-label="Item actions"
              >
                <MoreHorizontal size={14} />
              </button>
              {#if openItemMenu === item.position}
                <div
                  class="overflow-backdrop"
                  onclick={() => (openItemMenu = null)}
                  role="presentation"
                ></div>
                <div class="overflow-menu item-menu" role="menu">
                  <button
                    class="overflow-item destructive"
                    onclick={() => handleRemoveItem(item.position)}
                  >
                    <Trash2 size={13} /> Remove
                  </button>
                </div>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <!-- Rename modal -->
    {#if renameModalOpen}
      <div class="m-backdrop" onclick={() => (renameModalOpen = false)} role="presentation"></div>
      <div class="m-modal" role="dialog">
        <h2>{$t('collectionDetail.rename')}</h2>
        <input type="text" bind:value={draftName} maxlength="80" class="m-input" />
        <div class="m-footer">
          <button class="m-btn-secondary" onclick={() => (renameModalOpen = false)}>Cancel</button>
          <button
            class="m-btn-primary"
            onclick={submitRename}
            disabled={!draftName.trim()}
          >Save</button>
        </div>
      </div>
    {/if}

    <!-- Description modal -->
    {#if descriptionModalOpen}
      <div
        class="m-backdrop"
        onclick={() => (descriptionModalOpen = false)}
        role="presentation"
      ></div>
      <div class="m-modal" role="dialog">
        <h2>{$t('collectionDetail.editDescription')}</h2>
        <textarea
          bind:value={draftDescription}
          maxlength="400"
          class="m-input"
          rows="4"
        ></textarea>
        <div class="m-footer">
          <button class="m-btn-secondary" onclick={() => (descriptionModalOpen = false)}>
            Cancel
          </button>
          <button class="m-btn-primary" onclick={submitDescription}>Save</button>
        </div>
      </div>
    {/if}

    <!-- Delete confirm -->
    {#if confirmDeleteOpen}
      <div
        class="m-backdrop"
        onclick={() => (confirmDeleteOpen = false)}
        role="presentation"
      ></div>
      <div class="m-modal" role="dialog">
        <h2>{$t('collectionDetail.delete')}</h2>
        <p>
          {$t('collectionDetail.deleteConfirm', { values: { name: collection.name } })}
        </p>
        <div class="m-footer">
          <button class="m-btn-secondary" onclick={() => (confirmDeleteOpen = false)}>
            Cancel
          </button>
          <button class="m-btn-destructive" onclick={handleDelete}>
            {$t('collectionDetail.delete')}
          </button>
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .detail-view {
    padding: 24px 32px;
    color: var(--text-primary);
  }

  .back-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 6px 10px;
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    margin-bottom: 16px;
  }
  .back-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .loading,
  .empty-list,
  .not-found {
    padding: 40px;
    color: var(--text-muted);
    text-align: center;
  }

  /* ── Header ── */
  .detail-header {
    margin-bottom: 32px;
  }

  .header-content {
    display: flex;
    gap: 24px;
    align-items: flex-end;
  }

  .header-cover {
    flex-shrink: 0;
  }

  .header-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .eyebrow {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .kind-tag {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--accent-primary);
  }

  .title {
    margin: 0;
    font-size: 40px;
    font-weight: 700;
    line-height: 1.1;
    word-wrap: break-word;
  }

  .description {
    margin: 0;
    color: var(--text-secondary);
    font-size: 14px;
    max-width: 720px;
  }

  .meta {
    font-size: 13px;
    color: var(--text-muted);
    margin-top: 4px;
  }

  .header-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-top: 12px;
  }

  .primary-action {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px 22px;
    background: var(--accent-primary);
    color: #fff;
    border: none;
    border-radius: 999px;
    font-size: 14px;
    font-weight: 700;
    font-family: inherit;
    cursor: pointer;
  }
  .primary-action:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .primary-action:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .secondary-action {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 9px 16px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }
  .secondary-action:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .secondary-action:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .icon-action {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-family: inherit;
    cursor: pointer;
  }
  .icon-action:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .icon-action.small {
    width: 24px;
    height: 24px;
  }

  /* ── Overflow menu (shared by header ⋯ and row ⋯) ── */
  .overflow-wrap {
    position: relative;
  }

  .overflow-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
  }

  .overflow-menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    min-width: 200px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    z-index: 51;
  }

  .overflow-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    cursor: pointer;
    text-align: left;
    border-radius: 6px;
  }
  .overflow-item:hover {
    background: var(--bg-hover);
  }
  .overflow-item.destructive {
    color: var(--error, #e57373);
  }

  .item-menu {
    min-width: 140px;
  }

  /* ── Item list — matches existing track-list vocabulary ── */
  .item-list {
    border-top: 1px solid var(--bg-tertiary);
  }

  .item-list-header,
  .item-row {
    display: grid;
    grid-template-columns: 40px 1fr 140px 80px 90px 72px 60px 40px;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
  }

  .item-list-header {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--text-muted);
    border-bottom: 1px solid var(--bg-tertiary);
    padding-top: 12px;
    padding-bottom: 10px;
  }

  .item-row {
    position: relative;
  }
  .item-row:not(:last-child) {
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  }
  .item-row:hover {
    background: var(--bg-hover);
  }

  .col-idx {
    color: var(--text-muted);
    font-size: 13px;
    text-align: center;
  }

  .col-item {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .artwork {
    width: 36px;
    height: 36px;
    object-fit: cover;
    border-radius: 4px;
    flex-shrink: 0;
  }

  .artwork-placeholder {
    background: var(--bg-tertiary);
  }

  .item-meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 2px;
  }

  .item-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .item-subtitle {
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .type-cell {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-muted);
    font-size: 11px;
  }

  .type-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .col-tracks,
  .col-year {
    text-align: right;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .col-menu {
    display: flex;
    justify-content: flex-end;
    position: relative;
  }

  /* ── Inline modals (rename / description / delete confirm) ── */
  .m-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    z-index: 9998;
  }

  .m-modal {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 420px;
    max-width: 90vw;
    padding: 24px;
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 12px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    z-index: 9999;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .m-modal h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 700;
  }

  .m-modal p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 14px;
  }

  .m-input {
    width: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    font-family: inherit;
  }

  textarea.m-input {
    resize: vertical;
    min-height: 80px;
  }

  .m-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .m-btn-primary,
  .m-btn-secondary,
  .m-btn-destructive {
    padding: 10px 20px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }

  .m-btn-primary {
    background: var(--accent-primary);
    color: #fff;
    border: none;
  }
  .m-btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .m-btn-secondary {
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
  }

  .m-btn-destructive {
    background: var(--error, #e57373);
    color: #fff;
    border: none;
  }
</style>
