<script lang="ts">
  import { Play } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { cachedSrc } from '$lib/actions/cachedImage';
  import { extractPalette } from '$lib/utils/artworkPalette';

  interface Props {
    /** The seed item the radio is based on (album, artist, etc.) — used
     *  for the title and as the visual centerpiece of the card. */
    seedTitle: string;
    seedSubtitle?: string;
    artwork?: string;
    /** Wordmark rendered under the artwork. Defaults to "RADIO" but the
     *  same visual is reused for "TOP TRACKS" and "PLAYLIST" cards in
     *  spotlight, matching the legacy ForYouTab horizontal row. */
    label?: string;
    onPlay?: () => void;
    onClick?: () => void;
  }

  let { seedTitle, seedSubtitle, artwork, label = 'RADIO', onPlay, onClick }: Props = $props();

  /**
   * Qobuz-style radio card. Visual is a stylized stereo-component:
   * the seed's artwork sits centered inside a frame with vertical
   * "speaker bar" accents on each side and a "RADIO" word-mark below.
   * Background colour is extracted from the artwork's dominant palette
   * so each radio card feels tied to its seed.
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
    if ((e.target as HTMLElement).closest('.play-btn')) return;
    onClick?.();
  }
</script>

<div class="radio-wrap">
  <div
    class="radio-card"
    style:background-color={bg}
    role="button"
    tabindex="0"
    onclick={handleClick}
    onkeydown={(e) => e.key === 'Enter' && onClick?.()}
  >
    <div class="frame">
      <div class="artwork-wrap">
        {#if artwork}
          <img
            class="artwork"
            use:cachedSrc={artwork}
            alt={seedTitle}
            loading="lazy"
            decoding="async"
          />
        {:else}
          <div class="artwork artwork-placeholder"></div>
        {/if}
        {#if onPlay}
          <button class="play-btn" type="button" aria-label={$t('actions.play')} onclick={handlePlay}>
            <Play size={20} fill="currentColor" />
          </button>
        {/if}
      </div>
      <div class="label">{label}</div>
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
    width: 220px;
    flex-shrink: 0;
  }

  /* Stereo-component frame: artwork as the "display" in the middle,
     vertical speaker-bar pseudo-elements on each side, RADIO wordmark
     at the bottom. Background tinted with the artwork's dominant
     color so each radio card reads as tied to its seed. */
  .radio-card {
    position: relative;
    width: 220px;
    height: 220px;
    border-radius: 8px;
    overflow: hidden;
    cursor: pointer;
    flex-shrink: 0;
  }

  /* Side speaker bars — two thin verticals each side, evoking amp
     side panels. Pure pseudo-elements, no extra DOM. */
  .radio-card::before,
  .radio-card::after {
    content: '';
    position: absolute;
    top: 14%;
    bottom: 14%;
    width: 14px;
    background: linear-gradient(
      to right,
      rgba(255, 255, 255, 0.04) 0%,
      rgba(255, 255, 255, 0.18) 35%,
      rgba(255, 255, 255, 0.04) 50%,
      rgba(255, 255, 255, 0.22) 65%,
      rgba(255, 255, 255, 0.04) 100%
    );
    border-radius: 2px;
    pointer-events: none;
  }
  .radio-card::before { left: 10px; }
  .radio-card::after { right: 10px; }

  .frame {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 14% 22% 18%;
    gap: 8px;
  }

  .artwork-wrap {
    position: relative;
    width: 100%;
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .artwork {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 4px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.35);
  }

  .artwork-placeholder {
    background: rgba(0, 0, 0, 0.25);
  }

  .play-btn {
    position: absolute;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    border: none;
    background: rgba(255, 255, 255, 0.95);
    color: #000;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .radio-card:hover .play-btn {
    opacity: 1;
  }

  .label {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.45em;
    color: rgba(255, 255, 255, 0.95);
    text-transform: uppercase;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.4);
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
