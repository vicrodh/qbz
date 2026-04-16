<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { writeText as copyToClipboard } from '@tauri-apps/plugin-clipboard-manager';
  import { t } from '$lib/i18n';
  import Modal from './Modal.svelte';
  import Toggle from './Toggle.svelte';
  import { getConsoleLogsAsText } from '$lib/stores/consoleLogStore';
  import { showToast } from '$lib/stores/toastStore';
  import { LoaderCircle, Copy, Check } from 'lucide-svelte';
  import { collectDiagnosticsText } from '$lib/services/diagnosticsSnapshot';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  // Opt-out toggle: when ON (default) the Terminal paste upload bundles the
  // System Diagnostics snapshot + a 10s Chromecast/DLNA scan into the same
  // paste, so bug reporters share ONE link instead of two.
  // Persisted in localStorage.
  const BUNDLE_KEY = 'qbz-paste-bundle-diagnostics';
  let bundleDiagnostics = $state(true);
  try {
    const saved = localStorage.getItem(BUNDLE_KEY);
    if (saved !== null) bundleDiagnostics = saved === 'true';
  } catch { /* ignore */ }

  function setBundleDiagnostics(value: boolean) {
    bundleDiagnostics = value;
    try { localStorage.setItem(BUNDLE_KEY, String(value)); } catch { /* ignore */ }
  }

  let activeTab = $state<'terminal' | 'console'>('terminal');
  let terminalLogs = $state('');
  let consoleLogs = $state('');
  let isUploadingTerminal = $state(false);
  let isUploadingConsole = $state(false);
  let isLoading = $state(false);
  let terminalUrl = $state('');
  let consoleUrl = $state('');
  let copiedTerminal = $state(false);
  let copiedConsole = $state(false);
  let bundleStage = $state<'idle' | 'collecting' | 'uploading'>('idle');

  async function loadLogs() {
    isLoading = true;
    terminalUrl = '';
    consoleUrl = '';
    try {
      const lines: string[] = await invoke('v2_get_backend_logs');
      terminalLogs = lines.join('\n');
    } catch (e) {
      terminalLogs = `Error loading logs: ${e}`;
    }
    consoleLogs = getConsoleLogsAsText();
    isLoading = false;
  }

  $effect(() => {
    if (isOpen) loadLogs();
  });

  async function uploadTab(tab: 'terminal' | 'console') {
    let content = tab === 'terminal' ? terminalLogs : consoleLogs;
    if (tab === 'terminal') isUploadingTerminal = true;
    else isUploadingConsole = true;

    try {
      // Bundle the diagnostics snapshot + a 10s cast scan into the terminal
      // paste when the toggle is on. Console pastes stay lean.
      if (tab === 'terminal' && bundleDiagnostics) {
        bundleStage = 'collecting';
        try {
          const diag = await collectDiagnosticsText({ includeCastScan: true, castScanMs: 10000 });
          content = [
            '===== QBZ DIAGNOSTICS =====',
            diag,
            '===== TERMINAL LOG =====',
            content,
          ].join('\n\n');
        } catch (e) {
          console.warn('[LogsModal] Failed to collect diagnostics for bundle:', e);
        }
      }
      bundleStage = 'uploading';
      const url: string = await invoke('v2_upload_logs_to_paste', { content });
      if (tab === 'terminal') terminalUrl = url;
      else consoleUrl = url;
      await copyToClipboard(url);
      showToast($t('settings.developer.uploadSuccess'), 'success');
    } catch (e) {
      showToast(`${$t('settings.developer.uploadError')}: ${e}`, 'error');
    } finally {
      if (tab === 'terminal') isUploadingTerminal = false;
      else isUploadingConsole = false;
      bundleStage = 'idle';
    }
  }

  async function copyUrl(tab: 'terminal' | 'console') {
    const url = tab === 'terminal' ? terminalUrl : consoleUrl;
    if (!url) return;
    await copyToClipboard(url);
    if (tab === 'terminal') { copiedTerminal = true; setTimeout(() => copiedTerminal = false, 2000); }
    else { copiedConsole = true; setTimeout(() => copiedConsole = false, 2000); }
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
        {#if terminalUrl}<span class="tab-check">&#10003;</span>{/if}
      </button>
      <button
        class="tab"
        class:active={activeTab === 'console'}
        onclick={() => activeTab = 'console'}
      >
        {$t('settings.developer.tabConsole')}
        {#if consoleUrl}<span class="tab-check">&#10003;</span>{/if}
      </button>
    </div>

    <div class="log-container">
      {#if isLoading}
        <div class="loading">
          <LoaderCircle size={20} class="spin" />
        </div>
      {:else}
        <pre class="log-output">{activeTab === 'terminal' ? terminalLogs : consoleLogs}</pre>
      {/if}
    </div>
  {/snippet}

  {#snippet footer()}
    <div class="footer-content">
      <div class="footer-left">
        {#if activeTab === 'terminal'}
          <div class="bundle-row">
            <Toggle
              enabled={bundleDiagnostics}
              onchange={(v) => setBundleDiagnostics(v)}
            />
            <div class="bundle-text">
              <span class="bundle-label">{$t('settings.developer.bundleDiagnostics')}</span>
              <small class="bundle-note">{$t('settings.developer.bundleDiagnosticsDesc')}</small>
            </div>
          </div>
        {/if}
        <button
          class="upload-btn"
          onclick={() => uploadTab(activeTab)}
          disabled={activeTab === 'terminal' ? isUploadingTerminal : isUploadingConsole}
        >
          {#if (activeTab === 'terminal' ? isUploadingTerminal : isUploadingConsole)}
            <LoaderCircle size={14} class="spin" />
            {#if activeTab === 'terminal' && bundleStage === 'collecting'}
              {$t('settings.developer.collectingDiagnostics')}
            {:else}
              {$t('settings.developer.uploading')}
            {/if}
          {:else}
            {$t('settings.developer.uploadTab', { values: { tab: activeTab === 'terminal' ? $t('settings.developer.tabTerminal') : $t('settings.developer.tabConsole') } })}
          {/if}
        </button>
        {#if activeTab === 'terminal' && terminalUrl}
          <div class="url-row">
            <code class="uploaded-url">{terminalUrl}</code>
            <button class="copy-url-btn" onclick={() => copyUrl('terminal')}>
              {#if copiedTerminal}<Check size={12} />{:else}<Copy size={12} />{/if}
            </button>
          </div>
        {/if}
        {#if activeTab === 'console' && consoleUrl}
          <div class="url-row">
            <code class="uploaded-url">{consoleUrl}</code>
            <button class="copy-url-btn" onclick={() => copyUrl('console')}>
              {#if copiedConsole}<Check size={12} />{:else}<Copy size={12} />{/if}
            </button>
          </div>
        {/if}
      </div>

      {#if terminalUrl || consoleUrl}
        <p class="help-hint">
          {$t('settings.developer.bugReportHint')}
        </p>
      {/if}
    </div>
  {/snippet}
</Modal>

<style>
  .tabs {
    display: flex;
    gap: 4px;
    margin-bottom: 12px;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 16px;
    border: 1px solid var(--bg-tertiary);
    background: transparent;
    color: var(--text-muted);
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
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

  .tab-check {
    font-size: 11px;
    opacity: 0.8;
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

  .footer-content {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    width: 100%;
  }

  .footer-left {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .bundle-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .bundle-text {
    display: flex;
    flex-direction: column;
  }

  .bundle-label {
    font-size: 13px;
    color: var(--text-primary);
  }

  .bundle-note {
    font-size: 11px;
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
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
    white-space: nowrap;
  }

  .upload-btn:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .upload-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .url-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .uploaded-url {
    font-family: 'JetBrains Mono', 'Fira Code', 'SF Mono', monospace;
    font-size: 12px;
    color: var(--accent-primary);
    background: var(--bg-secondary);
    padding: 4px 8px;
    border-radius: 4px;
    user-select: all;
  }

  .copy-url-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 4px;
    background: transparent;
    border: none;
    color: var(--text-muted);
    border-radius: 4px;
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
  }

  .copy-url-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .help-hint {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.5;
    margin: 0;
    max-width: 280px;
    text-align: right;
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
