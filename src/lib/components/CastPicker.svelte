<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { t } from 'svelte-i18n';
  import { onMount, onDestroy } from 'svelte';
  import { X, Cast, LoaderCircle, Monitor, Wifi, Tv, Speaker, Power } from 'lucide-svelte';
  import {
    subscribe as subscribeCast,
    getCastState,
    connectToDevice,
    disconnect,
    type CastProtocol,
    type CastDevice
  } from '$lib/stores/castStore';
  import {
    connectToRemote,
    disconnectFromRemote,
    isRemoteMode,
    playbackTarget,
  } from '$lib/stores/playbackTargetStore';
  import { connectRemoteEvents, disconnectRemoteEvents } from '$lib/services/remoteEvents';
  import { pingRemote } from '$lib/services/remoteApi';

  interface QbzdDevice {
    name: string;
    host: string;
    port: number;
    baseUrl: string;
  }

  const QBZD_STORAGE_KEY = 'qbzd-saved-daemons';

  interface SavedDaemon {
    name: string;
    baseUrl: string;
    lastConnected: number;
  }

  function loadSavedDaemons(): SavedDaemon[] {
    try {
      const raw = localStorage.getItem(QBZD_STORAGE_KEY);
      return raw ? JSON.parse(raw) : [];
    } catch { return []; }
  }

  function saveDaemon(name: string, baseUrl: string) {
    const saved = loadSavedDaemons().filter(d => d.baseUrl !== baseUrl);
    saved.unshift({ name, baseUrl, lastConnected: Date.now() });
    // Keep max 5 recent
    localStorage.setItem(QBZD_STORAGE_KEY, JSON.stringify(saved.slice(0, 5)));
  }

  function removeSavedDaemon(baseUrl: string) {
    const saved = loadSavedDaemons().filter(d => d.baseUrl !== baseUrl);
    localStorage.setItem(QBZD_STORAGE_KEY, JSON.stringify(saved));
    savedDaemons = loadSavedDaemons();
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  // Feature flag: qbzd (QBZ Daemon) is not ready for release yet. Keep the
  // code paths but hide the tab and skip the backend invocations so users
  // don't see a broken option. Flip to `true` when qbzd ships.
  const QBZD_ENABLED = false;

  let activeProtocol = $state<CastProtocol | 'qbzd'>(QBZD_ENABLED ? 'qbzd' : 'chromecast');
  let manualAddress = $state('');
  let savedDaemons = $state<SavedDaemon[]>(loadSavedDaemons());
  let chromecastDevices = $state<CastDevice[]>([]);
  let dlnaDevices = $state<CastDevice[]>([]);
  let airplayDevices = $state<CastDevice[]>([]);
  let qbzdDevices = $state<QbzdDevice[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let discoveryStarted = $state(false);
  let connecting = $state(false);
  // Multicast discovery (DLNA / Chromecast) commonly needs 5-15s before
  // devices announce themselves. `loading` only covers the first poll, so we
  // keep a separate `scanning` flag that stays true long enough to show a
  // spinner instead of a premature "No devices found" message.
  let scanning = $state(false);
  let scanTimeout: ReturnType<typeof setTimeout> | null = null;
  const SCAN_DURATION_MS = 15000;

  // Cast state from store
  let castState = $state(getCastState());

  const devices = $derived(() => {
    switch (activeProtocol) {
      case 'chromecast': return chromecastDevices;
      case 'dlna': return dlnaDevices;
      case 'airplay': return airplayDevices;
      case 'qbzd': return [];  // qbzd uses its own list
    }
  });

  let unsubscribeCast: (() => void) | null = null;

  onMount(() => {
    unsubscribeCast = subscribeCast(() => {
      castState = getCastState();
    });

    if (isOpen) {
      startDiscovery();
    }
  });

  onDestroy(() => {
    unsubscribeCast?.();
    if (discoveryStarted) {
      stopDiscovery();
    }
  });

  $effect(() => {
    if (isOpen && !discoveryStarted) {
      startDiscovery();
    } else if (!isOpen && discoveryStarted) {
      stopDiscovery();
    }
  });

  async function startDiscovery() {
    loading = true;
    error = null;
    discoveryStarted = true;
    scanning = true;
    if (scanTimeout) clearTimeout(scanTimeout);
    scanTimeout = setTimeout(() => { scanning = false; }, SCAN_DURATION_MS);

    try {
      // Start discovery protocols in parallel
      // Note: AirPlay discovery disabled until RAOP streaming is implemented
      await Promise.allSettled([
        invoke('v2_cast_start_discovery'),
        invoke('v2_dlna_start_discovery'),
        ...(QBZD_ENABLED ? [invoke('v2_qbzd_start_discovery')] : []),
      ]);
      // Poll for devices
      pollDevices();
    } catch (err) {
      error = String(err);
      loading = false;
      scanning = false;
      if (scanTimeout) { clearTimeout(scanTimeout); scanTimeout = null; }
    }
  }

  async function stopDiscovery() {
    discoveryStarted = false;
    scanning = false;
    if (scanTimeout) { clearTimeout(scanTimeout); scanTimeout = null; }
    try {
      await Promise.allSettled([
        invoke('v2_cast_stop_discovery'),
        invoke('v2_dlna_stop_discovery'),
        ...(QBZD_ENABLED ? [invoke('v2_qbzd_stop_discovery')] : []),
      ]);
    } catch (err) {
      console.error('Failed to stop discovery:', err);
    }
  }

  async function pollDevices() {
    if (!discoveryStarted) return;

    try {
      // Poll active protocols in parallel
      // Note: AirPlay polling disabled until RAOP streaming is implemented
      const results = await Promise.allSettled([
        invoke<CastDevice[]>('v2_cast_get_devices'),
        invoke<CastDevice[]>('v2_dlna_get_devices'),
        ...(QBZD_ENABLED ? [invoke<QbzdDevice[]>('v2_qbzd_get_devices')] : []),
      ]);
      const [chromecast, dlna, qbzd] = results;

      if (chromecast.status === 'fulfilled') {
        chromecastDevices = chromecast.value as CastDevice[];
      }
      if (dlna.status === 'fulfilled') {
        dlnaDevices = dlna.value as CastDevice[];
      }
      if (QBZD_ENABLED && qbzd && qbzd.status === 'fulfilled') {
        qbzdDevices = qbzd.value as QbzdDevice[];
      }
    } catch (err) {
      console.error('Failed to get devices:', err);
    }

    loading = false;

    // Continue polling while open
    if (discoveryStarted) {
      setTimeout(pollDevices, 2000);
    }
  }

  async function handleConnect(device: CastDevice) {
    connecting = true;
    error = null;
    try {
      await connectToDevice(device, activeProtocol as CastProtocol);
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      connecting = false;
    }
  }

  async function handleDisconnect() {
    try {
      if ($isRemoteMode) {
        disconnectRemoteEvents();
        disconnectFromRemote();
      } else {
        await disconnect();
      }
    } catch (err) {
      error = String(err);
    }
  }

  async function handleQbzdConnect(device: QbzdDevice) {
    connecting = true;
    error = null;
    try {
      const reachable = await pingRemote(device.baseUrl, '');
      if (!reachable) {
        error = `Cannot reach ${device.name} at ${device.baseUrl}`;
        return;
      }
      connectToRemote(device.baseUrl, '', device.name);
      connectRemoteEvents();
      saveDaemon(device.name, device.baseUrl);
      savedDaemons = loadSavedDaemons();
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      connecting = false;
    }
  }

  async function handleManualConnect() {
    let addr = manualAddress.trim();
    if (!addr) return;
    // Add http:// if missing
    if (!addr.startsWith('http://') && !addr.startsWith('https://')) {
      addr = `http://${addr}`;
    }
    // Add default port if missing
    try {
      const url = new URL(addr);
      if (!url.port) {
        addr = `${url.protocol}//${url.hostname}:8182`;
      }
    } catch {
      error = 'Invalid address';
      return;
    }

    connecting = true;
    error = null;
    try {
      const reachable = await pingRemote(addr, '');
      if (!reachable) {
        error = `Cannot reach daemon at ${addr}`;
        return;
      }
      // Extract name from hostname
      const name = new URL(addr).hostname;
      connectToRemote(addr, '', name);
      connectRemoteEvents();
      saveDaemon(name, addr);
      savedDaemons = loadSavedDaemons();
      manualAddress = '';
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      connecting = false;
    }
  }

  async function handleSavedConnect(saved: SavedDaemon) {
    connecting = true;
    error = null;
    try {
      const reachable = await pingRemote(saved.baseUrl, '');
      if (!reachable) {
        error = `Cannot reach ${saved.name} at ${saved.baseUrl}`;
        return;
      }
      connectToRemote(saved.baseUrl, '', saved.name);
      connectRemoteEvents();
      saveDaemon(saved.name, saved.baseUrl);
      savedDaemons = loadSavedDaemons();
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      connecting = false;
    }
  }

  function getProtocolIcon(protocol: CastProtocol | 'qbzd') {
    switch (protocol) {
      case 'qbzd': return Monitor;
      case 'chromecast': return Cast;
      case 'dlna': return Tv;
      case 'airplay': return Speaker;
    }
  }

  function getProtocolLabel(protocol: CastProtocol | 'qbzd'): string {
    switch (protocol) {
      case 'qbzd': return 'QBZ Daemon';
      case 'chromecast': return 'Chromecast';
      case 'dlna': return 'DLNA';
      case 'airplay': return 'AirPlay';
    }
  }
</script>

{#if isOpen}
  <div class="overlay" onclick={onClose} onkeydown={(e) => e.key === 'Escape' && onClose()} role="presentation">
    <div class="picker" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()} role="dialog" tabindex="-1">
      <div class="header">
        <h3>{$t('player.castToDevice')}</h3>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      <!-- Connected Device Banner -->
      {#if $isRemoteMode}
        <div class="connected-banner">
          <div class="connected-info">
            <Monitor size={20} />
            <div class="connected-text">
              <span class="connected-label">{$t('player.connectedTo')}</span>
              <span class="connected-name">{$playbackTarget.name}</span>
            </div>
          </div>
          <button class="disconnect-btn" onclick={handleDisconnect}>
            <Power size={16} />
            <span>{$t('settings.integrations.disconnect')}</span>
          </button>
        </div>
      {:else if castState.isConnected && castState.device}
        {@const ProtocolIcon = getProtocolIcon(castState.protocol!)}
        <div class="connected-banner">
          <div class="connected-info">
            <ProtocolIcon size={20} />
            <div class="connected-text">
              <span class="connected-label">{$t('player.connectedTo')}</span>
              <span class="connected-name">{castState.device.name}</span>
            </div>
          </div>
          <button class="disconnect-btn" onclick={handleDisconnect}>
            <Power size={16} />
            <span>{ $t('settings.integrations.disconnect') }</span>
          </button>
        </div>
      {:else}
        <!-- Protocol Tabs (only show when not connected) -->
        <div class="protocol-tabs">
          {#if QBZD_ENABLED}
            <button
              class="protocol-tab"
              class:active={activeProtocol === 'qbzd'}
              onclick={() => activeProtocol = 'qbzd'}
            >
              <Monitor size={16} />
              <span>QBZ Daemon</span>
              {#if qbzdDevices.length > 0}
                <span class="count">{qbzdDevices.length}</span>
              {/if}
            </button>
          {/if}
          <button
            class="protocol-tab"
            class:active={activeProtocol === 'chromecast'}
            onclick={() => activeProtocol = 'chromecast'}
          >
            <Cast size={16} />
            <span>Chromecast</span>
            {#if chromecastDevices.length > 0}
              <span class="count">{chromecastDevices.length}</span>
            {/if}
          </button>
          <button
            class="protocol-tab"
            class:active={activeProtocol === 'dlna'}
            onclick={() => activeProtocol = 'dlna'}
          >
            <Tv size={16} />
            <span>DLNA</span>
            {#if dlnaDevices.length > 0}
              <span class="count">{dlnaDevices.length}</span>
            {/if}
          </button>
          <!-- AirPlay hidden until RAOP streaming is implemented -->
          <!-- See docs/AIRPLAY_IMPLEMENTATION_STATUS.md for details -->
        </div>

        <div class="content">
          {#if connecting}
            <div class="loading">
              <LoaderCircle size={32} class="spin" />
              <p>{$t('integrations.connecting')}</p>
            </div>
          {:else if error}
            <div class="error">
              <p>{error}</p>
            </div>
          {:else if activeProtocol === 'qbzd'}
            <!-- QBZ Daemon: manual connect + saved + discovered -->
            <div class="qbzd-section">
              <!-- Manual connect -->
              <div class="manual-connect">
                <input
                  type="text"
                  class="manual-input"
                  bind:value={manualAddress}
                  placeholder="192.168.1.50:8182"
                  onkeydown={(e) => e.key === 'Enter' && handleManualConnect()}
                />
                <button class="connect-btn" onclick={handleManualConnect} disabled={!manualAddress.trim()}>
                  {$t('integrations.connect')}
                </button>
              </div>

              <!-- Saved (recent) daemons -->
              {#if savedDaemons.length > 0}
                <div class="section-label">{$t('player.recentDaemons')}</div>
                <div class="devices">
                  {#each savedDaemons as saved}
                    <div class="device-row">
                      <button class="device" onclick={() => handleSavedConnect(saved)}>
                        <Monitor size={24} />
                        <div class="device-info">
                          <span class="device-name">{saved.name}</span>
                          <span class="device-ip">{saved.baseUrl}</span>
                        </div>
                        <Wifi size={20} class="cast-icon" />
                      </button>
                      <button class="remove-saved" onclick={() => removeSavedDaemon(saved.baseUrl)} title="Remove">
                        <X size={14} />
                      </button>
                    </div>
                  {/each}
                </div>
              {/if}

              <!-- Discovered via mDNS -->
              {#if qbzdDevices.length > 0}
                <div class="section-label">{$t('player.discoveredDaemons')}</div>
                <div class="devices">
                  {#each qbzdDevices as device}
                    <button class="device" onclick={() => handleQbzdConnect(device)}>
                      <Monitor size={24} />
                      <div class="device-info">
                        <span class="device-name">{device.name}</span>
                        <span class="device-ip">{device.host}:{device.port}</span>
                      </div>
                      <Wifi size={20} class="cast-icon" />
                    </button>
                  {/each}
                </div>
              {:else if loading}
                <div class="section-label">{$t('player.discoveredDaemons')}</div>
                <div class="scanning-hint">
                  <LoaderCircle size={14} class="spin" />
                  <span>{$t('toast.loadingDevices')}</span>
                </div>
              {/if}
            </div>
          {:else if devices().length === 0 && (loading || scanning)}
            <div class="loading">
              <LoaderCircle size={32} class="spin" />
              <p>{$t('toast.loadingDevices')}</p>
            </div>
          {:else if devices().length === 0}
            <div class="empty">
              <Wifi size={32} />
              <p>{$t('settings.integrations.noProtocolDeviceFound', { "values": { "protocol": getProtocolLabel(activeProtocol) } })}</p>
              <p class="hint">{$t('settings.integrations.noProtocolDeviceFoundHint')}</p>
            </div>
          {:else}
            <div class="devices">
              {#each devices() as device}
                {@const CastIcon = getProtocolIcon(activeProtocol)}
                <button class="device" onclick={() => handleConnect(device)}>
                  <Monitor size={24} />
                  <div class="device-info">
                    <span class="device-name">{device.name}</span>
                    <span class="device-ip">{device.ip}</span>
                  </div>
                  <CastIcon size={20} class="cast-icon" />
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 200;
    background-color: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .picker {
    width: 400px;
    max-height: 500px;
    background-color: var(--bg-secondary);
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .header h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
  }

  .close-btn:hover {
    color: var(--text-primary);
    background-color: var(--alpha-10);
  }

  .protocol-tabs {
    display: flex;
    padding: 8px;
    gap: 4px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .protocol-tab {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 10px 12px;
    background: none;
    border: none;
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
  }

  .protocol-tab:hover {
    background-color: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .protocol-tab.active {
    background-color: var(--accent-primary);
    color: white;
  }

  .protocol-tab .count {
    background-color: var(--alpha-20);
    padding: 2px 6px;
    border-radius: 10px;
    font-size: 11px;
  }

  .content {
    padding: 16px;
    max-height: 350px;
    overflow-y: auto;
  }

  .loading, .empty, .error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 32px;
    gap: 12px;
    color: var(--text-muted);
  }

  .loading :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .hint {
    text-align: center;
    font-size: 12px;
    color: #666666;
  }

  .error {
    color: #ff6b6b;
  }

  .devices {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .device {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: none;
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
    text-align: left;
    width: 100%;
    color: var(--text-primary);
  }

  .device:hover {
    background-color: var(--alpha-5);
    border-color: var(--accent-primary);
  }

  .device-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .device-name {
    font-size: 14px;
    font-weight: 500;
  }

  .device-ip {
    font-size: 12px;
    color: var(--text-muted);
  }

  .qbzd-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .manual-connect {
    display: flex;
    gap: 8px;
    padding: 0 16px;
  }

  .manual-input {
    flex: 1;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: monospace;
  }

  .manual-input::placeholder {
    color: var(--text-muted);
  }

  .manual-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .connect-btn {
    padding: 8px 16px;
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
  }

  .connect-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .connect-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .section-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 0 16px;
  }

  .device-row {
    display: flex;
    align-items: center;
  }

  .device-row .device {
    flex: 1;
  }

  .remove-saved {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 50%;
    margin-right: 8px;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .device-row:hover .remove-saved {
    opacity: 1;
  }

  .remove-saved:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .scanning-hint {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .device :global(.cast-icon) {
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .device:hover :global(.cast-icon) {
    opacity: 1;
    color: var(--accent-primary);
  }

  /* Connected Banner */
  .connected-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    background: linear-gradient(135deg, rgba(34, 197, 94, 0.15) 0%, rgba(34, 197, 94, 0.05) 100%);
    border-bottom: 1px solid rgba(34, 197, 94, 0.2);
  }

  .connected-info {
    display: flex;
    align-items: center;
    gap: 12px;
    color: #22c55e;
  }

  .connected-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .connected-label {
    font-size: 11px;
    color: rgba(34, 197, 94, 0.8);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .connected-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .disconnect-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 6px;
    color: #ef4444;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease, border-color 150ms ease, opacity 150ms ease;
  }

  .disconnect-btn:hover {
    background: rgba(239, 68, 68, 0.2);
    border-color: rgba(239, 68, 68, 0.5);
  }
</style>
