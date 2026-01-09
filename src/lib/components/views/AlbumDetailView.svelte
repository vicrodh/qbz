<script lang="ts">
  import { ArrowLeft, Play, Shuffle, Heart, Plus, MoreHorizontal } from 'lucide-svelte';
  import TrackRow from '../TrackRow.svelte';

  interface Track {
    id: number;
    number: number;
    title: string;
    artist?: string;
    duration: string;
    durationSeconds: number;
    quality?: string;
    hires?: boolean;
    bitDepth?: number;
    samplingRate?: number;
  }

  interface Props {
    album: {
      artwork: string;
      title: string;
      artist: string;
      year: string;
      label: string;
      genre: string;
      quality: string;
      trackCount: number;
      duration: string;
      tracks: Track[];
    };
    onBack: () => void;
    onArtistClick?: () => void;
    onTrackPlay?: (track: Track) => void;
    onPlayAll?: () => void;
    onShuffleAll?: () => void;
    onAddToQueue?: () => void;
  }

  let { album, onBack, onArtistClick, onTrackPlay, onPlayAll, onShuffleAll, onAddToQueue }: Props = $props();

  let currentTrack = $state<number | null>(null);
  let isFavorite = $state(false);
  let playBtnHovered = $state(false);
</script>

<div class="album-detail">
  <!-- Back Navigation -->
  <button class="back-btn" onclick={onBack}>
    <ArrowLeft size={16} />
    <span>Back</span>
  </button>

  <!-- Album Header -->
  <div class="album-header">
    <!-- Album Artwork -->
    <div class="artwork">
      <img src={album.artwork} alt={album.title} />
    </div>

    <!-- Album Metadata -->
    <div class="metadata">
      <h1 class="album-title">{album.title}</h1>
      <button class="artist-link" onclick={onArtistClick}>
        {album.artist}
      </button>
      <div class="album-info">{album.year} • {album.label} • {album.genre}</div>
      <div class="album-quality">{album.quality}</div>
      <div class="album-stats">{album.trackCount} tracks • {album.duration}</div>

      <!-- Action Buttons -->
      <div class="actions">
        <button
          class="play-btn"
          style="background-color: {playBtnHovered ? 'var(--accent-hover)' : 'var(--accent-primary)'}"
          onmouseenter={() => (playBtnHovered = true)}
          onmouseleave={() => (playBtnHovered = false)}
          onclick={onPlayAll}
        >
          <Play size={18} fill="white" color="white" />
          <span>Play</span>
        </button>
        <button class="secondary-btn" onclick={onShuffleAll}>
          <Shuffle size={18} />
          <span>Shuffle</span>
        </button>
        <button class="icon-btn" onclick={() => (isFavorite = !isFavorite)}>
          <Heart
            size={20}
            color={isFavorite ? 'var(--accent-primary)' : 'white'}
            fill={isFavorite ? 'var(--accent-primary)' : 'none'}
          />
        </button>
        <button class="icon-btn" onclick={onAddToQueue}>
          <Plus size={20} color="white" />
        </button>
        <button class="icon-btn">
          <MoreHorizontal size={20} color="white" />
        </button>
      </div>
    </div>
  </div>

  <!-- Divider -->
  <div class="divider"></div>

  <!-- Track List -->
  <div class="track-list">
    <!-- Table Header -->
    <div class="table-header">
      <div class="col-number">#</div>
      <div class="col-title">Title</div>
      <div class="col-duration">Duration</div>
      <div class="col-quality">Quality</div>
    </div>

    <!-- Track Rows -->
    <div class="tracks">
      {#each album.tracks as track}
        <TrackRow
          number={track.number}
          title={track.title}
          artist={track.artist}
          duration={track.duration}
          quality={track.quality}
          isPlaying={currentTrack === track.id}
          onPlay={() => {
            currentTrack = track.id;
            onTrackPlay?.(track);
          }}
        />
      {/each}
    </div>
  </div>
</div>

<style>
  .album-detail {
    width: 100%;
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

  .album-header {
    display: flex;
    gap: 32px;
    margin-bottom: 32px;
  }

  .artwork {
    flex-shrink: 0;
    width: 280px;
    height: 280px;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .artwork img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .metadata {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .album-title {
    font-size: 28px;
    font-weight: 700;
    color: var(--text-primary);
    margin-bottom: 4px;
  }

  .artist-link {
    font-size: 18px;
    font-weight: 500;
    color: var(--accent-primary);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    width: fit-content;
    margin-bottom: 8px;
  }

  .artist-link:hover {
    text-decoration: underline;
  }

  .album-info {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .album-quality {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .album-stats {
    font-size: 14px;
    color: #666666;
    margin-bottom: 24px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .play-btn {
    height: 40px;
    padding: 0 24px;
    border-radius: 20px;
    border: none;
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .play-btn span {
    font-size: 14px;
    font-weight: 500;
    color: white;
  }

  .secondary-btn {
    height: 40px;
    padding: 0 24px;
    border-radius: 20px;
    border: 1px solid rgba(255, 255, 255, 0.3);
    background: transparent;
    display: flex;
    align-items: center;
    gap: 8px;
    color: white;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .secondary-btn:hover {
    background-color: rgba(255, 255, 255, 0.1);
  }

  .secondary-btn span {
    font-size: 14px;
    font-weight: 500;
  }

  .icon-btn {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    border: none;
    background: transparent;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .icon-btn:hover {
    background-color: rgba(255, 255, 255, 0.1);
  }

  .divider {
    height: 1px;
    background-color: var(--bg-tertiary);
    margin: 32px 0;
  }

  .table-header {
    height: 40px;
    padding: 0 16px;
    display: flex;
    align-items: center;
    font-size: 12px;
    text-transform: uppercase;
    color: #666666;
    font-weight: 400;
  }

  .col-number {
    width: 48px;
  }

  .col-title {
    flex: 1;
  }

  .col-duration {
    width: 80px;
    text-align: right;
  }

  .col-quality {
    width: 80px;
    text-align: right;
  }
</style>
