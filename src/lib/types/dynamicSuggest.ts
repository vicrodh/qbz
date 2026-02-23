export interface DynamicTrackToAnalyseInput {
  trackId: number;
  artistId: number;
  genreId: number;
  labelId: number;
}

export interface DynamicSuggestTrack {
  id: number;
  title: string;
  duration?: number;
  performer?: { id?: number; name: string };
  album?: {
    id?: string;
    title: string;
    image?: { small?: string; thumbnail?: string; large?: string };
  };
  hires?: boolean;
  maximum_bit_depth?: number;
  maximum_sampling_rate?: number;
  streamable?: boolean;
}

export interface DynamicSuggestResponse {
  algorithm: string;
  tracks: {
    offset: number;
    limit: number;
    total: number;
    items: DynamicSuggestTrack[];
  };
}

export interface DynamicSuggestRequest {
  limit?: number;
  listenedTrackIds?: number[];
  tracksToAnalyse?: DynamicTrackToAnalyseInput[];
}
