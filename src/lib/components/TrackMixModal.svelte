<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { t } from 'svelte-i18n';
  import { LoaderCircle } from 'lucide-svelte';
  import { showToast } from '$lib/stores/toastStore';
  import { buildSizeOptions } from './trackMixModal.helpers';

  type Props = {
    open: boolean;
    collectionId: string;
    /** Raw item count from the collection, shown as the "of {total}" hint. */
    totalRawTracks: number;
    onClose: () => void;
    onConfirm: (sampleSize: number) => void;
  };

  let { open, collectionId, totalRawTracks, onClose, onConfirm }: Props = $props();

  let loading = $state(false);
  let uniqueCount = $state<number | null>(null);
  let cachedFor = $state<string | null>(null);
  let selectedSize = $state<number | null>(null);
  let aborter: AbortController | null = null;

  const sizeOptions = $derived(buildSizeOptions(uniqueCount));

  async function fetchUniqueCount() {
    if (aborter) aborter.abort();
    aborter = new AbortController();
    const signal = aborter.signal;
    const targetCollection = collectionId;
    loading = true;
    try {
      const count = await invoke<number>('v2_collection_unique_track_count', {
        collectionId: targetCollection,
      });
      if (signal.aborted) return;
      uniqueCount = count;
      cachedFor = targetCollection;
      const opts = buildSizeOptions(count);
      selectedSize = opts[0]?.size ?? null;
    } catch (err) {
      if (signal.aborted) return;
      console.error('[TrackMixModal] unique count fetch failed:', err);
      showToast($t('mixModal.loadFailed'), 'error');
      onClose();
    } finally {
      if (!signal.aborted) loading = false;
    }
  }

  $effect(() => {
    if (open && cachedFor !== collectionId) {
      // Either first open for this collection OR collectionId changed.
      uniqueCount = null;
      selectedSize = null;
      void fetchUniqueCount();
    }
    if (!open && aborter) {
      aborter.abort();
      aborter = null;
      loading = false;
    }
  });

  function handleConfirm() {
    if (selectedSize === null || loading) return;
    onConfirm(selectedSize);
  }
</script>

{#if open}
  <div
    class="tm-backdrop"
    onclick={onClose}
    role="presentation"
  ></div>
  <div class="tm-modal" role="dialog" aria-modal="true">
    <h2 class="tm-title">{$t('mixModal.title')}</h2>

    {#if loading}
      <div class="tm-loading">
        <LoaderCircle size={20} class="spin" />
        <span>{$t('mixModal.computing')}</span>
      </div>
    {:else if uniqueCount !== null && uniqueCount > 0}
      <p class="tm-subtitle">
        {$t('mixModal.subtitle', {
          values: { unique: uniqueCount, total: totalRawTracks },
        })}
      </p>
      <ul class="tm-options" role="radiogroup" aria-label={$t('mixModal.title')}>
        {#each sizeOptions as opt}
          <li>
            <button
              type="button"
              class="tm-option"
              class:selected={selectedSize === opt.size}
              role="radio"
              aria-checked={selectedSize === opt.size}
              onclick={() => (selectedSize = opt.size)}
            >
              {opt.isAll
                ? $t('mixModal.allOption', { values: { n: opt.size } })
                : opt.size}
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    <div class="tm-footer">
      <button type="button" class="tm-btn-secondary" onclick={onClose}>
        {$t('actions.cancel')}
      </button>
      <button
        type="button"
        class="tm-btn-primary"
        onclick={handleConfirm}
        disabled={selectedSize === null || loading}
      >
        {$t('mixModal.confirm')}
      </button>
    </div>
  </div>
{/if}

<style>
  .tm-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    z-index: 3000;
  }

  .tm-modal {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 3001;
    background: var(--surface-elevated, #1f1f23);
    color: var(--text-primary, #fff);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
    padding: 24px;
    width: min(440px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .tm-title {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
  }

  .tm-loading {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary, #aaa);
    font-size: 14px;
  }

  .tm-subtitle {
    margin: 0;
    font-size: 13px;
    color: var(--text-secondary, #aaa);
  }

  .tm-options {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    overflow-y: auto;
  }

  .tm-option {
    appearance: none;
    background: var(--surface, #2a2a30);
    color: var(--text-primary, #fff);
    border: 1px solid var(--border, #3a3a40);
    border-radius: 999px;
    padding: 10px 18px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: border-color 0.15s, background 0.15s;
    min-height: 40px;
  }

  .tm-option:hover {
    border-color: var(--text-secondary, #aaa);
  }

  .tm-option.selected {
    border-color: var(--primary, #2cb05a);
    background: color-mix(in oklab, var(--primary, #2cb05a) 18%, transparent);
  }

  .tm-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 8px;
  }

  .tm-btn-secondary,
  .tm-btn-primary {
    appearance: none;
    border: none;
    border-radius: 8px;
    padding: 10px 18px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s, opacity 0.15s;
  }

  .tm-btn-secondary {
    background: transparent;
    color: var(--text-primary, #fff);
    border: 1px solid var(--border, #3a3a40);
  }

  .tm-btn-secondary:hover {
    background: var(--surface, #2a2a30);
  }

  .tm-btn-primary {
    background: var(--primary, #2cb05a);
    color: #fff;
  }

  .tm-btn-primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .tm-btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
