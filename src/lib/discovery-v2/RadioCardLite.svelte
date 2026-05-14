<script lang="ts">
  import { Play } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { extractPalette } from '$lib/utils/artworkPalette';

  interface Props {
    /** The seed item the radio is based on. Title is rendered below
     *  the visual; the artwork sits centered inside the bezel. */
    seedTitle: string;
    seedSubtitle?: string;
    artwork?: string;
    /** Wordmark rendered over the artwork. Defaults to "RADIO" but the
     *  same visual is reused for "TOP TRACKS" and "PLAYLIST" cards in
     *  spotlight, matching the legacy ForYouTab horizontal row. */
    label?: string;
    onPlay?: () => void;
    onClick?: () => void;
  }

  let { seedTitle, seedSubtitle, artwork, label = 'RADIO', onPlay, onClick }: Props = $props();

  /**
   * Qobuz-style radio card matching the iOS reference (commit c8cee326).
   * Three layers stacked inside a 180x180 frame:
   *   1. Background color extracted from the artwork's dominant palette
   *      so each card feels tied to its seed.
   *   2. The 114x114 artwork centered as the "display" of the stereo.
   *   3. /image_radio_shadows.png overlay with mix-blend-mode: multiply
   *      and opacity 0.7 — the side speaker-bar shadows fade naturally
   *      into the bg color instead of sitting flatly on top.
   * RADIO wordmark anchors at the bottom over the blended layers.
   * Hover reveals a blurred dark overlay with a play button.
   */

  let bg = $state('var(--bg-tertiary)');

  $effect(() => {
    if (!artwork) {
      bg = 'var(--bg-tertiary)';
      return;
    }
    let cancelled = false;
    void extractPalette(artwork).then((palette) => {
      if (cancelled) return;
      bg = palette.dominant?.hex ?? 'var(--bg-tertiary)';
    });
    return () => {
      cancelled = true;
    };
  });

  function handlePlay(e: MouseEvent) {
    e.stopPropagation();
    onPlay?.();
  }

  function handleClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('.radio-overlay-play-btn')) return;
    onClick?.();
  }
</script>

<div class="radio-wrap">
  <div
    class="radio-card"
    role="button"
    tabindex="0"
    onclick={handleClick}
    onkeydown={(e) => e.key === 'Enter' && onClick?.()}
  >
    <div class="visual" style:background-color={bg}>
      {#if artwork}
        <img
          class="art"
          use:cachedSrc={artwork}
          alt={seedTitle}
          loading="lazy"
          decoding="async"
        />
      {:else}
        <div class="art art-placeholder"></div>
      {/if}
      <img class="shadow" src="/image_radio_shadows.png" alt="" aria-hidden="true" />
      <span class="label">{label}</span>
      {#if onPlay}
        <div class="hover-overlay">
          <button
            class="radio-overlay-play-btn"
            type="button"
            aria-label={$t('actions.play')}
            onclick={handlePlay}
          >
            <Play size={18} fill="white" color="white" />
          </button>
        </div>
      {/if}
    </div>
  </div>
  <div class="meta">
    <div class="seed-title">{seedTitle}</div>
    {#if seedSubtitle}<div class="seed-subtitle">{seedSubtitle}</div>{/if}
  </div>
</div>

<style>
  .radio-wrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 180px;
    flex-shrink: 0;
  }

  .radio-card {
    cursor: pointer;
    background: none;
    border: none;
    padding: 0;
  }

  /* Background-tinted frame. Artwork is centered inside as the stereo's
     "display" panel. transition makes the tint re-blend gently when the
     palette resolves a moment after artwork loads. */
  .visual {
    position: relative;
    width: 180px;
    height: 180px;
    border-radius: 8px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background-color 400ms ease;
  }

  .art {
    position: relative;
    z-index: 1;
    width: 114px;
    height: 114px;
    object-fit: cover;
    border-radius: 4px;
  }

  .art-placeholder {
    background: rgba(0, 0, 0, 0.25);
  }

  /* PNG bezel: side speaker-bar shadows over a transparent center.
     mix-blend-mode: multiply fades the gray shadows into whatever
     palette color the card is tinted with, so light tints get soft
     gray bars and dark tints get crushed shadows naturally. opacity
     0.7 dials back the darkness for high-contrast covers. */
  .shadow {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    pointer-events: none;
    mix-blend-mode: multiply;
    opacity: 0.7;
    z-index: 2;
  }

  /* RADIO wordmark anchored at the bottom. padding-left compensates the
     trailing letter-spacing on the last letter so the word looks
     optically centered instead of pushed left. */
  .label {
    position: absolute;
    bottom: 10px;
    left: 0;
    right: 0;
    text-align: center;
    font-size: 20px;
    font-weight: 300;
    letter-spacing: 0.35em;
    padding-left: 0.35em;
    color: rgba(255, 255, 255, 0.85);
    text-shadow: 0 2px 8px rgba(0, 0, 0, 0.6);
    pointer-events: none;
    z-index: 3;
    text-transform: uppercase;
  }

  /* Full-card backdrop-blur overlay with a play button on hover.
     Matches the AlbumCardLite hover overlay pattern. */
  .hover-overlay {
    position: absolute;
    inset: 0;
    z-index: 4;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 150ms ease;
    background: rgba(10, 10, 10, 0.75);
    backdrop-filter: blur(12px);
    -webkit-backdrop-filter: blur(12px);
    border-radius: inherit;
    pointer-events: none;
  }

  .radio-card:hover .hover-overlay {
    opacity: 1;
    pointer-events: auto;
  }

  .radio-overlay-play-btn {
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: white;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    box-shadow:
      inset 0 0 0 1px rgba(255, 255, 255, 0.85),
      0 0 1px rgba(0, 0, 0, 0.3);
    transition: transform 150ms ease, box-shadow 150ms ease;
  }

  .radio-overlay-play-btn:hover {
    box-shadow:
      inset 0 0 0 1px var(--accent-primary, #7c3aed),
      0 0 4px rgba(0, 0, 0, 0.5);
  }

  .meta {
    width: 100%;
  }

  .seed-title {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .seed-subtitle {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-top: 2px;
  }
</style>
