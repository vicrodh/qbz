<script lang="ts">
  import { t } from '$lib/i18n';

  interface LyricsLine {
    text: string;
  }

  interface Props {
    lines: LyricsLine[];
    activeIndex?: number;
    isLoading?: boolean;
    error?: string | null;
  }

  let {
    lines = [],
    activeIndex = -1,
    isLoading = false,
    error = null
  }: Props = $props();

  let previousIndex = $state(-1);
  let animationKey = $state(0);

  // Track index changes for animation
  $effect(() => {
    if (activeIndex !== previousIndex && activeIndex >= 0) {
      previousIndex = activeIndex;
      animationKey++;
    }
  });

  const currentLine = $derived(activeIndex >= 0 && activeIndex < lines.length ? lines[activeIndex]?.text : '');
  const hasLyrics = $derived(lines.length > 0);
</script>

<div class="lyrics-focus-panel">
  {#if isLoading}
    <div class="lyrics-state">
      <div class="spinner"></div>
    </div>
  {:else if error}
    <div class="lyrics-state">
      <span class="no-lyrics">{$t('player.noLyrics') || 'No lyrics available'}</span>
    </div>
  {:else if hasLyrics && currentLine}
    {#key animationKey}
      <div class="lyrics-line">
        <span class="line-text">{currentLine}</span>
      </div>
    {/key}
  {:else if hasLyrics}
    <div class="lyrics-state">
      <span class="waiting-text">♪</span>
    </div>
  {:else}
    <div class="lyrics-state">
      <span class="no-lyrics">{$t('player.noLyrics') || 'No lyrics available'}</span>
    </div>
  {/if}
</div>

<style>
  .lyrics-focus-panel {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 80px 60px 140px;
    z-index: 5;
  }

  .lyrics-line {
    text-align: center;
    max-width: 90%;
    animation: fadeInUp 500ms cubic-bezier(0.22, 1, 0.36, 1) forwards;
  }

  @keyframes fadeInUp {
    0% {
      opacity: 0;
      transform: translateY(40px) scale(0.95);
      filter: blur(4px);
    }
    100% {
      opacity: 1;
      transform: translateY(0) scale(1);
      filter: blur(0);
    }
  }

  .line-text {
    font-size: clamp(28px, 5vw, 56px);
    font-weight: 700;
    color: var(--text-primary, white);
    line-height: 1.3;
    text-shadow:
      0 0 40px rgba(255, 255, 255, 0.15),
      0 2px 20px rgba(0, 0, 0, 0.5),
      0 4px 40px rgba(0, 0, 0, 0.3);
    /* Word wrap for long lines */
    word-wrap: break-word;
    overflow-wrap: break-word;
    /* Subtle letter spacing for readability */
    letter-spacing: 0.01em;
  }

  .lyrics-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    font-size: 18px;
  }

  .waiting-text {
    font-size: 48px;
    opacity: 0.3;
  }

  .error-text {
    color: var(--alpha-40, rgba(255, 255, 255, 0.4));
  }

  .no-lyrics {
    font-style: italic;
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid var(--alpha-20, rgba(255, 255, 255, 0.2));
    border-top-color: var(--alpha-80, rgba(255, 255, 255, 0.8));
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Responsive */
  @media (max-width: 768px) {
    .lyrics-focus-panel {
      padding: 70px 30px 130px;
    }

    .line-text {
      font-size: clamp(22px, 6vw, 40px);
    }
  }
</style>
