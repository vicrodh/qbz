<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { Bug, Circle, Copy, Pause, Play, Power, Square, Trash2, Volume2, VolumeX } from 'lucide-svelte';
  import Modal from '$lib/components/Modal.svelte';
  import { t } from '$lib/i18n';
  import { showToast } from '$lib/stores/toastStore';
  import type {
    QconnectConnectionStatus,
    QconnectDiagnosticsEntry,
    QconnectSessionSnapshot
  } from '$lib/services/qconnectRuntime';
  import type {
    QconnectQueueItemSnapshot,
    QconnectQueueSnapshot,
    QconnectRendererTrackSnapshot,
    QconnectRendererSnapshot
  } from '$lib/services/qconnectRemoteQueue';
  import { resolveQconnectQueueDisplayItems } from '$lib/services/qconnectRemoteQueue';

  interface QconnectRendererInfo {
    renderer_id: number;
    friendly_name?: string | null;
    brand?: string | null;
    model?: string | null;
    device_type?: number | null;
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    status: QconnectConnectionStatus;
    busy?: boolean;
    onToggleConnection: () => void | Promise<void>;
    queueSnapshot?: QconnectQueueSnapshot | null;
    rendererSnapshot?: QconnectRendererSnapshot | null;
    sessionSnapshot?: QconnectSessionSnapshot | null;
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
    sessionSnapshot = null,
    showDevDiagnostics = false,
    diagnosticsLogs = [],
    onClearDiagnostics
  }: Props = $props();

  let diagnosticsExpanded = $state(false);
  let controllerBusy = $state(false);

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

  function playingStateLabel(state: number | null | undefined): string {
    if (state === null || state === undefined) return $t('qconnect.notAvailable');
    if (state === 1) return 'Stopped';
    if (state === 2) return 'Playing';
    if (state === 3) return 'Paused';
    return String(state);
  }

  function rendererDisplayName(renderer: QconnectRendererInfo): string {
    if (renderer.friendly_name) return renderer.friendly_name;
    const parts: string[] = [];
    if (renderer.brand) parts.push(renderer.brand);
    if (renderer.model) parts.push(renderer.model);
    return parts.length > 0 ? parts.join(' ') : `Renderer #${renderer.renderer_id}`;
  }

  async function sendControllerPlay(): Promise<void> {
    controllerBusy = true;
    try {
      await invoke('v2_qconnect_set_player_state', {
        request: { playing_state: 2 }
      });
    } catch (err) {
      console.warn('[QconnectPanel] set_player_state(play) failed:', err);
    } finally {
      controllerBusy = false;
    }
  }

  async function sendControllerPause(): Promise<void> {
    controllerBusy = true;
    try {
      await invoke('v2_qconnect_set_player_state', {
        request: { playing_state: 3 }
      });
    } catch (err) {
      console.warn('[QconnectPanel] set_player_state(pause) failed:', err);
    } finally {
      controllerBusy = false;
    }
  }

  async function sendControllerStop(): Promise<void> {
    controllerBusy = true;
    try {
      await invoke('v2_qconnect_set_player_state', {
        request: { playing_state: 1 }
      });
    } catch (err) {
      console.warn('[QconnectPanel] set_player_state(stop) failed:', err);
    } finally {
      controllerBusy = false;
    }
  }

  async function sendControllerVolume(event: Event): Promise<void> {
    const input = event.target as HTMLInputElement;
    const vol = parseInt(input.value, 10);
    try {
      await invoke('v2_qconnect_set_volume', {
        request: { volume: vol }
      });
    } catch (err) {
      console.warn('[QconnectPanel] set_volume failed:', err);
    }
  }

  async function sendControllerMute(mute: boolean): Promise<void> {
    try {
      await invoke('v2_qconnect_mute_volume', {
        request: { value: mute }
      });
    } catch (err) {
      console.warn('[QconnectPanel] mute_volume failed:', err);
    }
  }

  async function sendSetActiveRenderer(rendererId: number): Promise<void> {
    controllerBusy = true;
    try {
      await invoke('v2_qconnect_set_active_renderer', {
        request: { renderer_id: rendererId }
      });
    } catch (err) {
      console.warn('[QconnectPanel] set_active_renderer failed:', err);
    } finally {
      controllerBusy = false;
    }
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

  function formatTrackRef(
    track: QconnectQueueItemSnapshot | QconnectRendererTrackSnapshot | null | undefined
  ): string {
    if (!track || track.track_id == null || track.queue_item_id == null) {
      return $t('qconnect.notAvailable');
    }
    return `track ${track.track_id} / qid ${track.queue_item_id}`;
  }

  function queuePreview(queueSnapshotValue: QconnectQueueSnapshot | null | undefined): QconnectQueueItemSnapshot[] {
    return resolveQconnectQueueDisplayItems(queueSnapshotValue).slice(0, 6);
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

    {#if status.transport_connected && sessionSnapshot}
      <section class="session-card">
        <h4>{$t('qconnect.sessionTitle')}</h4>
        <div class="runtime-line">
          <span>{$t('qconnect.activeRenderer')}</span>
          <strong>
            {#if sessionSnapshot.active_renderer_id != null}
              {@const activeRndr = sessionSnapshot.renderers.find(
                (r) => r.renderer_id === sessionSnapshot.active_renderer_id
              )}
              {activeRndr ? rendererDisplayName(activeRndr) : `#${sessionSnapshot.active_renderer_id}`}
            {:else}
              {$t('qconnect.notAvailable')}
            {/if}
          </strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.renderersLabel')}</span>
          <strong>{sessionSnapshot.renderers.length}</strong>
        </div>

        {#if sessionSnapshot.renderers.length > 0}
          <div class="renderer-list">
            {#each sessionSnapshot.renderers as renderer (renderer.renderer_id)}
              <div
                class="renderer-item"
                class:active={renderer.renderer_id === sessionSnapshot.active_renderer_id}
              >
                <div class="renderer-info">
                  <Circle
                    size={8}
                    class={renderer.renderer_id === sessionSnapshot.active_renderer_id
                      ? 'renderer-dot active'
                      : 'renderer-dot'}
                  />
                  <span class="renderer-name">{rendererDisplayName(renderer)}</span>
                </div>
                {#if renderer.renderer_id !== sessionSnapshot.active_renderer_id}
                  <button
                    class="mini-btn"
                    onclick={() => sendSetActiveRenderer(renderer.renderer_id)}
                    disabled={controllerBusy}
                  >
                    {$t('qconnect.setActiveRenderer')}
                  </button>
                {/if}
              </div>
            {/each}
          </div>
        {:else}
          <p class="no-renderers">{$t('qconnect.noRenderers')}</p>
        {/if}
      </section>

      <section class="controller-card">
        <h4>{$t('qconnect.controllerTitle')}</h4>

        <div class="controller-transport">
          <button
            class="ctrl-btn"
            onclick={sendControllerPlay}
            disabled={controllerBusy}
            title={$t('qconnect.play')}
          >
            <Play size={16} />
          </button>
          <button
            class="ctrl-btn"
            onclick={sendControllerPause}
            disabled={controllerBusy}
            title={$t('qconnect.pause')}
          >
            <Pause size={16} />
          </button>
          <button
            class="ctrl-btn"
            onclick={sendControllerStop}
            disabled={controllerBusy}
            title={$t('qconnect.stop')}
          >
            <Square size={14} />
          </button>
        </div>

        <div class="controller-volume">
          <button
            class="ctrl-btn small"
            onclick={() => sendControllerMute(!(rendererSnapshot?.muted ?? false))}
            title={rendererSnapshot?.muted ? $t('qconnect.unmuteLabel') : $t('qconnect.muteLabel')}
          >
            {#if rendererSnapshot?.muted}
              <VolumeX size={14} />
            {:else}
              <Volume2 size={14} />
            {/if}
          </button>
          <input
            type="range"
            class="volume-slider"
            min="0"
            max="100"
            value={rendererSnapshot?.volume ?? 50}
            onchange={sendControllerVolume}
          />
          <span class="volume-value">{rendererSnapshot?.volume ?? '—'}</span>
        </div>
      </section>
    {/if}

    <section class="runtime-grid">
      <div class="runtime-card">
        <h4>{$t('qconnect.queueStateTitle')}</h4>
        <div class="runtime-line">
          <span>{$t('qconnect.queueVersionLabel')}</span>
          <strong>
            {#if queueSnapshot?.version}
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
        <div class="runtime-preview">
          <span class="preview-label">{$t('qconnect.queuePreviewTitle')}</span>
          {#if queueSnapshot && queueSnapshot.queue_items.length > 0}
            <div class="preview-list">
              {#each queuePreview(queueSnapshot) as queueItem, index (queueItem.queue_item_id)}
                <div class="preview-row">
                  <span class="preview-index">{index + 1}.</span>
                  <span class="preview-track">{formatTrackRef(queueItem)}</span>
                </div>
              {/each}
            </div>
          {:else}
            <strong>{$t('qconnect.notAvailable')}</strong>
          {/if}
        </div>
      </div>

      <div class="runtime-card">
        <h4>{$t('qconnect.rendererStateTitle')}</h4>
        <div class="runtime-line">
          <span>{$t('qconnect.playingStateLabel')}</span>
          <strong>{rendererSnapshot ? playingStateLabel(rendererSnapshot.playing_state) : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.volumeLabel')}</span>
          <strong>{rendererSnapshot ? safeValue(rendererSnapshot.volume) : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.mutedLabel')}</span>
          <strong>{rendererSnapshot ? safeValue(rendererSnapshot.muted) : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.currentTrackLabel')}</span>
          <strong>{rendererSnapshot ? formatTrackRef(rendererSnapshot.current_track) : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.nextTrackLabel')}</span>
          <strong>{rendererSnapshot ? formatTrackRef(rendererSnapshot.next_track) : $t('qconnect.notAvailable')}</strong>
        </div>
        <div class="runtime-line">
          <span>{$t('qconnect.positionMsLabel')}</span>
          <strong>{rendererSnapshot ? safeValue(rendererSnapshot.current_position_ms) : $t('qconnect.notAvailable')}</strong>
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
  .diagnostics-card,
  .session-card,
  .controller-card {
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

  /* Session card */
  .session-card,
  .controller-card {
    padding: 12px;
  }

  .session-card h4,
  .controller-card h4 {
    margin: 0 0 10px 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-primary);
  }

  .session-card h4::before,
  .controller-card h4::before {
    content: '';
    width: 8px;
    height: 8px;
    border-radius: 999px;
    background: var(--accent-primary, #6366f1);
    opacity: 0.8;
  }

  .renderer-list {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .renderer-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 8px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .renderer-item.active {
    background: rgba(99, 102, 241, 0.1);
    color: var(--text-primary);
  }

  .renderer-info {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .renderer-name {
    font-weight: 500;
  }

  :global(.renderer-dot) {
    color: var(--text-muted);
    fill: currentColor;
  }

  :global(.renderer-dot.active) {
    color: #22c55e;
    fill: currentColor;
  }

  .no-renderers {
    margin: 6px 0 0 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  /* Controller card */
  .controller-transport {
    display: flex;
    gap: 6px;
    margin-bottom: 10px;
  }

  .ctrl-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    padding: 8px 12px;
    cursor: pointer;
    transition: background 0.15s;
  }

  .ctrl-btn:hover:not(:disabled) {
    background: var(--bg-hover, rgba(255, 255, 255, 0.08));
  }

  .ctrl-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .ctrl-btn.small {
    padding: 6px 8px;
  }

  .controller-volume {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .volume-slider {
    flex: 1;
    height: 4px;
    accent-color: var(--accent-primary, #6366f1);
    cursor: pointer;
  }

  .volume-value {
    min-width: 28px;
    text-align: right;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    font-family: var(--font-mono, monospace);
  }

  /* Runtime grid */
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

  .runtime-preview {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .preview-label {
    font-size: 12px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .preview-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .preview-row {
    display: flex;
    gap: 6px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .preview-index {
    min-width: 18px;
    color: var(--text-muted);
  }

  .preview-track {
    word-break: break-word;
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
