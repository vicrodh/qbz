<script lang="ts">
  import { t } from '$lib/i18n';

  interface Props {
    artwork?: string;
    title?: string;
    artist?: string;
    album?: string;
  }

  let { artwork, title, artist, album }: Props = $props();

  let titleRef: HTMLDivElement | null = $state(null);
  let titleTextRef: HTMLSpanElement | null = $state(null);
  let artistRef: HTMLDivElement | null = $state(null);
  let artistTextRef: HTMLSpanElement | null = $state(null);
  let albumRef: HTMLDivElement | null = $state(null);
  let albumTextRef: HTMLSpanElement | null = $state(null);

  let titleOverflow = $state(0);
  let artistOverflow = $state(0);
  let albumOverflow = $state(0);

  const tickerSpeed = 40;
  const titleOffset = $derived(titleOverflow > 0 ? `-${titleOverflow + 16}px` : '0px');
  const artistOffset = $derived(artistOverflow > 0 ? `-${artistOverflow + 16}px` : '0px');
  const albumOffset = $derived(albumOverflow > 0 ? `-${albumOverflow + 16}px` : '0px');
  const titleDuration = $derived(titleOverflow > 0 ? `${(titleOverflow + 16) / tickerSpeed}s` : '0s');
  const artistDuration = $derived(artistOverflow > 0 ? `${(artistOverflow + 16) / tickerSpeed}s` : '0s');
  const albumDuration = $derived(albumOverflow > 0 ? `${(albumOverflow + 16) / tickerSpeed}s` : '0s');

  function updateOverflow(): void {
    if (titleRef && titleTextRef) {
      const overflow = titleTextRef.scrollWidth - titleRef.clientWidth;
      titleOverflow = overflow > 0 ? overflow : 0;
    }

    if (artistRef && artistTextRef) {
      const overflow = artistTextRef.scrollWidth - artistRef.clientWidth;
      artistOverflow = overflow > 0 ? overflow : 0;
    }

    if (albumRef && albumTextRef) {
      const overflow = albumTextRef.scrollWidth - albumRef.clientWidth;
      albumOverflow = overflow > 0 ? overflow : 0;
    }
  }

  $effect(() => {
    title;
    artist;
    album;
    requestAnimationFrame(() => {
      updateOverflow();
    });
  });
</script>

<div class="artwork-surface">
  {#if artwork}
    <img src={artwork} alt={title ?? $t('player.noTrackPlaying')} class="artwork" />
  {:else}
    <div class="artwork-placeholder" aria-hidden="true"></div>
  {/if}

  <div class="meta">
    <div
      class="title"
      class:scrollable={titleOverflow > 0}
      style="--ticker-offset: {titleOffset}; --ticker-duration: {titleDuration};"
      bind:this={titleRef}
    >
      <span class="title-text" bind:this={titleTextRef}>{title ?? $t('player.noTrackPlaying')}</span>
    </div>

    <div
      class="subtitle"
      class:scrollable={artistOverflow > 0}
      style="--ticker-offset: {artistOffset}; --ticker-duration: {artistDuration};"
      bind:this={artistRef}
    >
      <span class="subtitle-text" bind:this={artistTextRef}>{artist ?? '—'}</span>
    </div>

    <div
      class="subtitle album-line"
      class:scrollable={albumOverflow > 0}
      style="--ticker-offset: {albumOffset}; --ticker-duration: {albumDuration};"
      bind:this={albumRef}
    >
      <span class="subtitle-text" bind:this={albumTextRef}>{album ?? '—'}</span>
    </div>
  </div>
</div>

<style>
  .artwork-surface {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 16px 20px 12px;
    overflow: hidden;
  }

  .artwork,
  .artwork-placeholder {
    width: 100%;
    max-width: 320px;
    aspect-ratio: 1 / 1;
    border-radius: 10px;
    flex-shrink: 1;
    min-height: 0;
  }

  .artwork {
    object-fit: cover;
  }

  .artwork-placeholder {
    background: var(--alpha-8);
    border: 1px solid var(--alpha-12);
  }

  .meta {
    width: 100%;
    max-width: 320px;
    text-align: center;
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: hidden;
  }

  .title,
  .subtitle {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .subtitle {
    font-size: 12px;
    color: var(--text-muted);
  }

  .subtitle.album-line {
    color: var(--alpha-55);
  }

  .title.scrollable,
  .subtitle.scrollable {
    text-overflow: clip;
  }

  .title-text,
  .subtitle-text {
    display: inline-block;
    white-space: nowrap;
  }

  .artwork-surface:hover .title.scrollable .title-text,
  .artwork-surface:hover .subtitle.scrollable .subtitle-text {
    animation: mini-ticker var(--ticker-duration) linear infinite;
    will-change: transform;
  }

  @keyframes mini-ticker {
    0%, 20% {
      transform: translateX(0);
    }
    70%, 80% {
      transform: translateX(var(--ticker-offset));
    }
    90%, 100% {
      transform: translateX(0);
    }
  }
</style>
