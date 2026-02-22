<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { ChevronUp } from 'lucide-svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { listen, emitTo, type UnlistenFn } from '@tauri-apps/api/event';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';

  // Console log capture (must be early, before other imports log)
  import { initConsoleCapture } from '$lib/stores/consoleLogStore';
  initConsoleCapture();

  // Offline cache state management
  import {
    initOfflineCacheStates,
    startOfflineCacheEventListeners,
    stopOfflineCacheEventListeners,
    cacheTrackForOffline,
    cacheTracksForOfflineBatch,
    removeCachedTrack,
    getOfflineCacheState,
    openAlbumFolder,
    openTrackFolder,
    subscribe as subscribeOfflineCache,
    type OfflineCacheStatus
  } from '$lib/stores/offlineCacheState';

  // Toast state management
  import {
    showToast,
    hideToast,
    subscribe as subscribeToast,
    type Toast as ToastData
  } from '$lib/stores/toastStore';

  // Search state for performer search
  import { setSearchState, triggerSearchFocus } from '$lib/stores/searchState';

  // Playback context and preferences
  import {
    initPlaybackContextStore,
    setPlaybackContext,
    clearPlaybackContext,
    getCurrentContext,
    requestContextTrackFocus,
    type ContextType
  } from '$lib/stores/playbackContextStore';
  import {
    initPlaybackPreferences,
    getCachedPreferences,
    isAutoplayEnabled
  } from '$lib/stores/playbackPreferencesStore';
  import { initBlacklistStore, isBlacklisted as isArtistBlacklisted } from '$lib/stores/artistBlacklistStore';

  // UI state management
  import {
    subscribe as subscribeUI,
    openQueue,
    closeQueue,
    toggleQueue,
    openFullScreen,
    closeFullScreen,
    toggleFullScreen,
    openFocusMode,
    closeFocusMode,
    toggleFocusMode,
    openCastPicker,
    closeCastPicker,
    openPlaylistModal,
    closePlaylistModal,
    openPlaylistImport,
    closePlaylistImport,
    handleEscapeKey as handleUIEscape,
    getUIState,
    type UIState
  } from '$lib/stores/uiStore';

  // Sidebar state management
  import {
    subscribe as subscribeSidebar,
    initSidebarStore,
    getIsExpanded,
    toggleSidebar
  } from '$lib/stores/sidebarStore';

  // Title bar state management
  import {
    subscribe as subscribeTitleBar,
    initTitleBarStore,
    shouldShowTitleBar
  } from '$lib/stores/titleBarStore';

  // Keybindings system
  import {
    registerAction,
    unregisterAll,
    handleKeydown as keybindingHandler
  } from '$lib/stores/keybindingsStore';

  // Auth state management
  import {
    subscribe as subscribeAuth,
    setLoggedIn,
    setLoggedOut,
    getAuthState,
    type UserInfo
  } from '$lib/stores/authStore';
  import { setStorageUserId, migrateLocalStorage, getUserItem, setUserItem } from '$lib/utils/userStorage';

  // Favorites state management
  import { loadFavorites } from '$lib/stores/favoritesStore';
  import { loadAlbumFavorites } from '$lib/stores/albumFavoritesStore';
  import { loadArtistFavorites } from '$lib/stores/artistFavoritesStore';
  import { getDefaultFavoritesTab } from '$lib/utils/favorites';
  import type { FavoritesPreferences, ResolvedMusician } from '$lib/types';

  // Navigation state management
  import {
    subscribe as subscribeNav,
    navigateTo as navTo,
    navigateToFavorites,
    goBack as navGoBack,
    goForward as navGoForward,
    selectPlaylist,
    getNavigationState,
    getFavoritesTabFromView,
    getSelectedLocalAlbumId,
    isFavoritesView,
    restoreView,
    setRestoredPlaylistId,
    setRestoredLocalAlbumId,
    type ViewType,
    type NavigationState,
    type FavoritesTab
  } from '$lib/stores/navigationStore';

  // Player state management
  import {
    subscribe as subscribePlayer,
    setCurrentTrack,
    setIsPlaying,
    setIsFavorite,
    setIsSkipping,
    setQueueEnded,
    setOnTrackEnded,
    setOnResumeFromStop,
    setGaplessGetNextTrackId,
    setOnGaplessTransition,
    togglePlay,
    seek as playerSeek,
    setVolume as playerSetVolume,
    stop as stopPlayback,
    setPendingSessionRestore,
    startPolling,
    stopPolling,
    reset as resetPlayer,
    getPlayerState,
    getVolume,
    resyncPersistedVolume,
    type PlayingTrack,
    type PlayerState
  } from '$lib/stores/playerStore';

  // Queue state management
  import {
    subscribe as subscribeQueue,
    syncQueueState,
    toggleShuffle as queueToggleShuffle,
    toggleRepeat as queueToggleRepeat,
    addToQueueNext,
    addToQueue,
    addTracksToQueue,
    setQueue,
    clearQueue,
    playQueueIndex,
    nextTrack,
    previousTrack,
    moveQueueTrack,
    setLocalTrackIds,
    clearLocalTrackIds,
    isLocalTrack,
    getBackendQueueState,
    getQueueState,
    setOfflineMode as setQueueOfflineMode,
    startQueueEventListener,
    stopQueueEventListener,
    type QueueTrack,
    type BackendQueueTrack,
    type RepeatMode
  } from '$lib/stores/queueStore';

  type MediaControlPayload = {
    action: string;
    direction?: 'forward' | 'backward';
    offset_secs?: number;
    position_secs?: number;
    volume?: number;
  };

  const MEDIA_SEEK_FALLBACK_SECS = 10;

  // Types
  import type {
    QobuzTrack,
    QobuzAlbum,
    Track,
    AlbumDetail,
    ArtistDetail,
    LabelDetail,
    PlaylistTrack,
    DisplayTrack,
    LocalLibraryTrack,
    SongLinkResponse,
    PageArtistResponse,
    PageArtistTrack,
    PageArtistSimilarItem,
    ReleasesGridResponse
  } from '$lib/types';

  // Adapters
  import {
    convertQobuzAlbum,
    convertPageArtist,
    appendPageReleases,
    formatDuration
  } from '$lib/adapters/qobuzAdapters';

  // Services
  import {
    playTrack,
    checkTrackFavorite,
    toggleTrackFavorite,
    loadSystemNotificationsPreference,
    showTrackNotification,
    updateLastfmNowPlaying,
    cleanup as cleanupPlayback
  } from '$lib/services/playbackService';

  import {
    queueTrackNext,
    queueTrackLater,
    buildQueueTrackFromQobuz,
    buildQueueTrackFromAlbumTrack,
    buildQueueTrackFromPlaylistTrack,
    buildQueueTrackFromLocalTrack,
    queueQobuzTrackNext,
    queueQobuzTrackLater,
    queuePlaylistTrackNext,
    queuePlaylistTrackLater,
    queueLocalTrackNext,
    queueLocalTrackLater,
    queueDisplayTrackNext,
    queueDisplayTrackLater,
    handleAddToFavorites,
    addToPlaylist,
    shareQobuzTrackLink,
    shareSonglinkTrack
  } from '$lib/services/trackActions';

  // Internationalization
  import { t } from '$lib/i18n';

  // App bootstrap
  import { bootstrapApp, restoreLastfmSession } from '$lib/app/bootstrap';

  // Recommendation scoring
  import { trainScores } from '$lib/services/recoService';

  // Session persistence
  import {
    loadSessionState,
    saveSessionState,
    saveSessionPlaybackMode,
    debouncedSavePosition,
    flushPositionSave,
    clearSession,
    type PersistedQueueTrack,
    type PersistedSession
  } from '$lib/services/sessionService';

  // MiniPlayer - DISABLED: incomplete feature, re-enable when ready
  // import { enterMiniplayerMode } from '$lib/services/miniplayerService';

  // Sidebar mutual exclusion
  import { closeContentSidebar, subscribeContentSidebar, type ContentSidebarType } from '$lib/stores/sidebarStore';

  // Lyrics state management
  import {
    subscribe as subscribeLyrics,
    toggleSidebar as toggleLyricsSidebar,
    hideSidebar as hideLyricsSidebar,
    startWatching as startLyricsWatching,
    stopWatching as stopLyricsWatching,
    startActiveLineUpdates,
    stopActiveLineUpdates,
    getLyricsState,
    type LyricsLine
  } from '$lib/stores/lyricsStore';

  // Cast state management
  import {
    subscribe as subscribeCast,
    getCastState,
    isCasting,
    setOnAskContinueLocally
  } from '$lib/stores/castStore';

  // Components
  import TitleBar from '$lib/components/TitleBar.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import AboutModal from '$lib/components/AboutModal.svelte';
  import NowPlayingBar from '$lib/components/NowPlayingBar.svelte';
  import Toast from '$lib/components/Toast.svelte';

  // Views
  import LoginView from '$lib/components/views/LoginView.svelte';
  import HomeView from '$lib/components/views/HomeView.svelte';
  import SearchView from '$lib/components/views/SearchView.svelte';
  import SettingsView from '$lib/components/views/SettingsView.svelte';
  import AlbumDetailView from '$lib/components/views/AlbumDetailView.svelte';
  import ArtistDetailView from '$lib/components/views/ArtistDetailView.svelte';
  import MusicianPageView from '$lib/components/views/MusicianPageView.svelte';
  import LabelView from '$lib/components/views/LabelView.svelte';
  import LabelReleasesView from '$lib/components/views/LabelReleasesView.svelte';
  import PlaylistDetailView from '$lib/components/views/PlaylistDetailView.svelte';
  import FavoritesView from '$lib/components/views/FavoritesView.svelte';
  import LocalLibraryView from '$lib/components/views/LocalLibraryView.svelte';
  import PlaylistManagerView from '$lib/components/views/PlaylistManagerView.svelte';
  import BlacklistManagerView from '$lib/components/views/BlacklistManagerView.svelte';
  import DiscoverBrowseView from '$lib/components/views/DiscoverBrowseView.svelte';
  import DiscoverPlaylistsBrowseView from '$lib/components/views/DiscoverPlaylistsBrowseView.svelte';
  import PurchasesView from '$lib/components/views/PurchasesView.svelte';
  import PurchaseAlbumDetailView from '$lib/components/views/PurchaseAlbumDetailView.svelte';
  import DynamicSuggestView from '$lib/components/views/DynamicSuggestView.svelte';
  import WeeklySuggestView from '$lib/components/views/WeeklySuggestView.svelte';
  import FavQView from '$lib/components/views/FavQView.svelte';
  import TopQView from '$lib/components/views/TopQView.svelte';

  // Overlays
  import QueuePanel from '$lib/components/QueuePanel.svelte';
  import { ImmersivePlayer } from '$lib/components/immersive';
  import PlaylistModal from '$lib/components/PlaylistModal.svelte';
  import PlaylistImportModal from '$lib/components/PlaylistImportModal.svelte';
  import TrackInfoModal from '$lib/components/TrackInfoModal.svelte';
  import AlbumCreditsModal from '$lib/components/AlbumCreditsModal.svelte';
  import MusicianModal from '$lib/components/MusicianModal.svelte';
  import CastPicker from '$lib/components/CastPicker.svelte';
  import LyricsSidebar from '$lib/components/lyrics/LyricsSidebar.svelte';
  import OfflinePlaceholder from '$lib/components/OfflinePlaceholder.svelte';
  import UpdateAvailableModal from '$lib/components/updates/UpdateAvailableModal.svelte';
  import UpdateReminderModal from '$lib/components/updates/UpdateReminderModal.svelte';
  import WhatsNewModal from '$lib/components/updates/WhatsNewModal.svelte';
  import FlatpakWelcomeModal from '$lib/components/updates/FlatpakWelcomeModal.svelte';
  import KeyboardShortcutsModal from '$lib/components/KeyboardShortcutsModal.svelte';
  import KeybindingsSettings from '$lib/components/KeybindingsSettings.svelte';
  import LinkResolverModal from '$lib/components/LinkResolverModal.svelte';
  import type { ReleaseInfo } from '$lib/stores/updatesStore';
  import { refreshUpdatePreferences, resetUpdatesStore } from '$lib/stores/updatesStore';
  import { getShowPurchases, setShowPurchases } from '$lib/stores/purchasesStore';
  import {
    decideLaunchModals,
    disableUpdateChecks,
    ignoreReleaseVersion,
    markFlatpakWelcomeShown,
    openReleasePageAndAcknowledge,
    resetLaunchFlow,
  } from '$lib/services/updatesService';

  // Offline state
  import {
    subscribe as subscribeOffline,
    getStatus as getOfflineStatus,
    isOffline as checkIsOffline,
    getOfflineReason,
    setManualOffline,
    refreshStatus as refreshOfflineStatus,
    type OfflineStatus
  } from '$lib/stores/offlineStore';

  // Auth State (from authStore subscription)
  let isLoggedIn = $state(false);
  let userInfo = $state<UserInfo | null>(null);

  // Offline State (from offlineStore subscription)
  let offlineStatus = $state<OfflineStatus>(getOfflineStatus());

  // Sidebar State (from sidebarStore subscription)
  let sidebarExpanded = $state(getIsExpanded());

  // Purchases visibility
  let showPurchases = $state(getShowPurchases());

  // Title Bar State (from titleBarStore subscription)
  let showTitleBar = $state(shouldShowTitleBar());

  // View State (from navigationStore subscription)
  let activeView = $state<ViewType>('home');
  let selectedPlaylistId = $state<number | null>(null);
  let updatesCurrentVersion = $state('');
  let updateRelease = $state<ReleaseInfo | null>(null);
  let whatsNewRelease = $state<ReleaseInfo | null>(null);
  let isUpdateModalOpen = $state(false);
  let isReminderModalOpen = $state(false);
  let isWhatsNewModalOpen = $state(false);
  let isFlatpakWelcomeOpen = $state(false);
  let updatesLaunchTriggered = $state(false);
  let sessionReady = $state(false);

  // Sequential modal queue: Flatpak → What's new → Update available
  let pendingWhatsNewRelease = $state<ReleaseInfo | null>(null);
  let pendingUpdateRelease = $state<ReleaseInfo | null>(null);

  // Global back-to-top button
  let mainContentEl: HTMLElement | null = $state(null);
  let globalScrollTop = $state(0);
  let activeScrollTarget: HTMLElement | null = null;
  const showGlobalBackToTop = $derived(globalScrollTop > 800);

  $effect(() => {
    if (!mainContentEl) return;
    const handler = (e: Event) => {
      const target = e.target as HTMLElement;
      if (target !== mainContentEl) {
        globalScrollTop = target.scrollTop;
        activeScrollTarget = target;
      }
    };
    mainContentEl.addEventListener('scroll', handler, true);
    return () => mainContentEl!.removeEventListener('scroll', handler, true);
  });

  // Reset scroll state on view change
  $effect(() => {
    void activeView;
    globalScrollTop = 0;
    activeScrollTarget = null;
  });

  function globalScrollToTop() {
    activeScrollTarget?.scrollTo({ top: 0, behavior: 'smooth' });
  }

  // Album, Artist and Label data are fetched, so kept local
  let selectedAlbum = $state<AlbumDetail | null>(null);
  let selectedArtist = $state<ArtistDetail | null>(null);
  let artistTopTracks = $state<PageArtistTrack[]>([]);
  let artistSimilarArtists = $state<PageArtistSimilarItem[]>([]);
  let selectedLabel = $state<{ id: number; name: string } | null>(null);
  let selectedMusician = $state<ResolvedMusician | null>(null);
  let musicianModalData = $state<ResolvedMusician | null>(null);
  let isArtistAlbumsLoading = $state(false);

  // Purchase downloads state
  let selectedPurchaseAlbumId = $state<string | null>(null);

  function handlePurchaseAlbumClick(albumId: string) {
    selectedPurchaseAlbumId = albumId;
    navigateTo('purchase-album');
  }

  function waitForHomePaint(): Promise<void> {
    if (typeof window === 'undefined') return Promise.resolve();
    return new Promise((resolve) => {
      requestAnimationFrame(() => {
        requestAnimationFrame(() => resolve());
      });
    });
  }

  async function runLaunchUpdateFlow(): Promise<void> {
    // Ensure the UI has rendered and Home is visible before showing any modal.
    await tick();
    await waitForHomePaint();
    if (activeView !== 'home') return;

    const decision = await decideLaunchModals();
    updatesCurrentVersion = decision.currentVersion;

    // Store pending modals for sequential display
    // Order: Flatpak → What's new → Update available
    pendingWhatsNewRelease = decision.whatsNewRelease;
    pendingUpdateRelease = decision.updateRelease;

    // Show first modal in queue (Flatpak has highest priority)
    if (decision.showFlatpakWelcome) {
      isFlatpakWelcomeOpen = true;
      return;
    }

    // No Flatpak modal, try What's New
    showNextModalInQueue();
  }

  function showNextModalInQueue(): void {
    // What's New has second priority
    if (pendingWhatsNewRelease) {
      whatsNewRelease = pendingWhatsNewRelease;
      pendingWhatsNewRelease = null;
      isWhatsNewModalOpen = true;
      return;
    }

    // Update Available has lowest priority
    if (pendingUpdateRelease) {
      updateRelease = pendingUpdateRelease;
      pendingUpdateRelease = null;
      isUpdateModalOpen = true;
    }
  }

  function handleUpdateVisit(): void {
    if (!updateRelease) return;
    void openReleasePageAndAcknowledge(updateRelease);
    isUpdateModalOpen = false;
    updateRelease = null;
  }

  function handleUpdateClose(): void {
    isUpdateModalOpen = false;
    if (updateRelease) {
      isReminderModalOpen = true;
    }
  }

  function handleReminderClose(): void {
    isReminderModalOpen = false;
    updateRelease = null;
  }

  function handleReminderLater(): void {
    // No persistence by design.
  }

  function handleReminderIgnoreRelease(): void {
    if (!updateRelease) return;
    void ignoreReleaseVersion(updateRelease.version);
  }

  function handleReminderDisableUpdates(): void {
    void disableUpdateChecks();
  }

  function handleFlatpakWelcomeClose(): void {
    isFlatpakWelcomeOpen = false;
    void markFlatpakWelcomeShown();
    // Show next modal in queue
    showNextModalInQueue();
  }

  function handleWhatsNewClose(): void {
    isWhatsNewModalOpen = false;
    whatsNewRelease = null;
    // Show next modal in queue
    showNextModalInQueue();
  }

  $effect(() => {
    if (updatesLaunchTriggered) return;
    if (activeView !== 'home') return;
    if (!sessionReady) return; // Wait for activate_user_session to complete
    updatesLaunchTriggered = true;
    void runLaunchUpdateFlow();
  });

  // Artist albums for "By the same artist" section in album view
  let albumArtistAlbums = $state<{ id: string; title: string; artwork: string; quality: string; genre: string; releaseDate?: string }[]>([]);

  // Overlay States (from uiStore subscription)
  let isQueueOpen = $state(false);
  let isFullScreenOpen = $state(false);
  let isFocusModeOpen = $state(false);
  let isCastPickerOpen = $state(false);

  // Cast State
  let isCastConnected = $state(false);

  // Playlist Modal State (from uiStore subscription)
  let isPlaylistModalOpen = $state(false);
  let playlistModalMode = $state<'create' | 'edit' | 'addTrack'>('create');
  let playlistModalTrackIds = $state<number[]>([]);
  let playlistModalTracksAreLocal = $state(false);
  let isPlaylistImportOpen = $state(false);
  let isAboutModalOpen = $state(false);
  let isShortcutsModalOpen = $state(false);
  let isKeybindingsSettingsOpen = $state(false);
  let isLinkResolverOpen = $state(false);

  // Track Info Modal State
  let isTrackInfoOpen = $state(false);
  let trackInfoTrackId = $state<number | null>(null);
  let userPlaylists = $state<{ id: number; name: string; tracks_count: number }[]>([]);

  // Album Credits Modal State
  let isAlbumCreditsOpen = $state(false);
  let albumCreditsAlbumId = $state<string | null>(null);
  
  // Sidebar reference for refreshing playlists and search
  let sidebarRef = $state<{
    getPlaylists: () => { id: number; name: string; tracks_count: number }[];
    refreshPlaylists: () => void;
    refreshPlaylistSettings: () => void;
    refreshLocalTrackCounts: () => void;
    updatePlaylistCounts: (playlistId: number, qobuzCount: number, localCount: number) => void;
    focusSearch: () => void;
  } | undefined>(undefined);

  // Playback State (from playerStore subscription)
  let currentTrack = $state<PlayingTrack | null>(null);
  let isPlaying = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let volume = $state(getVolume()); // Load persisted volume from localStorage
  let isFavorite = $state(false);
  let normalizationEnabled = $state(false);
  let normalizationGain = $state<number | null>(null);
  // Queue/Shuffle State (from queueStore subscription)
  let isShuffle = $state(false);
  let repeatMode = $state<RepeatMode>('off');
  let queue = $state<QueueTrack[]>([]);
  let queueTotalTracks = $state(0);
  let queueRemainingTracks = $state(0); // Actual remaining tracks (total - current_index - 1)
  let historyTracks = $state<QueueTrack[]>([]);
  let infinitePlayEnabled = $state(false);

  // Toast State (from store subscription)
  let toast = $state<ToastData | null>(null);

  // Lyrics State (from lyricsStore subscription)
  let lyricsStatus = $state<'idle' | 'loading' | 'loaded' | 'error' | 'not_found'>('idle');
  let lyricsError = $state<string | null>(null);
  let lyricsLines = $state<LyricsLine[]>([]);
  let lyricsIsSynced = $state(false);
  let lyricsActiveIndex = $state(-1);
  let lyricsActiveProgress = $state(0);
  let lyricsSidebarVisible = $state(false);

  let favoritesDefaultTab = $state<FavoritesTab>('tracks');

  async function loadFavoritesDefaultTab(): Promise<void> {
    try {
      const prefs = await invoke<FavoritesPreferences>('v2_get_favorites_preferences');
      favoritesDefaultTab = getDefaultFavoritesTab(prefs.tab_order);
    } catch (err) {
      console.error('Failed to load favorites preferences:', err);
      favoritesDefaultTab = 'tracks';
    }
  }

  // Navigation wrapper (keeps debug logging)
  async function navigateTo(view: string) {
    console.log('navigateTo called with:', view, 'current activeView:', activeView);
    if (view === 'favorites') {
      await loadFavoritesDefaultTab();
      navigateToFavorites(favoritesDefaultTab);
      return;
    }
    // If already on search, trigger scroll to top and focus
    if (view === 'search' && activeView === 'search') {
      triggerSearchFocus();
      return;
    }
    navTo(view as ViewType);
  }

  async function handleAlbumClick(albumId: string) {
    try {
      showToast($t('toast.loadingAlbum'), 'info');
      const album = await invoke<QobuzAlbum>('v2_get_album', { albumId });

      const converted = convertQobuzAlbum(album);

      if (!converted || !converted.id) {
        console.error('convertQobuzAlbum returned invalid data:', converted);
        showToast($t('toast.failedLoadAlbum'), 'error');
        return;
      }

      selectedAlbum = converted;
      navigateTo('album');
      hideToast();

      // Fetch artist albums for "By the same artist" section (non-blocking)
      if (album.artist?.id) {
        fetchAlbumArtistAlbums(album.artist.id);
      } else {
        albumArtistAlbums = [];
      }
    } catch (err) {
      console.error('Failed to load album:', err);
      showToast($t('toast.failedLoadAlbum'), 'error');
    }
  }

  /**
   * Fetch artist albums for the "By the same artist" section
   * Only includes studio albums and live albums
   */
  async function fetchAlbumArtistAlbums(artistId: number) {
    try {
      const response = await invoke<PageArtistResponse>('v2_get_artist_page', { artistId });
      const artistDetail = convertPageArtist(response);

      // Combine studio albums and live albums, limit to 16
      const combined = [
        ...artistDetail.albums.map(a => ({
          id: a.id,
          title: a.title,
          artwork: a.artwork,
          quality: a.quality,
          genre: a.genre,
          releaseDate: a.releaseDate
        })),
        ...artistDetail.liveAlbums.map(a => ({
          id: a.id,
          title: a.title,
          artwork: a.artwork,
          quality: a.quality,
          genre: a.genre,
          releaseDate: a.releaseDate
        }))
      ].slice(0, 16);

      albumArtistAlbums = combined;
    } catch (err) {
      console.error('Failed to fetch artist albums for "By the same artist":', err);
      albumArtistAlbums = [];
    }
  }

  /**
   * Navigate to artist view and scroll to Discography section
   */
  function handleViewArtistDiscography() {
    if (selectedAlbum?.artistId) {
      // Store scroll target for artist view
      const artistId = selectedAlbum.artistId;
      handleArtistClick(artistId).then(() => {
        // Use setTimeout to allow the view to render before scrolling
        setTimeout(() => {
          const discographySection = document.querySelector('.artist-section');
          if (discographySection) {
            discographySection.scrollIntoView({ behavior: 'smooth', block: 'start' });
          }
        }, 100);
      });
    }
  }


  async function handleArtistClick(artistId: number) {
    try {
      showToast($t('toast.loadingArtist'), 'info');
      const response = await invoke<PageArtistResponse>('v2_get_artist_page', { artistId });
      console.log('Artist page:', response);

      selectedArtist = convertPageArtist(response);
      artistTopTracks = response.top_tracks || [];
      artistSimilarArtists = response.similar_artists?.items || [];
      navigateTo('artist');
      hideToast();
    } catch (err) {
      console.error('Failed to load artist:', err);
      showToast($t('toast.failedLoadArtist'), 'error');
    }
  }

  /**
   * Handle a resolved Qobuz link (from modal or OS scheme handler).
   */
  async function handleResolvedLink(resolved: { type: string; id: string | number }) {
    try {
      switch (resolved.type) {
        case 'OpenAlbum':
          await handleAlbumClick(String(resolved.id));
          break;
        case 'OpenTrack': {
          // Fetch track to get its album, then navigate there
          const track = await invoke<unknown>('v2_get_track', { trackId: Number(resolved.id) });
          const data = track as Record<string, unknown> | null;
          const nestedAlbum = (data?.album as Record<string, unknown> | undefined)?.id;
          const albumId = nestedAlbum ?? data?.album_id ?? data?.albumId;
          if (albumId !== undefined && albumId !== null) {
            await handleAlbumClick(String(albumId));
          }
          break;
        }
        case 'OpenArtist':
          await handleArtistClick(Number(resolved.id));
          break;
        case 'OpenPlaylist':
          selectPlaylist(Number(resolved.id));
          break;
        default:
          console.warn('Unknown resolved link type:', resolved.type);
      }
    } catch (err) {
      console.error('Failed to handle resolved link:', err);
      showToast($t('linkResolver.invalidLink'), 'error');
    }
  }

  function handleLabelClick(labelId: number, labelName?: string) {
    selectedLabel = { id: labelId, name: labelName || '' };
    navigateTo('label');
  }

  function handleNavigateLabelReleases(labelId: number, labelName: string) {
    selectedLabel = { id: labelId, name: labelName };
    navigateTo('label-releases');
  }

  /**
   * Handle musician click from credits
   * Resolves musician and routes based on confidence level:
   * - Confirmed (3): Navigate to Qobuz Artist Page
   * - Contextual (2): Navigate to Musician Page
   * - Weak (1), None (0): Show Informational Modal
   */
  async function handleMusicianClick(name: string, role: string) {
    showToast($t('toast.lookingUp', { values: { name } }), 'info');
    try {
      const musician = await invoke<ResolvedMusician>('v2_resolve_musician', { name, role });
      console.log('Resolved musician:', musician);

      switch (musician.confidence) {
        case 'confirmed':
          // Has a Qobuz artist page - navigate there
          if (musician.qobuz_artist_id) {
            handleArtistClick(musician.qobuz_artist_id);
          } else {
            // Fallback: show modal
            musicianModalData = musician;
          }
          break;

        case 'contextual':
          // Show full Musician Page
          selectedMusician = musician;
          navigateTo('musician');
          break;

        case 'weak':
        case 'none':
        default:
          // Show Informational Modal only
          musicianModalData = musician;
          break;
      }
    } catch (err) {
      console.error('Failed to resolve musician:', err);
      showToast($t('toast.failedLookupMusician'), 'error');
      // Fallback: open modal with basic info
      musicianModalData = {
        name,
        role,
        confidence: 'none',
        bands: [],
        appears_on_count: 0
      };
    }
  }

  function closeMusicianModal() {
    musicianModalData = null;
  }

  /**
   * Search for a performer by name (from track credits)
   */
  function searchForPerformer(name: string) {
    // Set search state with performer name, clear previous results to trigger auto-search
    setSearchState({
      query: name,
      activeTab: 'all',
      filterType: null,
      albumResults: null,
      trackResults: null,
      artistResults: null,
      playlistResults: null,
      allResults: null
    });
    navigateTo('search');
  }

  /**
   * Navigate to the source of current playback context
   */
  async function handleContextNavigation() {
    const context = getCurrentContext();
    if (!context) {
      console.log('[ContextNav] No context available');
      return;
    }

    console.log('[ContextNav] Navigating to:', context);

    const focusTrackId = currentTrack?.id;
    const requestFocus = (contextType: typeof context.type, contextId: string) => {
      if (typeof focusTrackId === 'number') {
        requestContextTrackFocus(contextType, contextId, focusTrackId);
      }
    };

    try {
      switch (context.type) {
        case 'album':
          // Navigate to album page
          requestFocus('album', context.id);
          await handleAlbumClick(context.id);
          break;

        case 'playlist':
          // Navigate to playlist page
          const playlistId = parseInt(context.id);
          if (!isNaN(playlistId)) {
            requestFocus('playlist', context.id);
            selectedPlaylistId = playlistId;
            navigateTo('playlist');
          }
          break;

        case 'artist_top':
          // Navigate to artist page
          const artistId = parseInt(context.id);
          if (!isNaN(artistId)) {
            requestFocus('artist_top', context.id);
            await handleArtistClick(artistId);
          }
          break;

        case 'favorites':
          // Navigate to favorites page
          requestFocus('favorites', 'favorites');
          navigateToFavorites('tracks');
          break;

        case 'home_list':
          // Navigate to home page
          navigateTo('home');
          break;

        case 'search':
          // Navigate to search (could restore query if needed)
          navigateTo('search');
          break;

        case 'daily_q':
          navigateTo('dailyq');
          break;

        case 'weekly_q':
          navigateTo('weeklyq');
          break;

        case 'fav_q':
          navigateTo('favq');
          break;

        case 'top_q':
          navigateTo('topq');
          break;

        case 'radio':
          // Radio is dynamic/endless - no specific page to navigate to
          console.log('[ContextNav] Radio is currently playing');
          break;

        default:
          console.warn('[ContextNav] Unknown context type:', context.type);
      }
    } catch (err) {
      console.error('[ContextNav] Navigation failed:', err);
      showToast($t('toast.failedNavigateSource'), 'error');
    }
  }

  async function loadMoreArtistReleases(releaseType: string) {
    if (!selectedArtist || isArtistAlbumsLoading) return;

    // Count current items for this release type to use as offset
    let currentCount = 0;
    switch (releaseType) {
      case 'album': currentCount = selectedArtist.albums.length; break;
      case 'ep': case 'single': currentCount = selectedArtist.epsSingles.length; break;
      case 'live': currentCount = selectedArtist.liveAlbums.length; break;
      case 'compilation': case 'other': currentCount = selectedArtist.others.length; break;
    }

    isArtistAlbumsLoading = true;
    try {
      const result = await invoke<ReleasesGridResponse>('get_releases_grid', {
        artistId: selectedArtist.id,
        releaseType,
        limit: 25,
        offset: currentCount
      });

      if (result.items.length === 0) return;

      selectedArtist = appendPageReleases(
        selectedArtist,
        releaseType,
        result.items,
        result.has_more
      );
    } catch (err) {
      console.error(`Failed to load more ${releaseType} releases:`, err);
      showToast($t('toast.failedLoadMoreAlbums'), 'error');
    } finally {
      isArtistAlbumsLoading = false;
    }
  }


  // Album-specific queue track builder (needs selectedAlbum context)
  function buildAlbumQueueTrack(track: Track): BackendQueueTrack {
    return buildQueueTrackFromAlbumTrack(
      track,
      selectedAlbum?.artwork || '',
      selectedAlbum?.artist || 'Unknown Artist',
      selectedAlbum?.title || '',
      selectedAlbum?.id,
      selectedAlbum?.artistId
    );
  }

  async function fetchAlbumDetail(albumId: string): Promise<AlbumDetail | null> {
    try {
      const album = await invoke<QobuzAlbum>('v2_get_album', { albumId });
      return convertQobuzAlbum(album);
    } catch (err) {
      console.error('Failed to load album:', err);
      showToast($t('toast.failedLoadAlbum'), 'error');
      return null;
    }
  }

  async function playAlbumById(albumId: string) {
    const album = await fetchAlbumDetail(albumId);
    if (!album?.tracks?.length) return;

    const artwork = album.artwork || '';
    const queueTracks: BackendQueueTrack[] = album.tracks.map(trk => ({
      id: trk.id,
      title: trk.title,
      artist: trk.artist || album.artist || 'Unknown Artist',
      album: album.title || '',
      duration_secs: trk.durationSeconds,
      artwork_url: artwork || null,
      hires: trk.hires ?? false,
      bit_depth: trk.bitDepth ?? null,
      sample_rate: trk.samplingRate ?? null,
      is_local: false,
      album_id: album.id,
      artist_id: trk.artistId ?? album.artistId
    }));

    await setQueue(queueTracks, 0, true);
    const firstTrack = album.tracks[0];
    const quality = firstTrack.hires && firstTrack.bitDepth && firstTrack.samplingRate
      ? `${firstTrack.bitDepth}bit/${firstTrack.samplingRate}kHz`
      : firstTrack.hires
        ? 'Hi-Res'
        : '-';

    await playTrack({
      id: firstTrack.id,
      title: firstTrack.title,
      artist: firstTrack.artist || album.artist || 'Unknown Artist',
      album: album.title || '',
      artwork,
      duration: firstTrack.durationSeconds,
      quality,
      bitDepth: firstTrack.bitDepth,
      samplingRate: firstTrack.samplingRate,
      albumId: album.id,
      artistId: firstTrack.artistId
    });
  }

  async function queueAlbumNextById(albumId: string) {
    const album = await fetchAlbumDetail(albumId);
    if (!album?.tracks?.length) return;

    const artwork = album.artwork || '';
    for (let i = album.tracks.length - 1; i >= 0; i--) {
      const trk = album.tracks[i];
      queueTrackNext({
        id: trk.id,
        title: trk.title,
        artist: trk.artist || album.artist || 'Unknown Artist',
        album: album.title || '',
        duration_secs: trk.durationSeconds,
        artwork_url: artwork || null,
        hires: trk.hires ?? false,
        bit_depth: trk.bitDepth ?? null,
        sample_rate: trk.samplingRate ?? null,
        is_local: false,
        album_id: album.id,
        artist_id: trk.artistId ?? album.artistId
      });
    }
    showToast($t('toast.playingTracksNext', { values: { count: album.tracks.length } }), 'success');
  }

  async function queueAlbumLaterById(albumId: string) {
    const album = await fetchAlbumDetail(albumId);
    if (!album?.tracks?.length) return;

    const artwork = album.artwork || '';
    const queueTracks: BackendQueueTrack[] = album.tracks.map(trk => ({
      id: trk.id,
      title: trk.title,
      artist: trk.artist || album.artist || 'Unknown Artist',
      album: album.title || '',
      duration_secs: trk.durationSeconds,
      artwork_url: artwork || null,
      hires: trk.hires ?? false,
      bit_depth: trk.bitDepth ?? null,
      sample_rate: trk.samplingRate ?? null,
      is_local: false,
      album_id: album.id,
      artist_id: trk.artistId ?? album.artistId
    }));

    const success = await addTracksToQueue(queueTracks);
    if (success) {
      showToast($t('toast.addedTracksToQueue', { values: { count: queueTracks.length } }), 'success');
    } else {
      showToast($t('toast.failedAddToQueue'), 'error');
    }
  }

  function shareAlbumQobuzLinkById(albumId: string) {
    const url = `https://play.qobuz.com/album/${albumId}`;
    writeText(url);
    showToast($t('toast.albumLinkCopied'), 'success');
  }

  async function shareAlbumSonglinkById(albumId: string) {
    try {
      showToast($t('toast.fetchingAlbumLink'), 'info');
      const album = await fetchAlbumDetail(albumId);
      if (!album) {
        showToast($t('toast.couldNotFetchDetails'), 'error');
        return;
      }
      const response = await invoke<{ pageUrl: string }>('v2_share_album_songlink', {
        upc: album.upc || null,
        albumId: album.id,
        title: album.title,
        artist: album.artist
      });
      writeText(response.pageUrl);
      showToast($t('toast.albumLinkCopiedSonglink'), 'success');
    } catch (err) {
      console.error('Failed to get Album.link:', err);
      showToast($t('toast.albumLinkError', { values: { error: String(err) } }), 'error');
    }
  }

  async function downloadAlbumById(albumId: string) {
    const album = await fetchAlbumDetail(albumId);
    if (!album) return;

    const tracksToDownload = album.tracks.filter(track => {
      const status = getOfflineCacheState(track.id).status;
      return status === 'none' || status === 'failed';
    });

    if (tracksToDownload.length === 0) {
      showToast($t('toast.allTracksOffline'), 'info');
      return;
    }

    showToast($t('toast.preparingTracksOffline', { values: { count: tracksToDownload.length, album: album.title } }), 'info');

    try {
      await cacheTracksForOfflineBatch(tracksToDownload.map(track => ({
        id: track.id,
        title: track.title,
        artist: track.artist || album.artist || 'Unknown',
        album: album.title,
        albumId: album.id,
        durationSecs: track.durationSeconds,
        quality: track.quality || '-',
        bitDepth: track.bitDepth,
        sampleRate: track.samplingRate,
      })));
    } catch (err) {
      console.error('Failed to batch queue downloads:', err);
    }
  }

  // ============ Playlist Handler Functions (for Search) ============

  interface PlaylistData {
    id: number;
    name: string;
    owner: { id: number; name: string };
    images?: string[];
    tracks_count: number;
    duration: number;
    tracks?: {
      items: Array<{
        id: number;
        title: string;
        duration: number;
        performer?: { id?: number; name: string };
        album?: {
          id: string;
          title: string;
          image?: { small?: string; thumbnail?: string; large?: string };
        };
        hires_streamable?: boolean;
        maximum_bit_depth?: number;
        maximum_sampling_rate?: number;
      }>;
    };
  }

  async function fetchPlaylistData(playlistId: number): Promise<PlaylistData | null> {
    try {
      const playlist = await invoke<PlaylistData>('v2_get_playlist', { playlistId });
      return playlist;
    } catch (err) {
      console.error('Failed to load playlist:', err);
      showToast($t('toast.failedLoadPlaylist'), 'error');
      return null;
    }
  }

  async function playPlaylistById(playlistId: number) {
    const playlist = await fetchPlaylistData(playlistId);
    if (!playlist?.tracks?.items?.length) {
      showToast($t('toast.playlistNoTracks'), 'info');
      return;
    }

    const tracks = playlist.tracks.items;
    const queueTracks: BackendQueueTrack[] = tracks.map(trk => ({
      id: trk.id,
      title: trk.title,
      artist: trk.performer?.name || 'Unknown Artist',
      album: trk.album?.title || '',
      duration_secs: trk.duration,
      artwork_url: trk.album?.image?.large || trk.album?.image?.thumbnail || trk.album?.image?.small || null,
      hires: trk.hires_streamable ?? false,
      bit_depth: trk.maximum_bit_depth ?? null,
      sample_rate: trk.maximum_sampling_rate ?? null,
      is_local: false,
      album_id: trk.album?.id,
      artist_id: trk.performer?.id
    }));

    await setQueue(queueTracks, 0);

    const firstTrack = tracks[0];
    const artwork = firstTrack.album?.image?.large || firstTrack.album?.image?.thumbnail || firstTrack.album?.image?.small || '';
    const quality = firstTrack.hires_streamable && firstTrack.maximum_bit_depth && firstTrack.maximum_sampling_rate
      ? `${firstTrack.maximum_bit_depth}bit/${firstTrack.maximum_sampling_rate}kHz`
      : firstTrack.hires_streamable
        ? 'Hi-Res'
        : '-';

    await playTrack({
      id: firstTrack.id,
      title: firstTrack.title,
      artist: firstTrack.performer?.name || 'Unknown Artist',
      album: firstTrack.album?.title || '',
      artwork,
      duration: firstTrack.duration,
      quality,
      bitDepth: firstTrack.maximum_bit_depth,
      samplingRate: firstTrack.maximum_sampling_rate,
      albumId: firstTrack.album?.id,
      artistId: firstTrack.performer?.id
    });
  }

  async function queuePlaylistNextById(playlistId: number) {
    const playlist = await fetchPlaylistData(playlistId);
    if (!playlist?.tracks?.items?.length) {
      showToast($t('toast.playlistNoTracks'), 'info');
      return;
    }

    const tracks = playlist.tracks.items;
    // Add in reverse order so they play in correct sequence
    for (let i = tracks.length - 1; i >= 0; i--) {
      const trk = tracks[i];
      queueTrackNext({
        id: trk.id,
        title: trk.title,
        artist: trk.performer?.name || 'Unknown Artist',
        album: trk.album?.title || '',
        duration_secs: trk.duration,
        artwork_url: trk.album?.image?.large || trk.album?.image?.thumbnail || trk.album?.image?.small || null,
        hires: trk.hires_streamable ?? false,
        bit_depth: trk.maximum_bit_depth ?? null,
        sample_rate: trk.maximum_sampling_rate ?? null,
        is_local: false,
        album_id: trk.album?.id,
        artist_id: trk.performer?.id
      });
    }
    showToast($t('toast.playingTracksNext', { values: { count: tracks.length } }), 'success');
  }

  async function queuePlaylistLaterById(playlistId: number) {
    const playlist = await fetchPlaylistData(playlistId);
    if (!playlist?.tracks?.items?.length) {
      showToast($t('toast.playlistNoTracks'), 'info');
      return;
    }

    const tracks = playlist.tracks.items;
    const queueTracks: BackendQueueTrack[] = tracks.map(trk => ({
      id: trk.id,
      title: trk.title,
      artist: trk.performer?.name || 'Unknown Artist',
      album: trk.album?.title || '',
      duration_secs: trk.duration,
      artwork_url: trk.album?.image?.large || trk.album?.image?.thumbnail || trk.album?.image?.small || null,
      hires: trk.hires_streamable ?? false,
      bit_depth: trk.maximum_bit_depth ?? null,
      sample_rate: trk.maximum_sampling_rate ?? null,
      is_local: false,
      album_id: trk.album?.id,
      artist_id: trk.performer?.id
    }));

    const success = await addTracksToQueue(queueTracks);
    if (success) {
      showToast($t('toast.addedTracksToQueue', { values: { count: queueTracks.length } }), 'success');
    } else {
      showToast($t('toast.failedAddToQueue'), 'error');
    }
  }

  async function copyPlaylistToLibraryById(playlistId: number) {
    try {
      showToast($t('toast.copyingToLibrary'), 'info');
      await invoke('v2_subscribe_playlist', { playlistId });
      sidebarRef?.refreshPlaylists();
      showToast($t('toast.playlistCopied'), 'success');
    } catch (err) {
      console.error('Failed to copy playlist:', err);
      showToast($t('toast.failedCopyPlaylist', { values: { error: String(err) } }), 'error');
    }
  }

  function sharePlaylistQobuzLinkById(playlistId: number) {
    const url = `https://play.qobuz.com/playlist/${playlistId}`;
    writeText(url);
    showToast($t('toast.playlistLinkCopied'), 'success');
  }

  async function removePlaylistFavoriteById(playlistId: number) {
    try {
      await invoke('v2_playlist_set_favorite', { playlistId, favorite: false });
      showToast($t('toast.playlistRemovedFavorites'), 'success');
      sidebarRef?.refreshPlaylists();
      sidebarRef?.refreshPlaylistSettings();
    } catch (err) {
      console.error('Failed to remove playlist favorite:', err);
      showToast($t('toast.failedRemoveFavorites', { values: { error: String(err) } }), 'error');
    }
  }

  // Playback Functions - QobuzTrack from search results
  async function handleTrackPlay(track: QobuzTrack) {
    console.log('Playing track:', track);

    const artwork = track.album?.image?.large || track.album?.image?.thumbnail || track.album?.image?.small || '';
    const quality = track.hires_streamable && track.maximum_bit_depth && track.maximum_sampling_rate
      ? `${track.maximum_bit_depth}bit/${track.maximum_sampling_rate}kHz`
      : track.hires_streamable
        ? 'Hi-Res'
        : '-';

    await playTrack({
      id: track.id,
      title: track.title,
      artist: track.performer?.name || 'Unknown Artist',
      album: track.album?.title || '',
      artwork,
      duration: track.duration,
      quality,
      bitDepth: track.maximum_bit_depth,
      samplingRate: track.maximum_sampling_rate,
      albumId: track.album?.id,
      artistId: track.performer?.id
    });
  }

  // Handle track play from album detail view
  async function handleAlbumTrackPlay(track: Track) {
    console.log('Playing album track:', track);

    // ALWAYS create context when playing from an album
    // The setting only affects menu options visibility, not implicit behavior
    if (selectedAlbum?.tracks) {
      const trackIndex = selectedAlbum.tracks.findIndex(trk => trk.id === track.id);
      const trackIds = selectedAlbum.tracks.map(trk => trk.id);
      
      console.log('[Album] Creating context with', trackIds.length, 'tracks, starting at', trackIndex);
      await setPlaybackContext(
        'album',
        selectedAlbum.id,
        selectedAlbum.title,
        'qobuz',
        trackIds,
        trackIndex >= 0 ? trackIndex : 0
      );
      console.log('[Album] Context created - stack icon should appear');
    } else {
      console.log('[Album] No album tracks found, cannot create context');
    }

    const artwork = selectedAlbum?.artwork || '';
    const quality = track.hires && track.bitDepth && track.samplingRate
      ? `${track.bitDepth}bit/${track.samplingRate}kHz`
      : track.hires
        ? 'Hi-Res'
        : '-';

    // Build queue from album tracks before playing (filter blacklisted artists)
    if (selectedAlbum?.tracks) {
      const album = selectedAlbum; // Capture for closure
      console.log('[Album Queue] Building queue from', album.tracks.length, 'album tracks');

      // Filter out blacklisted tracks
      const playableTracks = album.tracks.filter(trk => {
        const artistId = trk.artistId ?? album.artistId;
        return !artistId || !isArtistBlacklisted(artistId);
      });

      const trackIndex = playableTracks.findIndex(trk => trk.id === track.id);
      const queueTracks: BackendQueueTrack[] = playableTracks.map(trk => ({
        id: trk.id,
        title: trk.title,
        artist: trk.artist || album.artist || 'Unknown Artist',
        album: album.title || '',
        duration_secs: trk.durationSeconds,
        artwork_url: artwork || null,
        hires: trk.hires ?? false,
        bit_depth: trk.bitDepth ?? null,
        sample_rate: trk.samplingRate ?? null,
        is_local: false,
        album_id: album.id,
        artist_id: trk.artistId ?? album.artistId
      }));

      console.log('[Album Queue] Mapped to', queueTracks.length, 'queue tracks (filtered), startIndex:', trackIndex);
      console.log('[Album Queue] Track IDs:', queueTracks.map(trk => trk.id));

      // Set the queue starting at the clicked track
      await setQueue(queueTracks, trackIndex >= 0 ? trackIndex : 0, true);

      console.log('[Album Queue] Queue set successfully');
    }

    // Play track using unified service
    await playTrack({
      id: track.id,
      title: track.title,
      artist: track.artist || selectedAlbum?.artist || 'Unknown Artist',
      album: selectedAlbum?.title || '',
      artwork,
      duration: track.durationSeconds,
      quality,
      bitDepth: track.bitDepth,
      samplingRate: track.samplingRate,
      albumId: selectedAlbum?.id,
      artistId: track.artistId
    });
  }

  // Playback controls (delegating to playerStore)
  function handleSeek(time: number) {
    playerSeek(time);
  }

  function handleVolumeChange(newVolume: number) {
    playerSetVolume(newVolume);
  }

  async function toggleShuffle() {
    const result = await queueToggleShuffle();
    if (result.success) {
      showToast(result.enabled ? $t('toast.shuffleEnabled') : $t('toast.shuffleDisabled'), 'info');
      // Persist playback mode to session
      saveSessionPlaybackMode(result.enabled, repeatMode);
    }
  }

  async function toggleRepeat() {
    const result = await queueToggleRepeat();
    if (result.success) {
      const messages: Record<RepeatMode, string> = {
        off: $t('toast.repeatOff'),
        all: $t('toast.repeatAll'),
        one: $t('toast.repeatOne')
      };
      showToast(messages[result.mode], 'info');
      // Persist playback mode to session
      saveSessionPlaybackMode(isShuffle, result.mode);
    }
  }

  async function toggleFavorite() {
    if (!currentTrack) return;

    const result = await toggleTrackFavorite(currentTrack.id, isFavorite);
    if (result.success) {
      setIsFavorite(result.isFavorite);
      showToast(result.isFavorite ? $t('toast.addedToFavorites') : $t('toast.removedFromFavorites'), 'success');
    } else {
      showToast($t('toast.failedUpdateFavorites'), 'error');
    }
  }

  async function toggleNormalization() {
    const newState = !normalizationEnabled;
    try {
      await invoke('v2_set_audio_normalization_enabled', { enabled: newState });
      normalizationEnabled = newState;
    } catch (err) {
      console.error('Failed to toggle normalization:', err);
    }
  }

  // Add to Playlist handler for Now Playing track
  function openAddToPlaylistModal() {
    if (!currentTrack) return;
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    const isLocal = currentTrack.isLocal === true || isLocalTrack(currentTrack.id);
    openPlaylistModal('addTrack', [currentTrack.id], isLocal);
  }

  // Skip track handlers - wired to backend queue via queueStore
  async function handleSkipBack() {
    const playerState = getPlayerState();
    if (!playerState.currentTrack || playerState.isSkipping) return;
    // If more than 3 seconds in, restart track; otherwise go to previous
    if (playerState.currentTime > 3) {
      handleSeek(0);
      return;
    }

    setIsSkipping(true);
    try {
      const prevTrack = await previousTrack();
      if (prevTrack) {
        await playQueueTrack(prevTrack);
      } else {
        // No previous track, just restart
        handleSeek(0);
      }
    } catch (err) {
      console.error('Failed to go to previous track:', err);
      showToast($t('toast.failedPreviousTrack'), 'error');
    } finally {
      setIsSkipping(false);
    }
  }

  async function handleSkipForward() {
    const playerState = getPlayerState();
    if (!playerState.currentTrack || playerState.isSkipping) return;

    setIsSkipping(true);
    try {
      const nextTrackResult = await nextTrack();
      if (nextTrackResult) {
        await playQueueTrack(nextTrackResult);
      } else {
        // No next track - stop playback
        await stopPlayback();
        setIsPlaying(false);
        showToast($t('toast.queueEnded'), 'info');
      }
    } catch (err) {
      console.error('Failed to go to next track:', err);
      showToast($t('toast.failedNextTrack'), 'error');
    } finally {
      setIsSkipping(false);
    }
  }

  // Check if a track is available for playback (handles offline mode)
  async function isTrackAvailable(track: BackendQueueTrack): Promise<boolean> {
    // Always available when online
    if (!offlineStatus.isOffline) return true;

    // Local tracks are always available
    if (isLocalTrack(track.id)) return true;

    // Check if Qobuz track has a local copy
    try {
      const localIds = await invoke<number[]>('v2_playlist_get_tracks_with_local_copies', {
        trackIds: [track.id]
      });
      return localIds.includes(track.id);
    } catch {
      return false;
    }
  }

  // Helper to play a track from the queue (with offline skip support)
  async function playQueueTrack(track: BackendQueueTrack, skippedIds = new Set<number>(), gaplessTransition = false) {
    const source = track.source ?? (track.is_local ? 'local' : 'qobuz');
    const isLocal = source !== 'qobuz' || isLocalTrack(track.id);

    // In offline mode, check if track is available
    if (offlineStatus.isOffline && !isLocal) {
      const available = await isTrackAvailable(track);
      if (!available) {
        // Skip to next track (prevent infinite loop)
        if (skippedIds.has(track.id)) {
          // Already tried this track, stop to prevent infinite loop
          setQueueEnded(true);
          showToast($t('toast.noAvailableTracks'), 'info');
          return;
        }
        skippedIds.add(track.id);

        // Get next track and try to play it
        const nextTrackResult = await nextTrack();
        if (nextTrackResult) {
          await playQueueTrack(nextTrackResult, skippedIds);
        } else {
          setQueueEnded(true);
        }
        return;
      }
    }

    // Reset queue ended flag when playing a new track
    setQueueEnded(false);

    // Determine quality string from track data
    const quality = isLocal
      ? 'Local'
      : track.bit_depth && track.sample_rate
        ? `${track.bit_depth}bit/${track.sample_rate}kHz`
        : track.hires
          ? 'Hi-Res'
          : '-';

    // Play track using unified service
    await playTrack({
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork: track.artwork_url || '',
      duration: track.duration_secs,
      quality,
      bitDepth: track.bit_depth ?? undefined,
      // Only convert Hz to kHz for local tracks. Qobuz tracks are already in kHz.
      samplingRate: isLocal && track.sample_rate ? track.sample_rate / 1000 : track.sample_rate ?? undefined,
      isLocal,
      source,
      albumId: track.album_id ?? undefined,
      artistId: track.artist_id ?? undefined
    }, { isLocal, source: source as 'qobuz' | 'local' | 'plex', showLoadingToast: false, gaplessTransition });
  }

  // Play a specific track from the queue panel
  async function handleQueueTrackPlay(trackId: string) {
    try {
      // Find the index in the queue
      const queueState = await getBackendQueueState();
      if (!queueState) {
        showToast($t('toast.failedPlayTrack'), 'error');
        return;
      }

      const allTracks = [queueState.current_track, ...queueState.upcoming].filter(Boolean) as BackendQueueTrack[];
      const trackIndex = allTracks.findIndex(trk => String(trk.id) === trackId);

      if (trackIndex >= 0) {
        // If it's the current track (index 0), just ensure it's playing
        if (trackIndex === 0 && queueState.current_index !== null) {
          // Already current, nothing to do
          return;
        }

        // Play by index (accounting for current track offset)
        const actualIndex = queueState.current_index !== null
          ? queueState.current_index + trackIndex
          : trackIndex;

        const track = await playQueueIndex(actualIndex);
        if (track) {
          await playQueueTrack(track);
        }
      }
    } catch (err) {
      console.error('Failed to play queue track:', err);
      showToast($t('toast.failedPlayTrack'), 'error');
    }
  }

  // Clear the queue
  async function handleClearQueue() {
    const success = await clearQueue();
    if (success) {
      showToast($t('toast.queueCleared'), 'info');
    } else {
      showToast($t('toast.failedClearQueue'), 'error');
    }
  }

  // Reorder tracks in the queue
  async function handleQueueReorder(fromIndex: number, toIndex: number) {
    const success = await moveQueueTrack(fromIndex, toIndex);
    if (!success) {
      showToast($t('toast.failedReorderQueue'), 'error');
    }
  }

  // Remove a track from the upcoming queue by its position in the upcoming list (V2)
  async function handleRemoveFromQueue(upcomingIndex: number) {
    try {
      await invoke('v2_remove_upcoming_track', { upcomingIndex });
      await syncQueueState(); // Refresh UI
      showToast($t('toast.removedFromQueue'), 'info');
    } catch (err) {
      console.error('Failed to remove from queue:', err);
      showToast($t('toast.failedRemoveFromQueue'), 'error');
    }
  }

  // Add a queue track to playlist
  function handleQueueTrackAddToPlaylist(trackId: string) {
    const numericId = parseInt(trackId, 10);
    if (isNaN(numericId)) return;
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    const isLocal = isLocalTrack(numericId);
    openPlaylistModal('addTrack', [numericId], isLocal);
  }

  // Show track info for a queue track
  function handleQueueTrackInfo(trackId: string) {
    const numericId = parseInt(trackId, 10);
    if (isNaN(numericId)) return;
    trackInfoTrackId = numericId;
    isTrackInfoOpen = true;
  }

  // Save current queue as a new playlist
  function handleSaveQueueAsPlaylist() {
    // Collect all track IDs from queue (current track + upcoming)
    const trackIds: number[] = [];

    // Add current track if present
    if (currentTrack) {
      trackIds.push(currentTrack.id);
    }

    // Add all upcoming tracks
    for (const track of queue) {
      const numericId = parseInt(track.id, 10);
      if (!isNaN(numericId) && !trackIds.includes(numericId)) {
        trackIds.push(numericId);
      }
    }

    if (trackIds.length === 0) {
      showToast($t('toast.queueEmpty'), 'info');
      return;
    }

    // Open playlist modal in addTrack mode with queue tracks
    openAddToPlaylist(trackIds);
    // Close queue panel
    closeQueue();
  }

  // Toggle infinite play mode (auto-refill queue with similar tracks)
  function handleToggleInfinitePlay() {
    infinitePlayEnabled = !infinitePlayEnabled;
    // Persist to localStorage
    try {
      setUserItem('qbz-infinite-play', JSON.stringify(infinitePlayEnabled));
    } catch {
      // Ignore storage errors
    }
    showToast(infinitePlayEnabled ? $t('toast.infinitePlayEnabled') : $t('toast.infinitePlayDisabled'), 'info');
  }

  // Play a track from history
  async function handlePlayHistoryTrack(trackId: string) {
    try {
      // Get the full queue state to find the track in history
      const queueState = await getBackendQueueState();
      if (!queueState) {
        showToast($t('toast.failedPlayTrack'), 'error');
        return;
      }

      // Find the track in history
      const numericId = parseInt(trackId, 10);
      const historyTrack = queueState.history.find(trk => trk.id === numericId);
      if (!historyTrack) {
        showToast($t('toast.trackNotInHistory'), 'error');
        return;
      }

      // Play the track directly
      await handleTrackPlay({
        id: historyTrack.id,
        title: historyTrack.title,
        performer: { name: historyTrack.artist },
        album: { title: historyTrack.album, image: { large: historyTrack.artwork_url || '' } },
        duration: historyTrack.duration_secs,
        hires_streamable: historyTrack.hires,
        maximum_bit_depth: historyTrack.bit_depth ?? undefined,
        maximum_sampling_rate: historyTrack.sample_rate ?? undefined
      });
    } catch (err) {
      console.error('Failed to play history track:', err);
      showToast($t('toast.failedPlayTrack'), 'error');
    }
  }

  // Play all tracks from album (starting from first non-blacklisted track)
  async function handlePlayAllAlbum() {
    if (!selectedAlbum?.tracks?.length) return;
    const album = selectedAlbum; // Capture for closure
    // Find first non-blacklisted track
    const firstPlayableTrack = album.tracks.find(trk => {
      const artistId = trk.artistId ?? album.artistId;
      return !artistId || !isArtistBlacklisted(artistId);
    });
    if (!firstPlayableTrack) return;
    await handleAlbumTrackPlay(firstPlayableTrack);
  }

  // Shuffle play all tracks from album
  async function handleShuffleAlbum() {
    if (!selectedAlbum?.tracks?.length) return;
    const album = selectedAlbum; // Capture for closure

    // Filter out blacklisted tracks
    const playableTracks = album.tracks.filter(trk => {
      const artistId = trk.artistId ?? album.artistId;
      return !artistId || !isArtistBlacklisted(artistId);
    });

    if (playableTracks.length === 0) return;

    console.log('[Album Shuffle] Starting shuffle with', playableTracks.length, 'playable tracks');

    // Set shuffle mode first
    try {
      await invoke('v2_set_shuffle', { enabled: true });
      isShuffle = true;
    } catch (err) {
      console.error('Failed to enable shuffle:', err);
    }

    // Pick a random track to start with
    const randomIndex = Math.floor(Math.random() * playableTracks.length);
    const randomTrack = playableTracks[randomIndex];

    console.log('[Album Shuffle] Starting from random track index:', randomIndex, 'track:', randomTrack.title);

    // Play from random track (queue will be shuffled by backend)
    await handleAlbumTrackPlay(randomTrack);
    showToast($t('toast.shuffleEnabled'), 'info');
  }

  // Create album radio via Qobuz /radio/album API
  async function handleCreateAlbumRadio() {
    if (!selectedAlbum) return;
    try {
      const contextId = await invoke<string>('v2_create_album_radio', {
        albumId: String(selectedAlbum.id),
        albumName: selectedAlbum.title || 'Album Radio',
      });
      console.log(`[Radio] Album radio created: ${contextId}`);

      // Play first track from the radio queue
      const firstTrack = await playQueueIndex(0);
      if (firstTrack) {
        const quality = firstTrack.bit_depth && firstTrack.sample_rate
          ? `${firstTrack.bit_depth}bit/${firstTrack.sample_rate}kHz`
          : firstTrack.hires ? 'Hi-Res' : '-';
        playTrack({
          id: firstTrack.id,
          title: firstTrack.title,
          artist: firstTrack.artist,
          album: firstTrack.album,
          artwork: firstTrack.artwork_url || '',
          duration: firstTrack.duration_secs,
          quality,
          bitDepth: firstTrack.bit_depth ?? undefined,
          samplingRate: firstTrack.sample_rate ?? undefined,
          albumId: firstTrack.album_id ?? undefined,
          artistId: firstTrack.artist_id ?? undefined,
        });
      }
    } catch (err) {
      console.error('Failed to create album radio:', err);
    }
  }

  // Create QBZ track radio (used by PlaylistDetailView, FavoritesView, etc.)
  async function handleCreateQbzTrackRadio(trackId: number, trackTitle: string, artistId?: number) {
    try {
      await invoke<string>('v2_create_track_radio', {
        trackId,
        trackName: trackTitle,
        artistId: artistId ?? 0
      });

      const firstTrack = await playQueueIndex(0);
      if (firstTrack) {
        playTrack({
          id: firstTrack.id,
          title: firstTrack.title,
          artist: firstTrack.artist,
          album: firstTrack.album,
          artwork: firstTrack.artwork_url || '',
          duration: firstTrack.duration_secs,
          quality: firstTrack.bit_depth && firstTrack.sample_rate
            ? `${firstTrack.bit_depth}bit/${firstTrack.sample_rate}kHz`
            : firstTrack.hires ? 'Hi-Res' : '-',
          bitDepth: firstTrack.bit_depth ?? undefined,
          samplingRate: firstTrack.sample_rate ?? undefined,
          albumId: firstTrack.album_id ?? undefined,
          artistId: firstTrack.artist_id ?? undefined,
        });
      }
    } catch (err) {
      console.error('Failed to create QBZ track radio:', err);
    }
  }

  // Create Qobuz track radio (used by PlaylistDetailView, FavoritesView, etc.)
  async function handleCreateQobuzTrackRadio(trackId: number, trackTitle: string) {
    try {
      await invoke<string>('v2_create_qobuz_track_radio', {
        trackId,
        trackName: trackTitle
      });

      const firstTrack = await playQueueIndex(0);
      if (firstTrack) {
        playTrack({
          id: firstTrack.id,
          title: firstTrack.title,
          artist: firstTrack.artist,
          album: firstTrack.album,
          artwork: firstTrack.artwork_url || '',
          duration: firstTrack.duration_secs,
          quality: firstTrack.bit_depth && firstTrack.sample_rate
            ? `${firstTrack.bit_depth}bit/${firstTrack.sample_rate}kHz`
            : firstTrack.hires ? 'Hi-Res' : '-',
          bitDepth: firstTrack.bit_depth ?? undefined,
          samplingRate: firstTrack.sample_rate ?? undefined,
          albumId: firstTrack.album_id ?? undefined,
          artistId: firstTrack.artist_id ?? undefined,
        });
      }
    } catch (err) {
      console.error('Failed to create Qobuz track radio:', err);
    }
  }

  // Add all album tracks next in queue (after current track)
  async function handleAddAlbumToQueueNext() {
    if (!selectedAlbum?.tracks?.length) return;
    const album = selectedAlbum; // Capture for closure

    // Filter out blacklisted tracks
    const playableTracks = album.tracks.filter(trk => {
      const artistId = trk.artistId ?? album.artistId;
      return !artistId || !isArtistBlacklisted(artistId);
    });

    if (playableTracks.length === 0) return;

    const artwork = album.artwork || '';
    // Add in reverse order so first track ends up right after current
    for (let i = playableTracks.length - 1; i >= 0; i--) {
      const trk = playableTracks[i];
      queueTrackNext({
        id: trk.id,
        title: trk.title,
        artist: trk.artist || album.artist || 'Unknown Artist',
        album: album.title || '',
        duration_secs: trk.durationSeconds,
        artwork_url: artwork || null,
        hires: trk.hires ?? false,
        bit_depth: trk.bitDepth ?? null,
        sample_rate: trk.samplingRate ?? null,
        is_local: false,
        album_id: album.id,
        artist_id: trk.artistId ?? album.artistId
      });
    }
    showToast($t('toast.playingTracksNext', { values: { count: playableTracks.length } }), 'success');
  }

  // Add all album tracks to end of queue
  async function handleAddAlbumToQueueLater() {
    if (!selectedAlbum?.tracks?.length) return;
    const album = selectedAlbum; // Capture for closure

    // Filter out blacklisted tracks
    const playableTracks = album.tracks.filter(trk => {
      const artistId = trk.artistId ?? album.artistId;
      return !artistId || !isArtistBlacklisted(artistId);
    });

    if (playableTracks.length === 0) return;

    const artwork = album.artwork || '';
    const queueTracks: BackendQueueTrack[] = playableTracks.map(trk => ({
      id: trk.id,
      title: trk.title,
      artist: trk.artist || album.artist || 'Unknown Artist',
      album: album.title || '',
      duration_secs: trk.durationSeconds,
      artwork_url: artwork || null,
      hires: trk.hires ?? false,
      bit_depth: trk.bitDepth ?? null,
      sample_rate: trk.samplingRate ?? null,
      is_local: false,
      album_id: album.id,
      artist_id: trk.artistId ?? album.artistId
    }));

    const success = await addTracksToQueue(queueTracks);
    if (success) {
      showToast($t('toast.addedTracksToQueue', { values: { count: queueTracks.length } }), 'success');
    } else {
      showToast($t('toast.failedAddToQueue'), 'error');
    }
  }

  async function addAlbumToPlaylistById(albumId: string) {
    const album = await fetchAlbumDetail(albumId);
    addAlbumToPlaylist(album);
  }

  function addAlbumToPlaylist(album: AlbumDetail | null) {
    if (!album?.tracks?.length) return;
    const trackIds = album.tracks.map(trk => trk.id);
    openAddToPlaylist(trackIds);
  }

  // Share album Qobuz link
  function shareAlbumQobuzLink() {
    if (!selectedAlbum?.id) return;
    const url = `https://play.qobuz.com/album/${selectedAlbum.id}`;
    writeText(url);
    showToast($t('toast.albumLinkCopied'), 'success');
  }

  // Share album via album.link
  async function shareAlbumSonglink() {
    if (!selectedAlbum?.id) return;
    try {
      showToast($t('toast.fetchingAlbumLink'), 'info');
      const response = await invoke<{ pageUrl: string }>('v2_share_album_songlink', {
        upc: selectedAlbum.upc || null,
        albumId: selectedAlbum.id,
        title: selectedAlbum.title,
        artist: selectedAlbum.artist
      });
      writeText(response.pageUrl);
      showToast($t('toast.albumLinkCopiedSonglink'), 'success');
    } catch (err) {
      console.error('Failed to get Album.link:', err);
      showToast($t('toast.albumLinkError', { values: { error: String(err) } }), 'error');
    }
  }

  function handleAlbumTrackPlayNext(track: Track) {
    queueTrackNext(buildAlbumQueueTrack(track));
  }

  function handleAlbumTrackPlayLater(track: Track) {
    queueTrackLater(buildAlbumQueueTrack(track));
  }

  // Download handlers
  async function handleTrackDownload(track: Track) {
    try {
      await cacheTrackForOffline({
        id: track.id,
        title: track.title,
        artist: track.artist || selectedAlbum?.artist || 'Unknown',
        album: selectedAlbum?.title,
        albumId: selectedAlbum?.id,
        durationSecs: track.durationSeconds,
        quality: track.quality || '-',
        bitDepth: track.bitDepth,
        sampleRate: track.samplingRate,
      });
      showToast($t('toast.preparingTrackOffline', { values: { title: track.title } }), 'info');
    } catch (err) {
      console.error('Failed to cache for offline:', err);
      showToast($t('toast.failedPrepareOffline'), 'error');
    }
  }

  async function handleTrackRemoveDownload(trackId: number) {
    try {
      await removeCachedTrack(trackId);
      showToast($t('toast.removedFromOffline'), 'info');
    } catch (err) {
      console.error('Failed to remove from offline:', err);
      showToast($t('toast.failedRemoveOffline'), 'error');
    }
  }

  async function handleTrackOpenFolder(trackId: number) {
    try {
      await openTrackFolder(trackId);
    } catch (err) {
      console.error('Failed to open folder:', err);
      showToast($t('toast.failedOpenFolder'), 'error');
    }
  }

  async function handleTrackReDownload(track: Track | DisplayTrack) {
    try {
      // Re-download uses the same download function - backend handles overwriting
      await cacheTrackForOffline({
        id: track.id,
        title: track.title,
        artist: track.artist || selectedAlbum?.artist || 'Unknown',
        album: 'album' in track ? track.album : selectedAlbum?.title,
        albumId: 'albumId' in track ? track.albumId : selectedAlbum?.id,
        durationSecs: track.durationSeconds,
        quality: 'quality' in track ? track.quality || '-' : '-',
        bitDepth: 'bitDepth' in track ? track.bitDepth : undefined,
        sampleRate: 'samplingRate' in track ? track.samplingRate : undefined,
      });
      showToast($t('toast.refreshingTrackOffline', { values: { title: track.title } }), 'info');
    } catch (err) {
      console.error('Failed to refresh offline copy:', err);
      showToast($t('toast.failedRefreshOffline'), 'error');
    }
  }

  function checkTrackDownloaded(trackId: number): boolean {
    return getOfflineCacheState(trackId).status === 'ready';
  }

  async function handleDownloadAlbum() {
    if (!selectedAlbum) return;
    const album = selectedAlbum;

    const tracksToDownload = album.tracks.filter(track => {
      const status = getOfflineCacheState(track.id).status;
      return status === 'none' || status === 'failed';
    });

    if (tracksToDownload.length === 0) {
      showToast($t('toast.allTracksOffline'), 'info');
      return;
    }

    showToast($t('toast.preparingTracksOffline', { values: { count: tracksToDownload.length, album: album.title } }), 'info');

    try {
      await cacheTracksForOfflineBatch(tracksToDownload.map(track => ({
        id: track.id,
        title: track.title,
        artist: track.artist || album.artist || 'Unknown',
        album: album.title,
        albumId: album.id,
        durationSecs: track.durationSeconds,
        quality: track.quality || '-',
        bitDepth: track.bitDepth,
        sampleRate: track.samplingRate,
      })));
    } catch (err) {
      console.error('Failed to batch queue for offline:', err);
    }
  }

  async function handleOpenAlbumFolder() {
    if (!selectedAlbum) return;

    try {
      await openAlbumFolder(selectedAlbum.id);
    } catch (err) {
      console.error('Failed to open album folder:', err);
      showToast($t('toast.failedOpenAlbumFolder'), 'error');
    }
  }

  async function handleReDownloadAlbum() {
    if (!selectedAlbum) return;
    const album = selectedAlbum;

    showToast($t('toast.refreshingAlbumOffline', { values: { album: album.title } }), 'info');

    try {
      await cacheTracksForOfflineBatch(album.tracks.map(track => ({
        id: track.id,
        title: track.title,
        artist: track.artist || album.artist || 'Unknown',
        album: album.title,
        albumId: album.id,
        durationSecs: track.durationSeconds,
        quality: track.quality || '-',
        bitDepth: track.bitDepth,
        sampleRate: track.samplingRate,
      })));
    } catch (err) {
      console.error('Failed to batch refresh for offline:', err);
    }
  }

  async function openAlbumFolderById(albumId: string) {
    try {
      await openAlbumFolder(albumId);
    } catch (err) {
      console.error('Failed to open album folder:', err);
      showToast($t('toast.failedOpenAlbumFolder'), 'error');
    }
  }

  async function reDownloadAlbumById(albumId: string) {
    try {
      const album = await invoke<QobuzAlbum>('v2_get_album', { albumId });
      if (!album || !album.tracks || album.tracks.items.length === 0) {
        showToast($t('toast.failedLoadAlbumRefresh'), 'error');
        return;
      }

      showToast($t('toast.refreshingAlbumOffline', { values: { album: album.title } }), 'info');

      try {
        await cacheTracksForOfflineBatch(album.tracks.items.map(track => ({
          id: track.id,
          title: track.title,
          artist: track.performer?.name || album.artist?.name || 'Unknown',
          album: album.title,
          albumId: album.id,
          durationSecs: track.duration,
          quality: track.hires_streamable ? 'Hi-Res' : '-',
          bitDepth: track.maximum_bit_depth,
          sampleRate: track.maximum_sampling_rate,
        })));
      } catch (err) {
        console.error('Failed to batch refresh for offline:', err);
      }
    } catch (err) {
      console.error('Failed to load album:', err);
      showToast($t('toast.failedLoadAlbumRefresh'), 'error');
    }
  }

  function getTrackOfflineCacheStatus(trackId: number) {
    // Access downloadStateVersion to trigger reactivity
    void downloadStateVersion;
    return getOfflineCacheState(trackId);
  }

  async function handleDisplayTrackDownload(track: DisplayTrack) {
    try {
      const quality = track.bitDepth && track.samplingRate
        ? `${track.bitDepth}bit/${track.samplingRate}kHz`
        : track.hires
          ? 'Hi-Res'
          : '-';
      await cacheTrackForOffline({
        id: track.id,
        title: track.title,
        artist: track.artist || 'Unknown',
        album: track.album,
        albumId: track.albumId,
        durationSecs: track.durationSeconds,
        quality,
        bitDepth: track.bitDepth,
        sampleRate: track.samplingRate,
      });
      showToast($t('toast.preparingTrackOffline', { values: { title: track.title } }), 'info');
    } catch (err) {
      console.error('Failed to prepare for offline:', err);
      showToast($t('toast.failedPrepareOffline'), 'error');
    }
  }

  // Handler for SearchView's Track type (different from DisplayTrack)
  async function handleSearchTrackDownload(track: {
    id: number;
    title: string;
    duration: number;
    album?: { id?: string; title: string; image?: { small?: string; large?: string } };
    performer?: { id?: number; name: string };
    hires_streamable?: boolean;
    maximum_bit_depth?: number;
    maximum_sampling_rate?: number;
  }) {
    try {
      const quality = track.maximum_bit_depth && track.maximum_sampling_rate
        ? `${track.maximum_bit_depth}bit/${track.maximum_sampling_rate}kHz`
        : track.hires_streamable
          ? 'Hi-Res'
          : '-';
      await cacheTrackForOffline({
        id: track.id,
        title: track.title,
        artist: track.performer?.name || 'Unknown',
        album: track.album?.title,
        albumId: track.album?.id,
        durationSecs: track.duration,
        quality,
        bitDepth: track.maximum_bit_depth,
        sampleRate: track.maximum_sampling_rate,
      });
      showToast($t('toast.preparingTrackOffline', { values: { title: track.title } }), 'info');
    } catch (err) {
      console.error('Failed to prepare for offline:', err);
      showToast($t('toast.failedPrepareOffline'), 'error');
    }
  }

  /**
   * Handle playback of DisplayTrack (used by ArtistDetailView, PlaylistDetailView, FavoritesView)
   * This is fire-and-forget to match view callback signatures
   */
  function handleDisplayTrackPlay(track: DisplayTrack): void {
    console.log('Playing display track:', track);

    // Determine quality string:
    // - If we have exact bitDepth/samplingRate, show them
    // - If hires flag is true but no exact values, show "Hi-Res"
    // - Otherwise show "-" (unknown - will be updated when streaming returns quality info)
    // TODO: Update quality when backend returns actual streaming quality
    const quality = track.bitDepth && track.samplingRate
      ? `${track.bitDepth}bit/${track.samplingRate}kHz`
      : track.hires
        ? 'Hi-Res'
        : '-';

    // Fire-and-forget async call
    playTrack({
      id: track.id,
      title: track.title,
      artist: track.artist || 'Unknown Artist',
      album: track.album || 'Playlist',
      artwork: track.albumArt || '',
      duration: track.durationSeconds,
      quality,
      bitDepth: track.bitDepth,
      samplingRate: track.samplingRate,
      albumId: track.albumId,
      artistId: track.artistId
    });
  }

  /**
   * Helper: Create context and play display track
   */
  async function createContextAndPlayDisplayTrack(
    track: DisplayTrack,
    contextType: ContextType,
    contextId: string,
    contextLabel: string,
    trackIds: number[],
    startIndex: number
  ) {
    // Create context
    await setPlaybackContext(
      contextType,
      contextId,
      contextLabel,
      'qobuz',
      trackIds,
      startIndex
    );
    console.log(`[Context] Created for ${contextType}: ${contextLabel}, starting at index ${startIndex}`);
    
    // Play track
    handleDisplayTrackPlay(track);
  }

  async function handleLocalTrackPlay(track: LocalLibraryTrack) {
    console.log('Playing local track:', track);
    // DO NOT clear context - LocalLibraryView already sets it correctly
    // await clearPlaybackContext();

    const source = track.source === 'plex' ? 'plex' : 'local';
    const plexBaseUrl = getUserItem('qbz-plex-poc-base-url') || '';
    const plexToken = getUserItem('qbz-plex-poc-token') || '';
    const resolveArtwork = (path?: string): string => {
      if (!path) return '';
      if (/^https?:\/\//i.test(path)) return path;
      if (source === 'plex' && path.startsWith('/library/') && plexBaseUrl && plexToken) {
        const base = plexBaseUrl.replace(/\/+$/, '');
        const sep = path.includes('?') ? '&' : '?';
        return `${base}${path}${sep}X-Plex-Token=${encodeURIComponent(plexToken)}`;
      }
      return convertFileSrc(path);
    };
    const artwork = track.artwork_path
      ? resolveArtwork(track.artwork_path)
      : '';
    const quality = track.bit_depth && track.sample_rate
      ? (track.bit_depth >= 24 || track.sample_rate > 48000
        ? `${track.bit_depth}bit/${track.sample_rate / 1000}kHz`
        : track.format)
      : track.format;

    await playTrack({
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork,
      duration: track.duration_secs,
      quality,
      bitDepth: track.bit_depth,
      samplingRate: track.sample_rate ? track.sample_rate / 1000 : undefined,  // Convert Hz to kHz (44100 → 44.1) - NO ROUNDING
      format: track.format,
      isLocal: source !== 'plex',
      source
    }, { isLocal: source !== 'plex', source });
  }

  // Handle setting queue from local library (tracks need different playback command)
  function handleSetLocalQueue(trackIds: number[]) {
    // Set local track IDs via queueStore
    setLocalTrackIds(trackIds);
  }

  // Playlist Modal Functions
  function openCreatePlaylist() {
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    openPlaylistModal('create', []);
  }

  function openAddToPlaylist(trackIds: number[], isLocal = false) {
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    openPlaylistModal('addTrack', trackIds, isLocal);
  }

  function handlePlaylistCreated(playlist?: { id: number; name: string; tracks_count: number }) {
    const trackCount = playlistModalTrackIds.length;
    const isLocal = playlistModalTracksAreLocal;

    if (playlistModalMode === 'addTrack') {
      showToast($t('toast.tracksAddedToPlaylist'), 'success');
    } else {
      showToast($t('toast.playlistCreated'), 'success');
    }
    sidebarRef?.refreshPlaylists();
    sidebarRef?.refreshPlaylistSettings();
    sidebarRef?.refreshLocalTrackCounts();

    // If a newly created playlist is provided, ensure the sidebar has the correct count
    // This handles API eventual consistency where tracks_count might be stale
    if (playlist && playlist.id > 0 && trackCount > 0) {
      // Small delay to let refreshPlaylists complete, then override with correct count
      setTimeout(() => {
        const qobuzCount = isLocal ? 0 : trackCount;
        const localCount = isLocal ? trackCount : 0;
        sidebarRef?.updatePlaylistCounts(playlist.id, qobuzCount, localCount);
      }, 100);
    }
  }

  function openImportPlaylist() {
    openPlaylistImport();
  }

  function handlePlaylistImported(summary: {
    provider: 'Spotify' | 'AppleMusic' | 'Tidal' | 'Deezer';
    playlist_name: string;
    total_tracks: number;
    matched_tracks: number;
    skipped_tracks: number;
    qobuz_playlist_ids: number[];
    parts_created: number;
  }) {
    sidebarRef?.refreshPlaylists();
    sidebarRef?.refreshPlaylistSettings();
    if (summary.qobuz_playlist_ids.length > 0) {
      selectPlaylist(summary.qobuz_playlist_ids[0]);
    }
  }

  // Track Info Modal
  function showTrackInfo(trackId: number) {
    trackInfoTrackId = trackId;
    isTrackInfoOpen = true;
  }

  // Album Credits Modal
  function showAlbumCredits(albumId: string) {
    albumCreditsAlbumId = albumId;
    isAlbumCreditsOpen = true;
  }

  // Auth Handlers
  async function handleStartOffline() {
    // Enable manual offline mode and enter app without authentication
    await setManualOffline(true);

    // Activate offline session in backend (initializes minimal stores, sets session_activated=true)
    // This allows queue commands to work even without remote auth
    try {
      await invoke('v2_activate_offline_session');
    } catch (err) {
      console.error('[Offline] Failed to activate offline session:', err);
      // Continue anyway - offline mode should be best-effort
    }

    setLoggedIn({
      userName: 'Offline User',
      userId: 0,
      subscription: 'Local Library Only'
    });
    navigateTo('library');
    showToast($t('toast.offlineModeStarted'), 'info');
  }

  async function handleLoginSuccess(info: UserInfo) {
    // V2 login commands (v2_auto_login, v2_manual_login) now handle session activation internally.
    // They return success:false if activation fails, so if we get here, session is already active.
    // NO legacy activate_user_session call needed - that causes duplicate activation.

    // Validate userId before any session operations
    if (!info.userId || info.userId <= 0) {
      console.error('[Session] Invalid userId received:', info.userId, '- cannot proceed');
      return;
    }

    // Backend session already activated by V2 login - set UI login state
    setLoggedIn(info);

    // Set up per-user localStorage scoping and migrate old keys
    setStorageUserId(info.userId);
    migrateLocalStorage(info.userId);
    loadSystemNotificationsPreference();

    // Re-sync volume from the now-correct user-scoped localStorage key
    await resyncPersistedVolume();

    // Signal that per-user backend stores are ready — the launch update
    // flow $effect gates on this to avoid reading default preferences
    sessionReady = true;

    // Restore Last.fm session now that backend session is active
    restoreLastfmSession();

    showToast($t('toast.welcomeUser', { values: { name: info.userName } }), 'success');

    // Initialize per-user stores now that the backend session is active
    initOfflineCacheStates(); // has internal try/catch
    initPlaybackPreferences().catch(err => console.debug('[PlaybackPrefs] Init deferred:', err));
    initBlacklistStore().catch(err => console.debug('[Blacklist] Init deferred:', err));
    refreshUpdatePreferences().catch(err => console.debug('[Updates] Prefs refresh deferred:', err));

    // Load audio settings (normalization state) now that session is active
    invoke<{ normalization_enabled: boolean }>('v2_get_audio_settings').then((settings) => {
      normalizationEnabled = settings.normalization_enabled;
    }).catch((err) => {
      console.error('[AudioSettings] Failed to load:', err);
    });

    // Load favorites now that login is confirmed (sync with Qobuz)
    loadFavorites();        // Track favorites
    loadAlbumFavorites();   // Album favorites
    loadArtistFavorites();  // Artist favorites

    // Refresh offline status now that we're logged in
    await refreshOfflineStatus();

    // Train recommendation scores in background (fire-and-forget)
    trainScores().then(() => {
      console.log('[Reco] Scores trained after login');
    }).catch(err => {
      console.debug('[Reco] Score training failed:', err);
    });

    // Restore previous session if available
    try {
      const session = await loadSessionState();

      // Restore queue + track (visual only — paused at 0:00)
      if (session && session.queue_tracks.length > 0) {
        console.log('[Session] Restoring previous session...');

        const tracks: BackendQueueTrack[] = session.queue_tracks.map(trk => ({
          id: trk.id,
          title: trk.title,
          artist: trk.artist,
          album: trk.album,
          duration_secs: trk.duration_secs,
          artwork_url: trk.artwork_url,
          hires: trk.hires ?? false,
          bit_depth: trk.bit_depth ?? null,
          sample_rate: trk.sample_rate ?? null,
          is_local: trk.is_local ?? false,
          album_id: trk.album_id ?? null,
          artist_id: trk.artist_id ?? null
        }));

        await setQueue(tracks, session.current_index ?? 0, true);

        // Restore shuffle/repeat mode
        if (session.shuffle_enabled) {
          await invoke('v2_set_shuffle', { enabled: true });
        }
        if (session.repeat_mode !== 'off') {
          const v2Mode = session.repeat_mode.charAt(0).toUpperCase() + session.repeat_mode.slice(1);
          await invoke('v2_set_repeat_mode', { mode: v2Mode });
        }

        // Restore volume
        playerSetVolume(Math.round(session.volume * 100));

        // Visual-only track restore: show in player bar paused at 0:00
        if (session.current_index !== null && tracks[session.current_index]) {
          const track = tracks[session.current_index];

          // Use cached data for instant display (no network fetch needed)
          const quality = track.hires
            ? `${track.bit_depth ?? 24}/${track.sample_rate ? track.sample_rate / 1000 : 96}`
            : 'CD';
          setCurrentTrack({
            id: track.id,
            title: track.title,
            artist: track.artist,
            album: track.album,
            artwork: track.artwork_url || '',
            duration: track.duration_secs,
            quality,
            bitDepth: track.bit_depth ?? undefined,
            samplingRate: track.sample_rate ?? undefined,
          });

          // First play will load a fresh stream instead of seeking
          setPendingSessionRestore(track.id, 0);
          console.log(`[Session] Track ${track.id} restored visually (paused at 0:00)`);
        }

        console.log('[Session] Session restored successfully');
      }

      // Restore last page (opt-in via settings)
      if (session) {
        restoreLastView(session);
      }
    } catch (err) {
      console.error('[Session] Failed to restore session:', err);
    }
  }

  async function handleLogout() {
    try {
      // v2_logout handles full session deactivation internally via session_lifecycle::deactivate_session()
      // NO legacy deactivate_user_session call needed - that causes duplicate teardown
      await invoke('v2_logout');
      // Clear saved credentials from keyring
      try {
        await invoke('v2_clear_saved_credentials');
        console.log('Credentials cleared from keyring');
      } catch (clearErr) {
        console.error('Failed to clear credentials:', clearErr);
        // Don't block logout if clearing fails
      }
      // Clear per-user localStorage scoping
      setStorageUserId(null);
      // Clear session state
      await clearSession();
      setLoggedOut();
      sessionReady = false;
      updatesLaunchTriggered = false;
      resetLaunchFlow();
      resetUpdatesStore();
      currentTrack = null;
      isPlaying = false;
      showToast($t('toast.logoutSuccess'), 'info');
    } catch (err) {
      console.error('Logout error:', err);
      showToast($t('toast.failedLogout'), 'error');
    }
  }

  // Restore last page from session (opt-in via settings)
  function restoreLastView(session: PersistedSession) {
    const startupPage = getUserItem('qbz-startup-page') || 'home';
    if (startupPage !== 'last-view') return;

    const view = session.last_view as ViewType;
    if (!view || view === 'home') return;

    const contextId = session.view_context_id;
    const contextType = session.view_context_type;

    // Views that require context data to be fetched
    switch (view) {
      case 'album':
        if (contextId) {
          handleAlbumClick(contextId);
        }
        return;
      case 'artist':
        if (contextId) {
          handleArtistClick(Number(contextId));
        }
        return;
      case 'playlist':
        if (contextId) {
          setRestoredPlaylistId(Number(contextId));
          restoreView('playlist');
        }
        return;
      case 'library-album':
        if (contextId) {
          setRestoredLocalAlbumId(contextId);
          restoreView('library-album');
        }
        return;
      default:
        // Simple views (search, library, settings, favorites-*, etc.)
        restoreView(view);
        return;
    }
  }

  // Save session state before window closes
  async function saveSessionBeforeClose() {
    if (!isLoggedIn) return;

    try {
      // Build view context from current navigation state
      const currentView = activeView;
      let viewContextId: string | null = null;
      let viewContextType: string | null = null;

      switch (currentView) {
        case 'album':
          if (selectedAlbum?.id) {
            viewContextId = String(selectedAlbum.id);
            viewContextType = 'album';
          }
          break;
        case 'artist':
          if (selectedArtist?.id) {
            viewContextId = String(selectedArtist.id);
            viewContextType = 'artist';
          }
          break;
        case 'playlist':
          if (selectedPlaylistId) {
            viewContextId = String(selectedPlaylistId);
            viewContextType = 'playlist';
          }
          break;
        case 'library-album': {
          const localAlbumId = getSelectedLocalAlbumId();
          if (localAlbumId) {
            viewContextId = localAlbumId;
            viewContextType = 'library-album';
          }
          break;
        }
      }

      // Get ALL queue tracks from backend (uncapped, for full persistence)
      const snapshot = await invoke<{ tracks: BackendQueueTrack[]; current_index: number | null }>('v2_get_all_queue_tracks');
      const tracks = snapshot.tracks;
      const currentIndex = snapshot.current_index;

      const allTracks: PersistedQueueTrack[] = tracks.map(track => ({
        id: track.id,
        title: track.title,
        artist: track.artist,
        album: track.album,
        duration_secs: track.duration_secs,
        artwork_url: track.artwork_url,
        hires: track.hires,
        bit_depth: track.bit_depth ?? null,
        sample_rate: track.sample_rate ?? null,
        is_local: track.is_local ?? false,
        album_id: track.album_id ?? null,
        artist_id: track.artist_id ?? null,
      }));

      await saveSessionState(
        allTracks,
        currentIndex,
        currentTrack ? Math.floor(currentTime) : 0,
        volume / 100,
        isShuffle,
        repeatMode,
        isPlaying,
        currentView,
        viewContextId,
        viewContextType
      );
      console.log('[Session] Session saved on close');
    } catch (err) {
      console.error('[Session] Failed to save session on close:', err);
    }
  }

  // Keyboard Shortcuts - delegated to keybindings system
  function handleKeydown(e: KeyboardEvent) {
    if (!isLoggedIn) return;

    // Delegate to keybinding manager (handles input target filtering internally)
    keybindingHandler(e);
  }

  // Playback and queue state listeners
  // Always keep active to receive external events (e.g., from remote control)
  $effect(() => {
    startPolling();
    startQueueEventListener();

    return () => {
      stopPolling();
      stopQueueEventListener();
    };
  });

  // Debounced full session save (coalesces rapid state changes into a single save)
  let sessionSaveDebounce: ReturnType<typeof setTimeout> | null = null;
  function debouncedFullSessionSave() {
    if (sessionSaveDebounce) clearTimeout(sessionSaveDebounce);
    sessionSaveDebounce = setTimeout(() => {
      sessionSaveDebounce = null;
      saveSessionBeforeClose();
    }, 2000);
  }

  // Periodic full session save during playback
  let sessionSaveInterval: ReturnType<typeof setInterval> | null = null;

  $effect(() => {
    // Start periodic save when playing, stop when paused/stopped
    if (isPlaying && currentTrack && isLoggedIn) {
      if (!sessionSaveInterval) {
        sessionSaveInterval = setInterval(() => {
          saveSessionBeforeClose();
        }, 15000); // Save every 15 seconds during playback
      }
    } else {
      if (sessionSaveInterval) {
        clearInterval(sessionSaveInterval);
        sessionSaveInterval = null;
      }
    }

    return () => {
      if (sessionSaveInterval) {
        clearInterval(sessionSaveInterval);
        sessionSaveInterval = null;
      }
    };
  });

  // Download state update trigger
  let downloadStateVersion = $state(0);

  // Cache for album download statuses
  const albumDownloadCache = new Map<string, boolean>();

  async function checkAlbumFullyDownloaded(albumId: string): Promise<boolean> {
    // Trigger reactivity with downloadStateVersion
    void downloadStateVersion;
    
    try {
      const isDownloaded = await invoke<boolean>('v2_check_album_fully_cached', { albumId });
      albumDownloadCache.set(albumId, isDownloaded);
      return isDownloaded;
    } catch {
      albumDownloadCache.set(albumId, false);
      return false;
    }
  }

  function getAlbumOfflineCacheStatus(albumId: string): boolean {
    void downloadStateVersion;
    return albumDownloadCache.get(albumId) || false;
  }

  onMount(() => {
    // Bootstrap app (theme, mouse nav, Last.fm restore)
    const { cleanup: cleanupBootstrap } = bootstrapApp();

    // Keyboard navigation
    document.addEventListener('keydown', handleKeydown);

    // Suppress WebKit default context menu globally
    // Custom menus (TrackRow, sidebar, etc.) call e.stopPropagation() so they are unaffected
    const handleGlobalContextMenu = (e: MouseEvent) => e.preventDefault();
    document.addEventListener('contextmenu', handleGlobalContextMenu);

    // Register keybinding actions
    registerAction('playback.toggle', togglePlay);
    registerAction('playback.next', handleSkipForward);
    registerAction('playback.prev', handleSkipBack);
    registerAction('nav.back', navGoBack);
    registerAction('nav.forward', navGoForward);
    registerAction('nav.search', () => sidebarRef?.focusSearch());
    registerAction('ui.focusMode', toggleFocusMode);
    registerAction('ui.queue', toggleQueue);
    registerAction('ui.escape', handleUIEscape);
    registerAction('ui.showShortcuts', () => { isShortcutsModalOpen = true; });
    registerAction('ui.openLink', () => { isLinkResolverOpen = true; });

    // Session save on window close/hide
    const handleBeforeUnload = () => {
      saveSessionBeforeClose();
    };
    window.addEventListener('beforeunload', handleBeforeUnload);

    // Also save when visibility changes (app goes to background)
    const handleVisibilityChange = () => {
      if (document.visibilityState === 'hidden') {
        saveSessionBeforeClose();
      }
    };
    document.addEventListener('visibilitychange', handleVisibilityChange);

    // Start listening for offline cache events (just event listeners, no backend calls)
    startOfflineCacheEventListeners();

    // Initialize playback context store (local state only, no backend calls)
    initPlaybackContextStore();

    // Load infinite play preference
    try {
      const stored = getUserItem('qbz-infinite-play');
      if (stored !== null) {
        infinitePlayEnabled = JSON.parse(stored);
      }
    } catch {
      // Ignore storage errors
    }

    // NOTE: Audio settings are loaded AFTER session is ready (in handleLoginSuccess)
    // to avoid "No active session" errors from per-user state

    // Set up callback for cast disconnect handoff
    setOnAskContinueLocally(async (track, position) => {
      // Ask user if they want to continue locally
      const continueLocally = window.confirm(
        `Continue playing "${track.title}" on this device?`
      );

      if (continueLocally) {
        try {
          // Start local playback
          await playTrack(track, { showLoadingToast: false });

          // Seek to saved position after a short delay
          if (position > 5) {
            setTimeout(async () => {
              try {
                await playerSeek(position);
                console.log('[CastHandoff] Seeked to position:', position);
              } catch (seekErr) {
                console.log('[CastHandoff] Could not restore position:', seekErr);
              }
            }, 1000);
          }
        } catch (err) {
          console.error('[CastHandoff] Failed to resume local playback:', err);
        }
      }

      return continueLocally;
    });

    // Note: loadFavorites() is called in handleLoginSuccess after login is confirmed
    // This prevents API calls before authentication is complete

    // Subscribe to download state changes to trigger reactivity
    const unsubscribeOfflineCache = subscribeOfflineCache(() => {
      downloadStateVersion++;
    });

    // Subscribe to toast state changes
    const unsubscribeToast = subscribeToast((newToast) => {
      toast = newToast;
    });

    // Subscribe to UI state changes
    const unsubscribeUI = subscribeUI(() => {
      const uiState = getUIState();
      // Close network sidebar and lyrics when queue opens
      if (uiState.isQueueOpen && !isQueueOpen) {
        closeContentSidebar('network');
        hideLyricsSidebar();
      }
      isQueueOpen = uiState.isQueueOpen;
      isFullScreenOpen = uiState.isFullScreenOpen;
      isFocusModeOpen = uiState.isFocusModeOpen;
      isCastPickerOpen = uiState.isCastPickerOpen;
      isPlaylistModalOpen = uiState.isPlaylistModalOpen;
      playlistModalMode = uiState.playlistModalMode;
      playlistModalTrackIds = uiState.playlistModalTrackIds;
      playlistModalTracksAreLocal = uiState.playlistModalTracksAreLocal;
      isPlaylistImportOpen = uiState.isPlaylistImportOpen;
    });

    // Subscribe to auth state changes
    const unsubscribeAuth = subscribeAuth(() => {
      const authState = getAuthState();
      isLoggedIn = authState.isLoggedIn;
      userInfo = authState.userInfo;
    });

    // Initialize and subscribe to sidebar state changes
    initSidebarStore();
    const unsubscribeSidebar = subscribeSidebar(() => {
      sidebarExpanded = getIsExpanded();
    });

    // Initialize and subscribe to title bar state changes
    initTitleBarStore();
    const unsubscribeTitleBar = subscribeTitleBar(() => {
      showTitleBar = shouldShowTitleBar();
    });

    // Subscribe to offline state changes
    const unsubscribeOffline = subscribeOffline(() => {
      offlineStatus = getOfflineStatus();
      // Sync offline mode to queue store for track availability
      setQueueOfflineMode(offlineStatus.isOffline);
    });

    // Subscribe to navigation state changes
    const unsubscribeNav = subscribeNav(() => {
      const navState = getNavigationState();
      activeView = navState.activeView;
      selectedPlaylistId = navState.selectedPlaylistId;
    });

    // Subscribe to player state changes
    let prevTrackId: number | null = null;
    const unsubscribePlayer = subscribePlayer(() => {
      const playerState = getPlayerState();
      const wasPlaying = isPlaying;
      currentTrack = playerState.currentTrack;
      isPlaying = playerState.isPlaying;
      currentTime = playerState.currentTime;
      duration = playerState.duration;
      volume = playerState.volume;
      isFavorite = playerState.isFavorite;
      normalizationGain = playerState.normalizationGain;

      // Save position during playback (debounced to every 5s)
      if (isPlaying && currentTrack && currentTime > 0) {
        debouncedSavePosition(Math.floor(currentTime));
      }

      // Flush position save immediately when pausing
      if (wasPlaying && !isPlaying && currentTrack && currentTime > 0) {
        flushPositionSave(Math.floor(currentTime));
      }

      // Full session save on track change or pause (debounced 2s)
      const trackId = currentTrack?.id ?? null;
      if (trackId !== prevTrackId) {
        prevTrackId = trackId;
        if (trackId !== null) debouncedFullSessionSave();
      }
      if (wasPlaying && !isPlaying && currentTrack) {
        debouncedFullSessionSave();
      }

      // MiniPlayer IPC - DISABLED: incomplete feature, causes unnecessary IPC overhead
      // if (currentTrack) {
      //   emitTo('miniplayer', 'miniplayer:track', {
      //     id: currentTrack.id,
      //     title: currentTrack.title,
      //     artist: currentTrack.artist,
      //     artwork: currentTrack.artwork,
      //     isPlaying,
      //   }).catch(() => {}); // Ignore if miniplayer not open
      // }
      // emitTo('miniplayer', 'miniplayer:playback', {
      //   isPlaying,
      //   currentTime,
      //   duration,
      // }).catch(() => {}); // Ignore if miniplayer not open
    });

    // Subscribe to queue state changes
    const unsubscribeQueue = subscribeQueue(() => {
      const queueState = getQueueState();
      queue = queueState.queue;
      queueTotalTracks = queueState.queueTotalTracks;
      isShuffle = queueState.isShuffle;
      repeatMode = queueState.repeatMode;
    });

    // Subscribe to lyrics state changes
    const unsubscribeLyrics = subscribeLyrics(() => {
      const state = getLyricsState();
      lyricsStatus = state.status;
      lyricsError = state.error;
      lyricsLines = state.lines;
      lyricsIsSynced = state.isSynced;
      lyricsActiveIndex = state.activeIndex;
      lyricsActiveProgress = state.activeProgress;
      // Close network sidebar and queue when lyrics opens
      if (state.sidebarVisible && !lyricsSidebarVisible) {
        closeContentSidebar('network');
        closeQueue();
      }
      lyricsSidebarVisible = state.sidebarVisible;
    });

    // Subscribe to content sidebar for mutual exclusion (network closes lyrics/queue)
    const unsubscribeContentSidebar = subscribeContentSidebar((active: ContentSidebarType) => {
      if (active === 'network') {
        hideLyricsSidebar();
        closeQueue();
      }
    });

    // Subscribe to cast state changes
    const unsubscribeCast = subscribeCast(() => {
      isCastConnected = isCasting();
    });

    // Start lyrics watcher for track changes
    startLyricsWatching();

    // Set up track ended callback for auto-advance
    setOnTrackEnded(async () => {
      if (!isAutoplayEnabled()) {
        setQueueEnded(true);
        await stopPlayback();
        setIsPlaying(false);
        return;
      }
      const previousTrackId = currentTrack?.id ?? null;
      const nextTrackResult = await nextTrack();
      if (nextTrackResult) {
        // Defensive fallback for issue #80:
        // if backend returns same track on auto-advance while repeat-one is off,
        // force one additional advance attempt to break one-track loops.
        if (
          previousTrackId !== null &&
          nextTrackResult.id === previousTrackId &&
          repeatMode !== 'one'
        ) {
          console.warn(
            '[Player] Auto-advance returned same track id, forcing one extra nextTrack()',
            previousTrackId
          );
          const forcedNext = await nextTrack();
          if (forcedNext && forcedNext.id !== previousTrackId) {
            await playQueueTrack(forcedNext);
            return;
          }
        }
        await playQueueTrack(nextTrackResult);
      } else {
        // Queue ended - stop playback and clear player
        setQueueEnded(true);
        await stopPlayback();
        setIsPlaying(false);
      }
    });

    // Set up resume-from-stop callback: re-play the queue's current track
    setOnResumeFromStop(async () => {
      const queueState = await getBackendQueueState();
      if (!queueState) return;
      const tryQueueIndices = async (indices: number[]): Promise<BackendQueueTrack | null> => {
        for (const idx of indices) {
          if (idx < 0) continue;
          if (queueState.total_tracks > 0 && idx >= queueState.total_tracks) continue;
          const selected = await playQueueIndex(idx);
          if (selected) {
            console.log('[Player] Resume from stop: selected queue index', idx);
            return selected;
          }
        }
        return null;
      };

      // Normal case: current track already selected in queue, replay it.
      if (queueState.current_track && queueState.current_index !== null) {
        console.log('[Player] Resuming from stop, replaying queue index:', queueState.current_index);
        await playQueueTrack(queueState.current_track);
        return;
      }

      // Hybrid backend state: queue index exists but current_track is missing.
      // This can happen after queue mutations where selection metadata lags.
      if (!queueState.current_track && queueState.current_index !== null) {
        console.log('[Player] Resume from stop: hybrid queue state, trying [current,0,1]:', queueState.current_index);
        const selected = await tryQueueIndices([queueState.current_index, 0, 1]);
        if (selected) {
          await playQueueTrack(selected);
          return;
        }
      }

      // Empty chamber case: queue exists but no current track selected yet.
      if (queueState.current_index === null && queueState.upcoming.length > 0) {
        const firstUpcoming = queueState.upcoming[0];
        const firstTrack = await tryQueueIndices([0, 1]);

        // Preferred path: bind queue index first, then play selected track.
        if (firstTrack) {
          console.log('[Player] Resuming from stop, starting queue from first valid index');
          await playQueueTrack(firstTrack);
          return;
        }

        // Fallback #1: ask backend to advance/select next track from empty chamber state.
        // Some queue states can reject play_index(0) while still accepting next_track.
        const advancedTrack = await nextTrack();
        if (advancedTrack) {
          console.log('[Player] playQueueIndex(0) failed, resumed via nextTrack()');
          await playQueueTrack(advancedTrack);
          return;
        }

        // Fallback #2: play the first upcoming track directly.
        // Last-resort to avoid a visible no-op on Play button.
        console.warn('[Player] Queue resume fallback: playing first upcoming track directly');
        await playQueueTrack(firstUpcoming);
      }
    });

    // Gapless: provide callback to get next track ID for pre-queuing
    setGaplessGetNextTrackId(() => {
      try {
        const queueState = getQueueState();
        if (queueState.queue.length > 0) {
          const firstId = Number(queueState.queue[0].id);
          const currentId = currentTrack?.id ?? null;
          if (!Number.isNaN(firstId) && firstId > 0 && firstId !== currentId) {
            return firstId;
          }

          // Defensive fallback: skip stale first slot if it matches current track.
          if (queueState.queue.length > 1) {
            const secondId = Number(queueState.queue[1].id);
            if (!Number.isNaN(secondId) && secondId > 0 && secondId !== currentId) {
              return secondId;
            }
          }
        }
      } catch {
        // Ignore
      }
      return null;
    });

    // Gapless: handle transition when backend switches to pre-queued track
    setOnGaplessTransition(async (trackId: number) => {
      console.log('[Gapless] Handling transition to track', trackId);
      // Advance the queue to match backend state
      const advanced = await nextTrack();
      if (advanced && advanced.id === trackId) {
        // Queue advanced successfully — update UI metadata
        await playQueueTrack(advanced, undefined, true);
      } else {
        // Queue mismatch — sync from backend
        console.warn('[Gapless] Queue mismatch, syncing state');
        await syncQueueState();
      }
    });

    // Set up tray icon event listeners
    // Using disposed flag pattern to prevent listener leaks on fast unmount/HMR
    let disposed = false;
    let unlistenTrayPlayPause: UnlistenFn | null = null;
    let unlistenTrayNext: UnlistenFn | null = null;
    let unlistenTrayPrevious: UnlistenFn | null = null;
    let unlistenMediaControls: UnlistenFn | null = null;
    let unlistenLinkResolved: UnlistenFn | null = null;

    (async () => {
      const unlisten1 = await listen('tray:play_pause', () => {
        console.log('[Tray] Play/Pause');
        togglePlay();
      });
      if (disposed) { unlisten1(); return; }
      unlistenTrayPlayPause = unlisten1;

      const unlisten2 = await listen('tray:next', async () => {
        console.log('[Tray] Next');
        await handleSkipForward();
      });
      if (disposed) { unlisten2(); return; }
      unlistenTrayNext = unlisten2;

      const unlisten3 = await listen('tray:previous', async () => {
        console.log('[Tray] Previous');
        await handleSkipBack();
      });
      if (disposed) { unlisten3(); return; }
      unlistenTrayPrevious = unlisten3;

      const unlisten4 = await listen('media:control', async (event) => {
        const payload = event.payload as MediaControlPayload;
        if (!payload?.action) return;

        const playerState = getPlayerState();

        switch (payload.action) {
          case 'play':
            if (!playerState.isPlaying) {
              await togglePlay();
            }
            break;
          case 'pause':
            if (playerState.isPlaying) {
              await togglePlay();
            }
            break;
          case 'toggle':
            await togglePlay();
            break;
          case 'next':
            await handleSkipForward();
            break;
          case 'previous':
            await handleSkipBack();
            break;
          case 'stop':
            await stopPlayback();
            break;
          case 'seek': {
            const direction = payload.direction === 'backward' ? -1 : 1;
            const target = playerState.currentTime + direction * MEDIA_SEEK_FALLBACK_SECS;
            await playerSeek(target);
            break;
          }
          case 'seek_by': {
            if (typeof payload.offset_secs === 'number') {
              await playerSeek(playerState.currentTime + payload.offset_secs);
            }
            break;
          }
          case 'set_position': {
            if (typeof payload.position_secs === 'number') {
              await playerSeek(payload.position_secs);
            }
            break;
          }
          case 'set_volume': {
            if (typeof payload.volume === 'number') {
              const normalized = Math.max(0, Math.min(1, payload.volume));
              const newVolume = Math.round(normalized * 100);
              // Only update if volume actually changed (prevents MPRIS feedback loop)
              if (newVolume !== volume) {
                await playerSetVolume(newVolume);
              }
            }
            break;
          }
          default:
            break;
        }
      });
      if (disposed) { unlisten4(); return; }
      unlistenMediaControls = unlisten4;

      const unlisten5 = await listen('link:resolved', (event) => {
        const resolved = event.payload as { type: string; id: string | number };
        if (resolved?.type) {
          handleResolvedLink(resolved);
        }
      });
      if (disposed) { unlisten5(); return; }
      unlistenLinkResolved = unlisten5;
    })();

    return () => {
      // Mark as disposed to prevent listener leaks from pending async registrations
      disposed = true;
      // Clean up tray event listeners
      unlistenTrayPlayPause?.();
      unlistenTrayNext?.();
      unlistenTrayPrevious?.();
      unlistenMediaControls?.();
      unlistenLinkResolved?.();
      // Save session before cleanup
      saveSessionBeforeClose();
      cleanupBootstrap();
      document.removeEventListener('keydown', handleKeydown);
      document.removeEventListener('contextmenu', handleGlobalContextMenu);
      unregisterAll(); // Cleanup keybinding actions
      window.removeEventListener('beforeunload', handleBeforeUnload);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      stopOfflineCacheEventListeners();
      unsubscribeOfflineCache();
      unsubscribeToast();
      unsubscribeUI();
      unsubscribeAuth();
      unsubscribeSidebar();
      unsubscribeTitleBar();
      unsubscribeOffline();
      unsubscribeNav();
      unsubscribePlayer();
      unsubscribeQueue();
      unsubscribeLyrics();
      unsubscribeContentSidebar();
      unsubscribeCast();
      stopLyricsWatching();
      stopActiveLineUpdates();
      stopPolling();
      cleanupPlayback();
    };
  });

  // Sync queue state when opening queue panel (including history and remaining count)
  $effect(() => {
    if (isQueueOpen) {
      syncQueueState();
      updateQueueCounts();
    }
  });

  // Update remaining count when track changes while queue is open
  $effect(() => {
    // Track the currentTrack to trigger on change
    const trackId = currentTrack?.id;
    if (isQueueOpen && trackId !== undefined) {
      updateQueueCounts();
    }
  });

  // Sync queue state when immersive player is open and track changes
  $effect(() => {
    const trackId = currentTrack?.id;
    const immersiveOpen = isFullScreenOpen || isFocusModeOpen;
    if (immersiveOpen && trackId !== undefined) {
      syncQueueState();
      updateQueueCounts(); // Also sync history for coverflow
    }
  });

  // Helper function to fetch and update queue counts and history
  async function updateQueueCounts() {
    const state = await getBackendQueueState();
    if (state) {
      // In shuffle mode current_index is an absolute index in original order,
      // not a playback-position counter, so remaining is "all except current".
      if (state.shuffle && state.current_track && state.total_tracks > 0) {
        queueRemainingTracks = state.total_tracks - 1;
      } else if (state.current_index !== null && state.total_tracks > 0) {
        queueRemainingTracks = state.total_tracks - state.current_index - 1;
      } else {
        queueRemainingTracks = state.total_tracks;
      }

      if (state.history) {
        historyTracks = state.history.map(trk => ({
          id: String(trk.id),
          artwork: trk.artwork_url || '',
          title: trk.title,
          artist: trk.artist,
          duration: formatDuration(trk.duration_secs),
          trackId: trk.id
        }));
      }
    }
  }

  // Start/stop lyrics active line updates based on playback state and visibility
  $effect(() => {
    const lyricsVisible = lyricsSidebarVisible || isFocusModeOpen || isFullScreenOpen;
    if (isPlaying && lyricsIsSynced && lyricsVisible) {
      startActiveLineUpdates();
    } else {
      stopActiveLineUpdates();
    }
    // Cleanup on effect re-run or component unmount
    return () => {
      stopActiveLineUpdates();
    };
  });

  // Derived values for NowPlayingBar
  const currentQueueTrack = $derived<QueueTrack | null>(currentTrack ? {
    id: String(currentTrack.id),
    artwork: currentTrack.artwork,
    title: currentTrack.title,
    artist: currentTrack.artist,
    duration: formatDuration(currentTrack.duration),
    trackId: currentTrack.id // For favorite checking in QueuePanel
  } : null);
</script>

{#if !isLoggedIn}
  <LoginView onLoginSuccess={handleLoginSuccess} onStartOffline={handleStartOffline} />
{:else}
  <div class="app" class:no-titlebar={!showTitleBar}>
    <!-- Custom Title Bar (CSD) -->
    {#if showTitleBar}
      <TitleBar />
    {/if}

    <div class="app-body">
    <!-- Sidebar -->
    <Sidebar
      bind:this={sidebarRef}
      {activeView}
      {selectedPlaylistId}
      onNavigate={navigateTo}
      onPlaylistSelect={selectPlaylist}
      onCreatePlaylist={openCreatePlaylist}
      onImportPlaylist={openImportPlaylist}
      onPlaylistManagerClick={() => navigateTo('playlist-manager')}
      onSettingsClick={() => navigateTo('settings')}
      onKeybindingsClick={() => isKeybindingsSettingsOpen = true}
      onAboutClick={() => isAboutModalOpen = true}
      onLogout={handleLogout}
      userName={userInfo?.userName || 'User'}
      subscription={userInfo?.subscription || 'Qobuz™'}
      isExpanded={sidebarExpanded}
      onToggle={toggleSidebar}
      showTitleBar={showTitleBar}
      {showPurchases}
    />

    <!-- Content Area (main + lyrics sidebar) -->
    <div class="content-area">
    <!-- Main Content -->
    <main class="main-content" bind:this={mainContentEl}>
      {#if activeView === 'home'}
        {#if offlineStatus.isOffline}
          <OfflinePlaceholder
            reason={offlineStatus.reason}
            onGoToLibrary={() => navigateTo('library')}
          />
        {:else}
          <HomeView
            userName={userInfo?.userName}
            onAlbumClick={handleAlbumClick}
            onAlbumPlay={playAlbumById}
            onAlbumPlayNext={queueAlbumNextById}
            onAlbumPlayLater={queueAlbumLaterById}
            onAlbumShareQobuz={shareAlbumQobuzLinkById}
            onAlbumShareSonglink={shareAlbumSonglinkById}
            onAlbumDownload={downloadAlbumById}
            onOpenAlbumFolder={openAlbumFolderById}
            onReDownloadAlbum={reDownloadAlbumById}
            checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
            {downloadStateVersion}
            onArtistClick={handleArtistClick}
            onTrackPlay={handleDisplayTrackPlay}
            onTrackPlayNext={queueDisplayTrackNext}
            onTrackPlayLater={queueDisplayTrackLater}
            onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
            onAddAlbumToPlaylist={addAlbumToPlaylistById}
            onTrackShareQobuz={shareQobuzTrackLink}
            onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
            onTrackGoToAlbum={handleAlbumClick}
            onTrackGoToArtist={handleArtistClick}
            onTrackShowInfo={showTrackInfo}
            onTrackDownload={handleDisplayTrackDownload}
            onTrackReDownload={handleDisplayTrackDownload}
            onTrackRemoveDownload={handleTrackRemoveDownload}
            checkTrackDownloaded={checkTrackDownloaded}
            getTrackOfflineCacheStatus={getTrackOfflineCacheStatus}
            onPlaylistClick={selectPlaylist}
            onPlaylistPlay={playPlaylistById}
            onPlaylistPlayNext={queuePlaylistNextById}
            onPlaylistPlayLater={queuePlaylistLaterById}
            onPlaylistCopyToLibrary={copyPlaylistToLibraryById}
            onPlaylistShareQobuz={sharePlaylistQobuzLinkById}
            activeTrackId={currentTrack?.id ?? null}
            isPlaybackActive={isPlaying}
            onNavigateNewReleases={() => navigateTo('discover-new-releases')}
            onNavigateIdealDiscography={() => navigateTo('discover-ideal-discography')}
            onNavigateTopAlbums={() => navigateTo('discover-top-albums')}
            onNavigateQobuzissimes={() => navigateTo('discover-qobuzissimes')}
            onNavigateAlbumsOfTheWeek={() => navigateTo('discover-albums-of-the-week')}
            onNavigatePressAccolades={() => navigateTo('discover-press-accolades')}
            onNavigateQobuzPlaylists={() => navigateTo('discover-playlists')}
            onNavigateDailyQ={() => navigateTo('dailyq')}
            onNavigateWeeklyQ={() => navigateTo('weeklyq')}
            onNavigateFavQ={() => navigateTo('favq')}
            onNavigateTopQ={() => navigateTo('topq')}
          />
        {/if}
      {:else if activeView === 'search'}
        {#if offlineStatus.isOffline}
          <OfflinePlaceholder
            reason={offlineStatus.reason}
            onGoToLibrary={() => navigateTo('library')}
          />
        {:else}
          <SearchView
            onAlbumClick={handleAlbumClick}
            onAlbumPlay={playAlbumById}
            onAlbumPlayNext={queueAlbumNextById}
            onAlbumPlayLater={queueAlbumLaterById}
            onAlbumShareQobuz={shareAlbumQobuzLinkById}
            onAlbumShareSonglink={shareAlbumSonglinkById}
            onAlbumDownload={downloadAlbumById}
            onOpenAlbumFolder={openAlbumFolderById}
            onReDownloadAlbum={reDownloadAlbumById}
            checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
            {downloadStateVersion}
            onTrackPlay={handleTrackPlay}
            onTrackPlayNext={queueQobuzTrackNext}
            onTrackPlayLater={queueQobuzTrackLater}
            onTrackAddFavorite={handleAddToFavorites}
            onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
            onAddAlbumToPlaylist={addAlbumToPlaylistById}
            onTrackShareQobuz={shareQobuzTrackLink}
            onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
            onTrackGoToAlbum={handleAlbumClick}
            onTrackGoToArtist={handleArtistClick}
            onTrackShowInfo={showTrackInfo}
            onTrackDownload={handleSearchTrackDownload}
            onTrackReDownload={handleSearchTrackDownload}
            onTrackRemoveDownload={handleTrackRemoveDownload}
            checkTrackDownloaded={checkTrackDownloaded}
            onArtistClick={handleArtistClick}
            onPlaylistClick={selectPlaylist}
            onPlaylistPlay={playPlaylistById}
            onPlaylistPlayNext={queuePlaylistNextById}
            onPlaylistPlayLater={queuePlaylistLaterById}
            onPlaylistCopyToLibrary={copyPlaylistToLibraryById}
            onPlaylistShareQobuz={sharePlaylistQobuzLinkById}
            activeTrackId={currentTrack?.id ?? null}
            isPlaybackActive={isPlaying}
          />
        {/if}
      {:else if activeView === 'settings'}
        <SettingsView
          onBack={navGoBack}
          onLogout={handleLogout}
          onBlacklistManagerClick={() => navigateTo('blacklist-manager')}
          onPurchasesToggle={(v) => { showPurchases = v; }}
          userName={userInfo?.userName}
          subscription={userInfo?.subscription}
          subscriptionValidUntil={userInfo?.subscriptionValidUntil}
          showTitleBar={showTitleBar}
        />
      {:else if activeView === 'album' && !selectedAlbum}
        <!-- Defensive fallback: album view active but no data loaded (#43) -->
        <div class="view-error">
          <p>{$t('toast.failedLoadAlbum')}</p>
          <button class="view-error-back" onclick={navGoBack}>{$t('actions.back')}</button>
        </div>
      {:else if activeView === 'album' && selectedAlbum}
        <AlbumDetailView
          album={selectedAlbum}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
          onBack={navGoBack}
          onArtistClick={() => selectedAlbum?.artistId && handleArtistClick(selectedAlbum.artistId)}
          onLabelClick={handleLabelClick}
          onTrackPlay={handleAlbumTrackPlay}
          onTrackPlayNext={handleAlbumTrackPlayNext}
          onTrackPlayLater={handleAlbumTrackPlayLater}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onTrackShowInfo={showTrackInfo}
          onPlayAll={handlePlayAllAlbum}
          onShuffleAll={handleShuffleAlbum}
          onPlayAllNext={handleAddAlbumToQueueNext}
          onPlayAllLater={handleAddAlbumToQueueLater}
          onAddTrackToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onAddAlbumToPlaylist={() => addAlbumToPlaylist(selectedAlbum)}
          onTrackDownload={handleTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          onTrackReDownload={handleTrackReDownload}
          getTrackOfflineCacheStatus={getTrackOfflineCacheStatus}
          onDownloadAlbum={handleDownloadAlbum}
          onShareAlbumQobuz={shareAlbumQobuzLink}
          onShareAlbumSonglink={shareAlbumSonglink}
          onOpenAlbumFolder={handleOpenAlbumFolder}
          onReDownloadAlbum={handleReDownloadAlbum}
          {downloadStateVersion}
          artistAlbums={albumArtistAlbums}
          onRelatedAlbumClick={handleAlbumClick}
          onRelatedAlbumPlay={playAlbumById}
          onRelatedAlbumPlayNext={queueAlbumNextById}
          onRelatedAlbumPlayLater={queueAlbumLaterById}
          onRelatedAlbumDownload={downloadAlbumById}
          onRelatedAlbumShareQobuz={shareAlbumQobuzLinkById}
          onRelatedAlbumShareSonglink={shareAlbumSonglinkById}
          onViewArtistDiscography={handleViewArtistDiscography}
          checkRelatedAlbumDownloaded={checkAlbumFullyDownloaded}
          onShowAlbumCredits={() => selectedAlbum && showAlbumCredits(selectedAlbum.id)}
          onCreateAlbumRadio={handleCreateAlbumRadio}
        />
      {:else if activeView === 'artist' && selectedArtist}
        <ArtistDetailView
          artist={selectedArtist}
          initialTopTracks={artistTopTracks}
          initialSimilarArtists={artistSimilarArtists}
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onLoadMoreReleases={loadMoreArtistReleases}
          isLoadingMore={isArtistAlbumsLoading}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queueQobuzTrackNext}
          onTrackPlayLater={queueQobuzTrackLater}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onPlaylistClick={selectPlaylist}
          onLabelClick={handleLabelClick}
          onMusicianClick={handleMusicianClick}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'musician' && selectedMusician}
        <MusicianPageView
          musician={selectedMusician}
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'label' && selectedLabel}
        <LabelView
          labelId={selectedLabel.id}
          labelName={selectedLabel.name}
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
          onLabelClick={handleLabelClick}
          onNavigateReleases={handleNavigateLabelReleases}
          onPlaylistClick={selectPlaylist}
          onPlaylistPlay={playPlaylistById}
          onPlaylistPlayNext={queuePlaylistNextById}
          onPlaylistPlayLater={queuePlaylistLaterById}
          onPlaylistCopyToLibrary={copyPlaylistToLibraryById}
          onPlaylistShareQobuz={sharePlaylistQobuzLinkById}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queueQobuzTrackNext}
          onTrackPlayLater={queueQobuzTrackLater}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackGoToAlbum={handleAlbumClick}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'label-releases' && selectedLabel}
        <LabelReleasesView
          labelId={selectedLabel.id}
          labelName={selectedLabel.name}
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'library' || activeView === 'library-album'}
        <LocalLibraryView
          onTrackPlay={handleLocalTrackPlay}
          onTrackPlayNext={queueLocalTrackNext}
          onTrackPlayLater={queueLocalTrackLater}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId], true)}
          onSetLocalQueue={handleSetLocalQueue}
          onQobuzArtistClick={handleArtistClick}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'playlist' && selectedPlaylistId}
        <PlaylistDetailView
          playlistId={selectedPlaylistId}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
          onBack={navGoBack}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queuePlaylistTrackNext}
          onTrackPlayLater={queuePlaylistTrackLater}
          onTrackAddFavorite={handleAddToFavorites}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onTrackShowInfo={showTrackInfo}
          onTrackDownload={handleDisplayTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          onTrackReDownload={handleDisplayTrackDownload}
          onTrackCreateQbzRadio={handleCreateQbzTrackRadio}
          onTrackCreateQobuzRadio={handleCreateQobuzTrackRadio}
          getTrackOfflineCacheStatus={getTrackOfflineCacheStatus}
          {downloadStateVersion}
          onLocalTrackPlay={handleLocalTrackPlay}
          onLocalTrackPlayNext={queueLocalTrackNext}
          onLocalTrackPlayLater={queueLocalTrackLater}
          onSetLocalQueue={handleSetLocalQueue}
          onPlaylistCountUpdate={(playlistId, qobuzCount, localCount) =>
            sidebarRef?.updatePlaylistCounts(playlistId, qobuzCount, localCount)
          }
          onPlaylistUpdated={() => {
            sidebarRef?.refreshPlaylists();
            sidebarRef?.refreshPlaylistSettings();
            sidebarRef?.refreshLocalTrackCounts();
          }}
          onPlaylistDeleted={() => {
            sidebarRef?.refreshPlaylists();
            sidebarRef?.refreshPlaylistSettings();
            navGoBack();
          }}
        />
      {:else if isFavoritesView(activeView)}
        {#if offlineStatus.isOffline}
          <OfflinePlaceholder
            reason={offlineStatus.reason}
            onGoToLibrary={() => navigateTo('library')}
          />
        {:else}
          <FavoritesView
            onBack={navGoBack}
            onAlbumClick={handleAlbumClick}
            onAlbumPlay={playAlbumById}
            onAlbumPlayNext={queueAlbumNextById}
            onAlbumPlayLater={queueAlbumLaterById}
            onAlbumShareQobuz={shareAlbumQobuzLinkById}
            onAlbumShareSonglink={shareAlbumSonglinkById}
            onAlbumDownload={downloadAlbumById}
            onOpenAlbumFolder={openAlbumFolderById}
            onReDownloadAlbum={reDownloadAlbumById}
            checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
            {downloadStateVersion}
            onTrackPlay={handleDisplayTrackPlay}
            onArtistClick={handleArtistClick}
            onTrackPlayNext={queuePlaylistTrackNext}
            onTrackPlayLater={queuePlaylistTrackLater}
            onTrackAddFavorite={handleAddToFavorites}
            onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
            onTrackShareQobuz={shareQobuzTrackLink}
            onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
            onTrackGoToAlbum={handleAlbumClick}
            onTrackGoToArtist={handleArtistClick}
            onTrackShowInfo={showTrackInfo}
            onTrackDownload={handleDisplayTrackDownload}
            onTrackRemoveDownload={handleTrackRemoveDownload}
            onTrackReDownload={handleDisplayTrackDownload}
            onTrackCreateQbzRadio={handleCreateQbzTrackRadio}
            onTrackCreateQobuzRadio={handleCreateQobuzTrackRadio}
            getTrackOfflineCacheStatus={getTrackOfflineCacheStatus}
            onPlaylistSelect={selectPlaylist}
            onPlaylistPlay={playPlaylistById}
            onPlaylistPlayNext={queuePlaylistNextById}
            onPlaylistPlayLater={queuePlaylistLaterById}
            onPlaylistRemoveFavorite={removePlaylistFavoriteById}
            onPlaylistShareQobuz={sharePlaylistQobuzLinkById}
            selectedTab={getFavoritesTabFromView(activeView) ?? favoritesDefaultTab}
            onTabNavigate={(tab) => navigateToFavorites(tab)}
            activeTrackId={currentTrack?.id ?? null}
            isPlaybackActive={isPlaying}
          />
        {/if}
      {:else if activeView === 'playlist-manager'}
        <PlaylistManagerView
          onBack={navGoBack}
          onPlaylistSelect={selectPlaylist}
          onPlaylistsChanged={() => {
            sidebarRef?.refreshPlaylists();
            sidebarRef?.refreshPlaylistSettings();
            sidebarRef?.refreshLocalTrackCounts();
          }}
        />
      {:else if activeView === 'blacklist-manager'}
        <BlacklistManagerView
          onBack={navGoBack}
          onArtistSelect={handleArtistClick}
        />
      {:else if activeView === 'discover-new-releases'}
        <DiscoverBrowseView
          endpointType="newReleases"
          titleKey="discover.newReleases"
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'discover-ideal-discography'}
        <DiscoverBrowseView
          endpointType="idealDiscography"
          titleKey="discover.idealDiscography"
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'discover-top-albums'}
        <DiscoverBrowseView
          endpointType="mostStreamed"
          titleKey="discover.topAlbums"
          showRanking={true}
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'discover-qobuzissimes'}
        <DiscoverBrowseView
          endpointType="qobuzissimes"
          titleKey="discover.qobuzissimes"
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'discover-albums-of-the-week'}
        <DiscoverBrowseView
          endpointType="albumOfTheWeek"
          titleKey="discover.albumsOfTheWeek"
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'discover-press-accolades'}
        <DiscoverBrowseView
          endpointType="pressAward"
          titleKey="discover.pressAccolades"
          onBack={navGoBack}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
          onAlbumPlayNext={queueAlbumNextById}
          onAlbumPlayLater={queueAlbumLaterById}
          onAlbumShareQobuz={shareAlbumQobuzLinkById}
          onAlbumShareSonglink={shareAlbumSonglinkById}
          onAlbumDownload={downloadAlbumById}
          onOpenAlbumFolder={openAlbumFolderById}
          onReDownloadAlbum={reDownloadAlbumById}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          checkAlbumFullyDownloaded={checkAlbumFullyDownloaded}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'discover-playlists'}
        <DiscoverPlaylistsBrowseView
          onBack={navGoBack}
          onPlaylistClick={selectPlaylist}
          onPlaylistPlay={playPlaylistById}
          onPlaylistPlayNext={queuePlaylistNextById}
          onPlaylistPlayLater={queuePlaylistLaterById}
          onPlaylistCopyToLibrary={copyPlaylistToLibraryById}
          onPlaylistShareQobuz={sharePlaylistQobuzLinkById}
        />
      {:else if activeView === 'dailyq'}
        <DynamicSuggestView
          onBack={navGoBack}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queueDisplayTrackNext}
          onTrackPlayLater={queueDisplayTrackLater}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onTrackShowInfo={showTrackInfo}
          onTrackDownload={handleDisplayTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          onTrackReDownload={handleDisplayTrackDownload}
          getTrackOfflineCacheStatus={getTrackOfflineCacheStatus}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'weeklyq'}
        <WeeklySuggestView
          onBack={navGoBack}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queueDisplayTrackNext}
          onTrackPlayLater={queueDisplayTrackLater}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onTrackShowInfo={showTrackInfo}
          onTrackDownload={handleDisplayTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          onTrackReDownload={handleDisplayTrackDownload}
          getTrackOfflineCacheStatus={getTrackOfflineCacheStatus}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'favq'}
        <FavQView
          onBack={navGoBack}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queueDisplayTrackNext}
          onTrackPlayLater={queueDisplayTrackLater}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onTrackShowInfo={showTrackInfo}
          onTrackDownload={handleDisplayTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          onTrackReDownload={handleDisplayTrackDownload}
          getTrackOfflineCacheStatus={getTrackOfflineCacheStatus}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'topq'}
        <TopQView
          onBack={navGoBack}
          onTrackPlay={handleDisplayTrackPlay}
          onTrackPlayNext={queueDisplayTrackNext}
          onTrackPlayLater={queueDisplayTrackLater}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId])}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onTrackShowInfo={showTrackInfo}
          onTrackDownload={handleDisplayTrackDownload}
          onTrackRemoveDownload={handleTrackRemoveDownload}
          onTrackReDownload={handleDisplayTrackDownload}
          getTrackOfflineCacheStatus={getTrackOfflineCacheStatus}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'purchases'}
        <PurchasesView
          onAlbumClick={handlePurchaseAlbumClick}
          onArtistClick={handleArtistClick}
          onTrackPlay={handleDisplayTrackPlay}
          onAlbumPlay={playAlbumById}
          activeTrackId={currentTrack?.id}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'purchase-album' && selectedPurchaseAlbumId}
        <PurchaseAlbumDetailView
          albumId={selectedPurchaseAlbumId}
          onBack={navGoBack}
          onArtistClick={handleArtistClick}
          onTrackPlay={handleDisplayTrackPlay}
          onAlbumPlay={playAlbumById}
          activeTrackId={currentTrack?.id}
          isPlaybackActive={isPlaying}
        />
      {/if}
    </main>

    {#if showGlobalBackToTop}
      <button class="back-to-top-global" onclick={globalScrollToTop} title="Back to top">
        <ChevronUp size={20} />
      </button>
    {/if}

    <!-- Lyrics Sidebar -->
    {#if lyricsSidebarVisible && !isQueueOpen}
      <LyricsSidebar
        title={currentTrack?.title}
        artist={currentTrack?.artist}
        lines={lyricsLines.map(l => ({ text: l.text }))}
        activeIndex={lyricsActiveIndex}
        activeProgress={lyricsActiveProgress}
        isSynced={lyricsIsSynced}
        isLoading={lyricsStatus === 'loading'}
        error={lyricsStatus === 'error' ? lyricsError : (lyricsStatus === 'not_found' ? $t('player.noLyrics') : null)}
      />
    {/if}

    <!-- Queue Sidebar -->
    {#if isQueueOpen}
      <QueuePanel
        currentTrack={currentQueueTrack ?? undefined}
        upcomingTracks={queue}
        {queueTotalTracks}
        {queueRemainingTracks}
        {historyTracks}
        isRadioMode={getCurrentContext()?.type === 'radio'}
        onPlayTrack={handleQueueTrackPlay}
        onPlayHistoryTrack={handlePlayHistoryTrack}
        onClearQueue={handleClearQueue}
        onSaveAsPlaylist={handleSaveQueueAsPlaylist}
        onReorderTrack={handleQueueReorder}
        onToggleInfinitePlay={handleToggleInfinitePlay}
        {infinitePlayEnabled}
        {isPlaying}
        onRemoveFromQueue={handleRemoveFromQueue}
        onAddToPlaylist={handleQueueTrackAddToPlaylist}
        onShowTrackInfo={handleQueueTrackInfo}
      />
    {/if}
    </div>
    </div><!-- end app-body -->

    <!-- Now Playing Bar -->
    {#if currentTrack}
      <NowPlayingBar
        artwork={currentTrack.artwork}
        trackTitle={currentTrack.title}
        artist={currentTrack.artist}
        album={currentTrack.album}
        quality={currentTrack.quality}
        bitDepth={currentTrack.bitDepth}
        samplingRate={currentTrack.samplingRate}
        originalBitDepth={currentTrack.originalBitDepth}
        originalSamplingRate={currentTrack.originalSamplingRate}
        format={currentTrack.format}
        {isPlaying}
        onTogglePlay={togglePlay}
        onSkipBack={handleSkipBack}
        onSkipForward={handleSkipForward}
        {currentTime}
        {duration}
        onSeek={handleSeek}
        {volume}
        onVolumeChange={handleVolumeChange}
        {isShuffle}
        onToggleShuffle={toggleShuffle}
        {repeatMode}
        onToggleRepeat={toggleRepeat}
        {isFavorite}
        onToggleFavorite={toggleFavorite}
        onAddToPlaylist={openAddToPlaylistModal}
        onOpenQueue={toggleQueue}
        onOpenFullScreen={openFullScreen}
        onCast={openCastPicker}
        {isCastConnected}
        onToggleLyrics={toggleLyricsSidebar}
        lyricsActive={lyricsSidebarVisible}
        onArtistClick={() => {
          if (currentTrack?.isLocal) {
            showToast($t('toast.localTrackSearch'), 'info');
          } else if (currentTrack?.artistId) {
            handleArtistClick(currentTrack.artistId);
          }
        }}
        onAlbumClick={() => {
          if (currentTrack?.isLocal) {
            navigateTo('library');
          } else if (currentTrack?.albumId) {
            handleAlbumClick(currentTrack.albumId);
          }
        }}
        onContextClick={handleContextNavigation}
        queueOpen={isQueueOpen}
        {normalizationEnabled}
        {normalizationGain}
        onToggleNormalization={toggleNormalization}
        onTrackClick={() => {
          if (currentTrack && !currentTrack.isLocal) {
            trackInfoTrackId = currentTrack.id;
            isTrackInfoOpen = true;
          }
        }}
      />
    {:else}
      <NowPlayingBar
        onTogglePlay={togglePlay}
        onOpenQueue={toggleQueue}
        onOpenFullScreen={openFullScreen}
        onCast={openCastPicker}
        {isCastConnected}
        queueOpen={isQueueOpen}
        {volume}
        onVolumeChange={handleVolumeChange}
      />
    {/if}

    <!-- Immersive Player (replaces ExpandedPlayer + FocusMode) -->
    {#if currentTrack}
      <ImmersivePlayer
        isOpen={isFullScreenOpen || isFocusModeOpen}
        onClose={() => {
          if (isFullScreenOpen) closeFullScreen();
          if (isFocusModeOpen) closeFocusMode();
        }}
        artwork={currentTrack.artwork}
        trackTitle={currentTrack.title}
        artist={currentTrack.artist}
        album={currentTrack.album}
        trackId={currentTrack.id}
        artistId={currentTrack.artistId}
        quality={currentTrack.quality}
        bitDepth={currentTrack.bitDepth}
        samplingRate={currentTrack.samplingRate}
        originalBitDepth={currentTrack.originalBitDepth}
        originalSamplingRate={currentTrack.originalSamplingRate}
        format={currentTrack.format}
        {isPlaying}
        onTogglePlay={togglePlay}
        onSkipBack={handleSkipBack}
        onSkipForward={handleSkipForward}
        {currentTime}
        {duration}
        onSeek={handleSeek}
        {volume}
        onVolumeChange={handleVolumeChange}
        {isShuffle}
        onToggleShuffle={toggleShuffle}
        {repeatMode}
        onToggleRepeat={toggleRepeat}
        {isFavorite}
        onToggleFavorite={toggleFavorite}
        lyricsLines={lyricsLines}
        lyricsActiveIndex={lyricsActiveIndex}
        lyricsActiveProgress={lyricsActiveProgress}
        lyricsSynced={lyricsIsSynced}
        lyricsLoading={lyricsStatus === 'loading'}
        lyricsError={lyricsStatus === 'error' ? lyricsError : (lyricsStatus === 'not_found' ? $t('player.noLyrics') : null)}
        enableCredits={true}
        enableSuggestions={true}
        queueTracks={[
          ...historyTracks,
          ...(currentQueueTrack ? [currentQueueTrack] : []),
          ...queue
        ]}
        queueCurrentIndex={historyTracks.length}
        onQueuePlayTrack={(index) => {
          const historyLen = historyTracks.length;
          if (index < historyLen) {
            // Playing from history
            handlePlayHistoryTrack(historyTracks[index]?.id ?? '');
          } else if (index > historyLen) {
            // Playing from upcoming queue
            const queueIndex = index - historyLen - 1;
            handleQueueTrackPlay(queue[queueIndex]?.id?.toString() ?? '');
          }
          // index === historyLen is current track, do nothing
        }}
        onQueueClear={handleClearQueue}
        {historyTracks}
        onPlayHistoryTrack={handlePlayHistoryTrack}
        isInfinitePlay={infinitePlayEnabled}
        onToggleInfinitePlay={handleToggleInfinitePlay}
      />
    {/if}

    <!-- Toast -->
    {#if toast}
      <Toast
        message={toast.message}
        type={toast.type}
        persistent={toast.persistent}
        onClose={hideToast}
      />
    {/if}

    <!-- Playlist Modal -->
    <PlaylistModal
      isOpen={isPlaylistModalOpen}
      mode={playlistModalMode}
      trackIds={playlistModalTrackIds}
      isLocalTracks={playlistModalTracksAreLocal}
      {userPlaylists}
      onClose={closePlaylistModal}
      onSuccess={handlePlaylistCreated}
    />

    <!-- Playlist Import Modal -->
    <PlaylistImportModal
      isOpen={isPlaylistImportOpen}
      onClose={closePlaylistImport}
      onSuccess={handlePlaylistImported}
    />

    <!-- About Modal -->
    <AboutModal
      isOpen={isAboutModalOpen}
      onClose={() => isAboutModalOpen = false}
    />

    <!-- Keyboard Shortcuts Modal -->
    <KeyboardShortcutsModal
      isOpen={isShortcutsModalOpen}
      onClose={() => isShortcutsModalOpen = false}
      onOpenSettings={() => {
        isShortcutsModalOpen = false;
        isKeybindingsSettingsOpen = true;
      }}
    />

    <!-- Keybindings Settings Modal -->
    <KeybindingsSettings
      isOpen={isKeybindingsSettingsOpen}
      onClose={() => isKeybindingsSettingsOpen = false}
    />

    <!-- Link Resolver Modal -->
    <LinkResolverModal
      isOpen={isLinkResolverOpen}
      onClose={() => isLinkResolverOpen = false}
      onResolve={handleResolvedLink}
      onOpenImporter={() => { isLinkResolverOpen = false; openPlaylistImport(); }}
    />

    {#if updateRelease}
      <UpdateAvailableModal
        isOpen={isUpdateModalOpen}
        currentVersion={updatesCurrentVersion}
        newVersion={updateRelease.version}
        onClose={handleUpdateClose}
        onVisitReleasePage={handleUpdateVisit}
      />

      <UpdateReminderModal
        isOpen={isReminderModalOpen}
        onClose={handleReminderClose}
        onRemindLater={handleReminderLater}
        onIgnoreRelease={handleReminderIgnoreRelease}
        onDisableAllUpdates={handleReminderDisableUpdates}
      />
    {/if}

    {#if whatsNewRelease}
      <WhatsNewModal
        isOpen={isWhatsNewModalOpen}
        release={whatsNewRelease}
        {showTitleBar}
        onClose={handleWhatsNewClose}
      />
    {/if}

    <FlatpakWelcomeModal
      isOpen={isFlatpakWelcomeOpen}
      onClose={handleFlatpakWelcomeClose}
    />

    <!-- Track Info Modal -->
    <TrackInfoModal
      isOpen={isTrackInfoOpen}
      trackId={trackInfoTrackId}
      onClose={() => {
        isTrackInfoOpen = false;
        trackInfoTrackId = null;
      }}
      onArtistClick={handleArtistClick}
      onLabelClick={handleLabelClick}
      onMusicianClick={handleMusicianClick}
    />

    <!-- Album Credits Modal -->
    <AlbumCreditsModal
      isOpen={isAlbumCreditsOpen}
      albumId={albumCreditsAlbumId}
      onClose={() => {
        isAlbumCreditsOpen = false;
        albumCreditsAlbumId = null;
      }}
      onTrackPlay={(trackCredits) => {
        // Find the corresponding track in the selected album and play it
        if (selectedAlbum?.tracks) {
          const track = selectedAlbum.tracks.find(trk => trk.id === trackCredits.id);
          if (track) {
            handleAlbumTrackPlay(track);
          }
        }
      }}
      onLabelClick={handleLabelClick}
      onMusicianClick={handleMusicianClick}
    />

    <!-- Musician Modal (for confidence level 0-1) -->
    {#if musicianModalData}
      <MusicianModal
        musician={musicianModalData}
        onClose={closeMusicianModal}
        onNavigateToArtist={handleArtistClick}
      />
    {/if}

    <!-- Cast Picker -->
    <CastPicker
      isOpen={isCastPickerOpen}
      onClose={closeCastPicker}
    />


  </div>
{/if}

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    background-color: var(--bg-primary);
  }

  .app-body {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .content-area {
    display: flex;
    flex: 1;
    min-width: 0;
    height: calc(100vh - 136px); /* 104px NowPlayingBar + 32px TitleBar */
    overflow: hidden;
    position: relative;
  }

  .main-content {
    flex: 1;
    min-width: 0;
    height: calc(100vh - 136px); /* 104px NowPlayingBar + 32px TitleBar */
    overflow: hidden; /* Views handle their own scrolling */
    padding-right: 8px; /* Gap between scrollbar and window edge */
    background-color: var(--bg-primary, #0f0f0f);
  }

  /* Adjust heights when title bar is hidden */
  .app.no-titlebar .content-area,
  .app.no-titlebar .main-content {
    height: calc(100vh - 104px); /* Only 104px NowPlayingBar, no title bar */
  }

  .view-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    height: 100%;
    color: var(--text-muted);
    font-size: 15px;
  }

  .view-error-back {
    padding: 8px 20px;
    border-radius: 8px;
    border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
    background: var(--bg-tertiary);
    color: var(--text-primary);
    cursor: pointer;
    font-size: 14px;
  }

  .view-error-back:hover {
    background: var(--bg-hover);
  }

  /* Global back-to-top button */
  .back-to-top-global {
    position: fixed;
    bottom: 114px; /* 104px player + 10px gap */
    right: 24px;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--bg-secondary);
    border: 1px solid var(--alpha-12);
    color: var(--text-secondary);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
    transition: background 150ms ease, color 150ms ease;
    z-index: 50;
  }

  .back-to-top-global:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

</style>
