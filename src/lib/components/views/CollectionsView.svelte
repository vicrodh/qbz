<script lang="ts">
  import { onMount } from 'svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { Plus } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import {
    collectionsStore,
    loadCollections,
    type MixtapeCollection,
  } from '$lib/stores/mixtapeCollectionsStore';
  import CollectionMosaic from '../CollectionMosaic.svelte';

  function coverUrlFor(col: MixtapeCollection): string | null {
    return col.custom_artwork_path ? convertFileSrc(col.custom_artwork_path) : null;
  }

  interface Props {
    onOpen?: (id: string) => void;
    onCreate?: () => void;
  }
  let { onOpen, onCreate }: Props = $props();

  /** Both plain Collections and ArtistCollections live in this view. */
  const collections = $derived(
    $collectionsStore.filter(
      (col) => col.kind === 'collection' || col.kind === 'artist_collection',
    ),
  );

  function labelFor(col: MixtapeCollection): string {
    if (col.kind === 'artist_collection') {
      const artistLabel = $t('collections.artistLabel');
      return col.name ? `${artistLabel} · ${col.name}` : artistLabel;
    }
    return $t('collections.label');
  }

  onMount(() => {
    loadCollections();
  });
</script>

<div class="collections-view">
  <header class="view-header">
    <h1>{$t('collections.nav')}</h1>
    <div class="header-actions">
      <button
        type="button"
        class="primary-cta"
        onclick={() => onCreate?.()}
      >
        <Plus size={16} />
        <span>{$t('collections.empty.cta')}</span>
      </button>
    </div>
  </header>

  {#if collections.length === 0}
    <div class="empty-state">
      <CollectionMosaic items={[]} size={160} kind="collection" />
      <h2>{$t('collections.empty.title')}</h2>
      <div class="empty-actions">
        <button
          type="button"
          class="primary-cta"
          onclick={() => onCreate?.()}
        >
          {$t('collections.empty.cta')}
        </button>
      </div>
      <!--
        Artist Collections are created from the artist's own page via the
        circular action button next to Follow / Radio. Not exposed here
        to avoid shipping a CTA that needs an artist picker we haven't
        built yet.
      -->
    </div>
  {:else}
    <div class="grid">
      {#each collections as col (col.id)}
        <button
          type="button"
          class="card"
          onclick={() => onOpen?.(col.id)}
        >
          <CollectionMosaic
            items={col.items}
            size={184}
            kind={col.kind}
            customCoverUrl={coverUrlFor(col)}
          />
          <div class="card-label">{labelFor(col)}</div>
          <div class="card-name">{col.name}</div>
          <div class="card-meta">
            {$t('mixtapes.albumCount', { values: { count: col.items.length } })}
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .collections-view {
    padding: 24px 32px;
    color: var(--text-primary);
  }

  .view-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 24px;
    gap: 12px;
    flex-wrap: wrap;
  }
  .view-header h1 {
    margin: 0;
    font-size: 32px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .header-actions {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .primary-cta {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px 20px;
    background: var(--accent-primary);
    color: #ffffff;
    border: none;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
    transition: background 150ms ease;
  }
  .primary-cta:hover {
    filter: brightness(1.1);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(208px, 1fr));
    gap: 20px;
  }

  .card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
    padding: 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    transition: background 150ms ease, border-color 150ms ease;
  }
  .card:hover {
    background: var(--bg-hover);
    border-color: var(--bg-hover);
  }

  .card-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--accent-primary);
    margin-top: 4px;
  }

  .card-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.3;
    width: 100%;
    word-wrap: break-word;
  }

  .card-meta {
    font-size: 12px;
    color: var(--text-muted);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    padding: 80px 0;
    color: var(--text-muted);
  }
  .empty-state h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    color: var(--text-secondary);
  }
  .empty-actions {
    display: inline-flex;
    gap: 8px;
  }
</style>
