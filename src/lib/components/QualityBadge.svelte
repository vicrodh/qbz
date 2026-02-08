<script lang="ts">
  import { t } from '$lib/i18n';
  import { invoke } from '@tauri-apps/api/core';

  interface HardwareAudioStatus {
    hardware_sample_rate: number | null;
    hardware_format: string | null;
    is_active: boolean;
  }

  interface Props {
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    originalBitDepth?: number;
    originalSamplingRate?: number;
    format?: string;
    compact?: boolean;
  }

  let {
    quality = '',
    bitDepth,
    samplingRate,
    originalBitDepth,
    originalSamplingRate,
    format,
    compact = false
  }: Props = $props();

  // Hardware sample rate from /proc/asound (sub-ms read)
  let hardwareSampleRate = $state<number | null>(null);
  let isHovering = $state(false);

  // Poll hardware audio status when we have a sampling rate to compare against
  $effect(() => {
    if (!samplingRate) return;

    async function poll() {
      try {
        const status = await invoke<HardwareAudioStatus>('get_hardware_audio_status').catch(() => null);
        if (status?.is_active && status.hardware_sample_rate) {
          hardwareSampleRate = status.hardware_sample_rate;
        } else {
          hardwareSampleRate = null;
        }
      } catch {
        hardwareSampleRate = null;
      }
    }

    poll();
    const interval = setInterval(poll, 2000);
    return () => clearInterval(interval);
  });

  // Downgrade detection: compare decoded rate (kHz) vs hardware rate (Hz)
  const isDowngraded = $derived.by(() => {
    if (hardwareSampleRate && samplingRate) {
      // samplingRate is in kHz (e.g. 192), hardwareSampleRate is in Hz (e.g. 48000)
      const decodedHz = samplingRate * 1000;
      // Significant difference: hardware is more than 10% lower than decoded
      return hardwareSampleRate < decodedHz * 0.9;
    }
    // Fallback: compare original vs current (old logic, for when hardware unavailable)
    if (originalSamplingRate && samplingRate) {
      return samplingRate < originalSamplingRate * 0.9;
    }
    return false;
  });

  // Effective output rate for tooltip display (in kHz)
  const effectiveOutputRateKHz = $derived.by(() => {
    if (hardwareSampleRate) {
      return hardwareSampleRate / 1000;
    }
    return null;
  });

  // Determine quality tier
  const tier = $derived.by(() => {
    const fmt = (format || quality || '').toLowerCase();

    // Check for MP3 FIRST - it should never be "CD" quality
    if (fmt.includes('mp3')) {
      return 'mp3';
    }

    // Check bitDepth and samplingRate for lossless formats
    if (bitDepth && bitDepth >= 24 && samplingRate && samplingRate > 96) {
      return 'max';
    }
    if (bitDepth && bitDepth >= 24) {
      return 'hires';
    }
    if (bitDepth === 16 || (samplingRate && samplingRate >= 44.1 && samplingRate <= 48)) {
      return 'cd';
    }

    // Check quality string for streaming sources
    const q = quality.toLowerCase();
    if (q.includes('mp3') || q.includes('320')) {
      return 'lossy';
    }
    if (q.includes('hi-res') || q.includes('hires') || q.includes('24')) {
      return 'hires';
    }
    if (q.includes('cd') || q.includes('flac') || q.includes('lossless') || q.includes('16')) {
      return 'cd';
    }

    // Default fallback
    if (samplingRate && samplingRate >= 44.1) {
      return 'cd';
    }

    return 'cd'; // Default to CD instead of unknown
  });

  // Format the display text for full mode
  const displayText = $derived.by(() => {
    if (tier === 'max') {
      const depth = bitDepth || 24;
      const rate = samplingRate || 192;
      return `${depth}-bit / ${rate} kHz`;
    }
    if (tier === 'hires') {
      const depth = bitDepth || 24;
      const rate = samplingRate || 96;
      return `${depth}-bit / ${rate} kHz`;
    }
    if (tier === 'cd') {
      const depth = bitDepth || 16;
      const rate = samplingRate || 44.1;
      return `${depth}-bit / ${rate} kHz`;
    }
    if (tier === 'mp3') {
      const rate = samplingRate || 44.1;
      return `${rate} kHz`;
    }
    return '16-bit / 44.1 kHz';
  });

  // Format the compact display text
  const compactText = $derived.by(() => {
    if (tier === 'max' || tier === 'hires') {
      const depth = bitDepth || 24;
      const rate = samplingRate || (tier === 'max' ? 192 : 96);
      return `${depth}bit/${rate}kHz`;
    }
    if (tier === 'mp3') {
      return 'MP3';
    }
    return 'CD Quality';
  });

  const tierLabel = $derived.by(() => {
    if (tier === 'max') return 'Hi-Res';
    if (tier === 'hires') return 'Hi-Res';
    if (tier === 'cd') return 'CD';
    if (tier === 'mp3') return 'MP3';
    return 'CD';
  });

  // Get icon path based on tier
  const iconPath = $derived.by(() => {
    if (tier === 'max' || tier === 'hires') return '/hi-res.svg';
    if (tier === 'cd') return '/cd.svg';
    if (tier === 'mp3') return '/mp3.svg';
    return '/cd.svg';
  });

  const isHiRes = $derived(tier === 'max' || tier === 'hires');
</script>

{#if compact}
  <span class="quality-badge-compact" title="{tierLabel}: {displayText}">
    {compactText}
  </span>
{:else}
  <div
    class="quality-badge"
    class:downgraded={isDowngraded}
    title={isDowngraded ? undefined : `${tierLabel}: ${displayText}`}
    onmouseenter={() => { isHovering = true; }}
    onmouseleave={() => { isHovering = false; }}
  >
    <!-- Icon container with fixed width -->
    <div class="icon-container">
      <img
        src={iconPath}
        alt={tierLabel}
        class="badge-icon"
        class:hires={isHiRes}
      />
    </div>

    <!-- Text -->
    <div class="badge-text">
      <span class="tier-label">
        {tierLabel}
        <span class="downgrade-indicator" class:visible={isDowngraded}>↓</span>
      </span>
      <span class="quality-info">{displayText}</span>
    </div>

    <!-- Custom tooltip for degraded state -->
    {#if isDowngraded && isHovering}
      <div class="quality-tooltip">
        <div class="tooltip-section">
          <div class="tooltip-label">{$t('quality.tooltip.source')}</div>
          <div class="tooltip-value">{displayText}</div>
        </div>
        <div class="tooltip-section">
          <div class="tooltip-label">{$t('quality.tooltip.output')}</div>
          <div class="tooltip-value">{effectiveOutputRateKHz ? `${effectiveOutputRateKHz} kHz` : displayText}</div>
        </div>
        <div class="tooltip-divider"></div>
        <div class="tooltip-warning">{$t('quality.tooltip.resampled')}</div>
        <div class="tooltip-fix">{$t('quality.tooltip.fix')}</div>
      </div>
    {/if}
  </div>
{/if}

<style>
  /* Compact mode styles - simple text like TrackRow */
  .quality-badge-compact {
    display: inline-block;
    font-size: 12px;
    color: #666666;
    width: 80px;
    text-align: center;
    white-space: nowrap;
  }

  /* Full mode styles */
  .quality-badge {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    min-width: 136px;
    height: 36px;
    border-radius: 4px;
    background: var(--alpha-6);
    border: 1px solid var(--alpha-10);
    box-sizing: border-box;
    cursor: help;
  }

  .icon-container {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .badge-icon {
    width: 16px;
    height: 16px;
    object-fit: contain;
    filter: invert(1) brightness(0.7);
  }

  .badge-icon.hires {
    width: 24px;
    height: 24px;
    /* Sub-pixel drop-shadow reinforces thin black text strokes against */
    /* the gold gradient, counteracting chromatic anti-aliasing blur */
    filter: drop-shadow(0 0 0.4px rgba(0, 0, 0, 0.7));
  }

  .badge-text {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }

  .tier-label {
    font-family: 'LINE Seed JP', var(--font-sans);
    font-size: 8px;
    font-weight: 100;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: #b0b0b0;
  }

  .quality-info {
    font-family: 'LINE Seed JP', var(--font-sans);
    font-size: 9px;
    font-weight: 100;
    color: #999999;
  }

  /* Downgrade indicator styles */
  .quality-badge.downgraded {
    border-color: rgba(234, 179, 8, 0.3);
  }

  .downgrade-indicator {
    color: #eab308;
    font-weight: 600;
    margin-left: 2px;
    visibility: hidden;
  }

  .downgrade-indicator.visible {
    visibility: visible;
  }

  /* Custom tooltip for degraded quality */
  .quality-tooltip {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg-secondary);
    border: 1px solid var(--alpha-10);
    border-radius: 6px;
    padding: 8px 12px;
    min-width: 180px;
    max-width: 240px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    z-index: 9999;
    animation: tooltip-appear 150ms ease;
    pointer-events: none;
  }

  @keyframes tooltip-appear {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }

  .tooltip-section {
    margin-bottom: 4px;
  }

  .tooltip-label {
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--alpha-40);
    margin-bottom: 1px;
  }

  .tooltip-value {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .tooltip-divider {
    height: 1px;
    background: var(--alpha-10);
    margin: 6px 0;
  }

  .tooltip-warning {
    font-size: 11px;
    color: #eab308;
    font-weight: 500;
    margin-bottom: 2px;
  }

  .tooltip-fix {
    font-size: 10px;
    color: var(--alpha-50);
    line-height: 1.3;
  }
</style>
