<script lang="ts">
  import Modal from '../Modal.svelte';
  import { Package, Copy, Check } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  let copiedRequired = $state(false);
  let copiedOptional = $state(false);

  const requiredCommands = `sudo snap connect qbz-player:alsa
sudo snap connect qbz-player:pulseaudio
sudo snap connect qbz-player:pipewire
sudo snap connect qbz-player:mpris`;

  const optionalCommands = `sudo snap connect qbz-player:removable-media`;

  async function copyToClipboard(text: string, type: 'required' | 'optional'): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      if (type === 'required') {
        copiedRequired = true;
        setTimeout(() => { copiedRequired = false; }, 2000);
      } else {
        copiedOptional = true;
        setTimeout(() => { copiedOptional = false; }, 2000);
      }
    } catch {
      // Clipboard API may fail in some environments
    }
  }

  const environment = "Snap";
</script>

<div class="snap-welcome-modal">
  <Modal
    {isOpen}
    onClose={onClose}
    title={$t('updates.runningIn', { values: { env: environment } })}
    maxWidth="560px"
  >
    <div class="modal-content">
      <div class="icon-container">
        <Package size={48} strokeWidth={1.5} />
      </div>

      <p class="intro">
        {$t('updates.runningInsideSandbox', { values: { env: environment } })} {$t('updates.snap.intro')}
      </p>

      <div class="info-box">
        <div class="info-header">
          <h4>{$t('updates.snap.requiredPlugConnections')}</h4>
          <button
            class="copy-btn"
            type="button"
            onclick={() => copyToClipboard(requiredCommands, 'required')}
            title={$t('actions.copyCommand')}
          >
            {#if copiedRequired}
              <Check size={14} />
              <span>{$t('dacWizard.buttons.copied')}</span>
            {:else}
              <Copy size={14} />
              <span>{$t('dacWizard.buttons.copy')}</span>
            {/if}
          </button>
        </div>
        <pre class="commands">{requiredCommands}</pre>
      </div>

      <div class="info-box optional">
        <div class="info-header">
          <h4>{$t('updates.snap.optional')}</h4>
          <button
            class="copy-btn"
            type="button"
            onclick={() => copyToClipboard(optionalCommands, 'optional')}
            title={$t('actions.copyCommand')}
          >
            {#if copiedOptional}
              <Check size={14} />
              <span>{$t('dacWizard.buttons.copied')}</span>
            {:else}
              <Copy size={14} />
              <span>{$t('dacWizard.buttons.copy')}</span>
            {/if}
          </button>
        </div>
        <pre class="commands">{optionalCommands}</pre>
      </div>

      <p class="note">
        {$t('updates.snap.note')}
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

  .info-box.optional {
    background: var(--bg-secondary);
  }

  .info-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 10px;
  }

  .info-box h4 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .copy-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: 1px solid var(--border-primary);
    border-radius: 4px;
    padding: 4px 8px;
    font-size: 11px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: color 0.15s, border-color 0.15s;
  }

  .copy-btn:hover {
    color: var(--text-primary);
    border-color: var(--text-secondary);
  }

  .commands {
    margin: 0;
    padding: 10px 12px;
    background: var(--bg-primary);
    border-radius: 6px;
    font-family: 'JetBrains Mono', 'Fira Code', monospace;
    font-size: 12px;
    line-height: 1.6;
    color: var(--text-secondary);
    overflow-x: auto;
    white-space: pre;
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
