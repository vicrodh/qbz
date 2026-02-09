<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { getDevicePrettyName, isExternalDevice } from '$lib/utils/audioDeviceNames';
  import { isCasting, getConnectedDevice, getConnectedProtocol, subscribe as subscribeCast } from '$lib/stores/castStore';

  interface AudioSettings {
    output_device: string | null;
    exclusive_mode: boolean;
    dac_passthrough: boolean;
    preferred_sample_rate: number | null;
    backend_type: 'PipeWire' | 'Alsa' | 'Pulse' | null;
    alsa_plugin: 'Hw' | 'PlugHw' | 'Pcm' | null;
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

  interface AlsaDevice {
    id: string;
    name: string;
    description: string | null;
    is_default: boolean;
    max_sample_rate: number | null;
    supported_sample_rates: number[] | null;
    device_bus: string | null;
    is_hardware: boolean;
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
  let alsaDevices = $state<AlsaDevice[]>([]);
  let hardwareStatus = $state<HardwareAudioStatus | null>(null);
  let isHovering = $state(false);
  
  // Cast state
  let castConnected = $state(false);
  let castDeviceName = $state<string | null>(null);
  let castProtocol = $state<string | null>(null);

  // Ticker animation for long device names
  let deviceNameRef: HTMLDivElement | null = $state(null);
  let deviceNameTextRef: HTMLSpanElement | null = $state(null);
  let deviceNameOverflow = $state(0);
  const deviceNameOffset = $derived(deviceNameOverflow > 0 ? `-${deviceNameOverflow + 16}px` : '0px');
  const tickerSpeed = 40; // pixels per second
  const deviceNameDuration = $derived(deviceNameOverflow > 0 ? `${(deviceNameOverflow + 16) / tickerSpeed}s` : '0s');

  // Derived state
  const currentDevice = $derived(outputStatus?.device_name ?? null);

  // Get PipeWire description for current device
  const pipewireDescription = $derived.by(() => {
    if (!currentDevice) return null;
    const sink = pipewireSinks.find(s => s.name === currentDevice);
    return sink?.description ?? null;
  });

  // Get ALSA description for current device (fancy name from aplay -L)
  const alsaDescription = $derived.by(() => {
    if (!currentDevice) return null;
    const device = alsaDevices.find(d => d.id === currentDevice || d.name === currentDevice);
    return device?.description ?? null;
  });

  // Get volume for current device from PipeWire
  const currentVolume = $derived.by(() => {
    if (!currentDevice) return null;
    const sink = pipewireSinks.find(s => s.name === currentDevice);
    return sink?.volume ?? null;
  });

  // Use PipeWire/ALSA description if available, otherwise fall back to heuristic
  const prettyDeviceName = $derived(
    pipewireDescription ?? alsaDescription ?? (currentDevice ? getDevicePrettyName(currentDevice) : 'No device')
  );

  const isExternal = $derived(currentDevice ? isExternalDevice(currentDevice) : false);

  // ALSA Direct mode detection (bit-perfect hardware access)
  const isAlsaDirect = $derived(
    settings?.backend_type === 'Alsa' && settings?.alsa_plugin === 'Hw'
  );

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

  // Whether to show badges at all (only if at least one setting is enabled or device is external or casting)
  const shouldShowBadges = $derived(
    castConnected || settings?.dac_passthrough || settings?.exclusive_mode || isExternal || isAlsaDirect
  );

  async function loadStatus() {
    try {
      const [settingsResult, statusResult, sinksResult, alsaResult, hwStatus] = await Promise.all([
        invoke<AudioSettings>('get_audio_settings'),
        invoke<AudioOutputStatus>('get_audio_output_status'),
        invoke<PipewireSink[]>('get_pipewire_sinks').catch(() => [] as PipewireSink[]),
        invoke<AlsaDevice[]>('get_alsa_devices').catch(() => [] as AlsaDevice[]),
        invoke<HardwareAudioStatus>('get_hardware_audio_status').catch(() => null)
      ]);
      settings = settingsResult;
      outputStatus = statusResult;
      pipewireSinks = sinksResult;
      alsaDevices = alsaResult;
      hardwareStatus = hwStatus;
    } catch (err) {
      console.error('Failed to load audio status:', err);
    }
  }

  // Calculate overflow for ticker animation when tooltip is visible
  $effect(() => {
    if (isHovering && deviceNameRef && deviceNameTextRef) {
      const overflow = deviceNameTextRef.scrollWidth - deviceNameRef.clientWidth;
      deviceNameOverflow = overflow > 0 ? overflow : 0;
    }
  });

  // Reload status when playback starts
  $effect(() => {
    if (outputStatus?.is_playing) {
      loadStatus();
    }
  });

  onMount(() => {
    loadStatus();
    
    // Subscribe to cast state
    const unsubscribeCast = subscribeCast(() => {
      castConnected = isCasting();
      castDeviceName = getConnectedDevice()?.name ?? null;
      castProtocol = getConnectedProtocol();
    });

    // Lightweight polling: ONLY update hardware status (no device enumeration)
    // This reads /proc/asound which is very cheap, no CPAL/ALSA enumeration
    const pollInterval = setInterval(async () => {
      // Only poll if using bit-perfect modes AND not casting
      if (!castConnected && (settings?.dac_passthrough || settings?.backend_type === 'Alsa')) {
        try {
          hardwareStatus = await invoke<HardwareAudioStatus>('get_hardware_audio_status').catch(() => null);
        } catch (err) {
          // Silently fail - don't spam console
        }
      }
    }, 1000);

    // Cleanup on unmount
    return () => {
      clearInterval(pollInterval);
      unsubscribeCast();
    };
  });
</script>

<div
  class="audio-badges"
  role="group"
  aria-label="Audio output indicators"
  onmouseenter={() => { isHovering = true; loadStatus(); }}
  onmouseleave={() => isHovering = false}
>
  <!-- Casting Badge (shown when casting to external device) -->
  {#if castConnected}
    <div class="badge cast-badge active">
      <span class="badge-label">
        {#if castProtocol === 'dlna'}
          DLNA
        {:else if castProtocol === 'chromecast'}
          CAST
        {:else if castProtocol === 'airplay'}
          AIR
        {:else}
          CAST
        {/if}
      </span>
    </div>
  {:else if isAlsaDirect}
    <!-- Hardware Direct Badge (ALSA Direct mode) -->
    <div class="badge hw-badge active">
      <span class="badge-label">HW</span>
    </div>
  {:else}
    <!-- DAC Passthrough Badge (PipeWire mode) -->
    <div
      class="badge dac-badge"
      class:active={dacBadgeState === 'active'}
      class:warning={dacBadgeState === 'warning'}
      class:off={dacBadgeState === 'off'}
    >
      <span class="badge-label">DAC</span>
    </div>
  {/if}

  <!-- Exclusive Mode Badge (only show when not casting) -->
  {#if !castConnected}
  <div
    class="badge"
    class:active={exclusiveModeActive}
  >
    <span class="badge-label">EXC</span>
  </div>
  {/if}

  <!-- Unified Tooltip on hover -->
  {#if isHovering && showTooltips}
    <div class="device-tooltip">
      <!-- Output Device -->
      <div class="tooltip-section">
        <div class="tooltip-label">{castConnected ? $t('audioBadges.castingTo') : $t('audioBadges.outputDevice')}</div>
        {#if castConnected}
          <div class="tooltip-device cast-device">
            {#if castProtocol === 'dlna'}
              <img src="/dlna.svg" alt="DLNA" class="cast-icon-svg" />
            {:else}
              <img src="/cast_audio.svg" alt="Cast" class="cast-icon-svg" />
            {/if}
            <span class="device-name-text">{castDeviceName || 'Unknown Device'}</span>
          </div>
          <div class="tooltip-raw">{$t('audioBadges.streaming', { values: { protocol: castProtocol?.toUpperCase() ?? '' } })}</div>
        {:else}
        <div
          class="tooltip-device"
          class:scrollable={deviceNameOverflow > 0}
          style="--ticker-offset: {deviceNameOffset}; --ticker-duration: {deviceNameDuration};"
          bind:this={deviceNameRef}
        >
          <span class="device-name-text" bind:this={deviceNameTextRef}>
            {prettyDeviceName}
          </span>
        </div>
        {#if currentDevice && currentDevice !== prettyDeviceName}
          <div class="tooltip-raw">{currentDevice}</div>
        {/if}
        {#if currentVolume !== null}
          <div class="tooltip-volume">
            <span class="volume-label">{$t('player.volume')}:</span>
            <span class="volume-value">{currentVolume}%</span>
          </div>
        {/if}
        {/if}
      </div>

      <!-- Audio Settings (only show when not casting) -->
      {#if !castConnected && (isAlsaDirect || settings?.dac_passthrough || settings?.exclusive_mode)}
        <div class="tooltip-section">
          <div class="tooltip-label">{$t('audioBadges.audioSettings')}</div>
          {#if isAlsaDirect}
            <div class="tooltip-setting">
              <span class="setting-icon hw-icon">●</span>
              <span class="setting-text">
                ✓ {$t('audioBadges.hwDirectBitPerfect')}
                {#if hardwareStatus?.hardware_sample_rate}
                  <br><span class="setting-detail">{$t('audioBadges.kHzNative', { values: { rate: (hardwareStatus.hardware_sample_rate / 1000).toFixed(1) } })}</span>
                {/if}
                <br><span class="setting-detail">{prettyDeviceName}</span>
              </span>
            </div>
          {/if}
          {#if settings.dac_passthrough}
            <div class="tooltip-setting" class:warning={dacBadgeState === 'warning'}>
              <span class="setting-icon" class:active={dacBadgeState === 'active'} class:warning={dacBadgeState === 'warning'}>●</span>
              <span class="setting-text">
                {#if dacBadgeState === 'warning'}
                  ⚠ {$t('audioBadges.resamplingError')}
                  {#if hardwareStatus?.hardware_sample_rate && samplingRate}
                    <br><span class="setting-detail">{$t('audioBadges.resamplingDetail', { values: { trackRate: samplingRate, hwRate: (hardwareStatus.hardware_sample_rate / 1000).toFixed(1) } })}</span>
                    <br><span class="setting-help">{$t('audioBadges.resamplingFix')}</span>
                  {/if}
                {:else if dacBadgeState === 'active'}
                  ✓ {$t('audioBadges.dacPassthroughActive')}
                  {#if hardwareStatus?.hardware_sample_rate}
                    <br><span class="setting-detail">{$t('audioBadges.kHzNative', { values: { rate: (hardwareStatus.hardware_sample_rate / 1000).toFixed(1) } })}</span>
                  {/if}
                {:else}
                  {$t('audioBadges.dacDisabled')}
                {/if}
              </span>
            </div>
          {/if}
          {#if settings.exclusive_mode}
            <div class="tooltip-setting">
              <span class="setting-icon" class:active={exclusiveModeActive}>●</span>
              <span class="setting-text">{$t('audioBadges.exclusiveMode')}</span>
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
    color: var(--alpha-15);
    border: 1px solid var(--alpha-6);
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
    color: var(--alpha-15);
    border-color: var(--alpha-6);
  }

  /* HW Badge (ALSA Direct) - Blue like audio equipment LED */
  .badge.hw-badge.active {
    background: rgba(59, 130, 246, 0.2);
    color: #3b82f6;
    border-color: rgba(59, 130, 246, 0.4);
  }

  /* Cast Badge - Purple for network streaming */
  .badge.cast-badge {
    background: rgba(168, 85, 247, 0.2);
    color: #a855f7;
    border-color: rgba(168, 85, 247, 0.4);
  }

  .badge-label {
    line-height: 1;
  }

  .device-tooltip {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--alpha-10);
    border-radius: 6px;
    padding: 8px 12px;
    min-width: 120px;
    max-width: 200px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
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
    color: var(--alpha-40);
    margin-bottom: 4px;
  }

  .tooltip-device {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tooltip-device.scrollable {
    text-overflow: clip;
  }

  .device-name-text {
    display: inline-block;
    white-space: nowrap;
  }

  .tooltip-device.scrollable .device-name-text {
    animation: device-name-ticker var(--ticker-duration) linear infinite;
    will-change: transform;
  }

  @keyframes device-name-ticker {
    0%, 20% { transform: translateX(0); }
    70%, 80% { transform: translateX(var(--ticker-offset)); }
    90%, 100% { transform: translateX(0); }
  }

  .tooltip-raw {
    font-size: 9px;
    color: var(--alpha-30);
    font-family: var(--font-mono, monospace);
    margin-top: 4px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Cast device styling in tooltip */
  .tooltip-device.cast-device {
    display: flex;
    align-items: center;
    gap: 6px;
    color: #a855f7;
  }

  .cast-icon-svg {
    width: 14px;
    height: 14px;
    filter: invert(55%) sepia(80%) saturate(2000%) hue-rotate(240deg) brightness(100%);
  }

  .tooltip-volume {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--alpha-10);
  }

  .volume-label {
    font-size: 10px;
    color: var(--alpha-40);
  }

  .volume-value {
    font-size: 11px;
    font-weight: 500;
    color: var(--alpha-80);
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
    color: var(--alpha-30);
    line-height: 1.4;
  }

  .setting-icon.active {
    color: #22c55e;
  }

  .setting-icon.warning {
    color: #eab308;
  }

  .setting-icon.hw-icon {
    color: #3b82f6;
  }

  .setting-text {
    color: var(--alpha-70);
    line-height: 1.4;
  }

  .tooltip-setting.warning .setting-text {
    color: #eab308;
  }

  .setting-detail {
    font-size: 10px;
    color: var(--alpha-50);
    font-family: var(--font-mono, monospace);
  }
</style>
