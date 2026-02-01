<script lang="ts">
  import LyricsLines from '../../lyrics/LyricsLines.svelte';
  import { t } from '$lib/i18n';

  interface LyricsLine {
    text: string;
    timeMs?: number; // Timing for CSS-only karaoke animation
  }

  interface Props {
    lines: LyricsLine[];
    activeIndex?: number;
    activeProgress?: number;
    isSynced?: boolean;
    isLoading?: boolean;
    error?: string | null;
  }

  let {
    lines = [],
    activeIndex = -1,
    activeProgress = 0,
    isSynced = false,
    isLoading = false,
    error = null
  }: Props = $props();

  const hasLyrics = $derived(lines.length > 0);
</script>

<div class="lyrics-panel">
  {#if isLoading}
    <div class="lyrics-state">
      <div class="spinner"></div>
      <span>{$t('player.fetchingLyrics')}</span>
    </div>
  {:else if error}
    <div class="lyrics-state">
      <span class="no-lyrics">{$t('player.noLyrics') || 'No lyrics available'}</span>
    </div>
  {:else if hasLyrics}
    <div class="lyrics-container">
      <LyricsLines
        {lines}
        {activeIndex}
        {activeProgress}
        {isSynced}
        center={false}
        compact={false}
        immersive={true}
      />
    </div>
  {:else}
    <div class="lyrics-state">
      <span class="no-lyrics">{$t('player.noLyrics') || 'No lyrics available'}</span>
    </div>
  {/if}
</div>

<style>
  .lyrics-panel {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .lyrics-container {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    mask-image: linear-gradient(
      to bottom,
      transparent 0%,
      black 8%,
      black 85%,
      transparent 100%
    );
    -webkit-mask-image: linear-gradient(
      to bottom,
      transparent 0%,
      black 8%,
      black 85%,
      transparent 100%
    );
  }

  .lyrics-container :global(.lyrics-lines) {
    --text-primary: var(--alpha-95, rgba(255, 255, 255, 0.95));
    --text-secondary: var(--alpha-50, rgba(255, 255, 255, 0.5));
    --text-muted: var(--alpha-25, rgba(255, 255, 255, 0.25));
    --bg-tertiary: transparent;
    padding: 0;
    height: 100%;
    /* Firefox - hide scrollbar by default */
    scrollbar-color: transparent transparent;
  }

  /* Show scrollbar on hover - Firefox */
  .lyrics-container:hover :global(.lyrics-lines) {
    scrollbar-color: rgba(255, 255, 255, 0.3) transparent;
  }

  /* WebKit - hide scrollbar thumb by default */
  .lyrics-container :global(.lyrics-lines::-webkit-scrollbar-thumb) {
    background: transparent !important;
  }

  /* Show scrollbar on hover - WebKit */
  .lyrics-container:hover :global(.lyrics-lines::-webkit-scrollbar-thumb) {
    background: rgba(255, 255, 255, 0.3) !important;
  }

  .lyrics-container:hover :global(.lyrics-lines::-webkit-scrollbar-thumb:hover) {
    background: rgba(255, 255, 255, 0.5) !important;
  }

  .lyrics-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    height: 100%;
    min-height: 200px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    font-size: 14px;
  }

  .no-lyrics {
    font-style: italic;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--alpha-20, rgba(255, 255, 255, 0.2));
    border-top-color: var(--alpha-80, rgba(255, 255, 255, 0.8));
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
