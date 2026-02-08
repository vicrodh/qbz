<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { writeText as copyToClipboard } from '@tauri-apps/plugin-clipboard-manager';
  import { t } from '$lib/i18n';
  import Modal from './Modal.svelte';
  import { getConsoleLogsAsText } from '$lib/stores/consoleLogStore';
  import { showToast } from '$lib/stores/toastStore';
  import { Loader2 } from 'lucide-svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  let activeTab = $state<'terminal' | 'console'>('terminal');
  let terminalLogs = $state('');
  let consoleLogs = $state('');
  let isUploading = $state(false);
  let isLoading = $state(false);

  async function loadLogs() {
    isLoading = true;
    try {
      const lines: string[] = await invoke('get_backend_logs');
      terminalLogs = lines.join('\n');
    } catch (e) {
      terminalLogs = `Error loading logs: ${e}`;
    }
    consoleLogs = getConsoleLogsAsText();
    isLoading = false;
  }

  function handleOpen() {
    if (isOpen) {
      loadLogs();
    }
  }

  $effect(() => {
    handleOpen();
  });

  async function handleUpload() {
    isUploading = true;
    try {
      const combined = `=== QBZ Terminal Logs ===\n${terminalLogs}\n\n=== QBZ Console Logs ===\n${consoleLogs}`;
      const url: string = await invoke('upload_logs_to_paste', { content: combined });
      await copyToClipboard(url);
      showToast($t('settings.developer.uploadSuccess'), 'success');
    } catch (e) {
      showToast(`${$t('settings.developer.uploadError')}: ${e}`, 'error');
    } finally {
      isUploading = false;
    }
  }
</script>

<Modal {isOpen} {onClose} title={$t('settings.developer.logsTitle')} maxWidth="760px">
  {#snippet children()}
    <div class="tabs">
      <button
        class="tab"
        class:active={activeTab === 'terminal'}
        onclick={() => activeTab = 'terminal'}
      >
        {$t('settings.developer.tabTerminal')}
      </button>
      <button
        class="tab"
        class:active={activeTab === 'console'}
        onclick={() => activeTab = 'console'}
      >
        {$t('settings.developer.tabConsole')}
      </button>
    </div>

    <div class="log-container">
      {#if isLoading}
        <div class="loading">
          <Loader2 size={20} class="spin" />
        </div>
      {:else}
        <pre class="log-output">{activeTab === 'terminal' ? terminalLogs : consoleLogs}</pre>
      {/if}
    </div>
  {/snippet}

  {#snippet footer()}
    <button
      class="upload-btn"
      onclick={handleUpload}
      disabled={isUploading}
    >
      {#if isUploading}
        <Loader2 size={14} class="spin" />
        {$t('settings.developer.uploading')}
      {:else}
        {$t('settings.developer.uploadLogs')}
      {/if}
    </button>
  {/snippet}
</Modal>

<style>
  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
  }

  .tab {
    padding: 6px 16px;
    border: 1px solid var(--bg-tertiary);
    background: transparent;
    color: var(--text-muted);
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
    transition: all 150ms ease;
  }

  .tab:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .tab.active {
    background: var(--accent-primary);
    color: white;
    border-color: var(--accent-primary);
  }

  .log-container {
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    max-height: 400px;
    overflow: auto;
    padding: 12px;
  }

  .log-output {
    font-family: 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-secondary);
    white-space: pre-wrap;
    word-break: break-all;
    margin: 0;
  }

  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 40px;
    color: var(--text-muted);
  }

  .upload-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .upload-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .upload-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
