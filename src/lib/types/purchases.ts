export interface PurchaseResponse {
  albums: PaginatedList<PurchasedAlbum>;
  tracks: PaginatedList<PurchasedTrack>;
}

export interface PaginatedList<T> {
  offset: number;
  limit: number;
  total: number;
  items: T[];
}

export interface PurchasedAlbum {
  id: string;
  title: string;
  artist: { id: number; name: string };
  image: { small?: string; large?: string; thumbnail?: string };
  release_date_original?: string;
  label?: { id: number; name: string };
  genre?: { id: number; name: string };
  tracks_count?: number;
  duration?: number;
  hires: boolean;
  maximum_bit_depth?: number;
  maximum_sampling_rate?: number;
  downloadable: boolean;
  purchased_at?: string;
  tracks?: PaginatedList<PurchasedTrack>;
}

export interface PurchasedTrack {
  id: number;
  title: string;
  track_number: number;
  media_number?: number;
  duration: number;
  performer: { id: number; name: string };
  album?: {
    id: string;
    title: string;
    image: { small?: string; large?: string; thumbnail?: string };
  };
  hires: boolean;
  maximum_bit_depth?: number;
  maximum_sampling_rate?: number;
  streamable: boolean;
  purchased_at?: string;
}

export interface PurchaseDownloadProgress {
  albumId?: string;
  trackId?: number;
  trackIndex: number;
  totalTracks: number;
  bytesDownloaded: number;
  totalBytes?: number;
  status: 'queued' | 'downloading' | 'processing' | 'complete' | 'failed';
  error?: string;
}

export interface PurchaseFormatOption {
  id: number;
  label: string;
  bit_depth: number | null;
  sampling_rate: number | null;
}
