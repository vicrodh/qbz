<script lang="ts">
  import { t } from '$lib/i18n';
  import Modal from '$lib/discovery-v2/ModalLite.svelte';
  import type { PlaylistDuplicateResult } from '$lib/types/index';

  interface Props {
    isOpen: boolean;
    duplicateResult: PlaylistDuplicateResult | null;
    loading?: boolean;
    onAddAll: () => void;
    onSkipDuplicates: () => void;
    onCancel: () => void;
  }

  let {
    isOpen,
    duplicateResult,
    loading = false,
    onAddAll,
    onSkipDuplicates,
    onCancel
  }: Props = $props();

  function handleClose() {
    if (!loading) {
      onCancel();
    }
  }
</script>

<Modal {isOpen} onClose={handleClose} title={$t('playlist.duplicates.title')}>
  {#if duplicateResult}
    <div class="duplicate-info">
      <p>
        {$t('playlist.duplicates.description', { values: { duplicateCount: duplicateResult.duplicate_count, totalCount: duplicateResult.total_tracks } })}
      </p>
    </div>
  {/if}

  {#snippet footer()}
    <div class="footer-right">
      <button
        class="btn btn-secondary"
        onclick={onAddAll}
        disabled={loading}
      >
        {#if loading}
          {$t('playlist.duplicates.adding')}
        {:else}
          {$t('playlist.duplicates.addAll')}
        {/if}
      </button>

      <button
        class="btn btn-primary"
        onclick={onSkipDuplicates}
        disabled={loading || duplicateResult?.duplicate_count === 0}
      >
        {#if loading}
          {$t('playlist.duplicates.adding')}
        {:else}
          {$t('playlist.duplicates.addNew')}
        {/if}
      </button>
    </div>
  {/snippet}
</Modal>

<style>
  .duplicate-info {
    margin-bottom: 24px;
  }

  .duplicate-info p {
    margin: 0;
    color: var(--text-secondary);
    line-height: 1.4;
  }

  .footer-right {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: auto;
  }
</style>
