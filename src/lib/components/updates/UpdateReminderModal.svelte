<script lang="ts">
  import Modal from '../Modal.svelte';

  type ReminderChoice = 'later' | 'ignore_release' | 'disable_all';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onRemindLater: () => void;
    onIgnoreRelease: () => void;
    onDisableAllUpdates: () => void;
  }

  let { isOpen, onClose, onRemindLater, onIgnoreRelease, onDisableAllUpdates }: Props = $props();

  let choice = $state<ReminderChoice>('later');

  function handleSubmit(): void {
    if (choice === 'ignore_release') {
      onIgnoreRelease();
    } else if (choice === 'disable_all') {
      onDisableAllUpdates();
    } else {
      onRemindLater();
    }
    onClose();
  }
</script>

<Modal {isOpen} onClose={onClose} title="New version reminder" maxWidth="600px">
  <div class="reminder-modal">
    <label class="option">
      <input type="radio" name="reminder" bind:group={choice} value="later" />
      <span>Remind me later</span>
    </label>

    <label class="option">
      <input type="radio" name="reminder" bind:group={choice} value="ignore_release" />
      <span>Do not notify me again about this release</span>
    </label>

    <label class="option">
      <input type="radio" name="reminder" bind:group={choice} value="disable_all" />
      <span>Do not notify me about new releases</span>
    </label>

    <p class="hint">Can be re-enabled from Settings</p>
  </div>

  {#snippet footer()}
    <div class="footer-actions">
      <button class="btn btn-primary" type="button" onclick={handleSubmit}>OK, close</button>
    </div>
  {/snippet}
</Modal>

<style>
  .reminder-modal {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .option {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-primary);
    font-size: 14px;
  }

  .option input[type='radio'] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-primary);
  }

  .hint {
    margin: 4px 0 0;
    color: var(--text-muted);
    font-size: 13px;
  }

  .footer-actions {
    display: flex;
    width: 100%;
    justify-content: flex-end;
  }
</style>

