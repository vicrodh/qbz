<script lang="ts">
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

  // Get icon path based on tier
  const iconPath = $derived.by(() => {
    if (tier === 'max' || tier === 'hires') return '/hi-res-gray.svg';
    if (tier === 'cd') return '/cd.svg';
    if (tier === 'lossy') return '/mp3.svg';
    return '/cd.svg';
  });

  const isHiRes = $derived(tier === 'max' || tier === 'hires');
</script>

<div class="quality-badge" title="{tierLabel}: {displayText}">
  <!-- Icon -->
  <img
    src={iconPath}
    alt={tierLabel}
    class="badge-icon"
    class:hires={isHiRes}
  />

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
    padding: 5px 10px;
    min-width: 120px;
    border-radius: 4px;
    background: var(--alpha-6);
    border: 1px solid var(--alpha-10);
    box-sizing: border-box;
    cursor: help;
  }
  
  [data-theme="light"] .quality-badge {
    background: rgba(40, 42, 54, 0.85);
    border: 1px solid rgba(40, 42, 54, 0.95);
  }
  
  [data-theme="light"] .tier-label,
  [data-theme="light"] .quality-info {
    color: #ffffff;
  }
  
  [data-theme="light"] .badge-icon {
    filter: invert(1) brightness(1.5);
  }
  
  [data-theme="light"] .badge-icon.hires {
    filter: brightness(1.2);
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
    filter: none; /* Hi-Res logo keeps original colors */
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
</style>
