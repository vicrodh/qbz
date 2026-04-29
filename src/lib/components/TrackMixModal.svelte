<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import { LoaderCircle } from 'lucide-svelte';
  import { showToast } from '$lib/stores/toastStore';
  import Modal from './Modal.svelte';
  import Dropdown from './Dropdown.svelte';
  import { buildSizeOptions } from './trackMixModal.helpers';

  type Props = {
    open: boolean;
    collectionId: string;
    /** Raw item count from the collection. Currently informational only;
     * kept on the API surface in case callers want to surface it later. */
    totalRawTracks: number;
    onClose: () => void;
    onConfirm: (sampleSize: number) => void;
  };

  let { open, collectionId, totalRawTracks: _totalRawTracks, onClose, onConfirm }: Props = $props();

  let loading = $state(false);
  let uniqueCount = $state<number | null>(null);
  let cachedFor = $state<string | null>(null);
  let selectedSize = $state<number | null>(null);
  let aborter: AbortController | null = null;

  const sizeOptions = $derived(buildSizeOptions(uniqueCount));

  function handleDropdownChange(label: string) {
    const n = Number.parseInt(label, 10);
    if (!Number.isNaN(n)) {
      const match = sizeOptions.find((o) => o.size === n && !o.isAll);
      if (match) {
        selectedSize = n;
        return;
      }
    }
    const allOpt = sizeOptions.find((o) => o.isAll);
    if (allOpt) selectedSize = allOpt.size;
  }

  function isSelectedAll(): boolean {
    if (selectedSize === null) return false;
    return sizeOptions.find((o) => o.size === selectedSize)?.isAll ?? false;
  }

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

<Modal
  isOpen={open}
  onClose={onClose}
  title={$t('mixModal.title')}
  maxWidth="440px"
>
  <div class="tm-content">
    <p class="tm-body">{$t('mixModal.body')}</p>

    {#if loading}
      <div class="tm-loading">
        <LoaderCircle size={18} class="spin" />
        <span>{$t('mixModal.computing')}</span>
      </div>
    {:else if uniqueCount !== null && uniqueCount > 0}
      <div class="tm-field">
        <label class="tm-field-label" for="tm-size-dropdown">
          {$t('mixModal.numberOfSongs')}
        </label>
        <div id="tm-size-dropdown" class="tm-dropdown-wrap">
          <Dropdown
            value={selectedSize === null
              ? ''
              : isSelectedAll()
                ? $t('mixModal.allOption', { values: { n: selectedSize } })
                : String(selectedSize)}
            options={sizeOptions.map((o) =>
              o.isAll
                ? $t('mixModal.allOption', { values: { n: o.size } })
                : String(o.size),
            )}
            onchange={handleDropdownChange}
            wide
          />
        </div>
        <p class="tm-helper">
          {$t('mixModal.songsAvailable', { values: { n: uniqueCount } })}
        </p>
      </div>
    {/if}
  </div>

  {#snippet footer()}
    <div class="tm-actions">
      <button type="button" class="btn btn-secondary" onclick={onClose}>
        {$t('actions.cancel')}
      </button>
      <button
        type="button"
        class="btn btn-primary"
        onclick={handleConfirm}
        disabled={selectedSize === null || loading}
      >
        {$t('mixModal.confirm')}
      </button>
    </div>
  {/snippet}
</Modal>

<style>
  .tm-content {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .tm-body {
    margin: 0;
    font-size: 14px;
    line-height: 1.5;
    color: var(--text-secondary, #c9c9d0);
  }

  .tm-loading {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-secondary, #aaa);
    font-size: 14px;
  }

  .tm-field {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .tm-field-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary, #fff);
  }

  .tm-dropdown-wrap {
    width: 100%;
  }

  .tm-helper {
    margin: 0;
    font-size: 12px;
    color: var(--text-tertiary, #888);
  }

  .tm-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    width: 100%;
  }
</style>
