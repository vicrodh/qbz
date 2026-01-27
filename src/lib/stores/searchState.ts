export type SearchTab = 'all' | 'albums' | 'tracks' | 'artists';

// Search filter types supported by Qobuz API
export type SearchFilterType = 'MainArtist' | 'Performer' | 'Composer' | 'Label' | 'ReleaseName' | null;

export interface SearchResults<T> {
  items: T[];
  total: number;
  offset: number;
  limit: number;
}

export interface SearchAllResults<Album, Track, Artist> {
  albums: SearchResults<Album>;
  tracks: SearchResults<Track>;
  artists: SearchResults<Artist>;
}

export interface SearchState<Album, Track, Artist> {
  query: string;
  activeTab: SearchTab;
  filterType: SearchFilterType;
  albumResults: SearchResults<Album> | null;
  trackResults: SearchResults<Track> | null;
  artistResults: SearchResults<Artist> | null;
  allResults: SearchAllResults<Album, Track, Artist> | null;
}

let searchState: SearchState<unknown, unknown, unknown> = {
  query: '',
  activeTab: 'all',
  filterType: null,
  albumResults: null,
  trackResults: null,
  artistResults: null,
  allResults: null,
};

export function getSearchState<Album, Track, Artist>(): SearchState<Album, Track, Artist> {
  return searchState as SearchState<Album, Track, Artist>;
}

export function setSearchState<Album, Track, Artist>(next: SearchState<Album, Track, Artist>): void {
  searchState = next as SearchState<unknown, unknown, unknown>;
}
