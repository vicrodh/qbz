<script lang="ts">
  import { Music } from 'lucide-svelte';
  import HeroSection from '../HeroSection.svelte';
  import HorizontalScrollRow from '../HorizontalScrollRow.svelte';
  import AlbumCard from '../AlbumCard.svelte';

  interface Album {
    id?: string;
    artwork: string;
    title: string;
    artist: string;
    quality?: string;
  }

  interface Props {
    featuredAlbum?: {
      artwork: string;
      title: string;
      artist: string;
      year: string;
    };
    recentAlbums?: Album[];
    recommendedAlbums?: Album[];
    newReleases?: Album[];
    onAlbumClick?: (albumId: string) => void;
  }

  let { featuredAlbum, recentAlbums = [], recommendedAlbums = [], newReleases = [], onAlbumClick }: Props = $props();

  // Only show featured if we have actual data
  const hasFeatured = $derived(featuredAlbum || recentAlbums.length > 0);
  const featured = $derived(featuredAlbum ?? (recentAlbums[0] ? {
    artwork: recentAlbums[0].artwork,
    title: recentAlbums[0].title,
    artist: recentAlbums[0].artist,
    year: '2024'
  } : null));

  const hasContent = $derived(recentAlbums.length > 0 || recommendedAlbums.length > 0 || newReleases.length > 0);
</script>

<div class="home-view">
  {#if hasContent}
    <!-- Hero/Featured Section -->
    {#if featured}
      <HeroSection
        artwork={featured.artwork}
        title={featured.title}
        artist={featured.artist}
        year={featured.year}
      />
    {/if}

    <!-- Recently Played -->
    {#if recentAlbums.length > 0}
      <HorizontalScrollRow title="Escuchado recientemente">
        {#snippet children()}
          {#each recentAlbums as album}
            <AlbumCard
              artwork={album.artwork}
              title={album.title}
              artist={album.artist}
              quality={album.quality}
              onclick={() => onAlbumClick?.(album.id ?? '')}
            />
          {/each}
          <div class="spacer"></div>
        {/snippet}
      </HorizontalScrollRow>
    {/if}

    <!-- Recommended For You -->
    {#if recommendedAlbums.length > 0}
      <HorizontalScrollRow title="Recomendado para ti">
        {#snippet children()}
          {#each recommendedAlbums as album}
            <AlbumCard
              artwork={album.artwork}
              title={album.title}
              artist={album.artist}
              quality={album.quality}
              size="large"
              onclick={() => onAlbumClick?.(album.id ?? '')}
            />
          {/each}
          <div class="spacer"></div>
        {/snippet}
      </HorizontalScrollRow>
    {/if}

    <!-- New Releases -->
    {#if newReleases.length > 0}
      <HorizontalScrollRow title="Nuevos lanzamientos">
        {#snippet children()}
          {#each newReleases as album}
            <AlbumCard
              artwork={album.artwork}
              title={album.title}
              artist={album.artist}
              quality={album.quality}
              onclick={() => onAlbumClick?.(album.id ?? '')}
            />
          {/each}
          <div class="spacer"></div>
        {/snippet}
      </HorizontalScrollRow>
    {/if}
  {:else}
    <!-- Welcome/Empty State -->
    <div class="welcome-state">
      <Music size={64} />
      <h1>Welcome to QBZ</h1>
      <p>Use the search to discover music on Qobuz</p>
    </div>
  {/if}
</div>

<style>
  .home-view {
    width: 100%;
    min-height: calc(100vh - 160px);
  }

  .spacer {
    width: 60px;
    flex-shrink: 0;
  }

  .welcome-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: calc(100vh - 200px);
    color: var(--text-muted);
    text-align: center;
    gap: 16px;
  }

  .welcome-state h1 {
    font-size: 28px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .welcome-state p {
    font-size: 16px;
    margin: 0;
  }
</style>
