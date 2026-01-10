<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import { X, Cast, Loader2, Monitor, Wifi, Tv, Speaker } from 'lucide-svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onConnect: (deviceId: string, protocol: CastProtocol) => void;
  }

  type CastProtocol = 'chromecast' | 'dlna' | 'airplay';

  interface CastDevice {
    id: string;
    name: string;
    ip: string;
    port: number;
  }

  let { isOpen, onClose, onConnect }: Props = $props();

  let activeProtocol = $state<CastProtocol>('chromecast');
  let chromecastDevices = $state<CastDevice[]>([]);
  let dlnaDevices = $state<CastDevice[]>([]);
  let airplayDevices = $state<CastDevice[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let discoveryStarted = $state(false);

  const devices = $derived(() => {
    switch (activeProtocol) {
      case 'chromecast': return chromecastDevices;
      case 'dlna': return dlnaDevices;
      case 'airplay': return airplayDevices;
    }
  });

  onMount(() => {
    if (isOpen) {
      startDiscovery();
    }
  });

  onDestroy(() => {
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

    try {
      // Start all discovery protocols in parallel
      await Promise.allSettled([
        invoke('cast_start_discovery'),
        invoke('dlna_start_discovery'),
        invoke('airplay_start_discovery')
      ]);
      // Poll for devices
      pollDevices();
    } catch (err) {
      error = String(err);
      loading = false;
    }
  }

  async function stopDiscovery() {
    discoveryStarted = false;
    try {
      await Promise.allSettled([
        invoke('cast_stop_discovery'),
        invoke('dlna_stop_discovery'),
        invoke('airplay_stop_discovery')
      ]);
    } catch (err) {
      console.error('Failed to stop discovery:', err);
    }
  }

  async function pollDevices() {
    if (!discoveryStarted) return;

    try {
      // Poll all protocols in parallel
      const [chromecast, dlna, airplay] = await Promise.allSettled([
        invoke<CastDevice[]>('cast_get_devices'),
        invoke<CastDevice[]>('dlna_get_devices'),
        invoke<CastDevice[]>('airplay_get_devices')
      ]);

      if (chromecast.status === 'fulfilled') {
        chromecastDevices = chromecast.value;
      }
      if (dlna.status === 'fulfilled') {
        dlnaDevices = dlna.value;
      }
      if (airplay.status === 'fulfilled') {
        airplayDevices = airplay.value;
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
    try {
      switch (activeProtocol) {
        case 'chromecast':
          await invoke('cast_connect', { deviceId: device.id });
          break;
        case 'dlna':
          await invoke('dlna_connect', { deviceId: device.id });
          break;
        case 'airplay':
          await invoke('airplay_connect', { deviceId: device.id });
          break;
      }
      onConnect(device.id, activeProtocol);
      onClose();
    } catch (err) {
      error = String(err);
    }
  }

  function getProtocolIcon(protocol: CastProtocol) {
    switch (protocol) {
      case 'chromecast': return Cast;
      case 'dlna': return Tv;
      case 'airplay': return Speaker;
    }
  }

  function getDeviceCount(protocol: CastProtocol): number {
    switch (protocol) {
      case 'chromecast': return chromecastDevices.length;
      case 'dlna': return dlnaDevices.length;
      case 'airplay': return airplayDevices.length;
    }
  }
</script>

{#if isOpen}
  <div class="overlay" onclick={onClose} onkeydown={(e) => e.key === 'Escape' && onClose()} role="presentation">
    <div class="picker" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()} role="dialog" tabindex="-1">
      <div class="header">
        <h3>Cast to Device</h3>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      <!-- Protocol Tabs -->
      <div class="protocol-tabs">
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
        <button
          class="protocol-tab"
          class:active={activeProtocol === 'airplay'}
          onclick={() => activeProtocol = 'airplay'}
        >
          <Speaker size={16} />
          <span>AirPlay</span>
          {#if airplayDevices.length > 0}
            <span class="count">{airplayDevices.length}</span>
          {/if}
        </button>
      </div>

      <div class="content">
        {#if loading && devices().length === 0}
          <div class="loading">
            <Loader2 size={32} class="spin" />
            <p>Searching for devices...</p>
          </div>
        {:else if error}
          <div class="error">
            <p>{error}</p>
          </div>
        {:else if devices().length === 0}
          <div class="empty">
            <Wifi size={32} />
            <p>No {activeProtocol === 'chromecast' ? 'Chromecast' : activeProtocol === 'dlna' ? 'DLNA' : 'AirPlay'} devices found</p>
            <p class="hint">Make sure devices are on the same network</p>
          </div>
        {:else}
          <div class="devices">
            {#each devices() as device}
              <button class="device" onclick={() => handleConnect(device)}>
                <Monitor size={24} />
                <div class="device-info">
                  <span class="device-name">{device.name}</span>
                  <span class="device-ip">{device.ip}</span>
                </div>
                <svelte:component this={getProtocolIcon(activeProtocol)} size={20} class="cast-icon" />
              </button>
            {/each}
          </div>
        {/if}
      </div>
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
    background-color: rgba(255, 255, 255, 0.1);
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
    transition: all 150ms ease;
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
    background-color: rgba(255, 255, 255, 0.2);
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
    transition: all 150ms ease;
    text-align: left;
    width: 100%;
    color: var(--text-primary);
  }

  .device:hover {
    background-color: rgba(255, 255, 255, 0.05);
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

  .device :global(.cast-icon) {
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .device:hover :global(.cast-icon) {
    opacity: 1;
    color: var(--accent-primary);
  }
</style>
