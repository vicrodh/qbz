import type { MiniPlayerSurface } from '$lib/stores/uiStore';

export type { MiniPlayerSurface };

export interface MiniPlayerQueueTrack {
  id: string;
  title: string;
  artist: string;
  artwork?: string;
  quality?: string;
}
