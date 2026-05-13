<script lang="ts">
  import { t } from '$lib/i18n';
  import ModalLite from '$lib/discovery-v2/ModalLite.svelte';

  // First-time warning shown to users running without hardware acceleration
  // when they enter immersive mode. Five known-heavy panels are already
  // disabled by default in CPU mode; this modal tells the user why so they
  // know where to look if they want to override or fix their graphics
  // configuration.
  //
  // Triggered by the parent (+page.svelte) when immersive opens, HW accel
  // is off, and the user hasn't dismissed the warning before.

  interface Props {
    isOpen: boolean;
    onClose: (dontShowAgain: boolean) => void;
  }

  let { isOpen, onClose }: Props = $props();

  let dontShowAgain = $state(true);

  function handleAcknowledge() {
    onClose(dontShowAgain);
  }

  function handleClose() {
    onClose(dontShowAgain);
  }
</script>

<ModalLite
  {isOpen}
  onClose={handleClose}
  title={$t('settings.appearance.immersivePanels.warningModal.title')}
  maxWidth="520px"
  footerAlign="end"
>
  <p class="warning-body">{$t('settings.appearance.immersivePanels.warningModal.body')}</p>
  <p class="warning-hint">{$t('settings.appearance.immersivePanels.warningModal.hint')}</p>

  <label class="dont-show-row">
    <input type="checkbox" bind:checked={dontShowAgain} />
    <span>{$t('settings.appearance.immersivePanels.warningModal.dontShowAgain')}</span>
  </label>

  {#snippet footer()}
    <button class="btn-primary" onclick={handleAcknowledge}>
      {$t('settings.appearance.immersivePanels.warningModal.acknowledge')}
    </button>
  {/snippet}
</ModalLite>

<style>
  .warning-body {
    margin: 0 0 12px;
    color: var(--text-primary);
    line-height: 1.5;
  }

  .warning-hint {
    margin: 0 0 16px;
    padding: 10px 12px;
    background: var(--alpha-05, rgba(255, 255, 255, 0.04));
    border-left: 3px solid var(--warning, #f59e0b);
    border-radius: 4px;
    color: var(--text-secondary, var(--text-muted));
    font-size: 13px;
    line-height: 1.4;
  }

  .dont-show-row {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    color: var(--text-secondary, var(--text-muted));
    font-size: 13px;
  }

  .dont-show-row input[type="checkbox"] {
    width: 16px;
    height: 16px;
    accent-color: var(--accent-primary, #7c3aed);
    cursor: pointer;
  }

  .btn-primary {
    padding: 8px 16px;
    background: var(--accent-primary, #7c3aed);
    color: var(--btn-primary-text, white);
    border: none;
    border-radius: 6px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:hover {
    filter: brightness(1.1);
  }
</style>
