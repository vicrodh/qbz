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

  interface PipewireSink {
    name: string;
    description: string;
    volume: number | null;
    is_default: boolean;
  }

  // Props
  interface Props {
    showTooltips?: boolean;
  }

  let { showTooltips = true }: Props = $props();

  // State
  let settings = $state<AudioSettings | null>(null);
  let outputStatus = $state<AudioOutputStatus | null>(null);
  let pipewireSinks = $state<PipewireSink[]>([]);
  let isHovering = $state(false);

  // Derived state
  const currentDevice = $derived(outputStatus?.device_name ?? null);

  // Get PipeWire description for current device
  const pipewireDescription = $derived.by(() => {
    if (!currentDevice) return null;
    const sink = pipewireSinks.find(s => s.name === currentDevice);
    return sink?.description ?? null;
  });

  // Get volume for current device from PipeWire
  const currentVolume = $derived.by(() => {
    if (!currentDevice) return null;
    const sink = pipewireSinks.find(s => s.name === currentDevice);
    return sink?.volume ?? null;
  });

  // Use PipeWire description if available, otherwise fall back to heuristic
  const prettyDeviceName = $derived(
    pipewireDescription ?? (currentDevice ? getDevicePrettyName(currentDevice) : 'No device')
  );

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
      const [settingsResult, statusResult, sinksResult] = await Promise.all([
        invoke<AudioSettings>('get_audio_settings'),
        invoke<AudioOutputStatus>('get_audio_output_status'),
        invoke<PipewireSink[]>('get_pipewire_sinks').catch(() => [] as PipewireSink[])
      ]);
      settings = settingsResult;
      outputStatus = statusResult;
      pipewireSinks = sinksResult;
    } catch (err) {
      console.error('Failed to load audio status:', err);
    }
  }

  onMount(() => {
    loadStatus();
    // No polling - data is loaded once on mount and refreshed on hover
  });
</script>

<div
  class="audio-badges"
  onmouseenter={() => { isHovering = true; loadStatus(); }}
  onmouseleave={() => isHovering = false}
>
  <!-- DAC Badge -->
  <div
    class="badge"
    class:active={dacPassthroughActive}
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
      {#if currentVolume !== null}
        <div class="tooltip-volume">
          <span class="volume-label">Volume:</span>
          <span class="volume-value">{currentVolume}%</span>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .audio-badges {
    display: flex;
    align-items: stretch;
    gap: 3px;
    position: relative;
    width: 100%;
    height: 100%;
  }

  .badge {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    border-radius: 3px;
    font-size: 7px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.3px;
    background: transparent;
    color: rgba(255, 255, 255, 0.15);
    border: 1px solid rgba(255, 255, 255, 0.06);
    transition: all 200ms ease;
  }

  .badge.active {
    background: rgba(34, 197, 94, 0.2);
    color: #22c55e;
    border-color: rgba(34, 197, 94, 0.4);
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

  .tooltip-volume {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid rgba(255, 255, 255, 0.1);
  }

  .volume-label {
    font-size: 10px;
    color: rgba(255, 255, 255, 0.4);
  }

  .volume-value {
    font-size: 11px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.8);
  }
</style>
