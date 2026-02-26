<script lang="ts">
  import type { LyricsLine } from '$lib/stores/lyricsStore';
  import type { MiniPlayerSurface, MiniPlayerQueueTrack } from './types';
  import MiniPlayerCompactSurface from './MiniPlayerCompactSurface.svelte';
  import MiniPlayerArtworkSurface from './MiniPlayerArtworkSurface.svelte';
  import MiniPlayerQueueSurface from './MiniPlayerQueueSurface.svelte';
  import MiniPlayerLyricsSurface from './MiniPlayerLyricsSurface.svelte';
  import MiniPlayerFooter from './MiniPlayerFooter.svelte';

  interface Props {
    activeSurface: MiniPlayerSurface;
    isPinned: boolean;
    artwork?: string;
    title?: string;
    artist?: string;
    album?: string;
    queueTracks: MiniPlayerQueueTrack[];
    currentTrackId?: string;
    lyricsLines: LyricsLine[];
    lyricsActiveIndex: number;
    lyricsActiveProgress: number;
    lyricsIsSynced: boolean;
    isPlaying: boolean;
    currentTime: number;
    duration: number;
    volume: number;
    isShuffle: boolean;
    repeatMode: 'off' | 'all' | 'one';
    onSurfaceChange: (surface: MiniPlayerSurface) => void;
    onTogglePin: () => void;
    onExpand: () => void;
    onClose: () => void;
    onStartDrag: (event: MouseEvent) => void;
    onTogglePlay: () => void;
    onSkipBack: () => void;
    onSkipForward: () => void;
    onSeek: (time: number) => void;
    onVolumeChange: (volume: number) => void;
    onToggleShuffle: () => void;
    onToggleRepeat: () => void;
    onQueueTrackPlay?: (trackId: string) => void;
  }

  let {
    activeSurface,
    isPinned,
    artwork,
    title,
    artist,
    album,
    queueTracks,
    currentTrackId,
    lyricsLines,
    lyricsActiveIndex,
    lyricsActiveProgress,
    lyricsIsSynced,
    isPlaying,
    currentTime,
    duration,
    volume,
    isShuffle,
    repeatMode,
    onSurfaceChange,
    onTogglePin,
    onExpand,
    onClose,
    onStartDrag,
    onTogglePlay,
    onSkipBack,
    onSkipForward,
    onSeek,
    onVolumeChange,
    onToggleShuffle,
    onToggleRepeat,
    onQueueTrackPlay
  }: Props = $props();

  const compactSurface = $derived(activeSurface === 'compact');
  const microSurface = $derived(activeSurface === 'micro');
  let windowHovered = $state(false);
</script>

<div
  class="mini-player-window"
  class:compact={compactSurface}
  class:micro={microSurface}
  onmouseenter={() => {
    windowHovered = true;
  }}
  onmouseleave={() => {
    windowHovered = false;
  }}
>
  {#if !microSurface}
    <div class="surface-area">
      {#if activeSurface === 'compact'}
        <MiniPlayerCompactSurface {artwork} {title} {artist} />
      {:else if activeSurface === 'artwork'}
        <MiniPlayerArtworkSurface {artwork} {title} {artist} {album} />
      {:else if activeSurface === 'queue'}
        <MiniPlayerQueueSurface
          tracks={queueTracks}
          {currentTrackId}
          isPlaying={isPlaying}
          onTrackPlay={onQueueTrackPlay}
        />
      {:else}
        <MiniPlayerLyricsSurface
          lines={lyricsLines}
          activeIndex={lyricsActiveIndex}
          activeProgress={lyricsActiveProgress}
          isSynced={lyricsIsSynced}
        />
      {/if}
    </div>
  {/if}

  <MiniPlayerFooter
    compact={compactSurface}
    micro={microSurface}
    trackTitle={title}
    trackArtist={artist}
    activeSurface={activeSurface}
    isPinned={isPinned}
    {isPlaying}
    {currentTime}
    {duration}
    {volume}
    {isShuffle}
    {repeatMode}
    onTogglePlay={onTogglePlay}
    onSkipBack={onSkipBack}
    onSkipForward={onSkipForward}
    onSeek={onSeek}
    onVolumeChange={onVolumeChange}
    onToggleShuffle={onToggleShuffle}
    onToggleRepeat={onToggleRepeat}
    onSurfaceChange={onSurfaceChange}
    onTogglePin={onTogglePin}
    onExpand={onExpand}
    onClose={onClose}
    onStartDrag={onStartDrag}
    showWindowControls={windowHovered}
  />
</div>

<style>
  .mini-player-window {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg-primary);
    color: var(--text-primary);
    border: 1px solid var(--alpha-10);
    border-radius: 10px;
    overflow: hidden;
    box-shadow: 0 10px 34px rgba(0, 0, 0, 0.34);
  }

  .mini-player-window.compact {
    border-radius: 9px;
  }

  .mini-player-window.micro {
    border-radius: 9px;
  }

  .surface-area {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
</style>
