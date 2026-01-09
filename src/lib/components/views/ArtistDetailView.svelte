<script lang="ts">
  import { ArrowLeft, User, ChevronDown, ChevronUp } from 'lucide-svelte';
  import AlbumCard from '../AlbumCard.svelte';

  interface Album {
    id: string;
    title: string;
    artwork: string;
    year?: string;
    quality: string;
  }

  interface Biography {
    summary?: string;
    content?: string;
    source?: string;
  }

  interface Props {
    artist: {
      id: number;
      name: string;
      image?: string;
      albumsCount?: number;
      biography?: Biography;
      albums: Album[];
      totalAlbums: number;
    };
    onBack: () => void;
    onAlbumClick?: (albumId: string) => void;
    onLoadMore?: () => void;
    isLoadingMore?: boolean;
  }

  let { artist, onBack, onAlbumClick, onLoadMore, isLoadingMore = false }: Props = $props();

  let bioExpanded = $state(false);
  let imageError = $state(false);

  function handleImageError() {
    imageError = true;
  }

  // Get biography text (prefer summary, fall back to content)
  let bioText = $derived(
    artist.biography?.summary || artist.biography?.content || null
  );

  // Truncate bio for collapsed view
  let truncatedBio = $derived(
    bioText && bioText.length > 300 ? bioText.slice(0, 300) + '...' : bioText
  );

  let hasMoreAlbums = $derived(artist.albums.length < artist.totalAlbums);
</script>

<div class="artist-detail">
  <!-- Back Navigation -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>Back</span>
  </button>

  <!-- Artist Header -->
  <div class="artist-header">
    <!-- Artist Image -->
    <div class="artist-image-container">
      {#if imageError || !artist.image}
        <div class="artist-image-placeholder">
          <User size={60} />
        </div>
      {:else}
        <img
          src={artist.image}
          alt={artist.name}
          class="artist-image"
          onerror={handleImageError}
        />
      {/if}
    </div>

    <!-- Artist Info -->
    <div class="artist-info">
      <h1 class="artist-name">{artist.name}</h1>
      <div class="artist-stats">
        {artist.totalAlbums || artist.albumsCount || 0} albums
      </div>

      <!-- Biography -->
      {#if bioText}
        <div class="biography">
          <p class="bio-text">
            {bioExpanded ? bioText : truncatedBio}
          </p>
          {#if bioText.length > 300}
            <button class="bio-toggle" onclick={() => bioExpanded = !bioExpanded}>
              {#if bioExpanded}
                <ChevronUp size={16} />
                <span>Show less</span>
              {:else}
                <ChevronDown size={16} />
                <span>Read more</span>
              {/if}
            </button>
          {/if}
          {#if artist.biography?.source}
            <div class="bio-source">Source: {artist.biography.source}</div>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <!-- Divider -->
  <div class="divider"></div>

  <!-- Discography Section -->
  <div class="discography">
    <h2 class="section-title">Discography</h2>

    {#if artist.albums.length === 0}
      <div class="no-albums">No albums found</div>
    {:else}
      <div class="albums-grid">
        {#each artist.albums as album}
          <AlbumCard
            artwork={album.artwork}
            title={album.title}
            artist={album.year || ''}
            quality={album.quality}
            onclick={() => onAlbumClick?.(album.id)}
          />
        {/each}
      </div>

      {#if hasMoreAlbums}
        <div class="load-more-container">
          <button
            class="load-more-btn"
            onclick={onLoadMore}
            disabled={isLoadingMore}
          >
            {isLoadingMore ? 'Loading...' : `Load More (${artist.albums.length} of ${artist.totalAlbums})`}
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .artist-detail {
    width: 100%;
    padding-bottom: 24px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    margin-bottom: 24px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-secondary);
  }

  .artist-header {
    display: flex;
    gap: 32px;
    margin-bottom: 32px;
  }

  .artist-image-container {
    flex-shrink: 0;
  }

  .artist-image {
    width: 220px;
    height: 220px;
    border-radius: 50%;
    object-fit: cover;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .artist-image-placeholder {
    width: 220px;
    height: 220px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, var(--bg-tertiary) 0%, var(--bg-secondary) 100%);
    color: var(--text-muted);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .artist-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .artist-name {
    font-size: 36px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 8px;
  }

  .artist-stats {
    font-size: 16px;
    color: var(--text-muted);
    margin-bottom: 16px;
  }

  .biography {
    max-width: 600px;
  }

  .bio-text {
    font-size: 14px;
    line-height: 1.6;
    color: var(--text-secondary);
    margin-bottom: 8px;
  }

  .bio-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
    color: var(--accent-primary);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
  }

  .bio-toggle:hover {
    text-decoration: underline;
  }

  .bio-source {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 8px;
  }

  .divider {
    height: 1px;
    background-color: var(--bg-tertiary);
    margin: 32px 0;
  }

  .section-title {
    font-size: 24px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 24px;
  }

  .no-albums {
    color: var(--text-muted);
    font-size: 14px;
  }

  .albums-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 24px;
  }

  .load-more-container {
    display: flex;
    justify-content: center;
    padding: 32px 0;
  }

  .load-more-btn {
    padding: 12px 32px;
    background-color: var(--bg-tertiary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .load-more-btn:hover:not(:disabled) {
    background-color: var(--accent-primary);
    border-color: var(--accent-primary);
  }

  .load-more-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  /* Responsive */
  @media (max-width: 768px) {
    .artist-header {
      flex-direction: column;
      align-items: center;
      text-align: center;
    }

    .artist-name {
      font-size: 28px;
    }

    .biography {
      max-width: 100%;
    }
  }
</style>
