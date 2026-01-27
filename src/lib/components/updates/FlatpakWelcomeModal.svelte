<script lang="ts">
  import Modal from '../Modal.svelte';
  import { Package } from 'lucide-svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();
</script>

<div class="flatpak-welcome-modal">
  <Modal
    {isOpen}
    onClose={onClose}
    title="Running in Flatpak"
    maxWidth="520px"
  >
    <div class="modal-content">
      <div class="icon-container">
        <Package size={48} strokeWidth={1.5} />
      </div>

      <p class="intro">
        QBZ detected it's running inside a <strong>Flatpak sandbox</strong>.
        There are some considerations to ensure the best experience.
      </p>

      <div class="info-box">
        <h4>What you should know:</h4>
        <ul>
          <li>Access to external music folders (NAS, network mounts) requires manual permission grants</li>
          <li>Some audio features may behave differently due to sandbox restrictions</li>
          <li>The <strong>Settings</strong> page includes Flatpak-specific commands you can copy and run</li>
        </ul>
      </div>

      <p class="note">
        Check the <strong>Flatpak Sandbox</strong> section in Settings for detailed instructions and copy-paste commands.
      </p>
    </div>

    {#snippet footer()}
      <div class="footer-actions">
        <button class="btn btn-primary" type="button" onclick={onClose}>
          Got it
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
