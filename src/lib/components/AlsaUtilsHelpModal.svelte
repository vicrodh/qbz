<script lang="ts">
  import Modal from './Modal.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Check, AlertTriangle, Copy, Terminal } from 'lucide-svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  interface LinuxDistroInfo {
    distro_id: string;
    distro_name: string;
    install_command: string;
  }

  let { isOpen, onClose }: Props = $props();

  let isInstalled = $state<boolean | null>(null);
  let distroInfo = $state<LinuxDistroInfo | null>(null);
  let copied = $state(false);
  let isLoading = $state(true);

  async function checkStatus() {
    isLoading = true;
    try {
      const [installed, distro] = await Promise.all([
        invoke<boolean>('check_alsa_utils_installed'),
        invoke<LinuxDistroInfo>('get_linux_distro')
      ]);
      isInstalled = installed;
      distroInfo = distro;
    } catch (e) {
      console.error('Failed to check alsa-utils status:', e);
      isInstalled = false;
      distroInfo = null;
    } finally {
      isLoading = false;
    }
  }

  async function copyCommand() {
    if (!distroInfo?.install_command) return;
    try {
      await navigator.clipboard.writeText(distroInfo.install_command);
      copied = true;
      setTimeout(() => copied = false, 2000);
    } catch (e) {
      console.error('Failed to copy:', e);
    }
  }

  $effect(() => {
    if (isOpen) {
      checkStatus();
    }
  });
</script>

<Modal {isOpen} {onClose} title="Bit-perfect Device Detection" maxWidth="480px">
  {#snippet children()}
    <div class="help-content">
      {#if isLoading}
        <div class="loading">Checking system...</div>
      {:else}
        <div class="status-section">
          <div class="status-row" class:installed={isInstalled} class:missing={!isInstalled}>
            {#if isInstalled}
              <Check size={18} />
              <span>alsa-utils is installed</span>
            {:else}
              <AlertTriangle size={18} />
              <span>alsa-utils is not installed</span>
            {/if}
          </div>
        </div>

        <div class="explanation">
          <p>
            The <strong>alsa-utils</strong> package provides hardware enumeration tools
            that QBZ uses to detect bit-perfect capable devices (<code>hw:X,Y</code>).
          </p>
          <p>
            Without it, only generic ALSA devices are shown, and you won't see the
            <span class="badge">BP</span> badge for true bit-perfect outputs.
          </p>
        </div>

        {#if !isInstalled && distroInfo}
          <div class="install-section">
            <div class="install-header">
              <Terminal size={14} />
              <span>Install command{distroInfo.distro_name ? ` for ${distroInfo.distro_name}` : ''}</span>
            </div>
            <div class="command-box">
              <code>{distroInfo.install_command}</code>
              <button class="copy-btn" onclick={copyCommand} title="Copy to clipboard">
                {#if copied}
                  <Check size={14} />
                {:else}
                  <Copy size={14} />
                {/if}
              </button>
            </div>
            <p class="install-note">
              After installing, restart QBZ to detect bit-perfect devices.
            </p>
          </div>
        {:else if isInstalled}
          <div class="all-good">
            <p>Your system is properly configured for bit-perfect device detection.</p>
            <p>If you don't see your device listed as bit-perfect, ensure it's connected and recognized by your system.</p>
          </div>
        {/if}
      {/if}
    </div>
  {/snippet}
</Modal>

<style>
  .help-content {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .loading {
    text-align: center;
    color: var(--text-muted);
    padding: 20px;
  }

  .status-section {
    padding-bottom: 16px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .status-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
  }

  .status-row.installed {
    background: rgba(34, 197, 94, 0.1);
    color: #22c55e;
  }

  .status-row.missing {
    background: rgba(234, 179, 8, 0.1);
    color: #eab308;
  }

  .explanation {
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-secondary);
  }

  .explanation p {
    margin: 0 0 8px 0;
  }

  .explanation p:last-child {
    margin-bottom: 0;
  }

  .explanation code {
    background: var(--bg-tertiary);
    padding: 2px 5px;
    border-radius: 3px;
    font-family: monospace;
    font-size: 12px;
  }

  .explanation strong {
    color: var(--text-primary);
  }

  .badge {
    display: inline-block;
    padding: 2px 5px;
    border-radius: 3px;
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    background: rgba(59, 130, 246, 0.2);
    color: #3b82f6;
    border: 1px solid rgba(59, 130, 246, 0.3);
    vertical-align: middle;
  }

  .install-section {
    background: var(--bg-secondary);
    border-radius: 8px;
    padding: 12px;
  }

  .install-header {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .command-box {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 10px 12px;
  }

  .command-box code {
    flex: 1;
    font-family: monospace;
    font-size: 13px;
    color: var(--text-primary);
    word-break: break-all;
  }

  .copy-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .copy-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .install-note {
    margin: 8px 0 0 0;
    font-size: 11px;
    color: var(--text-muted);
  }

  .all-good {
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-secondary);
  }

  .all-good p {
    margin: 0 0 8px 0;
  }

  .all-good p:last-child {
    margin-bottom: 0;
  }
</style>
