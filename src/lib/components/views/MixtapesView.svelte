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

  const mixtapes = $derived(
    $collectionsStore.filter((mc) => mc.kind === 'mixtape'),
  );

  onMount(() => {
    loadCollections();
  });
</script>

<div class="mixtapes-view">
  <header class="view-header">
    <h1>{$t('mixtapes.nav')}</h1>
    <button
      type="button"
      class="primary-cta"
      onclick={() => onCreate?.()}
    >
      <Plus size={16} />
      <span>{$t('mixtapes.empty.cta')}</span>
    </button>
  </header>

  {#if mixtapes.length === 0}
    <div class="empty-state">
      <CollectionMosaic items={[]} size={160} kind="mixtape" />
      <h2>{$t('mixtapes.empty.title')}</h2>
      <button
        type="button"
        class="primary-cta"
        onclick={() => onCreate?.()}
      >
        {$t('mixtapes.empty.cta')}
      </button>
    </div>
  {:else}
    <div class="grid">
      {#each mixtapes as mc (mc.id)}
        <button
          type="button"
          class="card"
          onclick={() => onOpen?.(mc.id)}
        >
          <CollectionMosaic
            items={mc.items}
            size={184}
            kind={mc.kind}
            customCoverUrl={coverUrlFor(mc)}
          />
          <div class="card-label">{$t('mixtapes.label')}</div>
          <div class="card-name">{mc.name}</div>
          <div class="card-meta">
            {$t('mixtapes.albumCount', { values: { count: mc.items.length } })}
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .mixtapes-view {
    padding: 24px 32px;
    color: var(--text-primary);
  }

  .view-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 24px;
  }

  .view-header h1 {
    margin: 0;
    font-size: 32px;
    font-weight: 700;
    color: var(--text-primary);
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
</style>
