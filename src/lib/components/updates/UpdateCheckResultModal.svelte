<script lang="ts">
  import Modal from '../Modal.svelte';
  import type { UpdateCheckStatus } from '$lib/stores/updatesStore';

  interface Props {
    isOpen: boolean;
    status: UpdateCheckStatus;
    newVersion: string;
    onClose: () => void;
    onVisitReleasePage: () => void;
  }

  let { isOpen, status, newVersion, onClose, onVisitReleasePage }: Props = $props();
</script>

<Modal {isOpen} onClose={onClose} title="Check for updates" maxWidth="460px">
  <div class="result-body">
    {#if status === 'update_available'}
      <p class="message">New version available: v{newVersion}</p>
    {:else}
      <p class="message">No updates found.</p>
    {/if}
  </div>

  {#snippet footer()}
    <div class="footer-actions">
      <button class="btn btn-ghost" type="button" onclick={onClose}>Close</button>
      {#if status === 'update_available'}
        <button class="btn btn-primary" type="button" onclick={onVisitReleasePage}>Visit release page</button>
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

