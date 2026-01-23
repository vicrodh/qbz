<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Mic2, Upload } from 'lucide-svelte';

  interface LocalArtist {
    name: string;
    album_count: number;
    track_count: number;
  }

  interface Props {
    artists: LocalArtist[];
    artistImages: Map<string, string>;
    showSettings: boolean;
    onArtistClick: (name: string) => void;
    onUploadImage: (name: string, e: MouseEvent) => void;
  }

  let {
    artists,
    artistImages,
    showSettings,
    onArtistClick,
    onUploadImage,
  }: Props = $props();

  // Constants
  const CARD_MIN_WIDTH = 160;
  const CARD_HEIGHT = 180; // Approximate height of artist card
  const GAP = 24;
  const BUFFER_ROWS = 2;

  // State
  let containerEl: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let containerHeight = $state(0);
  let containerWidth = $state(0);

  // Computed: number of columns
  let columns = $derived.by(() => {
    if (containerWidth === 0) return 1;
    return Math.max(1, Math.floor((containerWidth + GAP) / (CARD_MIN_WIDTH + GAP)));
  });

  // Computed: number of rows
  let totalRows = $derived(Math.ceil(artists.length / columns));

  // Computed: total height
  let totalHeight = $derived(totalRows * (CARD_HEIGHT + GAP) - GAP);

  // Computed: visible row range
  let visibleRowRange = $derived.by(() => {
    const rowHeight = CARD_HEIGHT + GAP;
    const firstVisibleRow = Math.floor(scrollTop / rowHeight);
    const visibleRowCount = Math.ceil(containerHeight / rowHeight) + 1;

    const startRow = Math.max(0, firstVisibleRow - BUFFER_ROWS);
    const endRow = Math.min(totalRows - 1, firstVisibleRow + visibleRowCount + BUFFER_ROWS);

    return { startRow, endRow };
  });

  // Computed: visible artists with positions
  let visibleArtists = $derived.by(() => {
    const { startRow, endRow } = visibleRowRange;
    const result: { artist: LocalArtist; top: number; left: number }[] = [];
    const rowHeight = CARD_HEIGHT + GAP;
    const cardWidth = (containerWidth - GAP * (columns - 1)) / columns;

    for (let row = startRow; row <= endRow; row++) {
      for (let col = 0; col < columns; col++) {
        const idx = row * columns + col;
        if (idx >= artists.length) break;

        result.push({
          artist: artists[idx],
          top: row * rowHeight,
          left: col * (cardWidth + GAP),
        });
      }
    }

    return result;
  });

  function handleScroll(e: Event) {
    scrollTop = (e.target as HTMLDivElement).scrollTop;
  }

  let resizeObserver: ResizeObserver | null = null;

  onMount(() => {
    if (containerEl) {
      containerHeight = containerEl.clientHeight;
      containerWidth = containerEl.clientWidth;

      resizeObserver = new ResizeObserver((entries) => {
        for (const entry of entries) {
          containerHeight = entry.contentRect.height;
          containerWidth = entry.contentRect.width;
        }
      });
      resizeObserver.observe(containerEl);
    }
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
  });
</script>

<div class="virtual-container" bind:this={containerEl} onscroll={handleScroll}>
  <div class="virtual-content" style="height: {totalHeight}px;">
    {#each visibleArtists as { artist, top, left } (artist.name)}
      {@const artistImage = artistImages.get(artist.name)}
      <div
        class="artist-card"
        style="transform: translate({left}px, {top}px);"
        role="button"
        tabindex="0"
        onclick={() => onArtistClick(artist.name)}
        onkeydown={(e) => e.key === 'Enter' && onArtistClick(artist.name)}
      >
        <div class="artist-icon" class:has-image={!!artistImage}>
          {#if artistImage}
            <img src={artistImage} alt={artist.name} class="artist-image" loading="lazy" />
          {:else}
            <Mic2 size={32} />
          {/if}
        </div>
        {#if showSettings}
          <button
            class="artist-image-btn"
            onclick={(e) => onUploadImage(artist.name, e)}
            title="Upload custom image"
          >
            <Upload size={14} />
          </button>
        {/if}
        <div class="artist-name">{artist.name}</div>
        <div class="artist-stats">
          {artist.album_count} albums &bull; {artist.track_count} tracks
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  .virtual-container {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
  }

  .virtual-content {
    position: relative;
    width: 100%;
  }

  .artist-card {
    position: absolute;
    width: calc((100% - 24px * 5) / 6);
    min-width: 160px;
    max-width: 200px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 16px;
    background: var(--bg-secondary);
    border-radius: 12px;
    cursor: pointer;
    transition: background 150ms ease, transform 150ms ease;
    will-change: transform;
  }

  .artist-card:hover {
    background: var(--bg-tertiary);
  }

  .artist-icon {
    width: 80px;
    height: 80px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
    overflow: hidden;
  }

  .artist-icon.has-image {
    background: none;
  }

  .artist-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .artist-image-btn {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--bg-tertiary);
    border: none;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .artist-card:hover .artist-image-btn {
    opacity: 1;
  }

  .artist-name {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .artist-stats {
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }
</style>
