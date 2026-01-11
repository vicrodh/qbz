<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { getDevicePrettyName, isExternalDevice } from '$lib/utils/audioDeviceNames';

  interface AudioSettings {
    output_device: string | null;
    exclusive_mode: boolean;
    dac_passthrough: boolean;
    preferred_sample_rate: number | null;
  }

  interface AudioOutputStatus {
    device_name: string | null;
    is_playing: boolean;
  }

  // Props
  interface Props {
    showTooltips?: boolean;
  }

  let { showTooltips = true }: Props = $props();

  // State
  let settings = $state<AudioSettings | null>(null);
  let outputStatus = $state<AudioOutputStatus | null>(null);
  let isHovering = $state(false);

  // Derived state
  const currentDevice = $derived(outputStatus?.device_name ?? null);
  const prettyDeviceName = $derived(currentDevice ? getDevicePrettyName(currentDevice) : 'No device');
  const isExternal = $derived(currentDevice ? isExternalDevice(currentDevice) : false);

  // Badge states - based on settings AND actual device capability
  const dacPassthroughActive = $derived(
    settings?.dac_passthrough === true && isExternal
  );
  const exclusiveModeActive = $derived(
    settings?.exclusive_mode === true
  );

  // Whether to show badges at all (only if at least one setting is enabled or device is external)
  const shouldShowBadges = $derived(
    settings?.dac_passthrough || settings?.exclusive_mode || isExternal
  );

  async function loadStatus() {
    try {
      const [settingsResult, statusResult] = await Promise.all([
        invoke<AudioSettings>('get_audio_settings'),
        invoke<AudioOutputStatus>('get_audio_output_status')
      ]);
      settings = settingsResult;
      outputStatus = statusResult;
    } catch (err) {
      console.error('Failed to load audio status:', err);
    }
  }

  onMount(() => {
    loadStatus();

    // Poll for updates while playing
    const interval = setInterval(loadStatus, 5000);
    return () => clearInterval(interval);
  });
</script>

{#if shouldShowBadges}
  <div
    class="audio-badges"
    onmouseenter={() => isHovering = true}
    onmouseleave={() => isHovering = false}
  >
    <!-- DAC Badge -->
    <div
      class="badge"
      class:active={dacPassthroughActive}
      class:external={isExternal}
      title={showTooltips ? (dacPassthroughActive ? 'DAC Passthrough active' : 'DAC Passthrough inactive') : undefined}
    >
      <span class="badge-label">DAC</span>
    </div>

    <!-- Exclusive Mode Badge -->
    <div
      class="badge"
      class:active={exclusiveModeActive}
      title={showTooltips ? (exclusiveModeActive ? 'Exclusive Mode active' : 'Exclusive Mode inactive') : undefined}
    >
      <span class="badge-label">EXC</span>
    </div>

    <!-- Device Tooltip on hover -->
    {#if isHovering && showTooltips}
      <div class="device-tooltip">
        <div class="tooltip-label">Output Device</div>
        <div class="tooltip-device">{prettyDeviceName}</div>
        {#if currentDevice && currentDevice !== prettyDeviceName}
          <div class="tooltip-raw">{currentDevice}</div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .audio-badges {
    display: flex;
    align-items: center;
    gap: 4px;
    position: relative;
    margin-left: 8px;
  }

  .badge {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 2px 6px;
    border-radius: 3px;
    font-size: 8px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    background: rgba(255, 255, 255, 0.05);
    color: rgba(255, 255, 255, 0.25);
    border: 1px solid rgba(255, 255, 255, 0.08);
    transition: all 200ms ease;
  }

  .badge.active {
    background: rgba(34, 197, 94, 0.15);
    color: #22c55e;
    border-color: rgba(34, 197, 94, 0.3);
  }

  .badge.external:not(.active) {
    background: rgba(99, 102, 241, 0.1);
    color: rgba(99, 102, 241, 0.5);
    border-color: rgba(99, 102, 241, 0.2);
  }

  .badge-label {
    line-height: 1;
  }

  .device-tooltip {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    background: rgba(24, 24, 28, 0.98);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 6px;
    padding: 8px 12px;
    min-width: 180px;
    max-width: 300px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    z-index: 200;
    animation: tooltip-appear 150ms ease;
  }

  @keyframes tooltip-appear {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .tooltip-label {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: rgba(255, 255, 255, 0.4);
    margin-bottom: 4px;
  }

  .tooltip-device {
    font-size: 12px;
    font-weight: 500;
    color: white;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tooltip-raw {
    font-size: 9px;
    color: rgba(255, 255, 255, 0.3);
    font-family: var(--font-mono, monospace);
    margin-top: 4px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
