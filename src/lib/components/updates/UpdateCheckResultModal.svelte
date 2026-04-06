<script lang="ts">
  import Modal from '../Modal.svelte';
  import type { UpdateCheckStatus } from '$lib/stores/updatesStore';
  import { t } from '$lib/i18n';

  interface Props {
    isOpen: boolean;
    status: UpdateCheckStatus;
    newVersion: string;
    onClose: () => void;
    onVisitReleasePage: () => void;
  }

  let { isOpen, status, newVersion, onClose, onVisitReleasePage }: Props = $props();
</script>

<Modal {isOpen} onClose={onClose} title={ $t('updates.checkForUpdates') } maxWidth="460px">
  <div class="result-body">
    {#if status === 'update_available'}
      <p class="message">{$t('updates.newVersionAvailable', { values: { version: newVersion } })}</p>
    {:else}
      <p class="message">{$t('updates.noUpdatesFound')}</p>
    {/if}
  </div>

  {#snippet footer()}
    <div class="footer-actions">
      <button class="btn btn-ghost" type="button" onclick={onClose}>{$t('actions.close')}</button>
      {#if status === 'update_available'}
        <button class="btn btn-primary" type="button" onclick={onVisitReleasePage}>{$t('actions.visitReleasePage')}</button>
      {/if}
    </div>
  {/snippet}
</Modal>

<style>
  .result-body {
    display: flex;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 12px 0;
  }

  .message {
    margin: 0;
    color: var(--text-primary);
    font-weight: 500;
  }

  .footer-actions {
    display: flex;
    width: 100%;
    justify-content: flex-end;
    gap: 8px;
  }
</style>

