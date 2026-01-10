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
    artwork?: string;
    backgroundUrl?: string;
  }

  let {
    lines,
    activeIndex = -1,
    activeProgress = 0,
    align = 'center',
    title = '',
    artist = '',
    artwork,
    backgroundUrl
  }: Props = $props();

  const isCenter = $derived(align === 'center');
  const hasArtwork = $derived(!!artwork);
  const backgroundStyle = $derived(
    backgroundUrl ? `--lyrics-artwork-bg: url('${backgroundUrl}')` : undefined
  );
</script>

<section
  class="lyrics-immersive"
  class:center={isCenter}
  class:no-artwork={!hasArtwork}
  style={backgroundStyle}
>
  {#if hasArtwork}
    <div class="artwork-pane">
      <div class="artwork-frame">
        <img src={artwork} alt={title || 'Artwork'} />
      </div>
    </div>
  {/if}

  <div class="lyrics-pane">
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
  </div>
</section>

<style>
  .lyrics-immersive {
    display: grid;
    grid-template-columns: minmax(260px, 40%) minmax(0, 60%);
    gap: 48px;
    height: 100%;
    width: 100%;
    padding: 48px;
    color: var(--text-primary);
    background-image:
      radial-gradient(circle at top right, rgba(148, 92, 48, 0.35), transparent 55%),
      linear-gradient(120deg, rgba(12, 10, 9, 0.92), rgba(44, 30, 20, 0.9)),
      var(--lyrics-artwork-bg, none);
    background-size: cover;
    background-position: center;
    --lyrics-font-size: 20px;
    --lyrics-active-size: 28px;
    --lyrics-line-gap: 18px;
    --lyrics-line-height: 1.5;
    --lyrics-dimmed-opacity: 0.38;
    --lyrics-highlight-muted: rgba(255, 255, 255, 0.2);
  }

  .lyrics-immersive.no-artwork {
    grid-template-columns: 1fr;
  }

  .lyrics-immersive.center {
    align-items: center;
  }

  .artwork-pane {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .artwork-frame {
    width: min(420px, 100%);
    aspect-ratio: 1 / 1;
    border-radius: 18px;
    overflow: hidden;
    box-shadow: 0 28px 60px rgba(0, 0, 0, 0.55);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .artwork-frame img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .lyrics-pane {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .header {
    margin-bottom: 20px;
  }

  .title {
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 0.16em;
    color: var(--text-muted);
  }

  .artist {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
  }
</style>
