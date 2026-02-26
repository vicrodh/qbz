<script lang="ts">
  import { t } from '$lib/i18n';

  interface Props {
    artwork?: string;
    title?: string;
    artist?: string;
  }

  let { artwork, title, artist }: Props = $props();
</script>

<div class="compact-surface">
  {#if artwork}
    <img src={artwork} alt={title ?? $t('player.noTrackPlaying')} class="artwork" />
  {:else}
    <div class="artwork-placeholder" aria-hidden="true"></div>
  {/if}

  <div class="track-meta">
    <div class="title">{title ?? $t('player.noTrackPlaying')}</div>
    <div class="artist">{artist ?? '—'}</div>
  </div>
</div>

<style>
  .compact-surface {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 12px 6px;
    min-height: 0;
    flex: 1 1 auto;
    overflow: hidden;
  }

  .artwork,
  .artwork-placeholder {
    width: 50px;
    height: 50px;
    border-radius: 7px;
    flex-shrink: 0;
  }

  .artwork {
    object-fit: cover;
  }

  .artwork-placeholder {
    background: var(--alpha-8);
    border: 1px solid var(--alpha-12);
  }

  .track-meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .title,
  .artist {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .title {
    font-size: 13px;
    line-height: 1.2;
    font-weight: 600;
    color: var(--text-primary);
  }

  .artist {
    font-size: 11px;
    line-height: 1.2;
    color: var(--text-muted);
  }
</style>
