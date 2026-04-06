<script lang="ts">
  import Modal from '../Modal.svelte';
  import { Package } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();
  let environment = "Flatpak";
</script>

<div class="flatpak-welcome-modal">
  <Modal
    {isOpen}
    onClose={onClose}
    title={$t('updates.runningIn', { values: { env: environment } })}
    maxWidth="520px"
  >
    <div class="modal-content">
      <div class="icon-container">
        <Package size={48} strokeWidth={1.5} />
      </div>

      <p class="intro">
        {$t('updates.runningInsideSandbox', { values: { env: environment } })} {$t('updates.flatpak.intro')}
      </p>

      <div class="info-box">
        <h4>{$t('updates.flatpak.whatYouShouldKnow.heading')}</h4>
        <ul>
          <li>{$t('updates.flatpak.whatYouShouldKnow.externalMusicFolderAccess')}</li>
          <li>{$t('updates.flatpak.whatYouShouldKnow.audioFeatures')}</li>
          <li>{$t('updates.flatpak.whatYouShouldKnow.settingsPage')}</li>
        </ul>
      </div>

      <p class="note">
        {$t('updates.flatpak.note')}
      </p>
    </div>

    {#snippet footer()}
      <div class="footer-actions">
        <button class="btn btn-primary" type="button" onclick={onClose}>
          {$t('actions.gotIt')}
        </button>
      </div>
    {/snippet}
  </Modal>
</div>

<style>
  .modal-content {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .icon-container {
    display: flex;
    justify-content: center;
    color: var(--accent-primary, #4285f4);
    margin-bottom: 8px;
  }

  .intro {
    margin: 0;
    font-size: 14px;
    line-height: 1.6;
    color: var(--text-primary);
    text-align: center;
  }

  .info-box {
    background: var(--bg-tertiary);
    border-radius: 8px;
    padding: 16px;
  }

  .info-box h4 {
    margin: 0 0 12px 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .info-box ul {
    margin: 0;
    padding: 0 0 0 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .info-box li {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
  }

  .note {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-muted);
    text-align: center;
  }

  .footer-actions {
    display: flex;
    width: 100%;
    justify-content: flex-end;
  }
</style>
