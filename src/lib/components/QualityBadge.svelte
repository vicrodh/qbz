<script lang="ts">
  import { Disc, Music } from 'lucide-svelte';

  interface Props {
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
  }

  let {
    quality = '',
    bitDepth,
    samplingRate
  }: Props = $props();

  // Determine quality tier
  const tier = $derived.by(() => {
    // Check bitDepth and samplingRate first
    if (bitDepth && bitDepth >= 24 && samplingRate && samplingRate > 96) {
      return 'max';
    }
    if (bitDepth && bitDepth >= 24) {
      return 'hires';
    }
    if (bitDepth === 16 || (samplingRate && samplingRate >= 44.1 && samplingRate <= 48)) {
      return 'cd';
    }

    // Check quality string
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

  // Format the display text
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
    if (tier === 'lossy') {
      return '320 kbps';
    }
    return '16-bit / 44.1 kHz';
  });

  const tierLabel = $derived.by(() => {
    if (tier === 'max') return 'Hi-Res';
    if (tier === 'hires') return 'Hi-Res';
    if (tier === 'cd') return 'CD';
    if (tier === 'lossy') return 'MP3';
    return 'CD';
  });
</script>

<div class="quality-badge tier-{tier}">
  <!-- Icon -->
  <div class="badge-icon">
    {#if tier === 'max' || tier === 'hires'}
      <!-- Hi-Res Audio Icon -->
      <svg viewBox="0 0 24 24" fill="currentColor" class="hires-icon">
        <rect x="2" y="6" width="4" height="12" rx="1" />
        <rect x="8" y="3" width="4" height="18" rx="1" />
        <rect x="14" y="8" width="4" height="8" rx="1" />
        <rect x="20" y="5" width="2" height="14" rx="1" />
      </svg>
    {:else if tier === 'cd'}
      <Disc size={14} />
    {:else}
      <Music size={14} />
    {/if}
  </div>

  <!-- Text -->
  <div class="badge-text">
    <span class="tier-label">{tierLabel}</span>
    <span class="quality-info">{displayText}</span>
  </div>
</div>

<style>
  .quality-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-radius: 4px;
    font-family: var(--font-sans, system-ui);
  }

  .badge-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .hires-icon {
    width: 16px;
    height: 16px;
  }

  .badge-text {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }

  .tier-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .quality-info {
    font-size: 11px;
    font-weight: 500;
    opacity: 0.9;
  }

  /* Tier colors */
  .tier-max {
    background: linear-gradient(135deg, rgba(255, 170, 0, 0.15) 0%, rgba(255, 136, 0, 0.15) 100%);
    border: 1px solid rgba(255, 170, 0, 0.3);
    color: #ffaa00;
  }

  .tier-hires {
    background: rgba(59, 130, 246, 0.12);
    border: 1px solid rgba(59, 130, 246, 0.25);
    color: #60a5fa;
  }

  .tier-cd {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.7);
  }

  .tier-lossy {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.5);
  }
</style>
