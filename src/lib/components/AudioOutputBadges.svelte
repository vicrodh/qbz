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

  interface HardwareAudioStatus {
    hardware_sample_rate: number | null;
    hardware_format: string | null;
    is_active: boolean;
  }

  // Props
  interface Props {
    showTooltips?: boolean;
    samplingRate?: number; // Track sample rate in kHz (e.g., 192)
  }

  let { showTooltips = true, samplingRate }: Props = $props();

  // State
  let settings = $state<AudioSettings | null>(null);
  let outputStatus = $state<AudioOutputStatus | null>(null);
  let pipewireSinks = $state<PipewireSink[]>([]);
  let hardwareStatus = $state<HardwareAudioStatus | null>(null);
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

  // DAC badge state logic - 3 states: green, yellow, gray
  const dacBadgeState = $derived.by(() => {
    if (!settings?.dac_passthrough) {
      return 'off'; // Gray - setting disabled
    }

    // DAC passthrough is ON
    if (!hardwareStatus?.is_active || !hardwareStatus.hardware_sample_rate) {
      return 'active'; // Green - DAC on but no way to verify (trust it works)
    }

    // We have hardware info - check for resampling
    if (samplingRate && hardwareStatus.hardware_sample_rate) {
      const hwRate = hardwareStatus.hardware_sample_rate;
      const trackRate = samplingRate * 1000; // Convert kHz to Hz

      if (Math.abs(hwRate - trackRate) < 100) {
        // Rates match (within 100Hz tolerance)
        return 'active'; // Green - working correctly
      } else {
        // Rates don't match - resampling detected
        return 'warning'; // Yellow - resampling despite DAC passthrough
      }
    }

    return 'active'; // Default to green when DAC is on
  });

  const exclusiveModeActive = $derived(
    settings?.exclusive_mode === true
  );

  // Dynamic tooltip for DAC badge
  const dacTooltip = $derived.by(() => {
    if (!settings?.dac_passthrough) {
      return 'DAC Passthrough desactivado - el sistema puede resamplear';
    }

    if (dacBadgeState === 'warning' && hardwareStatus?.hardware_sample_rate && samplingRate) {
      const hwRate = (hardwareStatus.hardware_sample_rate / 1000).toFixed(1);
      return `⚠ DAC Passthrough activo pero hay resampling\nArchivo: ${samplingRate} kHz → Hardware: ${hwRate} kHz`;
    }

    if (hardwareStatus?.hardware_sample_rate) {
      const hwRate = (hardwareStatus.hardware_sample_rate / 1000).toFixed(1);
      return `DAC Passthrough activo - ${hwRate} kHz bit-perfect`;
    }

    return 'DAC Passthrough activo';
  });

  // Whether to show badges at all (only if at least one setting is enabled or device is external)
  const shouldShowBadges = $derived(
    settings?.dac_passthrough || settings?.exclusive_mode || isExternal
  );

  async function loadStatus() {
    try {
      const [settingsResult, statusResult, sinksResult, hwStatus] = await Promise.all([
        invoke<AudioSettings>('get_audio_settings'),
        invoke<AudioOutputStatus>('get_audio_output_status'),
        invoke<PipewireSink[]>('get_pipewire_sinks').catch(() => [] as PipewireSink[]),
        invoke<HardwareAudioStatus>('get_hardware_audio_status').catch(() => null)
      ]);
      settings = settingsResult;
      outputStatus = statusResult;
      pipewireSinks = sinksResult;
      hardwareStatus = hwStatus;
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
    class="badge dac-badge"
    class:active={dacBadgeState === 'active'}
    class:warning={dacBadgeState === 'warning'}
    class:off={dacBadgeState === 'off'}
  >
    <span class="badge-label">DAC</span>
  </div>

  <!-- Exclusive Mode Badge -->
  <div
    class="badge"
    class:active={exclusiveModeActive}
  >
    <span class="badge-label">EXC</span>
  </div>

  <!-- Unified Tooltip on hover -->
  {#if isHovering && showTooltips}
    <div class="device-tooltip">
      <!-- Output Device -->
      <div class="tooltip-section">
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

      <!-- DAC Status -->
      {#if settings?.dac_passthrough || settings?.exclusive_mode}
        <div class="tooltip-section">
          <div class="tooltip-label">Audio Settings</div>
          {#if settings.dac_passthrough}
            <div class="tooltip-setting" class:warning={dacBadgeState === 'warning'}>
              <span class="setting-icon" class:active={dacBadgeState === 'active'} class:warning={dacBadgeState === 'warning'}>●</span>
              <span class="setting-text">
                {#if dacBadgeState === 'warning'}
                  ⚠ DAC Passthrough: Resampling detected
                  {#if hardwareStatus?.hardware_sample_rate && samplingRate}
                    <br><span class="setting-detail">{samplingRate} kHz → {(hardwareStatus.hardware_sample_rate / 1000).toFixed(1)} kHz</span>
                  {/if}
                {:else if dacBadgeState === 'active'}
                  DAC Passthrough: Bit-perfect
                  {#if hardwareStatus?.hardware_sample_rate}
                    <br><span class="setting-detail">{(hardwareStatus.hardware_sample_rate / 1000).toFixed(1)} kHz</span>
                  {/if}
                {:else}
                  DAC Passthrough: Off
                {/if}
              </span>
            </div>
          {/if}
          {#if settings.exclusive_mode}
            <div class="tooltip-setting">
              <span class="setting-icon" class:active={exclusiveModeActive}>●</span>
              <span class="setting-text">Exclusive Mode</span>
            </div>
          {/if}
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
    cursor: help;
  }

  /* DAC Badge States */
  .badge.active {
    background: rgba(34, 197, 94, 0.2);
    color: #22c55e;
    border-color: rgba(34, 197, 94, 0.4);
  }

  .badge.warning {
    background: rgba(234, 179, 8, 0.2);
    color: #eab308;
    border-color: rgba(234, 179, 8, 0.4);
  }

  .badge.off {
    background: transparent;
    color: rgba(255, 255, 255, 0.15);
    border-color: rgba(255, 255, 255, 0.06);
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
    z-index: 9999;
    animation: tooltip-appear 150ms ease;
    pointer-events: none;
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

  .tooltip-section {
    margin-bottom: 8px;
  }

  .tooltip-section:last-child {
    margin-bottom: 0;
  }

  .tooltip-setting {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin-top: 6px;
    font-size: 11px;
  }

  .setting-icon {
    font-size: 8px;
    color: rgba(255, 255, 255, 0.3);
    line-height: 1.4;
  }

  .setting-icon.active {
    color: #22c55e;
  }

  .setting-icon.warning {
    color: #eab308;
  }

  .setting-text {
    color: rgba(255, 255, 255, 0.7);
    line-height: 1.4;
  }

  .tooltip-setting.warning .setting-text {
    color: #eab308;
  }

  .setting-detail {
    font-size: 10px;
    color: rgba(255, 255, 255, 0.5);
    font-family: var(--font-mono, monospace);
  }
</style>
