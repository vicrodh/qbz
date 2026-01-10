<script lang="ts">
  import LyricsLines from './LyricsLines.svelte';

  interface LyricsLine {
    text: string;
  }

  interface Props {
    lines: LyricsLine[];
    activeIndex?: number;
    activeProgress?: number;
    align?: 'left' | 'center';
    title?: string;
    artist?: string;
  }

  let {
    lines,
    activeIndex = -1,
    activeProgress = 0,
    align = 'center',
    title = '',
    artist = ''
  }: Props = $props();

  const isCenter = $derived(align === 'center');
</script>

<section class="lyrics-immersive" class:center={isCenter}>
  <div class="header">
    {#if title || artist}
      <div class="title">{title}</div>
      <div class="artist">{artist}</div>
    {/if}
  </div>

  <LyricsLines
    {lines}
    {activeIndex}
    {activeProgress}
    center={isCenter}
    compact={false}
  />
</section>

<style>
  .lyrics-immersive {
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
    padding: 40px 48px;
    color: var(--text-primary);
    background: radial-gradient(circle at top right, rgba(120, 74, 40, 0.35), transparent 55%),
      linear-gradient(120deg, rgba(14, 12, 10, 0.9), rgba(44, 32, 22, 0.85));
  }

  .lyrics-immersive.center {
    align-items: center;
  }

  .header {
    margin-bottom: 24px;
  }

  .title {
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 0.16em;
    color: var(--text-muted);
  }

  .artist {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }
</style>
