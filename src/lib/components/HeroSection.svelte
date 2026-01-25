<script lang="ts">
  import { Play, MoreHorizontal } from 'lucide-svelte';

  interface Props {
    artwork: string;
    title: string;
    artist: string;
    year?: string;
  }

  let { artwork, title, artist, year }: Props = $props();

  let playBtnHovered = $state(false);
</script>

<div class="hero" style="background-image: url({artwork})">
  <!-- Gradient Overlay -->
  <div class="gradient"></div>

  <!-- Content -->
  <div class="content">
    <h1 class="title">{title}</h1>
    <p class="artist">{artist}{year ? ` • ${year}` : ''}</p>
    <div class="actions">
      <button
        class="play-btn"
        style="background-color: {playBtnHovered ? 'var(--accent-hover)' : 'var(--accent-primary)'}"
        onmouseenter={() => (playBtnHovered = true)}
        onmouseleave={() => (playBtnHovered = false)}
      >
        <Play size={16} fill="white" color="white" />
        <span>Play</span>
      </button>
      <button class="more-btn">
        <MoreHorizontal size={16} color="white" />
        <span>More</span>
      </button>
    </div>
  </div>
</div>

<style>
  .hero {
    position: relative;
    width: 100%;
    height: 320px;
    border-radius: 16px;
    overflow: hidden;
    margin-bottom: 32px;
    background-size: cover;
    background-position: center;
  }

  .gradient {
    position: absolute;
    inset: 0;
    background: linear-gradient(to bottom, transparent 40%, rgba(0, 0, 0, 0.8) 100%);
  }

  .content {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 32px;
  }

  .title {
    font-size: 24px;
    font-weight: 700;
    color: white;
    margin-bottom: 8px;
  }

  .artist {
    font-size: 16px;
    color: #aaaaaa;
    margin-bottom: 16px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .play-btn {
    height: 36px;
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

  .more-btn {
    height: 36px;
    padding: 0 24px;
    border-radius: 20px;
    border: 1px solid var(--alpha-30);
    background: transparent;
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .more-btn:hover {
    background-color: var(--alpha-10);
  }

  .more-btn span {
    font-size: 14px;
    font-weight: 500;
    color: white;
  }
</style>
