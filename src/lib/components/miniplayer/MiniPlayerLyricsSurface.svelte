<script lang="ts">
  import { Mic2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import LyricsLines from '$lib/components/lyrics/LyricsLines.svelte';
  import type { LyricsLine } from '$lib/stores/lyricsStore';

  interface Props {
    lines: LyricsLine[];
    activeIndex: number;
    activeProgress: number;
    isSynced: boolean;
  }

  let { lines, activeIndex, activeProgress, isSynced }: Props = $props();
  const hasLines = $derived(lines.length > 0);
</script>

<div class="lyrics-surface">
  {#if !hasLines}
    <div class="lyrics-empty">
      <Mic2 size={30} strokeWidth={1.6} />
      <span>{$t('player.noLyrics')}</span>
    </div>
  {:else}
    <div class="lyrics-list">
      <LyricsLines
        {lines}
        {activeIndex}
        {activeProgress}
        {isSynced}
        compact={true}
        center={false}
        scrollToActive={true}
      />
    </div>
  {/if}
</div>

<style>
  .lyrics-surface {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .lyrics-empty {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: var(--text-muted);
    font-size: 13px;
    text-align: center;
    padding: 24px;
  }

  .lyrics-list {
    flex: 1;
    min-height: 0;
  }

  .lyrics-list :global(.lyrics-lines) {
    padding: 12px 18px 26px;
    gap: 10px;
  }
</style>
