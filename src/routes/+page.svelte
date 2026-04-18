<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { ChevronUp } from 'lucide-svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { listen, emitTo, type UnlistenFn } from '@tauri-apps/api/event';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';

  // Console log capture (must be early, before other imports log)
  import { initConsoleCapture, rehydrateVerboseCapture } from '$lib/stores/consoleLogStore';
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
  import {
    setSearchState,
    triggerSearchFocus,
    getSearchQuery,
    setSearchQuery,
    subscribeSearchQuery,
    clearSearchState
  } from '$lib/stores/searchState';

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
    isAutoplayEnabled,
    isInfinitePlayEnabled,
    setAutoplayMode
  } from '$lib/stores/playbackPreferencesStore';
  import { initBlacklistStore, isBlacklisted as isArtistBlacklisted } from '$lib/stores/artistBlacklistStore';
  import { initCustomArtistImageStore, clearCustomArtistImages } from '$lib/stores/customArtistImageStore';
  import { initCustomAlbumCoverStore, clearCustomAlbumCovers, resolveAlbumCover } from '$lib/stores/customAlbumCoverStore';
  import { getCachedImageUrl } from '$lib/services/imageCacheService';

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
    openQconnectPanel,
    closeQconnectPanel,
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
    expandSidebar,
    toggleSidebar
  } from '$lib/stores/sidebarStore';

  // Title bar state management
  import {
    subscribe as subscribeTitleBar,
    initTitleBarStore,
    shouldShowTitleBar,
    getShowWindowControls
  } from '$lib/stores/titleBarStore';

  // Window chrome (match system decoration radius)
  import {
    initWindowChromeStore,
    subscribe as subscribeWindowChrome,
    getMatchSystemWindowChrome,
    getCornerRadiusPx,
    setCornerRadiusPx,
    setWindowIsTransparent,
    getWindowIsTransparent,
  } from '$lib/stores/windowChromeStore';
  import { detectDesktopThemeCached } from '$lib/stores/windowControlsStore';

  // Search bar location store
  import {
    subscribe as subscribeSearchBarLocation,
    initSearchBarLocation,
    getSearchBarLocation
  } from '$lib/stores/searchBarLocationStore';

  // Window controls customization store
  import {
    subscribe as subscribeWindowControls,
    initWindowControlsStore,
    getWindowControls,
    type WindowControlsConfig
  } from '$lib/stores/windowControlsStore';

  // Titlebar navigation store
  import {
    subscribe as subscribeTitlebarNav,
    initTitlebarNavStore,
    isTitlebarNavEnabled,
    getResolvedPosition,
    getTitlebarNavConfig,
    isDiscoverInTitlebar,
    isFavoritesInTitlebar,
    isLibraryInTitlebar,
    isPurchasesInTitlebar
  } from '$lib/stores/titlebarNavStore';

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
  import { setStorageUserId, migrateLocalStorage, migrateLocalStorageV2, getUserItem, setUserItem } from '$lib/utils/userStorage';
  import {
    initWindowTitleStore,
    getWindowTitleEnabled,
    getWindowTitleTemplate,
    renderWindowTitle,
    subscribe as subscribeWindowTitle,
  } from '$lib/stores/windowTitleStore';

  // Favorites state management
  import { loadFavorites } from '$lib/stores/favoritesStore';
  import {
    startPolling as startUnlockingPolling,
    stopPolling as stopUnlockingPolling
  } from '$lib/stores/unlockingStore';
  import { loadAlbumFavorites } from '$lib/stores/albumFavoritesStore';
  import { loadArtistFavorites } from '$lib/stores/artistFavoritesStore';
  import { loadLabelFavorites } from '$lib/stores/labelFavoritesStore';
  import { loadAwardFavorites } from '$lib/stores/awardFavoritesStore';
  import { resolveAwardIdByName } from '$lib/stores/awardCatalogStore';
  import { getDefaultFavoritesTab } from '$lib/utils/favorites';
  import { platform } from '$lib/utils/platform';
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
    getActiveItemId,
    isBackForward,
    getFavoritesTabFromView,
    getSelectedLocalAlbumId,
    isFavoritesView,
    restoreView,
    setRestoredPlaylistId,
    setRestoredLocalAlbumId,
    saveScrollPosition,
    getSavedScrollPosition,
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
    setOnTogglePlayOverride,
    setGaplessGetNextTrackId,
    setOnGaplessTransition,
    togglePlay,
    seek as playerSeek,
    setVolume as playerSetVolume,
    stop as stopPlayback,
    setPendingSessionRestore,
    setRemoteControlMode,
    startPolling,
    stopPolling,
    reset as resetPlayer,
    getPlayerState,
    getVolume,
    resyncPersistedVolume,
    toggleMute,
    type PlayingTrack,
    type PlayerState
  } from '$lib/stores/playerStore';

  // Queue state management
  import {
    subscribe as subscribeQueue,
    syncQueueState,
    toggleShuffle as queueToggleShuffle,
    toggleRepeat as queueToggleRepeat,
    setQueue,
    clearQueue,
    playQueueIndex,
    playQueueUpcomingAt,
    nextTrack,
    nextTrackGuarded,
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
    cleanup as cleanupPlayback,
    updateMediaMetadata,
    type PlayTrackOptions
  } from '$lib/services/playbackService';
  import {
    isPlaybackSourceLocal,
    resolvePlaybackSource
  } from '$lib/services/playbackSource';

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
    shareSonglinkTrack,
    loadQconnectQueue
  } from '$lib/services/trackActions';
  import { replacePlaybackQueue } from '$lib/services/queuePlaybackService';
  import type {
    QconnectDiagnosticsPayload,
    QconnectQueueSnapshot,
    QconnectRendererReportDebugPayload,
    QconnectRendererSnapshot
  } from '$lib/services/qconnectRemoteQueue';
  import {
    DEFAULT_QCONNECT_CONNECTION_STATUS,
    QCONNECT_DIAGNOSTIC_LOG_LIMIT,
    SHOW_QCONNECT_DEV_DIAGNOSTICS,
    appendQconnectDiagnosticEntry,
    evaluateQconnectPlaybackReportSkip,
    evaluateQconnectSessionPersistence,
    fetchQconnectRuntimeState,
    isQconnectPeerRendererActive,
    isQconnectRemoteModeActive as computeQconnectRemoteModeActive,
    logQconnectPlaybackReport as appendQconnectPlaybackReport,
    qconnectAdmissionReasonKey,
    shouldQconnectSuppressLocalPlaybackAutomation,
    toggleQconnectConnection
  } from '$lib/services/qconnectRuntime';
  import type {
    QconnectAdmissionBlockedEvent,
    QconnectConnectionStatus,
    QconnectDiagnosticsEntry,
    QconnectSessionSnapshot
  } from '$lib/services/qconnectRuntime';

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

  import { enterMiniplayerMode } from '$lib/services/miniplayerService';

  // Sidebar mutual exclusion
  import { closeContentSidebar, restoreContentSidebar, subscribeContentSidebar, type ContentSidebarType } from '$lib/stores/sidebarStore';

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
  import TitleBarNav from '$lib/components/TitleBarNav.svelte';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import AboutModal from '$lib/components/AboutModal.svelte';
  import QualityFallbackModal from '$lib/components/QualityFallbackModal.svelte';
  import NowPlayingBar from '$lib/components/NowPlayingBar.svelte';
  import QconnectPanel from '$lib/components/QconnectPanel.svelte';
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
  import ReleaseWatchView from '$lib/components/views/ReleaseWatchView.svelte';
  import AwardView from '$lib/components/views/AwardView.svelte';
  import AwardAlbumsView from '$lib/components/views/AwardAlbumsView.svelte';
  import PurchasesView from '$lib/components/views/PurchasesView.svelte';
  import PurchaseAlbumDetailView from '$lib/components/views/PurchaseAlbumDetailView.svelte';
  import DynamicSuggestView from '$lib/components/views/DynamicSuggestView.svelte';
  import WeeklySuggestView from '$lib/components/views/WeeklySuggestView.svelte';
  import FavQView from '$lib/components/views/FavQView.svelte';
  import TopQView from '$lib/components/views/TopQView.svelte';
  import ArtistsByLocationView from '$lib/components/views/ArtistsByLocationView.svelte';

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
  import { reloadLyricsDisplay } from '$lib/stores/lyricsDisplayStore';
  import OfflinePlaceholder from '$lib/components/OfflinePlaceholder.svelte';
  import UpdateAvailableModal from '$lib/components/updates/UpdateAvailableModal.svelte';
  import UpdateReminderModal from '$lib/components/updates/UpdateReminderModal.svelte';
  import WhatsNewModal from '$lib/components/updates/WhatsNewModal.svelte';
  import FlatpakWelcomeModal from '$lib/components/updates/FlatpakWelcomeModal.svelte';
  import SnapWelcomeModal from '$lib/components/updates/SnapWelcomeModal.svelte';
  import KeyboardShortcutsModal from '$lib/components/KeyboardShortcutsModal.svelte';
  import KeybindingsSettings from '$lib/components/KeybindingsSettings.svelte';
  import LinkResolverModal from '$lib/components/LinkResolverModal.svelte';
  import type { ReleaseInfo } from '$lib/stores/updatesStore';
  import { isAutoUpdateEligible, refreshUpdatePreferences, resetUpdatesStore } from '$lib/stores/updatesStore';
  import { getShowPurchases, setShowPurchases, rehydratePurchasesStore } from '$lib/stores/purchasesStore';
  import {
    decideLaunchModals,
    disableUpdateChecks,
    ignoreReleaseVersion,
    markFlatpakWelcomeShown,
    markSnapWelcomeShown,
    openReleasePageAndAcknowledge,
    performAutoUpdate,
    resetLaunchFlow,
  } from '$lib/services/updatesService';
  import type { AutoUpdateProgress } from '$lib/services/updatesService';
  import UpdateProgressModal from '$lib/components/updates/UpdateProgressModal.svelte';

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
  let matchSystemChrome = $state(getMatchSystemWindowChrome());
  let chromeRadiusPx = $state(getCornerRadiusPx());
  let windowTransparent = $state(getWindowIsTransparent());
  let showWindowControls = $state(getShowWindowControls());

  // Search Bar Location State
  let searchBarLocationPref = $state(getSearchBarLocation());
  let titlebarSearchQuery = $state(getSearchQuery());

  // Window Controls State
  let windowControlsConfig = $state<WindowControlsConfig>(getWindowControls());

  // Titlebar Nav State
  let titlebarNavEnabled = $state(isTitlebarNavEnabled());
  let titlebarNavPosition = $state<'left' | 'right'>('left');
  let tbNavDiscover = $state(isDiscoverInTitlebar());
  let tbNavFavorites = $state(isFavoritesInTitlebar());
  let tbNavLibrary = $state(isLibraryInTitlebar());
  let tbNavPurchases = $state(isPurchasesInTitlebar());

  // Window floating state (not maximized/tiled — for rounded corners + shadow)
  let isWindowFloating = $state(false);

  // View State (from navigationStore subscription)
  let activeView = $state<ViewType>('home');
  let homeTab = $state<'home' | 'editorPicks' | 'forYou' | undefined>(undefined);
  let selectedPlaylistId = $state<number | null>(null);
  let updatesCurrentVersion = $state('');
  let updateRelease = $state<ReleaseInfo | null>(null);
  let whatsNewRelease = $state<ReleaseInfo | null>(null);
  let isUpdateModalOpen = $state(false);
  let isReminderModalOpen = $state(false);
  let isWhatsNewModalOpen = $state(false);
  let isFlatpakWelcomeOpen = $state(false);
  let isSnapWelcomeOpen = $state(false);
  let updatesLaunchTriggered = $state(false);
  let sessionReady = $state(false);

  // Sequential modal queue: Flatpak → What's new → Update available
  let pendingWhatsNewRelease = $state<ReleaseInfo | null>(null);
  let pendingUpdateRelease = $state<ReleaseInfo | null>(null);

  // Auto-update state
  let isAutoUpdating = $state(false);
  let autoUpdateProgress = $state<AutoUpdateProgress>({ state: 'checking' });

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
    return () => mainContentEl?.removeEventListener('scroll', handler, true);
  });

  // Reset scroll state on view or item change (but not on back/forward — that restores saved position)
  $effect(() => {
    void activeView;
    void currentNavItemId;
    globalScrollTop = 0;
    // Scroll to top for forward navigation (not back/forward, which restores saved position)
    if (!isBackForward()) {
      tick().then(() => {
        if (activeScrollTarget) {
          activeScrollTarget.scrollTop = 0;
        }
      });
    }
  });

  function globalScrollToTop() {
    activeScrollTarget?.scrollTo({ top: 0, behavior: 'smooth' });
  }

  // Album, Artist and Label data are fetched, so kept local
  let selectedAlbum = $state<AlbumDetail | null>(null);
  let selectedArtist = $state<ArtistDetail | null>(null);
  let selectedArtistKnownMbid = $state<string | null>(null);
  let artistTopTracks = $state<PageArtistTrack[]>([]);
  let artistSimilarArtists = $state<PageArtistSimilarItem[]>([]);
  let selectedLabel = $state<{ id: number; name: string } | null>(null);
  let selectedAward = $state<{ id: string; name: string } | null>(null);
  let selectedMusician = $state<ResolvedMusician | null>(null);
  let musicianModalData = $state<ResolvedMusician | null>(null);
  let isArtistAlbumsLoading = $state(false);

  // Scene discovery state
  interface ArtistsByLocationContext {
    sourceArtistMbid: string;
    sourceArtistName: string;
    sourceArtistType: 'Person' | 'Group' | 'Other';
    location: {
      city?: string;
      areaId?: string;
      country?: string;
      countryCode?: string;
      displayName: string;
      precision: 'city' | 'state' | 'country';
    };
    affinitySeeds: {
      genres: string[];
      tags: string[];
      normalizedSeeds: string[];
    };
  }
  let artistsByLocationContext = $state<ArtistsByLocationContext | null>(null);

  function handleLocationClick(ctx: ArtistsByLocationContext) {
    artistsByLocationContext = ctx;
    navigateTo('artists-by-location');
  }

  // Track current itemId for scroll position save on navigation
  let currentNavItemId = $state<string | number | undefined>(undefined);

  // Purchase downloads state
  let selectedPurchaseAlbumId = $state<string | null>(null);

  function handlePurchaseAlbumClick(albumId: string) {
    selectedPurchaseAlbumId = albumId;
    navigateTo('purchase-album', albumId);
  }

  function isSessionRestoreSafeView(view: ViewType): boolean {
    switch (view) {
      case 'search':
      case 'library':
      case 'settings':
      case 'playlist-manager':
      case 'blacklist-manager':
      case 'favorites-tracks':
      case 'favorites-albums':
      case 'favorites-artists':
      case 'favorites-labels':
      case 'favorites-playlists':
      case 'discover-new-releases':
      case 'discover-ideal-discography':
      case 'discover-top-albums':
      case 'discover-qobuzissimes':
      case 'discover-albums-of-the-week':
      case 'discover-press-accolades':
      case 'discover-playlists':
      case 'discover-release-watch':
      case 'purchases':
      case 'dailyq':
      case 'weeklyq':
      case 'favq':
      case 'topq':
        return true;
      default:
        return false;
    }
  }

  function getSessionFallbackView(view: ViewType): ViewType {
    switch (view) {
      case 'library-album':
        return 'library';
      case 'purchase-album':
        return 'purchases';
      default:
        return 'home';
    }
  }

  function getPersistedSessionViewState(): {
    view: ViewType;
    viewContextId: string | null;
    viewContextType: string | null;
  } {
    switch (activeView) {
      case 'album':
        if (selectedAlbum?.id) {
          return {
            view: 'album',
            viewContextId: String(selectedAlbum.id),
            viewContextType: 'album',
          };
        }
        break;
      case 'artist':
        if (selectedArtist?.id) {
          return {
            view: 'artist',
            viewContextId: String(selectedArtist.id),
            viewContextType: 'artist',
          };
        }
        break;
      case 'playlist':
        if (selectedPlaylistId) {
          return {
            view: 'playlist',
            viewContextId: String(selectedPlaylistId),
            viewContextType: 'playlist',
          };
        }
        break;
      case 'library-album': {
        const localAlbumId = getSelectedLocalAlbumId();
        if (localAlbumId) {
          return {
            view: 'library-album',
            viewContextId: localAlbumId,
            viewContextType: 'library-album',
          };
        }
        break;
      }
      case 'purchase-album':
        if (selectedPurchaseAlbumId) {
          return {
            view: 'purchase-album',
            viewContextId: selectedPurchaseAlbumId,
            viewContextType: 'purchase-album',
          };
        }
        break;
    }

    if (isSessionRestoreSafeView(activeView)) {
      return {
        view: activeView,
        viewContextId: null,
        viewContextType: null,
      };
    }

    const fallbackView = getSessionFallbackView(activeView);
    console.warn('[Session] Persist fallback applied for unsupported or incomplete view:', {
      activeView,
      fallbackView,
    });

    return {
      view: fallbackView,
      viewContextId: null,
      viewContextType: null,
    };
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
    // Order: Flatpak/Snap → What's new → Update available
    pendingWhatsNewRelease = decision.whatsNewRelease;
    pendingUpdateRelease = decision.updateRelease;

    // Show first modal in queue (sandbox welcome has highest priority)
    if (decision.showFlatpakWelcome) {
      isFlatpakWelcomeOpen = true;
      return;
    }
    if (decision.showSnapWelcome) {
      isSnapWelcomeOpen = true;
      return;
    }

    // No sandbox modal, try What's New
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

  function handleAutoUpdate(): void {
    isUpdateModalOpen = false;
    isAutoUpdating = true;
    autoUpdateProgress = { state: 'checking' };
    void performAutoUpdate(
      (progress) => {
        if (isAutoUpdating) autoUpdateProgress = progress;
      },
      () => !isAutoUpdating,
    );
  }

  function handleAutoUpdateCancel(): void {
    isAutoUpdating = false;
    if (updateRelease) {
      isUpdateModalOpen = true;
    }
  }

  function handleAutoUpdateFallbackManual(): void {
    isAutoUpdating = false;
    if (updateRelease) {
      void openReleasePageAndAcknowledge(updateRelease);
      updateRelease = null;
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

  function handleSnapWelcomeClose(): void {
    isSnapWelcomeOpen = false;
    void markSnapWelcomeShown();
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
  let isQconnectPanelOpen = $state(false);
  let showQconnectDevButton = $state(localStorage.getItem('qbz-qconnect-dev-button') === 'true');
  let isQobuzConnectConnected = $state(false);
  let qobuzConnectBusy = $state(false);
  let qobuzConnectRefreshBusy = $state(false);
  let qobuzConnectStatus = $state<QconnectConnectionStatus>(DEFAULT_QCONNECT_CONNECTION_STATUS);
  let qobuzConnectQueueSnapshot = $state<QconnectQueueSnapshot | null>(null);
  let qobuzConnectRendererSnapshot = $state<QconnectRendererSnapshot | null>(null);
  let qobuzConnectSessionSnapshot = $state<QconnectSessionSnapshot | null>(null);
  let qobuzConnectDiagnosticsLogs = $state<QconnectDiagnosticsEntry[]>([]);
  const showQconnectDevDiagnostics = SHOW_QCONNECT_DEV_DIAGNOSTICS;
  let qconnectSessionPersistenceSkipLogged = false;
  let lastQconnectReportSkipSignature = '';

  // Playlist Modal State (from uiStore subscription)
  let isPlaylistModalOpen = $state(false);
  let playlistModalMode = $state<'create' | 'edit' | 'addTrack'>('create');
  let playlistModalTrackIds = $state<number[]>([]);
  let playlistModalTracksAreLocal = $state(false);
  let playlistModalEditPlaylist = $state<{ id: number; name: string; tracks_count: number } | undefined>(undefined);
  let playlistModalEditIsHidden = $state(false);
  let playlistModalEditCurrentFolderId = $state<string | null>(null);
  let isPlaylistImportOpen = $state(false);
  let isAboutModalOpen = $state(false);
  let isShortcutsModalOpen = $state(false);
  let isKeybindingsSettingsOpen = $state(false);
  let isLinkResolverOpen = $state(false);

  // Quality Fallback Modal State
  let isQualityFallbackOpen = $state(false);
  let qualityFallbackTrackTitle = $state('');
  let qualityFallbackTrack = $state<PlayingTrack | null>(null);
  let qualityFallbackOptions = $state<PlayTrackOptions>({});

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

  // TitleBar reference for focusing search
  let titlebarRef = $state<{ focusSearch: () => void } | undefined>(undefined);

  // Window-title (OS title bar) preference — bumped on store changes so the
  // effect below recomputes immediately when the user toggles the setting.
  let windowTitlePrefVersion = $state(0);

  // Playback State (from playerStore subscription)
  let currentTrack = $state<PlayingTrack | null>(null);
  let isPlaying = $state(false);
  let currentTime = $state(0);
  let duration = $state(0);
  let volume = $state(getVolume()); // Load persisted volume from localStorage
  let isFavorite = $state(false);
  let normalizationEnabled = $state(false);
  let normalizationGain = $state<number | null>(null);
  let bufferProgress = $state<number | null>(null);
  let isAlsaDirectHw = $state(false); // ALSA Direct hw: locks volume to 100%
  // Queue/Shuffle State (from queueStore subscription)
  let isShuffle = $state(false);
  let repeatMode = $state<RepeatMode>('off');
  let queue = $state<QueueTrack[]>([]);
  let queueTotalTracks = $state(0);
  let queueRemainingTracks = $state(0); // Actual remaining tracks (total - current_index - 1)
  let historyTracks = $state<QueueTrack[]>([]);
  let infinitePlayEnabled = $state(false);
  let sessionPersistEnabled = $state(false);
  let radioLoading = $state(false);
  let qconnectRemoteClockMs = $state(Date.now());
  let qconnectRemoteProjectedTrackId = $state<number | null>(null);

  const qconnectPeerRendererActive = $derived(
    isQconnectPeerRendererActive(qobuzConnectSessionSnapshot)
  );
  const qconnectSuppressLocalPlaybackAutomation = $derived(
    shouldQconnectSuppressLocalPlaybackAutomation(
      isQobuzConnectConnected,
      qobuzConnectSessionSnapshot
    )
  );
  const effectiveIsPlaying = $derived(
    qconnectPeerRendererActive
      ? qobuzConnectRendererSnapshot?.playing_state === 2
      : isPlaying
  );
  const effectiveCurrentTime = $derived(
    qconnectPeerRendererActive
      ? (() => {
          const remotePositionMs = Math.max(0, qobuzConnectRendererSnapshot?.current_position_ms ?? 0);
          const remoteUpdatedAtMs = qobuzConnectRendererSnapshot?.updated_at_ms ?? 0;
          const extrapolatedMs =
            effectiveIsPlaying && remoteUpdatedAtMs > 0
              ? remotePositionMs + Math.max(0, qconnectRemoteClockMs - remoteUpdatedAtMs)
              : remotePositionMs;
          const maxDurationMs = Math.max(0, duration * 1000);
          return (maxDurationMs > 0 ? Math.min(extrapolatedMs, maxDurationMs) : extrapolatedMs) / 1000;
        })()
      : currentTime
  );

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

  function pushQobuzConnectDiagnostic(
    channel: string,
    level: 'info' | 'warn' | 'error',
    payload: unknown
  ): void {
    qobuzConnectDiagnosticsLogs = appendQconnectDiagnosticEntry(
      qobuzConnectDiagnosticsLogs,
      channel,
      level,
      payload,
      QCONNECT_DIAGNOSTIC_LOG_LIMIT
    );
  }

  function clearQobuzConnectDiagnostics(): void {
    qobuzConnectDiagnosticsLogs = [];
  }

  function logQconnectPlaybackReport(
    source: 'interval' | 'player_transition',
    payload: Record<string, unknown>
  ): void {
    qobuzConnectDiagnosticsLogs = appendQconnectPlaybackReport(
      qobuzConnectDiagnosticsLogs,
      source,
      payload
    );
  }

  function shouldSkipQconnectPlaybackReport(currentTrackId: number | null | undefined): boolean {
    const decision = evaluateQconnectPlaybackReportSkip({
      currentTrackId,
      queueSnapshot: qobuzConnectQueueSnapshot,
      rendererSnapshot: qobuzConnectRendererSnapshot,
      lastSkipSignature: lastQconnectReportSkipSignature
    });

    lastQconnectReportSkipSignature = decision.nextSkipSignature;
    if (decision.diagnosticPayload) {
      pushQobuzConnectDiagnostic('qconnect:report_playback_state:skip', 'warn', decision.diagnosticPayload);
    }

    return decision.shouldSkip;
  }

  function isQconnectRemoteModeActive(): boolean {
    return computeQconnectRemoteModeActive(isQobuzConnectConnected, qobuzConnectStatus);
  }

  function shouldPersistLocalSession(): boolean {
    const decision = evaluateQconnectSessionPersistence(
      isQconnectRemoteModeActive(),
      qconnectSessionPersistenceSkipLogged
    );
    qconnectSessionPersistenceSkipLogged = decision.nextSkipLogged;
    if (decision.shouldLogSkip) {
      console.log('[Session] Skipping local session persistence while Qobuz Connect remote mode is active');
    }
    return decision.shouldPersist;
  }

  function applyQobuzConnectStatus(status: QconnectConnectionStatus): void {
    qobuzConnectStatus = status;
    const nextConnected = Boolean(status.transport_connected);
    isQobuzConnectConnected = nextConnected;
  }

  $effect(() => {
    setRemoteControlMode(qconnectSuppressLocalPlaybackAutomation);
  });

  function mapBackendQueueTrackToPlayingTrack(track: BackendQueueTrack): PlayingTrack {
    const rawRate = track.sample_rate ?? undefined;
    const normalizedRate = rawRate == null
      ? undefined
      : (track.is_local || track.source === 'plex')
        ? rawRate / 1000
        : rawRate;

    return {
      id: track.id,
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork: track.artwork_url || '',
      duration: track.duration_secs,
      quality: track.hires ? 'Hi-Res' : 'CD Quality',
      bitDepth: track.bit_depth ?? undefined,
      samplingRate: normalizedRate,
      isLocal: track.is_local,
      source: track.source ?? undefined,
      albumId: track.album_id ?? undefined,
      artistId: track.artist_id ?? undefined,
      parental_warning: track.parental_warning ?? undefined
    };
  }

  async function syncQconnectRemoteProjection(
    rendererSnapshot: QconnectRendererSnapshot | null | undefined,
    peerRendererActive: boolean
  ): Promise<void> {
    if (!peerRendererActive) {
      qconnectRemoteProjectedTrackId = null;
      return;
    }

    await syncQueueState();
    const state = await getBackendQueueState();
    if (!state) {
      return;
    }

    if (state.shuffle && state.current_track && state.total_tracks > 0) {
      queueRemainingTracks = state.total_tracks - 1;
    } else if (state.current_index !== null && state.total_tracks > 0) {
      queueRemainingTracks = state.total_tracks - state.current_index - 1;
    } else {
      queueRemainingTracks = state.total_tracks;
    }

    historyTracks = state.history.map((track) => ({
      id: String(track.id),
      artwork: track.artwork_url || '',
      title: track.title,
      artist: track.artist,
      duration: formatDuration(track.duration_secs),
      trackId: track.id
    }));

    const remoteTrack = state.current_track;
    const remoteTrackId = remoteTrack?.id ?? null;

    if (remoteTrack) {
      currentTrack = mapBackendQueueTrackToPlayingTrack(remoteTrack);
      duration = remoteTrack.duration_secs;
    } else if (!rendererSnapshot?.current_track) {
      currentTrack = null;
      duration = 0;
    }

    if (rendererSnapshot?.playing_state != null) {
      isPlaying = rendererSnapshot.playing_state === 2;
    }
    if (rendererSnapshot?.current_position_ms != null) {
      currentTime = Math.max(0, rendererSnapshot.current_position_ms / 1000);
    }

    if (remoteTrackId !== qconnectRemoteProjectedTrackId) {
      qconnectRemoteProjectedTrackId = remoteTrackId;
      if (remoteTrackId == null) {
        isFavorite = false;
      } else {
        isFavorite = await checkTrackFavorite(remoteTrackId);
      }
    }
  }

  async function refreshQobuzConnectStatus(): Promise<void> {
    try {
      const runtimeState = await fetchQconnectRuntimeState();
      applyQobuzConnectStatus(runtimeState.status);
    } catch {
      applyQobuzConnectStatus(DEFAULT_QCONNECT_CONNECTION_STATUS);
    }
  }

  async function refreshQobuzConnectSnapshots(): Promise<void> {
    if (!isQobuzConnectConnected) {
      qobuzConnectQueueSnapshot = null;
      qobuzConnectRendererSnapshot = null;
      qobuzConnectSessionSnapshot = null;
      qconnectRemoteProjectedTrackId = null;
      return;
    }

    const runtimeState = await fetchQconnectRuntimeState();
    applyQobuzConnectStatus(runtimeState.status);
    qobuzConnectQueueSnapshot = runtimeState.queueSnapshot;
    qobuzConnectRendererSnapshot = runtimeState.rendererSnapshot;
    qobuzConnectSessionSnapshot = runtimeState.sessionSnapshot;
    const peerRendererActive = isQconnectPeerRendererActive(runtimeState.sessionSnapshot);
    await syncQconnectRemoteProjection(runtimeState.rendererSnapshot, peerRendererActive);

    if (runtimeState.snapshotError) {
      pushQobuzConnectDiagnostic('snapshot', 'warn', runtimeState.snapshotError);
    }
  }

  async function refreshQobuzConnectRuntimeState(): Promise<void> {
    if (qobuzConnectRefreshBusy) return;
    qobuzConnectRefreshBusy = true;
    try {
      const runtimeState = await fetchQconnectRuntimeState();
      applyQobuzConnectStatus(runtimeState.status);
      qobuzConnectQueueSnapshot = runtimeState.queueSnapshot;
      qobuzConnectRendererSnapshot = runtimeState.rendererSnapshot;
      qobuzConnectSessionSnapshot = runtimeState.sessionSnapshot;
      const peerRendererActive = isQconnectPeerRendererActive(runtimeState.sessionSnapshot);
      await syncQconnectRemoteProjection(runtimeState.rendererSnapshot, peerRendererActive);
      if (runtimeState.snapshotError) {
        pushQobuzConnectDiagnostic('snapshot', 'warn', runtimeState.snapshotError);
      }
    } finally {
      qobuzConnectRefreshBusy = false;
    }
  }

  async function handleQconnectTogglePlayOverride(): Promise<boolean> {
    if (!isQobuzConnectConnected) {
      return false;
    }

    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_toggle_play_if_remote');
      if (handledRemotely) {
        await refreshQobuzConnectRuntimeState();
      }
      return handledRemotely;
    } catch (err) {
      pushQobuzConnectDiagnostic('qconnect:toggle_play_handoff', 'error', {
        error: String(err)
      });
      throw err;
    }
  }

  async function handleQobuzConnectButton(): Promise<void> {
    if (qobuzConnectBusy) return;
    qobuzConnectBusy = true;
    try {
      await toggleQconnectConnection(isQobuzConnectConnected);
    } catch (err) {
      console.error('Qobuz Connect toggle failed:', err);
      pushQobuzConnectDiagnostic('toggle', 'error', err);
    } finally {
      await refreshQobuzConnectRuntimeState();
      qobuzConnectBusy = false;
    }
  }

  function openQobuzConnectPanelFromNowPlaying(): void {
    openQconnectPanel();
    void refreshQobuzConnectRuntimeState();
  }

  $effect(() => {
    if (!qconnectPeerRendererActive || !effectiveIsPlaying) {
      return;
    }

    qconnectRemoteClockMs = Date.now();
    const intervalId = window.setInterval(() => {
      qconnectRemoteClockMs = Date.now();
    }, 1000);

    return () => {
      window.clearInterval(intervalId);
    };
  });

  // Navigation wrapper (keeps debug logging)
  async function navigateTo(view: string, itemId?: string | number) {
    console.log('navigateTo called with:', view, 'itemId:', itemId, 'current activeView:', activeView);
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
    // Set homeTab when navigating to a specific home tab (Discover menu)
    if (view === 'home' && (itemId === 'home' || itemId === 'editorPicks' || itemId === 'forYou')) {
      homeTab = itemId;
    }
    navTo(view as ViewType, itemId);
  }

  /**
   * Restore item data when navigating back/forward.
   * Re-fetches the specific album/artist/playlist/label so the correct page is shown.
   */
  async function restoreItemFromHistory(view: ViewType, itemId: string | number) {
    try {
      switch (view) {
        case 'home':
          if (itemId === 'home' || itemId === 'editorPicks' || itemId === 'forYou') {
            homeTab = itemId;
          }
          break;
        case 'album':
          await handleAlbumClick(String(itemId));
          break;
        case 'artist':
          await handleArtistClick(Number(itemId));
          break;
        case 'playlist':
          selectedPlaylistId = Number(itemId);
          selectPlaylist(Number(itemId));
          break;
        case 'label':
        case 'label-releases':
          selectedLabel = { id: Number(itemId), name: selectedLabel?.name || '' };
          break;
        case 'award':
        case 'award-albums':
          selectedAward = { id: String(itemId), name: selectedAward?.name || '' };
          break;
        case 'musician':
          // Musician data is already in selectedMusician from the original navigation
          break;
        case 'purchase-album':
          selectedPurchaseAlbumId = String(itemId);
          break;
        case 'library-album':
          setRestoredLocalAlbumId(String(itemId));
          break;
      }
    } catch (err) {
      console.error('[Nav] Failed to restore item from history:', view, itemId, err);
    }
  }

  // Effective search-in-titlebar: only when custom titlebar is shown AND user preference is 'titlebar'
  function isSearchInTitlebar(): boolean {
    return showTitleBar && searchBarLocationPref === 'titlebar';
  }

  // Titlebar search handlers (mirrors Sidebar search logic)
  const TITLEBAR_SEARCH_NAV_THRESHOLD = 3;

  function handleTitlebarSearchInput(query: string) {
    titlebarSearchQuery = query;
    setSearchQuery(query);
    if (query.trim().length >= TITLEBAR_SEARCH_NAV_THRESHOLD && activeView !== 'search') {
      navigateTo('search');
    }
  }

  function handleTitlebarSearchClear() {
    titlebarSearchQuery = '';
    clearSearchState();
    titlebarRef?.focusSearch();
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
      navTo('album', albumId);
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
  async function handleViewArtistDiscography() {
    if (selectedAlbum?.artistId) {
      const artistId = selectedAlbum.artistId;
      await handleArtistClick(artistId);
      await tick();
      const discographySection = document.querySelector('.artist-section');
      if (discographySection) {
        discographySection.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
    }
  }


  async function handleArtistClick(artistId: number, mbid?: string) {
    try {
      selectedArtistKnownMbid = mbid ?? null;
      showToast($t('toast.loadingArtist'), 'info');
      const response = await invoke<PageArtistResponse>('v2_get_artist_page', { artistId });
      console.log('Artist page:', response);

      selectedArtist = convertPageArtist(response);
      artistTopTracks = response.top_tracks || [];
      artistSimilarArtists = response.similar_artists?.items || [];
      navTo('artist', artistId);
      hideToast();

      // The artist/page endpoint sometimes omits EPs & Singles from its
      // releases array. If they're missing, fetch them explicitly.
      // If the artist/page response didn't include EPs & Singles, fetch them
      const hasEpGroup = (response.releases || []).some(
        g => g.type === 'ep' || g.type === 'single' || g.type === 'epSingle'
      );
      if (!hasEpGroup && selectedArtist.epsSingles.length === 0) {
        invoke<ReleasesGridResponse>('get_releases_grid', {
          artistId, releaseType: 'epSingle', limit: 25, offset: 0
        }).then(result => {
          if (result.items.length > 0 && selectedArtist?.id === artistId) {
            selectedArtist = appendPageReleases(selectedArtist, 'epSingle', result.items, result.has_more);
            console.log(`[Artist] Backfilled ${result.items.length} EPs & Singles`);
          }
        }).catch(err => console.debug('[Artist] EP backfill failed:', err));
      }
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
    navigateTo('label', labelId);
  }

  function handleNavigateLabelReleases(labelId: number, labelName: string) {
    selectedLabel = { id: labelId, name: labelName };
    navigateTo('label-releases', labelId);
  }

  function handleNavigateAwardAlbums(awardId: string, awardName: string) {
    if (!awardId) return;
    selectedAward = { id: awardId, name: awardName };
    navigateTo('award-albums', awardId);
  }

  async function handleAwardClick(awardId: string, awardName: string) {
    // Happy path: /album/get included the id — navigate immediately.
    if (awardId) {
      selectedAward = { id: awardId, name: awardName };
      navigateTo('award', awardId);
      return;
    }
    // /album/get sometimes returns an award entry with only a name.
    // Resolve via the cached /award/explore catalog. One HTTP round-trip
    // the first time, instant afterwards.
    const resolved = await resolveAwardIdByName(awardName);
    if (!resolved) {
      showToast($t('toast.awardUnavailable'), 'info');
      return;
    }
    selectedAward = { id: resolved, name: awardName };
    navigateTo('award', resolved);
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
          navigateTo('musician', musician.qobuz_artist_id ?? name);
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
            navigateTo('playlist', playlistId);
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

        case 'home_list': {
          // Navigate to home page with the correct tab
          const tabFromId = context.id.split(':')[0];
          const tab = (tabFromId === 'forYou' || tabFromId === 'editorPicks') ? tabFromId : 'home';
          homeTab = tab;
          navigateTo('home', tab);
          break;
        }

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

    // Map UI release type to API release type
    const apiReleaseType = releaseType === 'ep' || releaseType === 'single' ? 'epSingle' : releaseType;

    // Count current items for this release type to use as offset
    let currentCount = 0;
    switch (releaseType) {
      case 'album': currentCount = selectedArtist.albums.length; break;
      case 'ep': case 'single': case 'epSingle': currentCount = selectedArtist.epsSingles.length; break;
      case 'live': currentCount = selectedArtist.liveAlbums.length; break;
      case 'compilation': case 'other': currentCount = selectedArtist.others.length; break;
    }

    isArtistAlbumsLoading = true;
    try {
      const result = await invoke<ReleasesGridResponse>('get_releases_grid', {
        artistId: selectedArtist.id,
        releaseType: apiReleaseType,
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

    await replacePlaybackQueue(queueTracks, 0, {
      debugLabel: 'page:play-album'
    });

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
    let queuedCount = 0;
    for (let i = album.tracks.length - 1; i >= 0; i--) {
      const trk = album.tracks[i];
      const queued = await queueTrackNext({
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
        artist_id: trk.artistId ?? album.artistId,
        streamable: trk.streamable ?? true,
        source: 'qobuz',
        parental_warning: trk.parental_warning ?? false
      }, false, { silent: true });
      if (queued) {
        queuedCount += 1;
      }
    }
    if (queuedCount > 0) {
      showToast($t('toast.playingTracksNext', { values: { count: queuedCount } }), 'success');
    } else {
      showToast($t('toast.failedAddToQueue'), 'error');
    }
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

    let queuedCount = 0;
    for (const queueTrack of queueTracks) {
      const queued = await queueTrackLater(queueTrack, false, { silent: true });
      if (queued) {
        queuedCount += 1;
      }
    }

    if (queuedCount > 0) {
      showToast($t('toast.addedTracksToQueue', { values: { count: queuedCount } }), 'success');
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
        parental_warning?: boolean;
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

    await replacePlaybackQueue(queueTracks, 0, {
      debugLabel: 'page:play-playlist'
    });

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
    let queuedCount = 0;
    for (let i = tracks.length - 1; i >= 0; i--) {
      const trk = tracks[i];
      const queued = await queueTrackNext({
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
        artist_id: trk.performer?.id,
        streamable: true,
        source: 'qobuz',
        parental_warning: trk.parental_warning ?? false
      }, false, { silent: true });
      if (queued) {
        queuedCount += 1;
      }
    }

    if (queuedCount > 0) {
      showToast($t('toast.playingTracksNext', { values: { count: queuedCount } }), 'success');
    } else {
      showToast($t('toast.failedAddToQueue'), 'error');
    }
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

    let queuedCount = 0;
    for (const queueTrack of queueTracks) {
      const queued = await queueTrackLater(queueTrack, false, { silent: true });
      if (queued) {
        queuedCount += 1;
      }
    }

    if (queuedCount > 0) {
      showToast($t('toast.addedTracksToQueue', { values: { count: queuedCount } }), 'success');
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
    console.log('Playing track:', track.id, track.title);

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
    console.log('Playing album track:', track.id, track.title);

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
      await replacePlaybackQueue(queueTracks, trackIndex >= 0 ? trackIndex : 0, {
        debugLabel: 'page:album-track-play'
      });

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

  async function handleVolumeChange(newVolume: number) {
    // ALSA Direct hw: locks volume at 100% — unless controlling a remote renderer
    if (isAlsaDirectHw && !qconnectPeerRendererActive) return;

    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_set_volume_if_remote', { volume: newVolume });
      if (handledRemotely) return;
    } catch {
      // Fall through to local
    }

    playerSetVolume(newVolume);
    // Report volume change to QConnect server when acting as renderer
    if (isQobuzConnectConnected) {
      invoke('v2_qconnect_report_volume', { volume: newVolume }).catch(() => {});
    }
  }

  async function handleToggleMute() {
    // ALSA Direct hw: locks volume at 100% — unless controlling a remote renderer
    if (isAlsaDirectHw && !qconnectPeerRendererActive) return;

    // Determine current mute state from volume
    const currentlyMuted = volume === 0;
    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_mute_if_remote', { value: !currentlyMuted });
      if (handledRemotely) return;
    } catch {
      // Fall through to local
    }

    await toggleMute();
    // Report volume change to QConnect server when acting as renderer
    if (isQobuzConnectConnected) {
      const newVolume = getVolume();
      invoke('v2_qconnect_report_volume', { volume: newVolume }).catch(() => {});
    }
  }

  async function toggleShuffle() {
    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_toggle_shuffle_if_remote');
      if (handledRemotely) {
        await refreshQobuzConnectRuntimeState();
        return;
      }
    } catch (err) {
      console.error('Failed to hand off shuffle toggle to remote renderer:', err);
      return;
    }

    const result = await queueToggleShuffle();
    if (result.success) {
      showToast(result.enabled ? $t('toast.shuffleEnabled') : $t('toast.shuffleDisabled'), 'info');
      // Persist playback mode to session
      if (shouldPersistLocalSession()) {
        saveSessionPlaybackMode(result.enabled, repeatMode);
      }
    }
  }

  async function toggleRepeat() {
    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_cycle_repeat_if_remote');
      if (handledRemotely) {
        await refreshQobuzConnectRuntimeState();
        return;
      }
    } catch (err) {
      console.error('Failed to hand off repeat cycle to remote renderer:', err);
      return;
    }

    const result = await queueToggleRepeat();
    if (result.success) {
      const messages: Record<RepeatMode, string> = {
        off: $t('toast.repeatOff'),
        all: $t('toast.repeatAll'),
        one: $t('toast.repeatOne')
      };
      showToast(messages[result.mode], 'info');
      // Persist playback mode to session
      if (shouldPersistLocalSession()) {
        saveSessionPlaybackMode(isShuffle, result.mode);
      }
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
    if (playerState.isSkipping) return;

    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_skip_previous_if_remote');
      if (handledRemotely) {
        await refreshQobuzConnectRuntimeState();
        return;
      }
    } catch (err) {
      console.error('Failed to hand off previous track to remote renderer:', err);
      showToast($t('toast.failedPreviousTrack'), 'error');
      return;
    }

    if (!playerState.currentTrack) return;
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
    if (playerState.isSkipping) return;

    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_skip_next_if_remote');
      if (handledRemotely) {
        await refreshQobuzConnectRuntimeState();
        return;
      }
    } catch (err) {
      console.error('Failed to hand off next track to remote renderer:', err);
      showToast($t('toast.failedNextTrack'), 'error');
      return;
    }

    if (!playerState.currentTrack) return;

    setIsSkipping(true);
    try {
      let nextTrackResult = await nextTrack();
      if (!nextTrackResult && infinitePlayEnabled) {
        // Queue ended with infinite play on — fetch radio tracks based on
        // the last 5 played + current track, append, then advance.
        const recentIds: number[] = [];
        if (playerState.currentTrack) recentIds.push(playerState.currentTrack.id);
        for (const item of historyTracks.slice(0, 5)) {
          const numericId = (item as any).trackId;
          if (typeof numericId === 'number') recentIds.push(numericId);
        }
        if (recentIds.length > 0) {
          try {
            const radioTracks = await invoke<BackendQueueTrack[]>('v2_create_infinite_radio', {
              recentTrackIds: recentIds.slice(0, 5)
            });
            if (radioTracks && radioTracks.length > 0) {
              await invoke('v2_bulk_add_to_queue', { tracks: radioTracks });
              nextTrackResult = await nextTrack();
            }
          } catch (err) {
            console.error('Failed to extend queue with infinite radio:', err);
          }
        }
      }
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
    const source = resolvePlaybackSource(track);
    const isLocal = isPlaybackSourceLocal(source, isLocalTrack(track.id));

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
      artistId: track.artist_id ?? undefined,
      parental_warning: track.parental_warning ?? false
    }, { isLocal, source: source as 'qobuz' | 'local' | 'plex', showLoadingToast: false, gaplessTransition });
  }

  // Play a specific track from the queue panel (shuffle-aware, fixes issue #327)
  async function handleQueueTrackPlay(trackId: string, upcomingIndex: number) {
    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_play_track_if_remote', { trackId: parseInt(trackId, 10) });
      if (handledRemotely) {
        await refreshQobuzConnectRuntimeState();
        return;
      }
    } catch (err) {
      console.error('Failed to hand off queue track play to remote renderer:', err);
      return;
    }

    try {
      const track = await playQueueUpcomingAt(upcomingIndex);
      if (track) {
        await playQueueTrack(track);
      } else {
        showToast($t('toast.failedPlayTrack'), 'error');
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
      // Immediately persist the empty state so it survives app close
      if (sessionPersistEnabled) {
        saveSessionBeforeClose();
      }
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
  async function handleToggleInfinitePlay() {
    const next = !infinitePlayEnabled;
    try {
      await setAutoplayMode(next ? 'infinite' : 'continue');
      infinitePlayEnabled = next;
      showToast(next ? $t('toast.infinitePlayEnabled') : $t('toast.infinitePlayDisabled'), 'info');
    } catch (err) {
      console.error('Failed to set autoplay mode:', err);
      showToast($t('toast.failedToSavePreference'), 'error');
    }
  }

  // Play a track from history
  async function handlePlayHistoryTrack(trackId: string) {
    try {
      const handledRemotely = await invoke<boolean>('v2_qconnect_play_track_if_remote', { trackId: parseInt(trackId, 10) });
      if (handledRemotely) {
        await refreshQobuzConnectRuntimeState();
        return;
      }
    } catch (err) {
      console.error('Failed to hand off history track play to remote renderer:', err);
      return;
    }

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
    radioLoading = true;
    showToast($t('radio.creating'), 'info');
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
    } finally {
      radioLoading = false;
    }
  }

  // Create QBZ track radio (used by PlaylistDetailView, FavoritesView, etc.)
  async function handleCreateQbzTrackRadio(trackId: number, trackTitle: string, artistId?: number) {
    radioLoading = true;
    showToast($t('radio.creating'), 'info');
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
    } finally {
      radioLoading = false;
    }
  }

  // Create Qobuz track radio (used by PlaylistDetailView, FavoritesView, etc.)
  async function handleCreateQobuzTrackRadio(trackId: number, trackTitle: string) {
    radioLoading = true;
    showToast($t('radio.creating'), 'info');
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
    } finally {
      radioLoading = false;
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
    let queuedCount = 0;
    for (let i = playableTracks.length - 1; i >= 0; i--) {
      const trk = playableTracks[i];
      const queued = await queueTrackNext({
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
        artist_id: trk.artistId ?? album.artistId,
        streamable: trk.streamable ?? true,
        source: 'qobuz',
        parental_warning: trk.parental_warning ?? false
      }, false, { silent: true });
      if (queued) {
        queuedCount += 1;
      }
    }

    if (queuedCount > 0) {
      showToast($t('toast.playingTracksNext', { values: { count: queuedCount } }), 'success');
    } else {
      showToast($t('toast.failedAddToQueue'), 'error');
    }
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

    let queuedCount = 0;
    for (const queueTrack of queueTracks) {
      const queued = await queueTrackLater(queueTrack, false, { silent: true });
      if (queued) {
        queuedCount += 1;
      }
    }

    if (queuedCount > 0) {
      showToast($t('toast.addedTracksToQueue', { values: { count: queuedCount } }), 'success');
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
    console.log('Playing display track:', track.id, track.title);

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
    console.log('Playing local track:', track.id, track.title);
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
  function clearPlaylistEditContext() {
    playlistModalEditPlaylist = undefined;
    playlistModalEditIsHidden = false;
    playlistModalEditCurrentFolderId = null;
  }

  function handleSidebarPlaylistEdit(payload: {
    id: number;
    name: string;
    tracks_count: number;
    isHidden: boolean;
    currentFolderId: string | null;
  }) {
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    playlistModalEditPlaylist = {
      id: payload.id,
      name: payload.name,
      tracks_count: payload.tracks_count
    };
    playlistModalEditIsHidden = payload.isHidden;
    playlistModalEditCurrentFolderId = payload.currentFolderId;
    openPlaylistModal('edit', []);
  }

  function handlePlaylistModalClose() {
    clearPlaylistEditContext();
    closePlaylistModal();
  }

  function openCreatePlaylist() {
    clearPlaylistEditContext();
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    openPlaylistModal('create', []);
  }

  function openAddToPlaylist(trackIds: number[], isLocal = false) {
    clearPlaylistEditContext();
    userPlaylists = sidebarRef?.getPlaylists() ?? [];
    openPlaylistModal('addTrack', trackIds, isLocal);
  }

  function handlePlaylistCreated(playlist?: { id: number; name: string; tracks_count: number }) {
    const trackCount = playlistModalTrackIds.length;
    const isLocal = playlistModalTracksAreLocal;

    if (playlistModalMode === 'addTrack') {
      showToast($t('toast.tracksAddedToPlaylist'), 'success');
    } else if (playlistModalMode === 'edit') {
      showToast($t('toast.playlistUpdated'), 'success');
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

    clearPlaylistEditContext();
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
    // Activate offline session FIRST — this initializes per-user stores
    // (library, offline cache, audio settings) using the last known user profile.
    // Must happen before setManualOffline which requires an active store.
    try {
      await invoke('v2_activate_offline_session');
    } catch (err) {
      console.error('[Offline] Failed to activate offline session:', err);
      // If no previous session exists, show friendly message
      const errStr = String(err);
      if (errStr.includes('No active session') || errStr.includes('no previous session')) {
        showToast($t('offline.noPreviousSession'), 'error');
        return;
      }
      // Continue for other errors - offline mode should be best-effort
    }

    // Now that stores are initialized, enable manual offline mode
    try {
      await setManualOffline(true);
    } catch (err) {
      console.warn('[Offline] setManualOffline failed (non-fatal):', err);
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
    migrateLocalStorageV2(info.userId);

    // Re-read stores that were initialised at module-load (before userId was set)
    rehydratePurchasesStore();
    rehydrateVerboseCapture();
    showPurchases = getShowPurchases();

    loadSystemNotificationsPreference();

    // Re-sync volume from the now-correct user-scoped localStorage key
    await resyncPersistedVolume();
    reloadLyricsDisplay();

    // Signal that per-user backend stores are ready — the launch update
    // flow $effect gates on this to avoid reading default preferences
    sessionReady = true;

    // Restore Last.fm session now that backend session is active
    restoreLastfmSession();

    showToast($t('toast.welcomeUser', { values: { name: info.userName } }), 'success');

    // Initialize playback preferences first — session restore depends on this
    initOfflineCacheStates(); // has internal try/catch
    await initPlaybackPreferences().then(() => {
      sessionPersistEnabled = getCachedPreferences().persist_session;
      console.log('[Session] Persist session enabled:', sessionPersistEnabled);
    }).catch(err => console.debug('[PlaybackPrefs] Init deferred:', err));

    // Restore previous session EARLY (before network-heavy init) so the
    // player bar shows the last track instantly instead of "No track playing"
    if (!sessionPersistEnabled) {
      console.log('[Session] Session persistence disabled, skipping restore');
    }
    try {
      await refreshQobuzConnectRuntimeState();
      const qconnectRuntimeActive = Boolean(qobuzConnectStatus.running || isQconnectRemoteModeActive());

      if (qconnectRuntimeActive) {
        await syncQueueState();
      }

      if (sessionPersistEnabled && !qconnectRuntimeActive) {
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
              albumId: track.album_id ?? undefined,
              artistId: track.artist_id ?? undefined,
              isLocal: track.is_local ?? false,
              source: track.source ?? 'qobuz',
              parental_warning: track.parental_warning ?? false,
            });

            // Update MPRIS metadata so playerctl shows track info
            updateMediaMetadata({
              title: track.title,
              artist: track.artist,
              album: track.album,
              durationSecs: track.duration_secs,
              coverUrl: track.artwork_url || null
            });

            // Restore playback context (album, playlist, etc.)
            try {
              const savedCtx = localStorage.getItem('qbz-playback-context');
              if (savedCtx) {
                const ctx = JSON.parse(savedCtx);
                await setPlaybackContext(ctx.type, ctx.id, ctx.label, ctx.source, ctx.track_ids, ctx.current_position);
                console.log(`[Session] Playback context restored: ${ctx.type} · ${ctx.label}`);
              }
            } catch {
              console.debug('[Session] Could not restore playback context');
            }

            // First play will load a fresh stream instead of seeking
            setPendingSessionRestore(track.id);
            console.log(`[Session] Track ${track.id} restored visually (paused at 0:00)`);
          }

          console.log('[Session] Session restored successfully');
        }

        // Restore last page (opt-in via settings)
        if (session) {
          restoreLastView(session);
        }
      } else if (qconnectRuntimeActive) {
        console.log('[Session] Skipping local session restore while Qobuz Connect remote mode is active');
      }
    } catch (err) {
      console.error('[Session] Failed to restore session:', err);
    }

    // Continue with remaining store initialization (non-blocking for session)
    initBlacklistStore().catch(err => console.debug('[Blacklist] Init deferred:', err));
    initCustomArtistImageStore().catch(err => console.debug('[CustomArtistImages] Init deferred:', err));
    initCustomAlbumCoverStore().catch(err => console.debug('[CustomAlbumCovers] Init deferred:', err));
    refreshUpdatePreferences().catch(err => console.debug('[Updates] Prefs refresh deferred:', err));

    // Load audio settings (normalization state + backend info) now that session is active
    invoke<{ normalization_enabled: boolean; backend_type: string | null; alsa_plugin: string | null }>('v2_get_audio_settings').then((settings) => {
      normalizationEnabled = settings.normalization_enabled;
      const alsaHw = settings.backend_type === 'Alsa' && settings.alsa_plugin === 'Hw';
      isAlsaDirectHw = alsaHw;
      if (alsaHw && volume !== 100) {
        playerSetVolume(100);
        volume = 100;
      }
    }).catch((err) => {
      console.error('[AudioSettings] Failed to load:', err);
    });

    // Load favorites now that login is confirmed (sync with Qobuz)
    loadFavorites();        // Track favorites
    loadAlbumFavorites();   // Album favorites
    loadArtistFavorites();  // Artist favorites
    loadLabelFavorites();   // Label favorites
    loadAwardFavorites();   // Award favorites

    // Refresh offline status now that we're logged in
    refreshOfflineStatus().catch(err => console.debug('[Offline] Status refresh deferred:', err));

    // Train recommendation scores in background (fire-and-forget)
    trainScores().then(() => {
      console.log('[Reco] Scores trained after login');
    }).catch(err => {
      console.debug('[Reco] Score training failed:', err);
    });
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
      clearCustomArtistImages();
      clearCustomAlbumCovers();
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
          return;
        }
        break;
      case 'purchase-album':
        if (contextId) {
          selectedPurchaseAlbumId = contextId;
          restoreView('purchase-album');
          return;
        }
        break;
      default:
        if (isSessionRestoreSafeView(view)) {
          restoreView(view);
          return;
        }
        break;
    }

    const fallbackView = getSessionFallbackView(view);
    console.warn('[Session] Skipping invalid last-view restore and falling back:', {
      view,
      contextId,
      contextType,
      fallbackView,
    });
    restoreView(fallbackView);
  }

  // Save session state before window closes
  async function saveSessionBeforeClose() {
    if (!isLoggedIn) return;
    if (!shouldPersistLocalSession()) return;

    try {
      const {
        view: viewToPersist,
        viewContextId,
        viewContextType,
      } = getPersistedSessionViewState();

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
        viewToPersist,
        viewContextId,
        viewContextType
      );

      // Persist playback context for session restore
      const ctx = getCurrentContext();
      if (ctx) {
        localStorage.setItem('qbz-playback-context', JSON.stringify(ctx));
      } else {
        localStorage.removeItem('qbz-playback-context');
      }
      console.log('[Session] Session saved on close');
    } catch (err) {
      console.error('[Session] Failed to save session on close:', err);
    }
  }

  // Keyboard Shortcuts - delegated to keybindings system
  function handleKeydown(e: KeyboardEvent) {
    if (!isLoggedIn) return;

    // Global fullscreen toggle (F11) — works even outside immersive player
    if (e.key === 'F11') {
      e.preventDefault();
      const win = getCurrentWindow();
      win.isFullscreen().then(fs => win.setFullscreen(!fs));
      return;
    }

    // Delegate to keybinding manager (handles input target filtering internally)
    keybindingHandler(e);
  }

  // Playback and queue state listeners
  // Always keep active to receive external events (e.g., from remote control)
  $effect(() => {
    startPolling();
    startQueueEventListener();
    // Also listen for offline-cache unlock start/end events so the
    // track row can flip its play glyph to a padlock animation while
    // a CMAF bundle is being decrypted.
    void startUnlockingPolling();

    return () => {
      stopPolling();
      stopQueueEventListener();
      stopUnlockingPolling();
    };
  });

  // OS window title: opt-in to reflect currently playing track.
  // Depends on the store preference version + currentTrack so it reacts to
  // both setting changes and track changes immediately.
  $effect(() => {
    void windowTitlePrefVersion;
    const enabled = getWindowTitleEnabled();
    const template = getWindowTitleTemplate();
    const track = currentTrack;
    const trackTitle = track?.title;
    const trackArtist = track?.artist;
    const trackAlbum = track?.album;

    let nextTitle = 'QBZ';
    if (enabled && track) {
      const rendered = renderWindowTitle(template, {
        artist: trackArtist,
        title: trackTitle,
        album: trackAlbum,
      });
      if (rendered.length > 0) {
        nextTitle = rendered;
      }
    }

    try {
      getCurrentWindow().setTitle(nextTitle).catch((err) => {
        console.warn('[WindowTitle] setTitle failed:', err);
      });
    } catch (err) {
      console.warn('[WindowTitle] setTitle threw:', err);
    }
  });

  // Debounced full session save (coalesces rapid state changes into a single save)
  let sessionSaveDebounce: ReturnType<typeof setTimeout> | null = null;
  function debouncedFullSessionSave() {
    if (!shouldPersistLocalSession()) return;
    if (sessionSaveDebounce) clearTimeout(sessionSaveDebounce);
    sessionSaveDebounce = setTimeout(() => {
      sessionSaveDebounce = null;
      if (!shouldPersistLocalSession()) return;
      saveSessionBeforeClose();
    }, 2000);
  }

  // Periodic full session save during playback
  let sessionSaveInterval: ReturnType<typeof setInterval> | null = null;

  $effect(() => {
    const qconnectRemoteModeActive = isQconnectRemoteModeActive();

    // Start periodic save when playing, stop when paused/stopped
    if (isPlaying && currentTrack && isLoggedIn && !qconnectRemoteModeActive) {
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

  // Quality Fallback Modal handlers
  async function handleQualityFallbackTryLower() {
    isQualityFallbackOpen = false;
    if (qualityFallbackTrack) {
      await playTrack(qualityFallbackTrack, { ...qualityFallbackOptions, forceLowestQuality: true });
    }
  }

  async function handleQualityFallbackSkip() {
    isQualityFallbackOpen = false;
    const next = await nextTrack();
    if (next) {
      const nextSource = resolvePlaybackSource(next);
      const nextIsLocal = isPlaybackSourceLocal(nextSource, next.is_local ?? false);
      const nextSamplingRate = next.sample_rate == null
        ? undefined
        : nextIsLocal
          ? next.sample_rate / 1000
          : next.sample_rate;
      await playTrack({
        id: next.id,
        title: next.title,
        artist: next.artist,
        album: next.album,
        duration: next.duration_secs,
        artwork: next.artwork_url || '',
        quality: next.hires ? 'Hi-Res' : 'CD Quality',
        albumId: next.album_id || undefined,
        artistId: next.artist_id || undefined,
        bitDepth: next.bit_depth || undefined,
        samplingRate: nextSamplingRate,
        source: nextSource,
        isLocal: nextIsLocal
      }, {
        isLocal: nextIsLocal,
        source: nextSource,
        showLoadingToast: true,
        showSuccessToast: true
      });
    } else {
      setIsPlaying(false);
    }
  }

  onMount(() => {
    // Bootstrap app (theme, mouse nav, Last.fm restore)
    const { cleanup: cleanupBootstrap } = bootstrapApp();

    // Window-title preference: load from localStorage and subscribe so that
    // toggling the setting updates the OS title bar immediately.
    initWindowTitleStore();
    const unsubscribeWindowTitle = subscribeWindowTitle(() => {
      windowTitlePrefVersion = windowTitlePrefVersion + 1;
    });

    // Quality fallback modal listener
    function handleQualityFallbackPrompt(e: Event) {
      const detail = (e as CustomEvent).detail;
      qualityFallbackTrackTitle = detail.trackTitle;
      qualityFallbackTrack = detail.track;
      qualityFallbackOptions = detail.options;
      isQualityFallbackOpen = true;
    }
    window.addEventListener('quality-fallback-prompt', handleQualityFallbackPrompt);

    void refreshQobuzConnectRuntimeState();
    const qobuzConnectStatusInterval = setInterval(() => {
      if (isQconnectPanelOpen || isQobuzConnectConnected) {
        void refreshQobuzConnectRuntimeState();
      } else {
        void refreshQobuzConnectStatus();
      }
    }, 5000);

    // Periodic QConnect position reports (every 2s) so controllers see track progress.
    // Only fires when connected and playing. queue_item_ids auto-filled by backend.
    const qconnectPositionReportInterval = setInterval(() => {
      if (isQobuzConnectConnected && !qconnectPeerRendererActive && isPlaying && currentTrack) {
        if (shouldSkipQconnectPlaybackReport(currentTrack?.id ?? null)) {
          return;
        }
        // QConnect protocol uses milliseconds for position/duration.
        const positionMs = Math.round((currentTime || 0) * 1000);
        const durationMs = Math.round((duration || 0) * 1000);
        const payload = {
          playingState: 2,
          currentPosition: positionMs,
          duration: durationMs,
          currentQueueItemId: null,
          nextQueueItemId: null,
          currentTrackId: currentTrack?.id ?? null
        };
        logQconnectPlaybackReport('interval', payload);
        invoke('v2_qconnect_report_playback_state', payload).catch((err) => {
          pushQobuzConnectDiagnostic('qconnect:report_playback_state:error', 'warn', {
            source: 'interval',
            error: String(err),
            payload
          });
        });
      }
    }, 2000);

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
    registerAction('nav.search', () => {
      if (isSearchInTitlebar()) {
        titlebarRef?.focusSearch();
      } else {
        if (!getIsExpanded()) {
          expandSidebar();
        }
        sidebarRef?.focusSearch();
      }
    });
    registerAction('nav.settings', () => navigateTo('settings'));
    registerAction('ui.sidebar', toggleSidebar);
    registerAction('ui.focusMode', toggleFocusMode);
    registerAction('ui.miniPlayer', () => { void enterMiniplayerMode(); });
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

    infinitePlayEnabled = isInfinitePlayEnabled();

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
      const wasPlaylistModalOpen = isPlaylistModalOpen;
      // Close network sidebar and lyrics when queue opens; restore when it closes
      if (uiState.isQueueOpen && !isQueueOpen) {
        closeContentSidebar('network');
        hideLyricsSidebar();
      } else if (!uiState.isQueueOpen && isQueueOpen) {
        restoreContentSidebar();
      }
      isQueueOpen = uiState.isQueueOpen;
      isFullScreenOpen = uiState.isFullScreenOpen;
      isFocusModeOpen = uiState.isFocusModeOpen;
      isCastPickerOpen = uiState.isCastPickerOpen;
      isQconnectPanelOpen = uiState.isQconnectPanelOpen;
      isPlaylistModalOpen = uiState.isPlaylistModalOpen;
      playlistModalMode = uiState.playlistModalMode;
      playlistModalTrackIds = uiState.playlistModalTrackIds;
      playlistModalTracksAreLocal = uiState.playlistModalTracksAreLocal;
      if (wasPlaylistModalOpen && !uiState.isPlaylistModalOpen) {
        clearPlaylistEditContext();
      }
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

    // Declared upfront because subscribeTitleBar invokes the callback
    // immediately on subscription, which references this helper.
    const applyChromeClass = () => {
      if (typeof document === 'undefined') return;
      const active = matchSystemChrome && showTitleBar && windowTransparent;
      document.documentElement.classList.toggle('match-chrome-transparent', active);
    };

    // Initialize and subscribe to title bar state changes
    initTitleBarStore();
    const unsubscribeTitleBar = subscribeTitleBar(() => {
      showTitleBar = shouldShowTitleBar();
      showWindowControls = getShowWindowControls();
      applyChromeClass();
    });

    // Window chrome: subscribe to the "match system" toggle and fetch the
    // desktop corner radius once. The detection is already cached upstream
    // so this is cheap to call multiple times.
    initWindowChromeStore();
    const unsubscribeWindowChrome = subscribeWindowChrome(() => {
      matchSystemChrome = getMatchSystemWindowChrome();
      chromeRadiusPx = getCornerRadiusPx();
      windowTransparent = getWindowIsTransparent();
      applyChromeClass();
    });
    // Ask the Rust side whether the main window was actually built
    // transparent. If not, the radius CSS is a no-op (white corners
    // otherwise). The backend captured the decision at setup time.
    void (async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const transparent = (await invoke('v2_main_window_is_transparent')) as boolean;
        setWindowIsTransparent(transparent);
      } catch (e) {
        console.warn('[windowChrome] could not query transparency:', e);
      }
    })();
    void detectDesktopThemeCached().then((info) => {
      if (info && typeof info.windowCornerRadiusPx === 'number') {
        setCornerRadiusPx(info.windowCornerRadiusPx);
      }
    });

    // Initialize and subscribe to search bar location
    initSearchBarLocation();
    const unsubscribeSearchBarLocation = subscribeSearchBarLocation(() => {
      searchBarLocationPref = getSearchBarLocation();
    });

    // Initialize and subscribe to window controls customization
    initWindowControlsStore();
    const unsubscribeWindowControls = subscribeWindowControls(() => {
      windowControlsConfig = getWindowControls();
      // Recompute titlebar nav position when controls change
      titlebarNavPosition = getResolvedPosition(windowControlsConfig.position);
    });

    // Initialize and subscribe to titlebar nav store
    initTitlebarNavStore();
    const unsubscribeTitlebarNav = subscribeTitlebarNav(() => {
      titlebarNavEnabled = isTitlebarNavEnabled();
      titlebarNavPosition = getResolvedPosition(windowControlsConfig.position);
      tbNavDiscover = isDiscoverInTitlebar();
      tbNavFavorites = isFavoritesInTitlebar();
      tbNavLibrary = isLibraryInTitlebar();
      tbNavPurchases = isPurchasesInTitlebar();
    });

    // Detect floating window state (not maximized) for rounded corners + shadow
    let unlistenResize: (() => void) | undefined;
    (async () => {
      const appWindow = getCurrentWindow();
      isWindowFloating = !(await appWindow.isMaximized());
      unlistenResize = await appWindow.onResized(async () => {
        isWindowFloating = !(await appWindow.isMaximized());
      });
    })();

    // Sync titlebar search query with search state store
    const unsubscribeTitlebarSearch = subscribeSearchQuery((query) => {
      titlebarSearchQuery = query;
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
      const prevView = activeView;
      const prevItemId = currentNavItemId;

      // Save scroll position of the view we're leaving
      if (prevView !== navState.activeView || prevItemId !== navState.activeItemId) {
        const scrollTopToSave = activeScrollTarget?.scrollTop ?? globalScrollTop;
        saveScrollPosition(prevView, scrollTopToSave, prevItemId);
      }

      activeView = navState.activeView;
      selectedPlaylistId = navState.selectedPlaylistId;
      currentNavItemId = navState.activeItemId;

      // On back/forward navigation, reload the specific item from history
      if (navState.isBackForward && navState.activeItemId != null) {
        restoreItemFromHistory(navState.activeView, navState.activeItemId);
      }

      // Restore scroll position on back/forward
      if (navState.isBackForward) {
        const savedScroll = getSavedScrollPosition(navState.activeView, navState.activeItemId);
        tick().then(() => {
          if (activeScrollTarget) {
            activeScrollTarget.scrollTop = savedScroll;
            globalScrollTop = savedScroll;
          }
        });
      }
    });

    // Subscribe to player state changes
    let prevTrackId: number | null = null;
    const unsubscribePlayer = subscribePlayer(() => {
      const playerState = getPlayerState();
      const remotePeerActive = qconnectPeerRendererActive;
      const wasPlaying = isPlaying;
      volume = playerState.volume;
      normalizationGain = playerState.normalizationGain;
      bufferProgress = playerState.bufferProgress;
      if (remotePeerActive) {
        return;
      }
      currentTrack = playerState.currentTrack;
      isPlaying = playerState.isPlaying;
      currentTime = playerState.currentTime;
      duration = playerState.duration;
      isFavorite = playerState.isFavorite;
      const allowLocalSessionPersistence = shouldPersistLocalSession();

      // Save position during playback (debounced to every 5s)
      if (allowLocalSessionPersistence && isPlaying && currentTrack && currentTime > 0) {
        debouncedSavePosition(Math.floor(currentTime));
      }

      // Flush position save immediately when pausing
      if (allowLocalSessionPersistence && wasPlaying && !isPlaying && currentTrack && currentTime > 0) {
        flushPositionSave(Math.floor(currentTime));
      }

      // Full session save on track change or pause (debounced 2s)
      const trackId = currentTrack?.id ?? null;
      const trackChanged = trackId !== prevTrackId;
      // Always update prevTrackId (even during QConnect mode) to prevent
      // the QConnect reporter from treating every tick as a track change.
      if (trackChanged) {
        prevTrackId = trackId;
      }
      if (allowLocalSessionPersistence && trackChanged) {
        if (trackId !== null) debouncedFullSessionSave();
      }
      if (allowLocalSessionPersistence && wasPlaying && !isPlaying && currentTrack) {
        debouncedFullSessionSave();
      }

      // QConnect renderer state relay: report state transitions to server.
      // QConnect protocol uses milliseconds for position/duration.
      // queue_item_ids are auto-filled by the backend from renderer state.
      if (isQobuzConnectConnected && !qconnectPeerRendererActive) {
        const playingState = isPlaying ? 2 : (currentTrack ? 3 : 1);
        const positionMs = Math.round((currentTime || 0) * 1000);
        const durationMs = Math.round((playerState.duration || 0) * 1000);
        // Report immediately on play/pause change or track change
        if (wasPlaying !== isPlaying || trackChanged) {
          if (shouldSkipQconnectPlaybackReport(currentTrack?.id ?? null)) {
            return;
          }
          const payload = {
            playingState: playingState,
            currentPosition: positionMs,
            duration: durationMs,
            currentQueueItemId: null,
            nextQueueItemId: null,
            currentTrackId: currentTrack?.id ?? null
          };
          logQconnectPlaybackReport('player_transition', payload);
          invoke('v2_qconnect_report_playback_state', payload).catch((err) => {
            pushQobuzConnectDiagnostic('qconnect:report_playback_state:error', 'warn', {
              source: 'player_transition',
              error: String(err),
              payload
            });
          });
        }
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
      // Close network sidebar and queue when lyrics opens; restore when it closes
      if (state.sidebarVisible && !lyricsSidebarVisible) {
        closeContentSidebar('network');
        closeQueue();
      } else if (!state.sidebarVisible && lyricsSidebarVisible) {
        restoreContentSidebar();
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
      // When QConnect controls playback, the server/controller manages track advancement.
      // The frontend must NOT auto-advance or it will fight the remote controller.
      if (qconnectSuppressLocalPlaybackAutomation) {
        console.log('[Player] Auto-advance suppressed: QConnect is controlling playback');
        return;
      }
      // Only `track_only` mode stops auto-advance. Both `continue` and `infinite`
      // advance through the queue; `infinite` additionally refills it on end.
      if (!isAutoplayEnabled() && !isInfinitePlayEnabled()) {
        setQueueEnded(true);
        await stopPlayback();
        setIsPlaying(false);
        return;
      }
      const previousTrackId = currentTrack?.id ?? null;
      let nextTrackResult = await nextTrackGuarded();
      if (!nextTrackResult && isInfinitePlayEnabled()) {
        // Queue ended with infinite play on — extend with radio and retry.
        const recentIds: number[] = [];
        if (currentTrack) recentIds.push(currentTrack.id);
        for (const item of historyTracks.slice(0, 5)) {
          const numericId = (item as any).trackId;
          if (typeof numericId === 'number') recentIds.push(numericId);
        }
        if (recentIds.length > 0) {
          try {
            const radioTracks = await invoke<BackendQueueTrack[]>('v2_create_infinite_radio', {
              recentTrackIds: recentIds.slice(0, 5)
            });
            if (radioTracks && radioTracks.length > 0) {
              await invoke('v2_bulk_add_to_queue', { tracks: radioTracks });
              nextTrackResult = await nextTrackGuarded();
            }
          } catch (err) {
            console.error('[Player] Auto-advance: infinite radio extend failed:', err);
          }
        }
      }
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
          const forcedNext = await nextTrackGuarded();
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
      if (qconnectSuppressLocalPlaybackAutomation) return;
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

    setOnTogglePlayOverride(handleQconnectTogglePlayOverride);

    // Gapless: provide callback to get next track ID for pre-queuing
    setGaplessGetNextTrackId(() => {
      // Only suppress local gapless when a peer renderer owns playback.
      if (qconnectSuppressLocalPlaybackAutomation) return null;
      try {
        const queueState = getQueueState();
        if (queueState.queue.length > 0) {
          const currentId = currentTrack?.id ?? null;

          if (queueState.repeatMode == 'one') {
            return currentId;
          }
          
          const firstId = Number(queueState.queue[0].id);
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
      if (qconnectSuppressLocalPlaybackAutomation) return;
      console.log('[Gapless] Handling transition to track', trackId);
      // Advance the queue to match backend state
      const advanced = await nextTrackGuarded();
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
    let unlistenQconnectEvent: UnlistenFn | null = null;
    let unlistenQconnectError: UnlistenFn | null = null;
    let unlistenQconnectAdmissionBlocked: UnlistenFn | null = null;
    let unlistenQconnectDiagnostic: UnlistenFn | null = null;
    let unlistenQconnectRendererReportDebug: UnlistenFn | null = null;
    let unlistenAudioDeviceMissing: UnlistenFn | null = null;

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
          case 'stop': {
            try {
              const handledRemotely = await invoke<boolean>('v2_qconnect_stop_if_remote');
              if (!handledRemotely) {
                await stopPlayback();
              }
            } catch {
              await stopPlayback();
            }
            break;
          }
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
                await handleVolumeChange(newVolume);
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

      const unlisten6 = await listen('qconnect:event', (event) => {
        pushQobuzConnectDiagnostic('qconnect:event', 'info', event.payload);
        void refreshQobuzConnectRuntimeState();

        const payload = event.payload;
        if (payload && typeof payload === 'object') {
          const payloadObj = payload as Record<string, unknown>;
          // When QConnect connects while QBZ is already playing, push the current
          // queue to the server so controllers immediately see the right tracks.
          if ('TransportConnected' in payloadObj) {
            console.log('[QConnect] TransportConnected detected, checking if local queue should be pushed');
            if (isPlaying && currentTrack) {
              // Delay briefly so the QConnect session setup (ask_for_queue_state etc.)
              // completes before we try to push our queue.
              setTimeout(() => {
                const queueState = getQueueState();
                const trackIds = queueState.queue
                  .map(item => item.trackId ?? parseInt(item.id))
                  .filter((id): id is number => typeof id === 'number' && !isNaN(id) && id > 0);
                if (trackIds.length > 0) {
                  console.log('[QConnect] Pushing local queue to remote on connect (%d tracks)', trackIds.length);
                  loadQconnectQueue(trackIds, 0).then(ok => {
                    if (ok) console.log('[QConnect] Local queue pushed to remote on connect');
                    else console.warn('[QConnect] Local queue NOT pushed on connect (rejected or failed)');
                  }).catch(err => console.error('[QConnect] Local queue push on connect error:', err));
                }
              }, 2000);
            }
          }

          // Sync QBZ local queue when QConnect remote queue changes or
          // renderer commands move the current track (next/prev from controllers).
          const needsQueueSync =
            'QueueUpdated' in payload ||
            'RendererCommandApplied' in payload ||
            'PendingActionCompleted' in payload;
          if (needsQueueSync) {
            syncQueueState();
          }

          const needsQconnectSnapshotRefresh =
            'QueueUpdated' in payload ||
            'RendererUpdated' in payload ||
            'RendererCommandApplied' in payload ||
            'PendingActionCompleted' in payload ||
            'SessionManagementEvent' in payload;
          if (needsQconnectSnapshotRefresh) {
            void refreshQobuzConnectRuntimeState();
          }
        }
      });
      if (disposed) { unlisten6(); return; }
      unlistenQconnectEvent = unlisten6;

      const unlisten7 = await listen<string>('qconnect:error', (event) => {
        pushQobuzConnectDiagnostic('qconnect:error', 'error', event.payload);
        qobuzConnectStatus = {
          ...qobuzConnectStatus,
          last_error: event.payload
        };
      });
      if (disposed) { unlisten7(); return; }
      unlistenQconnectError = unlisten7;

      const unlisten8 = await listen<QconnectAdmissionBlockedEvent>('qconnect:admission_blocked', (event) => {
        pushQobuzConnectDiagnostic('qconnect:admission_blocked', 'warn', event.payload);
        showToast($t(qconnectAdmissionReasonKey(event.payload.reason)), 'warning');
      });
      if (disposed) { unlisten8(); return; }
      unlistenQconnectAdmissionBlocked = unlisten8;

      const unlisten9 = await listen<QconnectDiagnosticsPayload>('qconnect:diagnostic', (event) => {
        pushQobuzConnectDiagnostic(
          event.payload.channel,
          event.payload.level ?? 'info',
          event.payload.payload
        );
      });
      if (disposed) { unlisten9(); return; }
      unlistenQconnectDiagnostic = unlisten9;

      const unlisten10 = await listen<QconnectRendererReportDebugPayload>('qconnect:renderer_report_debug', (event) => {
        pushQobuzConnectDiagnostic('qconnect:renderer_report_debug', 'info', event.payload);
      });
      if (disposed) { unlisten10(); return; }
      unlistenQconnectRendererReportDebug = unlisten10;

      // Issue #307 — configured output device vanished mid-session
      // (KVM switched away, USB unplugged, sink removed). Backend emits
      // this right before play/resume when it detects the mismatch;
      // init_device already falls back to default automatically, we
      // just surface the fact to the user as a warning toast so they
      // understand why audio is now coming out of a different sink.
      const unlistenDeviceMissing = await listen<{ wanted: string; available: string[] }>('audio:device-missing', (event) => {
        const wanted = event.payload?.wanted ?? '';
        showToast(
          $t('toast.audioDeviceMissing', { values: { device: wanted } }),
          'warning',
          6000
        );
      });
      if (disposed) { unlistenDeviceMissing(); return; }
      unlistenAudioDeviceMissing = unlistenDeviceMissing;
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
      unlistenQconnectEvent?.();
      unlistenQconnectError?.();
      unlistenQconnectAdmissionBlocked?.();
      unlistenQconnectDiagnostic?.();
      unlistenQconnectRendererReportDebug?.();
      unlistenAudioDeviceMissing?.();
      unsubscribeWindowTitle();
      // Save session before cleanup
      saveSessionBeforeClose();
      cleanupBootstrap();
      document.removeEventListener('keydown', handleKeydown);
      document.removeEventListener('contextmenu', handleGlobalContextMenu);
      unregisterAll(); // Cleanup keybinding actions
      clearInterval(qobuzConnectStatusInterval);
      clearInterval(qconnectPositionReportInterval);
      window.removeEventListener('quality-fallback-prompt', handleQualityFallbackPrompt);
      window.removeEventListener('beforeunload', handleBeforeUnload);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      stopOfflineCacheEventListeners();
      unsubscribeOfflineCache();
      unsubscribeToast();
      unsubscribeUI();
      unsubscribeAuth();
      unsubscribeSidebar();
      unsubscribeTitleBar();
      unsubscribeWindowChrome();
      unsubscribeSearchBarLocation();
      unsubscribeWindowControls();
      unsubscribeTitlebarNav();
      unlistenResize?.();
      unsubscribeTitlebarSearch();
      unsubscribeOffline();
      unsubscribeNav();
      unsubscribePlayer();
      unsubscribeQueue();
      unsubscribeLyrics();
      unsubscribeContentSidebar();
      unsubscribeCast();
      setOnTogglePlayOverride(null);
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

  // Resolved artwork URL — proxied through backend cache so it works even when
  // WebKitGTK TLS is broken (AppImage on some distros). See GitHub #163.
  let resolvedArtwork = $state<string>('');
  $effect(() => {
    const raw = currentTrack
      ? (currentTrack.albumId ? resolveAlbumCover(currentTrack.albumId, currentTrack.artwork) : currentTrack.artwork)
      : '';
    if (!raw) { resolvedArtwork = ''; return; }
    // If already a local/asset URL, use directly
    if (raw.startsWith('asset://') || raw.startsWith('file://') || raw.startsWith('/')) {
      resolvedArtwork = raw;
      return;
    }
    // Proxy HTTPS URLs through backend cache
    getCachedImageUrl(raw).then(resolved => {
      resolvedArtwork = resolved;
    }).catch(() => {
      resolvedArtwork = raw;
    });
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

{#snippet titlebarNavSnippet()}
  <TitleBarNav
    {activeView}
    activeItemId={activeView === 'home' ? homeTab : undefined}
    onNavigate={navigateTo}
    position={titlebarNavPosition}
    showDiscover={tbNavDiscover}
    showFavorites={tbNavFavorites}
    showLibrary={tbNavLibrary}
    showPurchases={tbNavPurchases && showPurchases}
  />
{/snippet}

{#if !isLoggedIn}
  <LoginView onLoginSuccess={handleLoginSuccess} onStartOffline={handleStartOffline} />
{:else}
  <div
    class="app"
    class:no-titlebar={!showTitleBar}
    class:floating={isWindowFloating}
    class:match-chrome={matchSystemChrome && showTitleBar && windowTransparent}
    style="--chrome-radius: {chromeRadiusPx}px;"
  >
    <!-- macOS: drag region for window movement (overlay title bar has no native drag area) -->
    {#if !showTitleBar && platform === 'macos'}
      <div class="macos-drag-region" data-tauri-drag-region></div>
    {/if}
    <!-- Custom Title Bar (CSD) -->
    {#if showTitleBar}
      <TitleBar
        bind:this={titlebarRef}
        searchInTitlebar={isSearchInTitlebar()}
        searchQuery={titlebarSearchQuery}
        onSearchInput={handleTitlebarSearchInput}
        onSearchClear={handleTitlebarSearchClear}
        controlsPosition={windowControlsConfig.position}
        controlsShape={windowControlsConfig.shape}
        controlsSize={windowControlsConfig.size}
        controlsColors={{
          minimize: windowControlsConfig.minimizeColors,
          maximize: windowControlsConfig.maximizeColors,
          close: windowControlsConfig.closeColors,
        }}
        navSnippet={titlebarNavEnabled && showTitleBar ? titlebarNavSnippet : undefined}
        navPosition={titlebarNavPosition}
        {showWindowControls}
      />
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
      onEditPlaylist={handleSidebarPlaylistEdit}
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
      searchInTitlebar={isSearchInTitlebar()}
      discoverInTitlebar={tbNavDiscover && showTitleBar}
      favoritesInTitlebar={tbNavFavorites && showTitleBar}
      libraryInTitlebar={tbNavLibrary && showTitleBar}
      purchasesInTitlebar={tbNavPurchases && showTitleBar}
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
            onNavigateReleaseWatch={() => navigateTo('discover-release-watch')}
            onNavigateQobuzPlaylists={() => navigateTo('discover-playlists')}
            onNavigateDailyQ={() => navigateTo('dailyq')}
            onNavigateWeeklyQ={() => navigateTo('weeklyq')}
            onNavigateFavQ={() => navigateTo('favq')}
            onNavigateTopQ={() => navigateTo('topq')}
            {homeTab}
            onTabChange={(tab) => { homeTab = tab; navigateTo('home', tab); }}
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
            searchInTitlebar={isSearchInTitlebar()}
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
          onQconnectDevButtonChange={(v) => { showQconnectDevButton = v; }}
          onAudioBackendChange={(backendType, alsaPlugin) => {
            const alsaHw = backendType === 'Alsa' && alsaPlugin === 'Hw';
            isAlsaDirectHw = alsaHw;
            if (alsaHw && volume !== 100) {
              playerSetVolume(100);
              volume = 100;
            }
          }}
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
          onAwardClick={handleAwardClick}
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
          onBulkAddToPlaylist={(trackIds) => openAddToPlaylist(trackIds)}
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
          {radioLoading}
        />
      {:else if activeView === 'artist' && !selectedArtist}
        <!-- Defensive fallback: artist view active but no data loaded yet -->
        <div class="view-error">
          <p>{$t('toast.failedLoadArtist')}</p>
          <button class="view-error-back" onclick={navGoBack}>{$t('actions.back')}</button>
        </div>
      {:else if activeView === 'artist' && selectedArtist}
        <ArtistDetailView
          artist={selectedArtist}
          knownMbid={selectedArtistKnownMbid}
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
          onBulkAddToPlaylist={(trackIds) => openAddToPlaylist(trackIds)}
          onAddAlbumToPlaylist={addAlbumToPlaylistById}
          onTrackShareQobuz={shareQobuzTrackLink}
          onTrackShareSonglink={(track) => shareSonglinkTrack(track.id, track.isrc)}
          onTrackGoToAlbum={handleAlbumClick}
          onTrackGoToArtist={handleArtistClick}
          onPlaylistClick={selectPlaylist}
          onLabelClick={handleLabelClick}
          onMusicianClick={handleMusicianClick}
          onLocationClick={handleLocationClick}
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
          onBulkAddToPlaylist={(trackIds) => openAddToPlaylist(trackIds)}
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
      {:else if activeView === 'award' && selectedAward}
        <AwardView
          awardId={selectedAward.id}
          awardName={selectedAward.name}
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
          onNavigateAwardAlbums={handleNavigateAwardAlbums}
          onAwardClick={handleAwardClick}
          {downloadStateVersion}
          onArtistClick={handleArtistClick}
        />
      {:else if activeView === 'award-albums' && selectedAward}
        <AwardAlbumsView
          awardId={selectedAward.id}
          awardName={selectedAward.name}
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
      {:else if activeView === 'library' || activeView === 'library-album'}
        <LocalLibraryView
          onTrackPlay={handleLocalTrackPlay}
          onTrackPlayNext={queueLocalTrackNext}
          onTrackPlayLater={queueLocalTrackLater}
          onTrackAddToPlaylist={(trackId) => openAddToPlaylist([trackId], true)}
          onBulkAddToPlaylist={(trackIds) => openAddToPlaylist(trackIds, true)}
          onSetLocalQueue={handleSetLocalQueue}
          onQobuzArtistClick={handleArtistClick}
          activeTrackId={currentTrack?.id ?? null}
          isPlaybackActive={isPlaying}
        />
      {:else if activeView === 'playlist' && !selectedPlaylistId}
        <!-- Defensive fallback: playlist view active but no data loaded yet -->
        <div class="view-error">
          <p>{$t('toast.failedLoadPlaylist')}</p>
          <button class="view-error-back" onclick={navGoBack}>{$t('actions.back')}</button>
        </div>
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
          onBulkAddToPlaylist={(trackIds) => openAddToPlaylist(trackIds)}
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
            onBulkAddToPlaylist={(trackIds) => openAddToPlaylist(trackIds)}
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
            onRandomArtist={(artistId) => handleArtistClick(artistId)}
            onLabelClick={handleLabelClick}
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
      {:else if activeView === 'discover-release-watch'}
        <ReleaseWatchView
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
          onBulkAddToPlaylist={(trackIds) => openAddToPlaylist(trackIds)}
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
      {:else if activeView === 'artists-by-location' && artistsByLocationContext}
        <ArtistsByLocationView
          context={artistsByLocationContext}
          onBack={navGoBack}
          onArtistClick={handleArtistClick}
          onAlbumClick={handleAlbumClick}
          onAlbumPlay={playAlbumById}
        />
      {:else}
        <!-- Catch-all fallback: view has no matching data, show loading/error -->
        <div class="view-error">
          <p>{$t('actions.loading')}</p>
          <button class="view-error-back" onclick={() => navigateTo('home')}>{$t('actions.backToHome')}</button>
        </div>
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
        artwork={resolvedArtwork}
        trackTitle={currentTrack.title}
        artist={currentTrack.artist}
        album={currentTrack.album}
        quality={currentTrack.quality}
        bitDepth={currentTrack.bitDepth}
        samplingRate={currentTrack.samplingRate}
        originalBitDepth={currentTrack.originalBitDepth}
        originalSamplingRate={currentTrack.originalSamplingRate}
        format={currentTrack.format}
        isPlaying={effectiveIsPlaying}
        onTogglePlay={togglePlay}
        onSkipBack={handleSkipBack}
        onSkipForward={handleSkipForward}
        currentTime={effectiveCurrentTime}
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
        onOpenMiniPlayer={() => {
          void enterMiniplayerMode();
        }}
        onOpenFullScreen={openFullScreen}
        onCast={openCastPicker}
        {isCastConnected}
        onQobuzConnect={openQobuzConnectPanelFromNowPlaying}
        {isQobuzConnectConnected}
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
        explicit={currentTrack?.parental_warning === true}
        qconnectSessionSnapshot={qobuzConnectSessionSnapshot}
        onToggleQconnectConnection={handleQobuzConnectButton}
        qconnectBusy={qobuzConnectBusy}
        {showQconnectDevButton}
        volumeLocked={isAlsaDirectHw && !qconnectPeerRendererActive}
        {bufferProgress}
      />
    {:else}
      <NowPlayingBar
        onTogglePlay={togglePlay}
        onOpenQueue={toggleQueue}
        onOpenMiniPlayer={() => {
          void enterMiniplayerMode();
        }}
        onOpenFullScreen={openFullScreen}
        onCast={openCastPicker}
        {isCastConnected}
        onQobuzConnect={openQobuzConnectPanelFromNowPlaying}
        {isQobuzConnectConnected}
        queueOpen={isQueueOpen}
        {volume}
        onVolumeChange={handleVolumeChange}
        controlsDisabled={queue.length === 0}
        qconnectSessionSnapshot={qobuzConnectSessionSnapshot}
        onToggleQconnectConnection={handleQobuzConnectButton}
        qconnectBusy={qobuzConnectBusy}
        {showQconnectDevButton}
        volumeLocked={isAlsaDirectHw && !qconnectPeerRendererActive}
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
        artwork={resolvedArtwork}
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
        isPlaying={effectiveIsPlaying}
        onTogglePlay={togglePlay}
        onSkipBack={handleSkipBack}
        onSkipForward={handleSkipForward}
        currentTime={effectiveCurrentTime}
        {duration}
        onSeek={handleSeek}
        {volume}
        onVolumeChange={handleVolumeChange}
        onToggleMute={handleToggleMute}
        volumeLocked={isAlsaDirectHw && !qconnectPeerRendererActive}
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
            handleQueueTrackPlay(queue[queueIndex]?.id?.toString() ?? '', queueIndex);
          }
          // index === historyLen is current track, do nothing
        }}
        onQueueClear={handleClearQueue}
        {historyTracks}
        onPlayHistoryTrack={handlePlayHistoryTrack}
        isInfinitePlay={infinitePlayEnabled}
        onToggleInfinitePlay={handleToggleInfinitePlay}
        explicit={currentTrack?.parental_warning === true}
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
      playlist={playlistModalMode === 'edit' ? playlistModalEditPlaylist : undefined}
      trackIds={playlistModalTrackIds}
      isLocalTracks={playlistModalTracksAreLocal}
      isHidden={playlistModalMode === 'edit' ? playlistModalEditIsHidden : false}
      currentFolderId={playlistModalMode === 'edit' ? playlistModalEditCurrentFolderId : null}
      {userPlaylists}
      onClose={handlePlaylistModalClose}
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

    <!-- Quality Fallback Modal -->
    <QualityFallbackModal
      isOpen={isQualityFallbackOpen}
      trackTitle={qualityFallbackTrackTitle}
      onTryLower={handleQualityFallbackTryLower}
      onSkip={handleQualityFallbackSkip}
      onClose={() => isQualityFallbackOpen = false}
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
        autoUpdateEligible={isAutoUpdateEligible()}
        onClose={handleUpdateClose}
        onVisitReleasePage={handleUpdateVisit}
        onAutoUpdate={handleAutoUpdate}
      />

      <UpdateReminderModal
        isOpen={isReminderModalOpen}
        onClose={handleReminderClose}
        onRemindLater={handleReminderLater}
        onIgnoreRelease={handleReminderIgnoreRelease}
        onDisableAllUpdates={handleReminderDisableUpdates}
      />

      <UpdateProgressModal
        isOpen={isAutoUpdating}
        progress={autoUpdateProgress}
        onCancel={handleAutoUpdateCancel}
        onFallbackManual={handleAutoUpdateFallbackManual}
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

    <SnapWelcomeModal
      isOpen={isSnapWelcomeOpen}
      onClose={handleSnapWelcomeClose}
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

    <QconnectPanel
      isOpen={isQconnectPanelOpen}
      onClose={closeQconnectPanel}
      status={qobuzConnectStatus}
      busy={qobuzConnectBusy}
      onToggleConnection={handleQobuzConnectButton}
      queueSnapshot={qobuzConnectQueueSnapshot}
      rendererSnapshot={qobuzConnectRendererSnapshot}
      sessionSnapshot={qobuzConnectSessionSnapshot}
      showDevDiagnostics={showQconnectDevDiagnostics}
      diagnosticsLogs={qobuzConnectDiagnosticsLogs}
      onClearDiagnostics={clearQobuzConnectDiagnostics}
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

  .app.floating {
    border-radius: 0;
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.4), 0 0 1px rgba(0, 0, 0, 0.3);
    overflow: hidden;
  }

  /* Match system window chrome (Plasma / GNOME): apply the detected
     decoration radius and a thin edge outline so the window reads as
     its own surface against the desktop. Only takes effect in floating
     state (not maximized) and when the custom title bar is active.
     Requires the Tauri window to have been built transparent (Phase 2:
     match_system_window_chrome persists to window_settings and gates the
     transparency path at startup). */
  :global(html.match-chrome-transparent),
  :global(html.match-chrome-transparent body) {
    background: transparent !important;
    margin: 0 !important;
    padding: 0 !important;
  }
  /* Small win that works today: rounded corners matching the detected
     desktop decoration radius. clip-path + border-radius + GPU layer
     give clean anti-aliasing on WebKitGTK. A proper system shadow and
     frame need compositor hints that wry doesn't expose yet — tracked
     as a Phase 3 follow-up. */
  .app.match-chrome.floating {
    border-radius: var(--chrome-radius, 10px);
    clip-path: inset(0 round var(--chrome-radius, 10px));
    transform: translateZ(0);
    backface-visibility: hidden;
  }
  .app.match-chrome:not(.floating) {
    border-radius: 0;
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
    height: calc(100vh - var(--player-bar-height, 104px) - 44px);
    overflow: hidden;
    position: relative;
  }

  .main-content {
    flex: 1;
    min-width: 0;
    height: calc(100vh - var(--player-bar-height, 104px) - 44px);
    overflow: hidden; /* Views handle their own scrolling */
    padding-right: 8px; /* Gap between scrollbar and window edge */
    background-color: var(--bg-primary, #0f0f0f);
  }

  /* Adjust heights when title bar is hidden */
  .app.no-titlebar .content-area,
  .app.no-titlebar .main-content {
    height: calc(100vh - var(--player-bar-height, 104px));
  }

  /* macOS: pad main content to clear native overlay title bar */
  :global(html.macos) .main-content {
    padding-top: 16px;
    height: calc(100vh - 104px - 16px);
  }

  /* macOS: home view handles its own spacing */
  :global(html.macos) .main-content :global(.home-view) {
    margin-top: -16px;
  }

  /* macOS: invisible drag region for window movement (overlay title bar) */
  :global(html.macos) .macos-drag-region {
    height: 28px;
    width: 100%;
    position: absolute;
    top: 0;
    left: 0;
    z-index: 9999;
    -webkit-app-region: drag;
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
