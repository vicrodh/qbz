<script lang="ts">
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { Bug, Circle, Copy, Power, Trash2 } from 'lucide-svelte';
  import Modal from '$lib/components/Modal.svelte';
  import { t } from '$lib/i18n';
  import { showToast } from '$lib/stores/toastStore';

  interface QconnectConnectionStatus {
    running: boolean;
    transport_connected: boolean;
    endpoint_url?: string | null;
    last_error?: string | null;
  }

  interface QconnectQueueSnapshot {
    version: { major: number; minor: number };
    queue_items: Array<unknown>;
    autoplay_items: Array<unknown>;
    shuffle_mode: boolean;
    autoplay_mode: boolean;
  }

  interface QconnectRendererSnapshot {
    playing_state?: number | null;
    volume?: number | null;
    muted?: boolean | null;
  }

  interface QconnectDiagnosticsEntry {
    ts: number;
    level: 'info' | 'warn' | 'error';
    channel: string;
    message: string;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    status: QconnectConnectionStatus;
    busy?: boolean;
    onToggleConnection: () => void | Promise<void>;
    queueSnapshot?: QconnectQueueSnapshot | null;
    rendererSnapshot?: QconnectRendererSnapshot | null;
    showDevDiagnostics?: boolean;
    diagnosticsLogs?: QconnectDiagnosticsEntry[];
    onClearDiagnostics?: () => void;
  }

  let {
    isOpen,
    onClose,
    status,
    busy = false,
    onToggleConnection,
    queueSnapshot = null,
    rendererSnapshot = null,
    showDevDiagnostics = false,
    diagnosticsLogs = [],
    onClearDiagnostics
  }: Props = $props();

  let diagnosticsExpanded = $state(false);

  function statusKey(): string {
    return status.transport_connected ? 'qconnect.statusConnected' : 'qconnect.statusDisconnected';
  }

  function statusClass(): string {
    return status.transport_connected ? 'connected' : 'disconnected';
  }

  function safeValue(value: string | number | boolean | null | undefined): string {
    if (value === null || value === undefined || value === '') return $t('qconnect.notAvailable');
    return String(value);
  }

  async function copyDiagnostics(): Promise<void> {
    if (!diagnosticsLogs.length) return;
    const lines = diagnosticsLogs.map((entry) => {
      const ts = new Date(entry.ts).toISOString();
      return `${ts} [${entry.level}] ${entry.channel}: ${entry.message}`;
    });
    await writeText(lines.join('\n'));
    showToast($t('qconnect.logsCopied'), 'success');
  }

  function formatTimestamp(ts: number): string {
    return new Date(ts).toLocaleTimeString();
  }
</script>

<Modal
  {isOpen}
  {onClose}
  title={$t('qconnect.title')}
  maxWidth="680px"
>
  <div class="qconnect-panel">
    <section class="status-card">
      <div class="status-main">
        <div class="status-row">
          <Circle size={10} class={`status-dot ${statusClass()}`} />
          <span class="status-text">{$t(statusKey())}</span>
        </div>
        <button class="toggle-btn" onclick={onToggleConnection} disabled={busy}>
          <Power size={14} />
          <span>{status.transport_connected ? $t('qconnect.turnOff') : $t('qconnect.turnOn')}</span>
        </button>
      </div>
      <div class="status-meta">
        <div><strong>{$t('qconnect.endpointLabel')}:</strong> {safeValue(status.endpoint_url)}</div>
        <div><strong>{$t('qconnect.lastErrorLabel')}:</strong> {safeValue(status.last_error)}</div>
      </div>
    </section>

    <section class="runtime-grid">
      <div class="runtime-card">
        <h4>{$t('qconnect.queueStateTitle')}</h4>
        <div class="runtime-line">
          <span>{$t('qconnect.queueVersionLabel')}</span>
          <strong>
            {#if queueSnapshot}
              {queueSnapshot.version.major}.{queueSnapshot.version.minor}
            {:else}
              {$t('qconnect.notAvailable')}
            {/if}
          </strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.queueItemsLabel')}</span>
          <strong>{queueSnapshot ? queueSnapshot.queue_items.length : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.autoplayItemsLabel')}</span>
          <strong>{queueSnapshot ? queueSnapshot.autoplay_items.length : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.shuffleModeLabel')}</span>
          <strong>{queueSnapshot ? safeValue(queueSnapshot.shuffle_mode) : $t('qconnect.notAvailable')}</strong>
        </div>
      </div>

      <div class="runtime-card">
        <h4>{$t('qconnect.rendererStateTitle')}</h4>
        <div class="runtime-line">
          <span>{$t('qconnect.playingStateLabel')}</span>
          <strong>{rendererSnapshot ? safeValue(rendererSnapshot.playing_state) : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.volumeLabel')}</span>
          <strong>{rendererSnapshot ? safeValue(rendererSnapshot.volume) : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.mutedLabel')}</span>
          <strong>{rendererSnapshot ? safeValue(rendererSnapshot.muted) : $t('qconnect.notAvailable')}</strong>
        </div>
      </div>
    </section>

    {#if showDevDiagnostics}
      <section class="diagnostics-card">
        <button
          class="diagnostics-toggle"
          onclick={() => diagnosticsExpanded = !diagnosticsExpanded}
          aria-expanded={diagnosticsExpanded}
        >
          <div class="left">
            <Bug size={14} />
            <span>{$t('qconnect.devDiagnosticsTitle')}</span>
          </div>
          <span class="count">{diagnosticsLogs.length}</span>
        </button>

        {#if diagnosticsExpanded}
          <div class="diagnostics-toolbar">
            <button class="mini-btn" onclick={copyDiagnostics} disabled={diagnosticsLogs.length === 0}>
              <Copy size={12} />
              <span>{$t('qconnect.copyLogs')}</span>
            </button>
            <button class="mini-btn danger" onclick={onClearDiagnostics} disabled={diagnosticsLogs.length === 0}>
              <Trash2 size={12} />
              <span>{$t('qconnect.clearLogs')}</span>
            </button>
          </div>

          {#if diagnosticsLogs.length === 0}
            <p class="logs-empty">{$t('qconnect.diagnosticsEmpty')}</p>
          {:else}
            <div class="logs-list">
              {#each diagnosticsLogs as entry}
                <div class={`log-row ${entry.level}`}>
                  <span class="log-time">{formatTimestamp(entry.ts)}</span>
                  <span class="log-channel">{entry.channel}</span>
                  <span class="log-message">{entry.message}</span>
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      </section>
    {/if}
  </div>
</Modal>

<style>
  .qconnect-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .status-card,
  .runtime-card,
  .diagnostics-card {
    border: 1px solid var(--bg-tertiary);
    border-radius: 10px;
    background: var(--bg-secondary);
  }

  .status-card {
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .status-main {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  .status-row {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .status-dot.connected {
    color: #22c55e;
    fill: currentColor;
  }

  .status-dot.disconnected {
    color: var(--text-muted);
    fill: currentColor;
  }

  .toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    padding: 6px 10px;
    cursor: pointer;
  }

  .toggle-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .status-meta {
    display: grid;
    gap: 6px;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.4;
  }

  .runtime-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }

  .runtime-card {
    padding: 12px;
  }

  .runtime-card h4 {
    margin: 0 0 10px 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-primary);
  }

  .runtime-card h4::before {
    content: '';
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: var(--accent-primary, #6366f1);
    opacity: 0.8;
  }

  .runtime-line {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
    color: var(--text-secondary);
    margin-top: 6px;
  }

  .runtime-line strong {
    color: var(--text-primary);
    font-weight: 600;
  }

  .diagnostics-toggle {
    width: 100%;
    border: none;
    border-radius: 10px;
    background: transparent;
    color: var(--text-primary);
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 12px;
    cursor: pointer;
  }

  .diagnostics-toggle .left {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .diagnostics-toggle .count {
    color: var(--text-muted);
    font-family: var(--font-mono, monospace);
    font-size: 12px;
  }

  .diagnostics-toolbar {
    display: flex;
    gap: 8px;
    padding: 0 12px 10px 12px;
  }

  .mini-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    padding: 5px 8px;
    font-size: 12px;
    cursor: pointer;
  }

  .mini-btn.danger {
    color: #fda4af;
  }

  .mini-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .logs-empty {
    margin: 0;
    padding: 0 12px 12px 12px;
    color: var(--text-muted);
    font-size: 12px;
  }

  .logs-list {
    max-height: 220px;
    overflow-y: auto;
    border-top: 1px solid var(--bg-tertiary);
  }

  .log-row {
    display: grid;
    grid-template-columns: 72px 100px 1fr;
    gap: 8px;
    padding: 6px 12px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
    font-size: 11px;
    color: var(--text-secondary);
  }

  .log-row.info .log-channel {
    color: #93c5fd;
  }

  .log-row.warn .log-channel {
    color: #fbbf24;
  }

  .log-row.error .log-channel {
    color: #fda4af;
  }

  .log-time {
    font-family: var(--font-mono, monospace);
    color: var(--text-muted);
  }

  .log-message {
    white-space: pre-wrap;
    word-break: break-word;
  }

  @media (max-width: 900px) {
    .runtime-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
