<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import { X, Cast, Loader2, Monitor, Wifi, Tv, Speaker, Power } from 'lucide-svelte';
  import {
    subscribe as subscribeCast,
    getCastState,
    connectToDevice,
    disconnect,
    type CastProtocol,
    type CastDevice
  } from '$lib/stores/castStore';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  let activeProtocol = $state<CastProtocol>('chromecast');
  let chromecastDevices = $state<CastDevice[]>([]);
  let dlnaDevices = $state<CastDevice[]>([]);
  let airplayDevices = $state<CastDevice[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let discoveryStarted = $state(false);
  let connecting = $state(false);

  // Cast state from store
  let castState = $state(getCastState());

  const devices = $derived(() => {
    switch (activeProtocol) {
      case 'chromecast': return chromecastDevices;
      case 'dlna': return dlnaDevices;
      case 'airplay': return airplayDevices;
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

    try {
      // Start discovery protocols in parallel
      // Note: AirPlay discovery disabled until RAOP streaming is implemented
      await Promise.allSettled([
        invoke('cast_start_discovery'),
        invoke('dlna_start_discovery')
        // invoke('airplay_start_discovery')  // Disabled - see docs/AIRPLAY_IMPLEMENTATION_STATUS.md
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
        invoke('dlna_stop_discovery')
        // invoke('airplay_stop_discovery')  // Disabled
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
      const [chromecast, dlna] = await Promise.allSettled([
        invoke<CastDevice[]>('cast_get_devices'),
        invoke<CastDevice[]>('dlna_get_devices')
        // invoke<CastDevice[]>('airplay_get_devices')  // Disabled
      ]);

      if (chromecast.status === 'fulfilled') {
        chromecastDevices = chromecast.value;
      }
      if (dlna.status === 'fulfilled') {
        dlnaDevices = dlna.value;
      }
      // AirPlay disabled
      // if (airplay.status === 'fulfilled') {
      //   airplayDevices = airplay.value;
      // }
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
      await connectToDevice(device, activeProtocol);
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      connecting = false;
    }
  }

  async function handleDisconnect() {
    try {
      await disconnect();
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

  function getProtocolLabel(protocol: CastProtocol): string {
    switch (protocol) {
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
        <h3>Cast to Device</h3>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      <!-- Connected Device Banner -->
      {#if castState.isConnected && castState.device}
        <div class="connected-banner">
          <div class="connected-info">
            <svelte:component this={getProtocolIcon(castState.protocol!)} size={20} />
            <div class="connected-text">
              <span class="connected-label">Connected to</span>
              <span class="connected-name">{castState.device.name}</span>
            </div>
          </div>
          <button class="disconnect-btn" onclick={handleDisconnect}>
            <Power size={16} />
            <span>Disconnect</span>
          </button>
        </div>
      {:else}
        <!-- Protocol Tabs (only show when not connected) -->
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
          <!-- AirPlay hidden until RAOP streaming is implemented -->
          <!-- See docs/AIRPLAY_IMPLEMENTATION_STATUS.md for details -->
        </div>

        <div class="content">
          {#if connecting}
            <div class="loading">
              <Loader2 size={32} class="spin" />
              <p>Connecting...</p>
            </div>
          {:else if loading && devices().length === 0}
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
              <p>No {getProtocolLabel(activeProtocol)} devices found</p>
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
    transition: all 150ms ease;
  }

  .disconnect-btn:hover {
    background: rgba(239, 68, 68, 0.2);
    border-color: rgba(239, 68, 68, 0.5);
  }
</style>
