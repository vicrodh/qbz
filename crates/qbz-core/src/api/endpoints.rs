//! API endpoint definitions

pub const BASE_URL: &str = "https://www.qobuz.com/api.json/0.2";

/// Endpoint paths
pub mod paths {
    // User
    pub const USER_LOGIN: &str = "/user/login";

    // Track
    pub const TRACK_GET: &str = "/track/get";
    pub const TRACK_GET_LIST: &str = "/track/getList";
    pub const TRACK_SEARCH: &str = "/track/search";
    pub const TRACK_GET_FILE_URL: &str = "/track/getFileUrl";

    // Album
    pub const ALBUM_GET: &str = "/album/get";
    pub const ALBUM_SEARCH: &str = "/album/search";
    pub const ALBUM_GET_FEATURED: &str = "/album/getFeatured";

    // Artist
    pub const ARTIST_GET: &str = "/artist/get";
    pub const ARTIST_SEARCH: &str = "/artist/search";
    pub const ARTIST_GET_SIMILAR: &str = "/artist/getSimilarArtists";
    pub const ARTIST_PAGE: &str = "/artist/page";
    pub const ARTIST_RELEASES_GRID: &str = "/artist/getReleasesGrid";
    pub const ARTIST_RELEASES_LIST: &str = "/artist/getReleasesList";

    // Playlist
    pub const PLAYLIST_GET: &str = "/playlist/get";
    pub const PLAYLIST_SEARCH: &str = "/playlist/search";
    pub const PLAYLIST_GET_USER_PLAYLISTS: &str = "/playlist/getUserPlaylists";
    pub const PLAYLIST_CREATE: &str = "/playlist/create";
    pub const PLAYLIST_DELETE: &str = "/playlist/delete";
    pub const PLAYLIST_ADD_TRACKS: &str = "/playlist/addTracks";
    pub const PLAYLIST_DELETE_TRACKS: &str = "/playlist/deleteTracks";
    pub const PLAYLIST_UPDATE: &str = "/playlist/update";
    pub const PLAYLIST_GET_TAGS: &str = "/playlist/getTags";

    // Favorites
    pub const FAVORITE_GET_USER_FAVORITES: &str = "/favorite/getUserFavorites";
    pub const FAVORITE_CREATE: &str = "/favorite/create";
    pub const FAVORITE_DELETE: &str = "/favorite/delete";

    // Label
    pub const LABEL_GET: &str = "/label/get";

    // Genre
    pub const GENRE_LIST: &str = "/genre/list";

    // Catalog (combined search)
    pub const CATALOG_SEARCH: &str = "/catalog/search";

    // Discover (home page content)
    pub const DISCOVER_INDEX: &str = "/discover/index";
    pub const DISCOVER_PLAYLISTS: &str = "/discover/playlists";
    pub const DISCOVER_NEW_RELEASES: &str = "/discover/newReleases";
    pub const DISCOVER_IDEAL_DISCOGRAPHY: &str = "/discover/idealDiscography";
    pub const DISCOVER_MOST_STREAMED: &str = "/discover/mostStreamed";
    pub const DISCOVER_QOBUZISSIMS: &str = "/discover/qobuzissims";
    pub const DISCOVER_ALBUM_OF_THE_WEEK: &str = "/discover/albumOfTheWeek";
    pub const DISCOVER_PRESS_AWARD: &str = "/discover/pressAward";
}

/// Build full URL for an endpoint
pub fn build_url(endpoint: &str) -> String {
    format!("{}{}", BASE_URL, endpoint)
}
