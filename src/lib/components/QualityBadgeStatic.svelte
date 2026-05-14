<script lang="ts">
  // Static quality badge — visually identical to QualityBadge's full mode but
  // NEVER reactive to current playback state. Used in list/catalog views
  // (Collection/Mixtape detail, Discography Builder, etc.) where every row
  // advertises its own stored quality. No hardware polling, no downgrade
  // arrow, no plughw popup, no bit-perfect checkmark.
  interface Props {
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    format?: string;
    /** Strip the contained pill chrome (background, border, padding,
     *  min-width, fixed height) so the badge renders as just
     *  [icon][text-block] for inline use above lists/toolbars. */
    bare?: boolean;
    /** Render only the icon, mimicking Qobuz's title-row HiRes mark:
     *  - HiRes  → the brand SVG logo alone, no surrounding rectangle.
     *  - Non-HiRes → the format icon centered in a small framed square
     *    matching the HiRes logo's outer dimensions.
     *  Tooltip stays identical to the full badge so hover still surfaces
     *  the tier label + bitrate string. */
    iconOnly?: boolean;
  }

  let {
    quality = '',
    bitDepth,
    samplingRate,
    format,
    bare = false,
    iconOnly = false,
  }: Props = $props();

  const tier = $derived.by(() => {
    const fmt = (format || quality || '').toLowerCase();

    if (fmt.includes('mp3')) return 'mp3';

    if (bitDepth && bitDepth >= 24 && samplingRate && samplingRate > 96) {
      return 'max';
    }
    if (bitDepth && bitDepth >= 24) return 'hires';
    if (bitDepth === 16 || (samplingRate && samplingRate >= 44.1 && samplingRate <= 48)) {
      return 'cd';
    }

    const q = quality.toLowerCase();
    if (q.includes('mp3') || q.includes('320')) return 'lossy';
    if (q.includes('hi-res') || q.includes('hires') || q.includes('24')) return 'hires';
    if (q.includes('cd') || q.includes('flac') || q.includes('lossless') || q.includes('16')) {
      return 'cd';
    }

    if (samplingRate && samplingRate >= 44.1) return 'cd';
    return 'cd';
  });

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

  const tierLabel = $derived.by(() => {
    if (tier === 'max') return 'Hi-Res';
    if (tier === 'hires') return 'Hi-Res';
    if (tier === 'cd') return 'CD';
    if (tier === 'mp3') return 'MP3';
    return 'CD';
  });

  const iconPath = $derived.by(() => {
    if (tier === 'max' || tier === 'hires') return '/hi-res.svg';
    if (tier === 'cd') return '/cd.svg';
    if (tier === 'mp3') return '/mp3.svg';
    return '/cd.svg';
  });

  const isHiRes = $derived(tier === 'max' || tier === 'hires');
</script>

{#if iconOnly}
  <div
    class="quality-badge icon-only"
    class:icon-only-framed={!isHiRes}
    title={`${tierLabel}: ${displayText}`}
  >
    <img
      src={iconPath}
      alt={tierLabel}
      class="badge-icon"
      class:hires={isHiRes}
      class:icon-only-framed-img={!isHiRes}
    />
  </div>
{:else}
<div
  class="quality-badge"
  class:bare
  title={`${tierLabel}: ${displayText}`}
>
  <div class="icon-container">
    <img
      src={iconPath}
      alt={tierLabel}
      class="badge-icon"
      class:hires={isHiRes}
    />
  </div>

  <div class="badge-text">
    <span class="tier-label">{tierLabel}</span>
    <span class="quality-info">{displayText}</span>
  </div>
</div>
{/if}

<style>
  /* Styles copied 1:1 from QualityBadge.svelte full mode so the two render
     identically side-by-side. Intentionally omits the reactive state classes
     (.downgraded, .plughw, .adjusted, .bitperfect) and the tooltip block. */
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

  /* Bare mode: drop the contained pill chrome so the badge can sit
     inline above a tracklist toolbar without competing visually. */
  .quality-badge.bare {
    background: none;
    border: none;
    padding: 0;
    min-width: 0;
    height: auto;
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
    filter: drop-shadow(0 0 0.4px rgba(0, 0, 0, 0.7));
  }

  .badge-text {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
    /* Force start-aligned text regardless of the parent's `text-align`.
       The badge is used inside containers that center their content
       (VisualizerPanel, StaticPanel, etc.) — without this override, the
       inherited `text-align: center` would centre `Hi-Res` and the
       quality string within the text column, leaving them visually
       off relative to the badge icon. */
    text-align: start;
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

  /* Icon-only mode: just the format mark, sized to sit on a title row
     next to album / track names. The HiRes brand SVG renders bare (its
     own rounded rectangle is part of the artwork); non-HiRes icons sit
     inside a small framed square of identical outer dimensions so the
     two visual states line up under each other in a grid. */
  .quality-badge.icon-only {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    height: auto;
    min-width: 0;
    flex-shrink: 0;
  }

  /* Both badges are 32px tall — that's the visual height of two text
     lines (title 14px + artist 13px, both line-height 1.3, plus the
     2px gap between them). Width differs: the HiRes brand mark keeps
     its native ~3:2 aspect (~48px wide so the "Hi·Res / AUDIO" stack
     reads cleanly); the non-HiRes framed icon is square (32×32) so a
     single icon sits centered in a tidy box. The two badges never
     appear side-by-side in the same row, so the different widths
     don't compete. */
  .quality-badge.icon-only .badge-icon.hires {
    width: 46px;
    height: 30px;
    object-fit: contain;
    filter: drop-shadow(0 0 0.4px rgba(0, 0, 0, 0.7));
  }

  .quality-badge.icon-only-framed {
    width: 30px;
    height: 30px;
    background: var(--alpha-8, rgba(255, 255, 255, 0.06));
    border: 1px solid var(--alpha-12, rgba(255, 255, 255, 0.1));
    border-radius: 3px;
    box-sizing: border-box;
  }

  .badge-icon.icon-only-framed-img {
    width: 16px;
    height: 16px;
    filter: invert(1) brightness(0.7);
  }
</style>
