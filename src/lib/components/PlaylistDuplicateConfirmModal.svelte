<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import Modal from './Modal.svelte';
  import type { PlaylistDuplicateResult } from '$lib/types/index';

  export let isOpen = false;
  export let duplicateResult: PlaylistDuplicateResult | null = null;
  export let loading = false;

  const dispatch = createEventDispatcher();

  function handleAddAll() {
    dispatch('addAll');
  }

  function handleSkipDuplicates() {
    dispatch('skipDuplicates');
  }

  function handleCancel() {
    dispatch('cancel');
  }

  function handleClose() {
    if (!loading) {
      dispatch('cancel');
    }
  }
</script>

<Modal {isOpen} onClose={handleClose} title={'You\'ve added some of these tracks before'}>
  {#if duplicateResult}
    <div class="duplicate-info">
      <p>
        This playlist already contains <strong>{duplicateResult.duplicate_count}</strong> of the
        track{duplicateResult.duplicate_count !== 1 ? 's' : ''} you're adding.
        Add only the new ones, or add everything including duplicates ({duplicateResult.total_tracks} track{duplicateResult.total_tracks !== 1 ? 's' : ''}).
      </p>
    </div>
  {/if}

  {#snippet footer()}
    <div class="footer-right">
      <button 
        class="btn btn-secondary" 
        on:click={handleAddAll}
        disabled={loading}
      >
        {#if loading}
          Adding...
        {:else}
          Add all tracks
        {/if}
      </button>
      
      <button 
        class="btn btn-primary" 
        on:click={handleSkipDuplicates}
        disabled={loading || duplicateResult?.duplicate_count === 0}
      >
        {#if loading}
          Adding...
        {:else}
          Add only new tracks
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