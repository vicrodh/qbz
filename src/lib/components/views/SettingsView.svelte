<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { ArrowLeft, FolderOpen, ChevronDown, ChevronRight } from 'lucide-svelte';
  import Toggle from '../Toggle.svelte';
  import Dropdown from '../Dropdown.svelte';
  import VolumeSlider from '../VolumeSlider.svelte';
  import Tooltip from '../Tooltip.svelte';
  import {
    getDownloadCacheStats,
    clearDownloadCache,
    openDownloadCacheFolder,
    type DownloadCacheStats
  } from '$lib/stores/downloadState';
  import { clearCache as clearLyricsCache } from '$lib/stores/lyricsStore';
  import {
    getToastsEnabled,
    setToastsEnabled,
    loadToastsPreference
  } from '$lib/stores/toastStore';
  import {
    getSystemNotificationsEnabled,
    setSystemNotificationsEnabled,
    loadSystemNotificationsPreference
  } from '$lib/services/playbackService';
  import { setLocale, locale, t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import MigrationModal from '../MigrationModal.svelte';
  import {
    subscribe as subscribeOffline,
    getStatus as getOfflineStatus,
    getSettings as getOfflineSettings,
    setManualOffline,
    setShowPartialPlaylists,
    setAllowCastWhileOffline,
    setAllowImmediateScrobbling,
    setAllowAccumulatedScrobbling,
    setShowNetworkFoldersInManualOffline,
    checkNetwork,
    type OfflineStatus,
    type OfflineSettings
  } from '$lib/stores/offlineStore';
  import { showToast } from '$lib/stores/toastStore';

  interface Props {
    onBack?: () => void;
    onLogout?: () => void;
    userName?: string;
    userEmail?: string;
    subscription?: string;
  }

  interface CacheStats {
    cached_tracks: number;
    current_size_bytes: number;
    max_size_bytes: number;
    fetching_count: number;
  }

  interface PipewireSink {
    name: string;
    description: string;
    volume: number | null;
    is_default: boolean;
  }

  interface HardwareAudioStatus {
    hardware_sample_rate: number | null;
    hardware_format: string | null;
    is_active: boolean;
  }

  let { onBack, onLogout, userName = 'User', userEmail = '', subscription = 'Qobuz' }: Props = $props();

  // Cache state (memory cache)
  let cacheStats = $state<CacheStats | null>(null);
  let isClearing = $state(false);

  // Download cache state (offline storage)
  let downloadStats = $state<DownloadCacheStats | null>(null);
  let isClearingDownloads = $state(false);

  // Lyrics cache state
  let isClearingLyrics = $state(false);

  // Migration state
  let showMigrationModal = $state(false);
  let legacyTracksCount = $state(0);

  // Offline mode state
  let offlineStatus = $state<OfflineStatus>(getOfflineStatus());
  let offlineSettings = $state<OfflineSettings>(getOfflineSettings());
  let isCheckingNetwork = $state(false);

  // Section navigation
  let settingsViewEl: HTMLDivElement;
  let audioSection: HTMLElement;
  let playbackSection: HTMLElement;
  let offlineModeSection: HTMLElement;
  let appearanceSection: HTMLElement;
  let downloadsSection: HTMLElement;
  let librarySection: HTMLElement;
  let integrationsSection: HTMLElement;
  let storageSection: HTMLElement;
  let lyricsSection: HTMLElement;
  let apiKeysSection: HTMLElement;
  let activeSection = $state('audio');

  // Navigation section definitions (static, refs resolved at click/scroll time)
  const navSectionDefs = [
    { id: 'audio', label: 'Audio' },
    { id: 'playback', label: 'Playback' },
    { id: 'offline', label: 'Offline' },
    { id: 'appearance', label: 'Appearance' },
    { id: 'downloads', label: 'Downloads' },
    { id: 'library', label: 'Library' },
    { id: 'integrations', label: 'Integrations' },
    { id: 'storage', label: 'Storage' },
    { id: 'lyrics', label: 'Lyrics' },
    { id: 'api-keys', label: 'API Keys' },
  ];

  // Get section element by id (resolved at call time, not definition time)
  function getSectionEl(id: string): HTMLElement | undefined {
    switch (id) {
      case 'audio': return audioSection;
      case 'playback': return playbackSection;
      case 'offline': return offlineModeSection;
      case 'appearance': return appearanceSection;
      case 'downloads': return downloadsSection;
      case 'library': return librarySection;
      case 'integrations': return integrationsSection;
      case 'storage': return storageSection;
      case 'lyrics': return lyricsSection;
      case 'api-keys': return apiKeysSection;
      default: return undefined;
    }
  }

  function scrollToSection(id: string) {
    const el = getSectionEl(id);
    if (!el) return;
    activeSection = id;
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  // Audio device state - use PipeWire sinks directly for friendly names
  let pipewireSinks = $state<PipewireSink[]>([]);
  let hardwareStatus = $state<HardwareAudioStatus | null>(null);

  // Map of description -> sink name (for looking up sink name when user selects)
  const sinkDescriptionToName = $derived.by(() => {
    const map = new Map<string, string>();
    for (const sink of pipewireSinks) {
      map.set(sink.description, sink.name);
    }
    return map;
  });

  // Map of sink name -> description (for displaying current selection)
  const sinkNameToDescription = $derived.by(() => {
    const map = new Map<string, string>();
    for (const sink of pipewireSinks) {
      map.set(sink.name, sink.description);
    }
    return map;
  });

  // Options for dropdown - use PipeWire descriptions directly (already friendly names)
  let audioDeviceOptions = $derived(['System Default', ...pipewireSinks.map(s => s.description)]);

  // Theme mapping: display name -> data-theme value
  const themeMap: Record<string, string> = {
    'Dark': '',
    'Light': 'light',
    'OLED Black': 'oled',
    'Warm': 'warm'
  };

  const themeReverseMap: Record<string, string> = {
    '': 'Dark',
    'light': 'Light',
    'oled': 'OLED Black',
    'warm': 'Warm'
  };

  // Language mapping: display name -> locale code
  const languageToLocale: Record<string, string | null> = {
    'Auto': null,
    'English': 'en',
    'Español': 'es'
  };

  const localeToLanguage: Record<string, string> = {
    'en': 'English',
    'es': 'Español'
  };

  // Available languages (only those with translations)
  const availableLanguages = ['Auto', 'English', 'Español'];

  // Audio settings
  let streamingQuality = $state('Hi-Res');
  let preferHighest = $state(true);
  let outputDevice = $state('System Default');
  let exclusiveMode = $state(false);
  let dacPassthrough = $state(false);
  let selectedBackend = $state<string>('Auto');
  let selectedAlsaPlugin = $state<string>('Hw (Direct Hardware)');

  // Backend system state
  let availableBackends = $state<BackendInfo[]>([]);
  let backendDevices = $state<AudioDevice[]>([]);
  let alsaPlugins = $state<AlsaPluginInfo[]>([]);

  // Backend selector options (derived)
  let backendOptions = $derived(['Auto', ...availableBackends.filter(b => b.is_available).map(b => b.name)]);

  // Device options based on selected backend (derived)
  let deviceOptions = $derived(['System Default', ...backendDevices.map(d => d.name)]);

  // ALSA plugin options (derived)
  let alsaPluginOptions = $derived(alsaPlugins.map(p => p.name));

  // Show ALSA plugin selector only when ALSA backend is selected (derived)
  let showAlsaPluginSelector = $derived(selectedBackend === 'ALSA Direct');

  // Playback settings
  let gaplessPlayback = $state(true);
  let crossfade = $state(0);
  let normalizeVolume = $state(false);

  // UI scale settings
  const zoomOptions = ['80%', '90%', '100%', '110%', '125%', '150%', '175%', '200%'];
  const zoomMap: Record<string, number> = {
    '80%': 0.8,
    '90%': 0.9,
    '100%': 1,
    '110%': 1.1,
    '125%': 1.25,
    '150%': 1.5,
    '175%': 1.75,
    '200%': 2,
  };
  let zoomLevel = $state('100%');

  // Appearance settings
  let theme = $state('Dark');
  let toastsEnabled = $state(true);
  let systemNotificationsEnabled = $state(true);
  let language = $state('Auto');

  // Library settings
  let fetchQobuzArtistImages = $state(true);
  let showQobuzDownloadsInLibrary = $state(false);
  let downloadRoot = $state('');

  // Last.fm integration state
  let lastfmConnected = $state(false);
  let lastfmUsername = $state('');
  let scrobbling = $state(true);
  let lastfmApiKey = $state('');
  let lastfmApiSecret = $state('');
  let lastfmAuthToken = $state('');
  let lastfmConnecting = $state(false);
  let showLastfmConfig = $state(false);
  let hasEmbeddedCredentials = $state(false);

  // API Keys section state
  let apiKeysExpanded = $state(false);
  let spotifyClientId = $state('');
  let spotifyClientSecret = $state('');
  let tidalClientId = $state('');
  let tidalClientSecret = $state('');
  let discogsConsumerKey = $state('');
  let discogsConsumerSecret = $state('');
  let embeddedStatus = $state({ spotify: false, tidal: false, discogs: false, lastfm: false });
  let apiKeysSaving = $state(false);

  // Load saved settings on mount
  onMount(() => {
    // Load theme
    const savedTheme = localStorage.getItem('qbz-theme') || '';
    theme = themeReverseMap[savedTheme] || 'Dark';
    applyTheme(savedTheme);

    // Load streaming quality preference
    const savedQuality = localStorage.getItem('qbz-streaming-quality');
    if (savedQuality) {
      streamingQuality = savedQuality;
    }

    // Load prefer highest setting
    const savedPreferHighest = localStorage.getItem('qbz-prefer-highest');
    if (savedPreferHighest !== null) {
      preferHighest = savedPreferHighest === 'true';
    }

    // Load language setting from i18n locale
    const currentLocale = get(locale);
    if (currentLocale && localeToLanguage[currentLocale]) {
      language = localeToLanguage[currentLocale];
    } else {
      language = 'Auto';
    }

    // Load UI zoom level
    const savedZoom = localStorage.getItem('qbz-zoom-level');
    if (savedZoom) {
      const parsed = Number.parseFloat(savedZoom);
      const match = zoomOptions.find(option => Math.abs((zoomMap[option] ?? 1) - parsed) < 0.01);
      zoomLevel = match || '100%';
    }

    // Load library settings
    const savedFetchArtistImages = localStorage.getItem('qbz-fetch-artist-images');
    if (savedFetchArtistImages !== null) {
      fetchQobuzArtistImages = savedFetchArtistImages === 'true';
    }

    // Load download settings
    loadDownloadSettings();

    // Load cache stats
    loadCacheStats();

    // Load download cache stats
    loadDownloadStats();

    // Load audio devices first (includes PipeWire sinks), then settings
    // Also load backends and ALSA plugins
    Promise.all([
      loadAudioDevices(),
      loadBackends(),
      loadAlsaPlugins()
    ]).then(() => loadAudioSettings());

    // Load Last.fm state
    loadLastfmState();

    // Load API keys state
    loadApiKeysState();

    // Load notification preferences
    loadToastsPreference();
    toastsEnabled = getToastsEnabled();
    loadSystemNotificationsPreference();
    systemNotificationsEnabled = getSystemNotificationsEnabled();

    // Check for legacy downloads
    checkLegacyDownloads();

    // Subscribe to offline state changes
    const unsubscribeOffline = subscribeOffline(() => {
      offlineStatus = getOfflineStatus();
      offlineSettings = getOfflineSettings();
    });

    // Scroll tracking for navigation
    const handleScroll = () => {
      if (!settingsViewEl) return;
      const offset = 60; // Account for sticky nav height

      // Find which section is currently in view
      for (const def of navSectionDefs) {
        const el = getSectionEl(def.id);
        if (!el) continue;
        const rect = el.getBoundingClientRect();
        const containerRect = settingsViewEl.getBoundingClientRect();
        const relativeTop = rect.top - containerRect.top;

        if (relativeTop <= offset + 50 && relativeTop + rect.height > offset) {
          if (activeSection !== def.id) {
            activeSection = def.id;
          }
          break;
        }
      }
    };

    settingsViewEl?.addEventListener('scroll', handleScroll);

    return () => {
      unsubscribeOffline();
      settingsViewEl?.removeEventListener('scroll', handleScroll);
    };
  });

  async function loadLastfmState() {
    try {
      // Check if embedded (build-time) credentials are available
      hasEmbeddedCredentials = await invoke<boolean>('lastfm_has_embedded_credentials');

      // Load saved credentials from localStorage (for user-provided keys)
      const savedApiKey = localStorage.getItem('qbz-lastfm-api-key');
      const savedApiSecret = localStorage.getItem('qbz-lastfm-api-secret');
      const savedSessionKey = localStorage.getItem('qbz-lastfm-session-key');
      const savedUsername = localStorage.getItem('qbz-lastfm-username');
      const savedScrobbling = localStorage.getItem('qbz-lastfm-scrobbling');

      // If we have user-provided credentials, set them
      if (savedApiKey && savedApiSecret) {
        lastfmApiKey = savedApiKey;
        lastfmApiSecret = savedApiSecret;
        await invoke('lastfm_set_credentials', {
          apiKey: savedApiKey,
          apiSecret: savedApiSecret
        });
      }

      // Restore session if available
      if (savedSessionKey && savedUsername) {
        await invoke('lastfm_set_session', { sessionKey: savedSessionKey });
        lastfmConnected = true;
        lastfmUsername = savedUsername;
      }

      if (savedScrobbling !== null) {
        scrobbling = savedScrobbling === 'true';
      }
    } catch (err) {
      console.error('Failed to load Last.fm state:', err);
    }
  }

  async function handleLastfmConnect() {
    // If we don't have credentials (embedded or user-provided), show config
    const hasCredentials = hasEmbeddedCredentials || (lastfmApiKey && lastfmApiSecret);
    if (!hasCredentials) {
      showLastfmConfig = true;
      return;
    }

    lastfmConnecting = true;
    try {
      // If user provided credentials, save and set them
      if (lastfmApiKey && lastfmApiSecret) {
        localStorage.setItem('qbz-lastfm-api-key', lastfmApiKey);
        localStorage.setItem('qbz-lastfm-api-secret', lastfmApiSecret);
        await invoke('lastfm_set_credentials', {
          apiKey: lastfmApiKey,
          apiSecret: lastfmApiSecret
        });
      }

      // Get auth URL and token
      const [token, url] = await invoke<[string, string]>('lastfm_get_auth_url');
      lastfmAuthToken = token;

      // Open browser for authorization using Tauri's native opener
      try {
        await invoke('lastfm_open_auth_url', { url });
      } catch {
        // Fallback to window.open if native opener fails
        window.open(url, '_blank');
      }

      // Show the "I've Authorized" button
      showLastfmConfig = true;
    } catch (err) {
      console.error('Failed to start Last.fm auth:', err);
      alert(`Last.fm error: ${err}`);
    } finally {
      lastfmConnecting = false;
    }
  }

  async function handleLastfmCompleteAuth() {
    if (!lastfmAuthToken) {
      alert('Please start the authorization first');
      return;
    }

    lastfmConnecting = true;
    try {
      const session = await invoke<{ name: string; key: string }>('lastfm_authenticate', {
        token: lastfmAuthToken
      });

      lastfmConnected = true;
      lastfmUsername = session.name;
      showLastfmConfig = false;
      lastfmAuthToken = '';

      // Save session
      localStorage.setItem('qbz-lastfm-session-key', session.key);
      localStorage.setItem('qbz-lastfm-username', session.name);
    } catch (err) {
      console.error('Failed to complete Last.fm auth:', err);
      alert(`Authorization failed: ${err}`);
    } finally {
      lastfmConnecting = false;
    }
  }

  async function handleLastfmDisconnect() {
    try {
      await invoke('lastfm_disconnect');
      lastfmConnected = false;
      lastfmUsername = '';

      // Clear saved session
      localStorage.removeItem('qbz-lastfm-session-key');
      localStorage.removeItem('qbz-lastfm-username');
    } catch (err) {
      console.error('Failed to disconnect Last.fm:', err);
    }
  }

  function handleScrobblingChange(enabled: boolean) {
    scrobbling = enabled;
    localStorage.setItem('qbz-lastfm-scrobbling', String(enabled));
  }

  // API Keys management
  async function loadApiKeysState() {
    try {
      // Get embedded credentials status from backend
      embeddedStatus = await invoke<typeof embeddedStatus>('get_embedded_credentials_status');

      // Load user-provided credentials from localStorage
      spotifyClientId = localStorage.getItem('qbz-spotify-client-id') || '';
      spotifyClientSecret = localStorage.getItem('qbz-spotify-client-secret') || '';
      tidalClientId = localStorage.getItem('qbz-tidal-client-id') || '';
      tidalClientSecret = localStorage.getItem('qbz-tidal-client-secret') || '';
      discogsConsumerKey = localStorage.getItem('qbz-discogs-consumer-key') || '';
      discogsConsumerSecret = localStorage.getItem('qbz-discogs-consumer-secret') || '';

      // Restore credentials to backend state if we have saved ones
      if (spotifyClientId && spotifyClientSecret) {
        await invoke('set_spotify_credentials', {
          clientId: spotifyClientId,
          clientSecret: spotifyClientSecret
        });
      }
      if (tidalClientId && tidalClientSecret) {
        await invoke('set_tidal_credentials', {
          clientId: tidalClientId,
          clientSecret: tidalClientSecret
        });
      }
      if (discogsConsumerKey && discogsConsumerSecret) {
        await invoke('set_discogs_credentials', {
          consumerKey: discogsConsumerKey,
          consumerSecret: discogsConsumerSecret
        });
      }
    } catch (err) {
      console.error('Failed to load API keys state:', err);
    }
  }

  async function handleSaveSpotifyCredentials() {
    if (!spotifyClientId || !spotifyClientSecret) return;
    apiKeysSaving = true;
    try {
      await invoke('set_spotify_credentials', {
        clientId: spotifyClientId,
        clientSecret: spotifyClientSecret
      });
      localStorage.setItem('qbz-spotify-client-id', spotifyClientId);
      localStorage.setItem('qbz-spotify-client-secret', spotifyClientSecret);
    } catch (err) {
      console.error('Failed to save Spotify credentials:', err);
    } finally {
      apiKeysSaving = false;
    }
  }

  async function handleClearSpotifyCredentials() {
    apiKeysSaving = true;
    try {
      await invoke('clear_spotify_credentials');
      localStorage.removeItem('qbz-spotify-client-id');
      localStorage.removeItem('qbz-spotify-client-secret');
      spotifyClientId = '';
      spotifyClientSecret = '';
    } catch (err) {
      console.error('Failed to clear Spotify credentials:', err);
    } finally {
      apiKeysSaving = false;
    }
  }

  async function handleShowDownloadsChange(enabled: boolean) {
    try {
      await invoke('set_show_downloads_in_library', { show: enabled });
      showQobuzDownloadsInLibrary = enabled;
    } catch (e) {
      console.error('Failed to update show downloads setting:', e);
    }
  }

  async function handleSaveTidalCredentials() {
    if (!tidalClientId || !tidalClientSecret) return;
    apiKeysSaving = true;
    try {
      await invoke('set_tidal_credentials', {
        clientId: tidalClientId,
        clientSecret: tidalClientSecret
      });
      localStorage.setItem('qbz-tidal-client-id', tidalClientId);
      localStorage.setItem('qbz-tidal-client-secret', tidalClientSecret);
    } catch (err) {
      console.error('Failed to save Tidal credentials:', err);
    } finally {
      apiKeysSaving = false;
    }
  }

  async function handleClearTidalCredentials() {
    apiKeysSaving = true;
    try {
      await invoke('clear_tidal_credentials');
      localStorage.removeItem('qbz-tidal-client-id');
      localStorage.removeItem('qbz-tidal-client-secret');
      tidalClientId = '';
      tidalClientSecret = '';
    } catch (err) {
      console.error('Failed to clear Tidal credentials:', err);
    } finally {
      apiKeysSaving = false;
    }
  }

  async function handleSaveDiscogsCredentials() {
    if (!discogsConsumerKey || !discogsConsumerSecret) return;
    apiKeysSaving = true;
    try {
      await invoke('set_discogs_credentials', {
        consumerKey: discogsConsumerKey,
        consumerSecret: discogsConsumerSecret
      });
      localStorage.setItem('qbz-discogs-consumer-key', discogsConsumerKey);
      localStorage.setItem('qbz-discogs-consumer-secret', discogsConsumerSecret);
    } catch (err) {
      console.error('Failed to save Discogs credentials:', err);
    } finally {
      apiKeysSaving = false;
    }
  }

  async function handleClearDiscogsCredentials() {
    apiKeysSaving = true;
    try {
      await invoke('clear_discogs_credentials');
      localStorage.removeItem('qbz-discogs-consumer-key');
      localStorage.removeItem('qbz-discogs-consumer-secret');
      discogsConsumerKey = '';
      discogsConsumerSecret = '';
    } catch (err) {
      console.error('Failed to clear Discogs credentials:', err);
    } finally {
      apiKeysSaving = false;
    }
  }

  // Check if any user credentials are configured
  function hasAnyUserCredentials(): boolean {
    return !!(
      (spotifyClientId && spotifyClientSecret) ||
      (tidalClientId && tidalClientSecret) ||
      (discogsConsumerKey && discogsConsumerSecret)
    );
  }

  function handleQualityChange(quality: string) {
    streamingQuality = quality;
    localStorage.setItem('qbz-streaming-quality', quality);
  }

  function handlePreferHighestChange(enabled: boolean) {
    preferHighest = enabled;
    localStorage.setItem('qbz-prefer-highest', String(enabled));
  }

  // Offline mode handlers
  async function handleManualOfflineChange(enabled: boolean) {
    // If enabling offline mode, just do it directly
    if (enabled) {
      try {
        await setManualOffline(true);
      } catch (error) {
        console.error('Failed to enable manual offline mode:', error);
      }
      return;
    }

    // If disabling offline mode, verify network connectivity first
    isCheckingNetwork = true;
    try {
      const hasNetwork = await checkNetwork();
      if (hasNetwork) {
        await setManualOffline(false);
      } else {
        showToast($t('offline.noNetworkToast'), 'error');
      }
    } catch (error) {
      console.error('Failed to disable manual offline mode:', error);
      showToast($t('offline.noNetworkToast'), 'error');
    } finally {
      isCheckingNetwork = false;
    }
  }

  async function handleShowPartialPlaylistsChange(enabled: boolean) {
    try {
      await setShowPartialPlaylists(enabled);
    } catch (error) {
      console.error('Failed to set show partial playlists:', error);
    }
  }

  async function handleAllowCastChange(enabled: boolean) {
    try {
      await setAllowCastWhileOffline(enabled);
    } catch (error) {
      console.error('Failed to set allow cast while offline:', error);
    }
  }

  async function handleAllowImmediateScrobblingChange(enabled: boolean) {
    try {
      await setAllowImmediateScrobbling(enabled);
      // Mutually exclusive: if turning on, turn off the other
      if (enabled && offlineSettings.allowAccumulatedScrobbling) {
        await setAllowAccumulatedScrobbling(false);
      }
    } catch (error) {
      console.error('Failed to set allow immediate scrobbling:', error);
    }
  }

  async function handleAllowAccumulatedScrobblingChange(enabled: boolean) {
    try {
      await setAllowAccumulatedScrobbling(enabled);
      // Mutually exclusive: if turning on, turn off the other
      if (enabled && offlineSettings.allowImmediateScrobbling) {
        await setAllowImmediateScrobbling(false);
      }
    } catch (error) {
      console.error('Failed to set allow accumulated scrobbling:', error);
    }
  }

  async function handleShowNetworkFoldersChange(enabled: boolean) {
    try {
      await setShowNetworkFoldersInManualOffline(enabled);
    } catch (error) {
      console.error('Failed to set show network folders in manual offline:', error);
    }
  }

  async function handleLanguageChange(lang: string) {
    language = lang;
    const localeCode = languageToLocale[lang];
    if (localeCode) {
      // Set specific locale
      await setLocale(localeCode);
      // Clear artist cache to force refetch in new language
      try {
        await invoke('clear_artist_cache');
        console.log('Artist cache cleared after language change');
      } catch (error) {
        console.error('Failed to clear artist cache:', error);
      }
    } else {
      // 'Auto' - use browser locale, defaulting to 'en'
      const browserLocale = navigator.language.split('-')[0];
      const supportedLocale = ['en', 'es'].includes(browserLocale) ? browserLocale : 'en';
      await setLocale(supportedLocale);
      // Clear the stored locale so it uses browser detection on next load
      localStorage.removeItem('qbz-locale');
      // Also clear artist cache
      try {
        await invoke('clear_artist_cache');
        console.log('Artist cache cleared after language change');
      } catch (error) {
        console.error('Failed to clear artist cache:', error);
      }
    }
  }

  interface AudioSettings {
    output_device: string | null;
    exclusive_mode: boolean;
    dac_passthrough: boolean;
    preferred_sample_rate: number | null;
    backend_type: 'PipeWire' | 'Alsa' | 'Pulse' | null;
    alsa_plugin: 'Hw' | 'PlugHw' | 'Pcm' | null;
  }

  interface BackendInfo {
    backend_type: 'PipeWire' | 'Alsa' | 'Pulse';
    name: string;
    description: string;
    is_available: boolean;
  }

  interface AudioDevice {
    id: string;
    name: string;
    description: string | null;
    is_default: boolean;
    max_sample_rate: number | null;
  }

  interface AlsaPluginInfo {
    plugin: 'Hw' | 'PlugHw' | 'Pcm';
    name: string;
    description: string;
  }

  // Helper to get the current selected device sink name (or null for system default)
  function getCurrentDeviceSinkName(): string | null {
    if (outputDevice === 'System Default') {
      return null;
    }
    return sinkDescriptionToName.get(outputDevice) ?? null;
  }

  async function loadAudioDevices() {
    try {
      // Load PipeWire sinks - these have friendly descriptions already
      const sinks = await invoke<PipewireSink[]>('get_pipewire_sinks').catch(() => [] as PipewireSink[]);
      pipewireSinks = sinks;

      // Load hardware audio status
      const hwStatus = await invoke<HardwareAudioStatus>('get_hardware_audio_status').catch(() => null);
      hardwareStatus = hwStatus;

      console.log('[Audio] PipeWire sinks:', sinks.map(s => ({ name: s.name, desc: s.description })));
      console.log('[Audio] Hardware status:', hwStatus);
    } catch (err) {
      console.error('Failed to load audio devices:', err);
    }
  }

  async function loadBackends() {
    try {
      const backends = await invoke<BackendInfo[]>('get_available_backends');
      availableBackends = backends;
      console.log('[Audio] Available backends:', backends);
    } catch (err) {
      console.error('Failed to load backends:', err);
    }
  }

  async function loadAlsaPlugins() {
    try {
      const plugins = await invoke<AlsaPluginInfo[]>('get_alsa_plugins');
      alsaPlugins = plugins;
      console.log('[Audio] ALSA plugins:', plugins);
    } catch (err) {
      console.error('Failed to load ALSA plugins:', err);
    }
  }

  async function loadBackendDevices(backendType: 'PipeWire' | 'Alsa' | 'Pulse') {
    try {
      const devices = await invoke<AudioDevice[]>('get_devices_for_backend', { backendType });
      backendDevices = devices;
      console.log(`[Audio] Devices for ${backendType}:`, devices);
    } catch (err) {
      console.error(`Failed to load devices for ${backendType}:`, err);
      backendDevices = [];
    }
  }

  async function loadAudioSettings() {
    try {
      const settings = await invoke<AudioSettings>('get_audio_settings');
      // Convert stored device name to description for display
      if (settings.output_device) {
        // Look up the friendly description from the device name
        const description = sinkNameToDescription.get(settings.output_device);
        outputDevice = description ?? settings.output_device;
      } else {
        outputDevice = 'System Default';
      }
      exclusiveMode = settings.exclusive_mode;
      dacPassthrough = settings.dac_passthrough;

      // Load backend and plugin settings
      if (settings.backend_type) {
        const backend = availableBackends.find(b => b.backend_type === settings.backend_type);
        selectedBackend = backend?.name ?? 'Auto';

        // Load devices for selected backend
        if (settings.backend_type) {
          await loadBackendDevices(settings.backend_type);

          // Set selected device from backend devices
          if (settings.output_device) {
            const device = backendDevices.find(d => d.id === settings.output_device);
            outputDevice = device?.name ?? 'System Default';
          }
        }
      } else {
        selectedBackend = 'Auto';
      }

      if (settings.alsa_plugin) {
        const plugin = alsaPlugins.find(p => p.plugin === settings.alsa_plugin);
        selectedAlsaPlugin = plugin?.name ?? 'Hw (Direct Hardware)';
      } else {
        selectedAlsaPlugin = 'Hw (Direct Hardware)';
      }

      // Validate mutual exclusion: DAC Passthrough disables Gapless + Crossfade
      if (dacPassthrough) {
        if (gaplessPlayback) {
          console.warn('DAC Passthrough and Gapless both enabled - disabling Gapless');
          gaplessPlayback = false;
        }
        if (crossfade > 0) {
          console.warn('DAC Passthrough and Crossfade both enabled - disabling Crossfade');
          crossfade = 0;
        }
      }
    } catch (err) {
      console.error('Failed to load audio settings:', err);
    }
  }

  async function handleOutputDeviceChange(description: string) {
    outputDevice = description;

    // Convert description back to device name for storage
    const deviceName = sinkDescriptionToName.get(description);
    const deviceToStore = description === 'System Default' ? null : deviceName;

    try {
      // Save the preference
      await invoke('set_audio_output_device', { device: deviceToStore });

      // Reinitialize audio with the selected device
      // CRITICAL: Pass the actual CPAL device name, not null
      // CPAL can now find this device because we're using CPAL names
      await invoke('reinit_audio_device', { device: deviceToStore });

      console.log('[Audio] Output device changed:', description, '(device:', deviceName ?? 'default', ')');
    } catch (err) {
      console.error('[Audio] Failed to change audio output device:', err);
    }
  }

  async function handleExclusiveModeChange(enabled: boolean) {
    exclusiveMode = enabled;
    try {
      await invoke('set_audio_exclusive_mode', { enabled });

      // Reinitialize audio with currently selected device
      const deviceName = getCurrentDeviceSinkName();
      await invoke('reinit_audio_device', { device: deviceName });
      console.log('[Audio] Exclusive mode changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change exclusive mode:', err);
    }
  }

  async function handleDacPassthroughChange(enabled: boolean) {
    dacPassthrough = enabled;

    // Auto-disable incompatible playback settings
    if (enabled) {
      gaplessPlayback = false;
      crossfade = 0;
      console.log('[Audio] DAC passthrough enabled: disabled gapless playback and crossfade');
    }

    try {
      await invoke('set_audio_dac_passthrough', { enabled });

      // Reinitialize audio with currently selected device
      const deviceName = getCurrentDeviceSinkName();
      await invoke('reinit_audio_device', { device: deviceName });
      console.log('[Audio] DAC passthrough changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change DAC passthrough:', err);
    }
  }

  async function handleBackendChange(backendName: string) {
    selectedBackend = backendName;

    // Map UI name to backend type
    const backend = availableBackends.find(b => b.name === backendName);
    const backendType = backendName === 'Auto' ? null : backend?.backend_type ?? null;

    try {
      // Save backend preference
      await invoke('set_audio_backend_type', { backendType });
      console.log('[Audio] Backend changed:', backendName, '(type:', backendType ?? 'auto', ')');

      // Load devices for new backend
      if (backendType) {
        await loadBackendDevices(backendType);
        // Reset to default device when switching backends
        outputDevice = 'System Default';
        await invoke('set_audio_output_device', { device: null });
      }

      // Reinitialize audio
      await invoke('reinit_audio_device', { device: null });
    } catch (err) {
      console.error('[Audio] Failed to change backend:', err);
    }
  }

  async function handleAlsaPluginChange(pluginName: string) {
    selectedAlsaPlugin = pluginName;

    // Map UI name to plugin type
    const pluginInfo = alsaPlugins.find(p => p.name === pluginName);
    const plugin = pluginInfo?.plugin ?? null;

    try {
      await invoke('set_audio_alsa_plugin', { plugin });
      console.log('[Audio] ALSA plugin changed:', pluginName, '(type:', plugin ?? 'none', ')');

      // Reinitialize audio if ALSA backend is active
      if (selectedBackend === 'ALSA Direct') {
        const deviceName = getCurrentDeviceSinkName();
        await invoke('reinit_audio_device', { device: deviceName });
      }
    } catch (err) {
      console.error('[Audio] Failed to change ALSA plugin:', err);
    }
  }

  async function handleBackendDeviceChange(deviceName: string) {
    outputDevice = deviceName;

    // Get device ID from backendDevices
    const device = backendDevices.find(d => d.name === deviceName);
    const deviceId = deviceName === 'System Default' ? null : device?.id ?? null;

    try {
      await invoke('set_audio_output_device', { device: deviceId });
      await invoke('reinit_audio_device', { device: deviceId });
      console.log('[Audio] Backend device changed:', deviceName, '(id:', deviceId ?? 'default', ')');
    } catch (err) {
      console.error('[Audio] Failed to change backend device:', err);
    }
  }

  async function handleGaplessPlaybackChange(enabled: boolean) {
    gaplessPlayback = enabled;

    // Auto-disable DAC passthrough if incompatible
    if (enabled && dacPassthrough) {
      dacPassthrough = false;
      console.log('[Audio] Gapless playback enabled: disabled DAC passthrough');
      try {
        await invoke('set_audio_dac_passthrough', { enabled: false });

        // Reinitialize audio with currently selected device
        const deviceName = getCurrentDeviceSinkName();
        await invoke('reinit_audio_device', { device: deviceName });
      } catch (err) {
        console.error('[Audio] Failed to disable DAC passthrough:', err);
      }
    }
  }

  async function handleCrossfadeChange(value: number) {
    crossfade = value;

    // Auto-disable DAC passthrough if crossfade > 0
    if (value > 0 && dacPassthrough) {
      dacPassthrough = false;
      console.log('[Audio] Crossfade enabled: disabled DAC passthrough');
      try {
        await invoke('set_audio_dac_passthrough', { enabled: false });

        // Reinitialize audio with currently selected device
        const deviceName = getCurrentDeviceSinkName();
        await invoke('reinit_audio_device', { device: deviceName });
      } catch (err) {
        console.error('[Audio] Failed to disable DAC passthrough:', err);
      }
    }
  }

  async function loadCacheStats() {
    try {
      cacheStats = await invoke<CacheStats>('get_cache_stats');
    } catch (err) {
      console.error('Failed to load cache stats:', err);
    }
  }

  async function loadDownloadStats() {
    try {
      downloadStats = await getDownloadCacheStats();
    } catch (err) {
      console.error('Failed to load download stats:', err);
    }
  }

  async function loadDownloadSettings() {
    try {
      const settings = await invoke<{download_root: string, show_in_library: boolean}>('get_download_settings');
      showQobuzDownloadsInLibrary = settings.show_in_library;
      downloadRoot = settings.download_root;
    } catch (err) {
      console.error('Failed to load download settings:', err);
    }
  }

  async function checkLegacyDownloads() {
    try {
      const result = await invoke<{has_legacy_files: boolean, total_tracks: number}>('detect_legacy_downloads');
      if (result.has_legacy_files && result.total_tracks > 0) {
        legacyTracksCount = result.total_tracks;
        showMigrationModal = true;
      }
    } catch (err) {
      console.error('Failed to check for legacy downloads:', err);
    }
  }

  function closeMigrationModal() {
    showMigrationModal = false;
    // Refresh stats after migration
    loadDownloadStats();
  }

  async function handleChangeDownloadFolder() {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');

      const result = await open({
        title: 'Select Downloads Folder',
        directory: true,
        defaultPath: downloadRoot || undefined
      });

      if (result) {
        // Validate the path
        const valid = await invoke<boolean>('validate_download_path', { path: result });
        if (!valid) {
          alert('Invalid path or insufficient permissions. Please select a different folder.');
          return;
        }

        // Check if this is a network folder and warn user
        interface NetworkPathInfo {
          isNetwork: boolean;
          mountInfo: { fsType: string; kind: string } | null;
          path: string;
        }
        const networkInfo = await invoke<NetworkPathInfo>('check_network_path', { path: result });
        if (networkInfo.isNetwork) {
          const proceed = confirm(
            $t('settings.networkDownloadWarning') + '\n\n' +
            $t('settings.networkDownloadWarningDetail')
          );
          if (!proceed) {
            return;
          }
        }

        // Ask if user wants to move existing downloads
        const hasDownloads = downloadStats && downloadStats.readyTracks > 0;
        let moveFiles = false;
        
        if (hasDownloads) {
          moveFiles = confirm(
            `You have ${downloadStats!.readyTracks} downloaded tracks.\n\n` +
            'Would you like to move them to the new location?\n\n' +
            'Click OK to move files, or Cancel to keep them in the old location.'
          );
        }

        // Update download root
        const moveResult = await invoke<{moved: number, failed: string[]}>('set_download_root', { 
          path: result,
          moveExisting: moveFiles 
        });
        
        downloadRoot = result;
        
        if (moveFiles && moveResult.moved > 0) {
          alert(`Successfully moved ${moveResult.moved} tracks to the new location.`);
        }
        
        if (moveResult.failed.length > 0) {
          console.error('Failed to move files:', moveResult.failed);
        }

        // Reload download stats
        await loadDownloadStats();
      }
    } catch (err) {
      console.error('Failed to change download folder:', err);
      alert(`Failed to change download folder: ${err}`);
    }
  }

  async function handleClearDownloads() {
    if (isClearingDownloads) return;
    isClearingDownloads = true;
    try {
      await clearDownloadCache();
      await loadDownloadStats();
    } catch (err) {
      console.error('Failed to clear download cache:', err);
    } finally {
      isClearingDownloads = false;
    }
  }

  async function handleOpenDownloadFolder() {
    try {
      await openDownloadCacheFolder();
    } catch (err) {
      console.error('Failed to open download folder:', err);
    }
  }

  async function handleClearCache() {
    if (isClearing) return;
    isClearing = true;
    try {
      await invoke('clear_cache');
      await loadCacheStats();
    } catch (err) {
      console.error('Failed to clear cache:', err);
    } finally {
      isClearing = false;
    }
  }

  async function handleClearLyricsCache() {
    if (isClearingLyrics) return;
    isClearingLyrics = true;
    try {
      await clearLyricsCache();
      console.log('Lyrics cache cleared');
    } catch (err) {
      console.error('Failed to clear lyrics cache:', err);
    } finally {
      isClearingLyrics = false;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
  }

  function applyTheme(themeValue: string) {
    if (themeValue) {
      document.documentElement.setAttribute('data-theme', themeValue);
    } else {
      document.documentElement.removeAttribute('data-theme');
    }
  }

  function handleThemeChange(newTheme: string) {
    theme = newTheme;
    const themeValue = themeMap[newTheme] || '';
    applyTheme(themeValue);
    localStorage.setItem('qbz-theme', themeValue);
  }

  async function handleZoomChange(value: string) {
    zoomLevel = value;
    const zoom = zoomMap[value] ?? 1;
    localStorage.setItem('qbz-zoom-level', String(zoom));
    try {
      await getCurrentWebview().setZoom(zoom);
    } catch (err) {
      console.warn('Failed to set zoom:', err);
    }
  }
</script>

<div class="settings-view" bind:this={settingsViewEl}>
  <!-- Header -->
  <div class="header">
    {#if onBack}
      <button class="back-btn" onclick={onBack}>
        <ArrowLeft size={16} />
        <span>{$t('actions.back')}</span>
      </button>
    {/if}
    <h1 class="title">{$t('settings.title')}</h1>
  </div>

  <!-- Account Section -->
  <section class="section">
    <h3 class="section-title">{$t('settings.account.title')}</h3>
    <div class="account-card">
      <div class="avatar">{userName.charAt(0).toUpperCase()}</div>
      <div class="account-info">
        <div class="username">{userName}</div>
        {#if userEmail}
          <div class="email">{userEmail}</div>
        {/if}
        <div class="subscription">{subscription}</div>
      </div>
      <button class="logout-btn" onclick={onLogout}>{$t('settings.account.logout')}</button>
    </div>
  </section>

  <!-- Settings Navigation -->
  <nav class="settings-nav">
    {#each navSectionDefs as section}
      <button
        class="nav-link"
        class:active={activeSection === section.id}
        onclick={() => scrollToSection(section.id)}
      >
        {section.label}
      </button>
    {/each}
  </nav>

  <!-- Audio Section -->
  <section class="section" bind:this={audioSection}>
    <h3 class="section-title">{$t('settings.audio.title')}</h3>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.audio.streamingQuality')}</span>
      <Dropdown
        value={streamingQuality}
        options={['MP3', 'CD Quality', 'Hi-Res', 'Hi-Res+']}
        onchange={handleQualityChange}
      />
    </div>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.audio.preferHighest')}</span>
      <Toggle enabled={preferHighest} onchange={handlePreferHighestChange} />
    </div>
    <div class="setting-row">
      <div class="label-with-tooltip">
        <span class="setting-label">Audio Backend</span>
        <Tooltip text="Choose audio system: Auto (recommended), PipeWire (modern), ALSA Direct (bit-perfect, exclusive), or PulseAudio (legacy)" />
      </div>
      <Dropdown
        value={selectedBackend}
        options={backendOptions}
        onchange={handleBackendChange}
        wide
        expandLeft
        compact
      />
    </div>
    <div class="setting-row">
      <div class="label-with-tooltip">
        <span class="setting-label">{$t('settings.audio.outputDevice')}</span>
        <Tooltip text="Select your preferred audio output device. Devices shown are from the selected backend." />
      </div>
      <Dropdown
        value={outputDevice}
        options={selectedBackend === 'Auto' ? (audioDeviceOptions.length > 1 ? audioDeviceOptions : ['System Default']) : deviceOptions}
        onchange={selectedBackend === 'Auto' ? handleOutputDeviceChange : handleBackendDeviceChange}
        wide
        expandLeft
        compact
      />
    </div>
    {#if showAlsaPluginSelector}
    <div class="setting-row">
      <div class="label-with-tooltip">
        <span class="setting-label">ALSA Plugin</span>
        <Tooltip text="hw: Bit-perfect, exclusive. plughw: Auto-convert. pcm: Most compatible." />
      </div>
      <Dropdown
        value={selectedAlsaPlugin}
        options={alsaPluginOptions}
        onchange={handleAlsaPluginChange}
        wide
        expandLeft
        compact
      />
    </div>
    {/if}
    <div class="setting-row">
      <div class="label-with-tooltip">
        <span class="setting-label">{$t('settings.audio.exclusiveMode')}</span>
        <Tooltip text={$t('settings.audio.exclusiveModeDesc')} />
      </div>
      <Toggle enabled={exclusiveMode} onchange={handleExclusiveModeChange} />
    </div>
    <div class="setting-row">
      <div class="label-with-tooltip">
        <span class="setting-label">{$t('settings.audio.dacPassthrough')}</span>
        <Tooltip text={$t('settings.audio.dacPassthroughDesc')} />
      </div>
      <Toggle enabled={dacPassthrough} onchange={handleDacPassthroughChange} />
    </div>
    <div class="setting-row last">
      <span class="setting-label">{$t('settings.audio.currentSampleRate')}</span>
      <span class="setting-value" class:muted={!hardwareStatus?.is_active}>
        {#if hardwareStatus?.is_active && hardwareStatus.hardware_sample_rate}
          {(hardwareStatus.hardware_sample_rate / 1000).toFixed(1)} kHz
          {#if hardwareStatus.hardware_format}
            <span class="format-detail">({hardwareStatus.hardware_format})</span>
          {/if}
        {:else}
          {$t('settings.audio.noActivePlayback')}
        {/if}
      </span>
    </div>
  </section>

  <!-- Playback Section -->
  <section class="section" bind:this={playbackSection}>
    <h3 class="section-title">{$t('settings.playback.title')}</h3>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.playback.gapless')}</span>
      <Toggle enabled={gaplessPlayback} onchange={handleGaplessPlaybackChange} />
    </div>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.playback.crossfade')}</span>
      <div class="slider-container">
        <VolumeSlider value={crossfade} onchange={handleCrossfadeChange} max={12} showValue />
      </div>
    </div>
    <div class="setting-row last">
      <span class="setting-label">{$t('settings.playback.normalizeVolume')}</span>
      <Toggle enabled={normalizeVolume} onchange={(v) => (normalizeVolume = v)} />
    </div>
  </section>

  <!-- Offline Mode Section -->
  <section class="section" bind:this={offlineModeSection}>
    <h3 class="section-title">{$t('offline.title')}</h3>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('offline.status')}</span>
        <span class="setting-desc status-indicator" class:offline={offlineStatus.isOffline}>
          {#if offlineStatus.isOffline}
            {#if offlineStatus.reason === 'no_network'}
              {$t('offline.noNetwork')}
            {:else if offlineStatus.reason === 'not_logged_in'}
              {$t('offline.notLoggedIn')}
            {:else if offlineStatus.reason === 'manual_override'}
              {$t('offline.manualMode')}
            {:else}
              {$t('offline.offlineReason')}
            {/if}
          {:else}
            {$t('offline.online')}
          {/if}
        </span>
      </div>
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('offline.enableManual')}</span>
        <span class="setting-desc">{$t('offline.enableManualDesc')}</span>
      </div>
      <Toggle enabled={offlineSettings.manualOfflineMode} onchange={handleManualOfflineChange} />
    </div>
    <div class="setting-row" class:last={!offlineSettings.manualOfflineMode}>
      <div class="setting-info">
        <span class="setting-label">{$t('offline.showPartialPlaylists')}</span>
        <span class="setting-desc">{$t('offline.showPartialPlaylistsDesc')}</span>
      </div>
      <Toggle enabled={offlineSettings.showPartialPlaylists} onchange={handleShowPartialPlaylistsChange} />
    </div>

    <!-- Manual offline mode specific settings -->
    {#if offlineSettings.manualOfflineMode}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{$t('offline.allowCast')}</span>
          <span class="setting-desc">{$t('offline.allowCastDesc')}</span>
        </div>
        <Toggle enabled={offlineSettings.allowCastWhileOffline} onchange={handleAllowCastChange} />
      </div>
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{$t('offline.allowImmediateScrobbling')}</span>
          <span class="setting-desc">{$t('offline.allowImmediateScrobblingDesc')}</span>
        </div>
        <Toggle enabled={offlineSettings.allowImmediateScrobbling} onchange={handleAllowImmediateScrobblingChange} />
      </div>
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{$t('offline.allowAccumulatedScrobbling')}</span>
          <span class="setting-desc">{$t('offline.allowAccumulatedScrobblingDesc')}</span>
          <small class="setting-note">{$t('offline.scrobbleTimeLimit')}</small>
        </div>
        <Toggle enabled={offlineSettings.allowAccumulatedScrobbling} onchange={handleAllowAccumulatedScrobblingChange} />
      </div>
      <div class="setting-row last">
        <div class="setting-info">
          <span class="setting-label">{$t('offline.showNetworkFolders')}</span>
          <span class="setting-desc">{$t('offline.showNetworkFoldersDesc')}</span>
        </div>
        <Toggle enabled={offlineSettings.showNetworkFoldersInManualOffline} onchange={handleShowNetworkFoldersChange} />
      </div>
    {/if}
  </section>

  <!-- Appearance Section -->
  <section class="section" bind:this={appearanceSection}>
    <h3 class="section-title">{$t('settings.appearance.title')}</h3>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.appearance.theme')}</span>
      <Dropdown
        value={theme}
        options={['Dark', 'Light', 'OLED Black', 'Warm']}
        onchange={handleThemeChange}
      />
    </div>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.appearance.language')}</span>
      <Dropdown
        value={language}
        options={availableLanguages}
        onchange={handleLanguageChange}
      />
    </div>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.appearance.uiScale')}</span>
      <Dropdown
        value={zoomLevel}
        options={zoomOptions}
        onchange={handleZoomChange}
      />
    </div>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.appearance.inAppToasts')}</span>
      <Toggle enabled={toastsEnabled} onchange={(v) => { toastsEnabled = v; setToastsEnabled(v); }} />
    </div>
    <div class="setting-row last">
      <span class="setting-label">{$t('settings.appearance.systemNotifications')}</span>
      <Toggle enabled={systemNotificationsEnabled} onchange={(v) => { systemNotificationsEnabled = v; setSystemNotificationsEnabled(v); }} />
    </div>
  </section>

  <!-- Downloads Section -->
  <section class="section" bind:this={downloadsSection}>
    <h3 class="section-title">Downloads</h3>
    <div class="setting-row">
      <div class="setting-with-description">
        <span class="setting-label">Download Folder</span>
        <span class="setting-description">{downloadRoot || 'Default location'}</span>
      </div>
      <button class="secondary-btn" onclick={handleChangeDownloadFolder}>
        <FolderOpen size={14} />
        <span>Change</span>
      </button>
    </div>
    <div class="setting-row">
      <span class="setting-label">Downloaded Tracks</span>
      <span class="setting-value">
        {#if downloadStats}
          {downloadStats.readyTracks} tracks ({formatBytes(downloadStats.totalSizeBytes)})
        {:else}
          Loading...
        {/if}
      </span>
    </div>
    <div class="setting-row">
      <div class="setting-with-description">
        <span class="setting-label">Show in Local Library</span>
        <span class="setting-description">Display downloaded Qobuz tracks in your Local Library</span>
      </div>
      <Toggle enabled={showQobuzDownloadsInLibrary} onchange={handleShowDownloadsChange} />
    </div>
    <div class="setting-row">
      <span class="setting-label">Clear Downloads</span>
      <button
        class="clear-btn"
        onclick={handleClearDownloads}
        disabled={isClearingDownloads || !downloadStats || downloadStats.readyTracks === 0}
      >
        {isClearingDownloads ? 'Clearing...' : 'Clear All'}
      </button>
    </div>
    <div class="setting-row last">
      <span class="setting-label">Open Folder</span>
      <button
        class="folder-btn"
        onclick={handleOpenDownloadFolder}
        title="Open download folder"
      >
        <FolderOpen size={16} />
        <span>Open</span>
      </button>
    </div>
  </section>

  <!-- Library Section -->
  <section class="section" bind:this={librarySection}>
    <h3 class="section-title">{$t('settings.library.title')}</h3>
    <div class="setting-row last">
      <div class="setting-with-description">
        <span class="setting-label">{$t('settings.library.fetchArtistImages')}</span>
        <span class="setting-description">{$t('settings.library.fetchArtistImagesDesc')}</span>
      </div>
      <Toggle enabled={fetchQobuzArtistImages} onchange={(v) => {
        fetchQobuzArtistImages = v;
        localStorage.setItem('qbz-fetch-artist-images', String(v));
      }} />
    </div>
  </section>

  <!-- Integrations Section -->
  <section class="section" bind:this={integrationsSection}>
    <h3 class="section-title">{$t('settings.integrations.title')}</h3>

    {#if lastfmConnected}
      <div class="setting-row">
        <div class="lastfm-connected">
          <span class="setting-label">{$t('settings.integrations.lastfm')}</span>
          <span class="lastfm-username">{$t('settings.integrations.connectedAs', { values: { username: lastfmUsername }})}</span>
        </div>
        <button
          class="connect-btn connected"
          onclick={handleLastfmDisconnect}
        >
          {$t('settings.integrations.disconnect')}
        </button>
      </div>
      <div class="setting-row last">
        <span class="setting-label">{$t('settings.integrations.scrobbling')}</span>
        <Toggle enabled={scrobbling} onchange={handleScrobblingChange} />
      </div>
    {:else}
      <div class="setting-row" class:last={!showLastfmConfig && !lastfmAuthToken}>
        <span class="setting-label">{$t('settings.integrations.lastfm')}</span>
        <button
          class="connect-btn"
          onclick={handleLastfmConnect}
          disabled={lastfmConnecting}
        >
          {lastfmConnecting ? 'Connecting...' : $t('settings.integrations.connect')}
        </button>
      </div>

      {#if lastfmAuthToken}
        <!-- Waiting for user to authorize in browser -->
        <div class="lastfm-config">
          <p class="auth-info">
            A browser window has been opened. Please authorize QBZ on Last.fm, then click the button below.
          </p>
          <button
            class="auth-complete-btn"
            onclick={handleLastfmCompleteAuth}
            disabled={lastfmConnecting}
          >
            {lastfmConnecting ? 'Verifying...' : 'I\'ve Authorized'}
          </button>
          <button
            class="auth-cancel-btn"
            onclick={() => { lastfmAuthToken = ''; showLastfmConfig = false; }}
          >
            Cancel
          </button>
        </div>
      {:else if showLastfmConfig && !hasEmbeddedCredentials}
        <!-- No embedded credentials, user needs to provide their own -->
        <div class="lastfm-config">
          <p class="config-info">
            QBZ needs Last.fm API credentials to enable scrobbling.
            <a href="https://www.last.fm/api/account/create" target="_blank" rel="noopener">
              Create an API account
            </a> and enter your credentials below.
          </p>
          <div class="config-field">
            <label for="lastfm-api-key">API Key</label>
            <input
              id="lastfm-api-key"
              type="text"
              bind:value={lastfmApiKey}
              placeholder="Enter your API key"
            />
          </div>
          <div class="config-field">
            <label for="lastfm-api-secret">Shared Secret</label>
            <input
              id="lastfm-api-secret"
              type="password"
              bind:value={lastfmApiSecret}
              placeholder="Enter your shared secret"
            />
          </div>
          <button
            class="auth-start-btn"
            onclick={handleLastfmConnect}
            disabled={!lastfmApiKey || !lastfmApiSecret || lastfmConnecting}
          >
            {lastfmConnecting ? 'Opening...' : 'Authorize with Last.fm'}
          </button>
        </div>
      {/if}
    {/if}
  </section>

  <!-- Storage Section (Memory Cache) -->
  <section class="section" bind:this={storageSection}>
    <h3 class="section-title">{$t('settings.storage.title')}</h3>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.storage.cacheSize')}</span>
      <span class="setting-value">
        {#if cacheStats}
          {formatBytes(cacheStats.current_size_bytes)} / {formatBytes(cacheStats.max_size_bytes)}
        {:else}
          {$t('actions.loading')}
        {/if}
      </span>
    </div>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.storage.cachedTracks')}</span>
      <span class="setting-value">
        {#if cacheStats}
          {cacheStats.cached_tracks} {$t('album.tracks')}
        {:else}
          -
        {/if}
      </span>
    </div>
    <div class="setting-row last">
      <span class="setting-label">{$t('settings.storage.clearCache')}</span>
      <button
        class="clear-btn"
        onclick={handleClearCache}
        disabled={isClearing || !cacheStats || cacheStats.current_size_bytes === 0}
      >
        {isClearing ? $t('settings.storage.clearing') : $t('actions.clear')}
      </button>
    </div>
  </section>

  <!-- Lyrics Section -->
  <section class="section" bind:this={lyricsSection}>
    <h3 class="section-title">{$t('settings.lyrics.title')}</h3>
    <div class="setting-row">
      <span class="setting-label">Provider</span>
      <span class="setting-value">LRCLIB / lyrics.ovh</span>
    </div>
    <div class="setting-row last">
      <span class="setting-label">{$t('settings.lyrics.clearLyrics')}</span>
      <button
        class="clear-btn"
        onclick={handleClearLyricsCache}
        disabled={isClearingLyrics}
      >
        {isClearingLyrics ? $t('settings.storage.clearing') : $t('actions.clear')}
      </button>
    </div>
  </section>

  <!-- API Keys Section (collapsible) -->
  <section class="section api-keys-section" bind:this={apiKeysSection}>
    <button
      class="section-title-btn"
      onclick={() => apiKeysExpanded = !apiKeysExpanded}
    >
      {#if apiKeysExpanded}
        <ChevronDown size={18} />
      {:else}
        <ChevronRight size={18} />
      {/if}
      <span>{$t('settings.integrations.apiKeys')}</span>
      {#if hasAnyUserCredentials()}
        <span class="keys-badge">Custom</span>
      {/if}
    </button>

    {#if apiKeysExpanded}
      <div class="api-keys-info">
        <p>
          You don't need to configure anything here unless playlist import or album artwork features stop working.
          If you experience issues, you can provide your own API credentials to restore functionality.
        </p>
      </div>

      <!-- Spotify -->
      <div class="api-key-group">
        <div class="api-key-header">
          <span class="api-key-title">Spotify</span>
          {#if embeddedStatus.spotify}
            <span class="status-badge active">Active</span>
          {:else if spotifyClientId && spotifyClientSecret}
            <span class="status-badge custom">Custom</span>
          {:else}
            <span class="status-badge inactive">Not configured</span>
          {/if}
        </div>
        <p class="api-key-desc">
          Used for importing Spotify playlists.
          <a href="https://developer.spotify.com/dashboard" target="_blank" rel="noopener">Create an app</a> to get credentials.
        </p>
        <div class="api-key-fields">
          <input
            type="text"
            placeholder="Client ID"
            bind:value={spotifyClientId}
          />
          <input
            type="password"
            placeholder="Client Secret"
            bind:value={spotifyClientSecret}
          />
          <div class="api-key-actions">
            <button
              class="save-btn"
              onclick={handleSaveSpotifyCredentials}
              disabled={!spotifyClientId || !spotifyClientSecret || apiKeysSaving}
            >
              Save
            </button>
            {#if spotifyClientId || spotifyClientSecret}
              <button
                class="clear-btn-small"
                onclick={handleClearSpotifyCredentials}
                disabled={apiKeysSaving}
              >
                Clear
              </button>
            {/if}
          </div>
        </div>
      </div>

      <!-- Tidal -->
      <div class="api-key-group">
        <div class="api-key-header">
          <span class="api-key-title">Tidal</span>
          {#if embeddedStatus.tidal}
            <span class="status-badge active">Active</span>
          {:else if tidalClientId && tidalClientSecret}
            <span class="status-badge custom">Custom</span>
          {:else}
            <span class="status-badge inactive">Not configured</span>
          {/if}
        </div>
        <p class="api-key-desc">
          Used for importing Tidal playlists.
          <a href="https://developer.tidal.com/" target="_blank" rel="noopener">Register</a> to get API credentials.
        </p>
        <div class="api-key-fields">
          <input
            type="text"
            placeholder="Client ID"
            bind:value={tidalClientId}
          />
          <input
            type="password"
            placeholder="Client Secret"
            bind:value={tidalClientSecret}
          />
          <div class="api-key-actions">
            <button
              class="save-btn"
              onclick={handleSaveTidalCredentials}
              disabled={!tidalClientId || !tidalClientSecret || apiKeysSaving}
            >
              Save
            </button>
            {#if tidalClientId || tidalClientSecret}
              <button
                class="clear-btn-small"
                onclick={handleClearTidalCredentials}
                disabled={apiKeysSaving}
              >
                Clear
              </button>
            {/if}
          </div>
        </div>
      </div>

      <!-- Discogs -->
      <div class="api-key-group last">
        <div class="api-key-header">
          <span class="api-key-title">Discogs</span>
          {#if embeddedStatus.discogs}
            <span class="status-badge active">Active</span>
          {:else if discogsConsumerKey && discogsConsumerSecret}
            <span class="status-badge custom">Custom</span>
          {:else}
            <span class="status-badge inactive">Not configured</span>
          {/if}
        </div>
        <p class="api-key-desc">
          Used for fetching album artwork for local library.
          <a href="https://www.discogs.com/settings/developers" target="_blank" rel="noopener">Create an application</a> to get credentials.
        </p>
        <div class="api-key-fields">
          <input
            type="text"
            placeholder="Consumer Key"
            bind:value={discogsConsumerKey}
          />
          <input
            type="password"
            placeholder="Consumer Secret"
            bind:value={discogsConsumerSecret}
          />
          <div class="api-key-actions">
            <button
              class="save-btn"
              onclick={handleSaveDiscogsCredentials}
              disabled={!discogsConsumerKey || !discogsConsumerSecret || apiKeysSaving}
            >
              Save
            </button>
            {#if discogsConsumerKey || discogsConsumerSecret}
              <button
                class="clear-btn-small"
                onclick={handleClearDiscogsCredentials}
                disabled={apiKeysSaving}
              >
                Clear
              </button>
            {/if}
          </div>
        </div>
      </div>
    {/if}
  </section>
</div>

{#if isCheckingNetwork}
  <div class="network-check-overlay" aria-busy="true" aria-label={$t('offline.checkingNetwork')}>
    <div class="network-check-spinner"></div>
  </div>
{/if}

<style>
  .settings-view {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: 0 32px 24px 32px;
    padding-right: 24px; /* Less padding on right for scrollbar */
  }

  /* Scrollbar styling */
  .settings-view::-webkit-scrollbar {
    width: 8px;
  }

  .settings-view::-webkit-scrollbar-track {
    background: transparent;
  }

  .settings-view::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.15);
    border-radius: 4px;
  }

  .settings-view:hover::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.25);
  }

  .settings-view::-webkit-scrollbar-thumb:hover {
    background: rgba(255, 255, 255, 0.4);
  }

  .header {
    padding-top: 24px;
    margin-bottom: 32px;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    margin-bottom: 16px;
    transition: color 150ms ease;
  }

  .back-btn:hover {
    color: var(--text-secondary);
  }

  .title {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
  }

  /* Settings Navigation */
  .settings-nav {
    position: sticky;
    top: 0;
    z-index: 10;
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    padding: 12px 24px;
    margin: 0 -32px 24px -32px;
    background-color: var(--bg-primary);
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .nav-link {
    padding: 6px 0;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 12px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: color 150ms ease, border-color 150ms ease;
    white-space: nowrap;
  }

  .nav-link:hover {
    color: var(--text-secondary);
    border-bottom-color: var(--text-muted);
  }

  .nav-link.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-primary);
  }

  .section {
    scroll-margin-top: 60px;
    background-color: var(--bg-secondary);
    border-radius: 12px;
    padding: 24px;
    margin-bottom: 24px;
  }

  .section-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 16px;
  }

  .account-card {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px 0;
  }

  .avatar {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    background-color: var(--accent-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    font-size: 24px;
    font-weight: 600;
  }

  .account-info {
    flex: 1;
  }

  .username {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .email {
    font-size: 14px;
    color: var(--text-muted);
  }

  .subscription {
    font-size: 14px;
    color: var(--accent-primary);
  }

  .logout-btn {
    padding: 8px 24px;
    border-radius: 8px;
    border: 1px solid #ff6b6b;
    background: none;
    color: #ff6b6b;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .logout-btn:hover {
    background-color: rgba(255, 107, 107, 0.1);
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 48px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .setting-row.last {
    border-bottom: none;
  }

  .setting-label {
    font-size: 14px;
    color: var(--text-secondary);
  }

  .setting-with-description {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .setting-description {
    font-size: 12px;
    color: var(--text-muted);
  }

  .setting-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }

  .setting-desc {
    font-size: 12px;
    color: var(--text-muted);
  }

  .setting-note {
    font-size: 11px;
    color: var(--text-muted);
    opacity: 0.8;
    margin-top: 4px;
  }

  .setting-row:has(.setting-note) {
    height: auto;
    min-height: 48px;
    padding: 12px 0;
  }

  .setting-value {
    font-size: 14px;
    color: var(--text-muted);
  }

  .setting-value.muted {
    opacity: 0.5;
    font-style: italic;
  }

  .format-detail {
    font-size: 12px;
    opacity: 0.7;
    margin-left: 4px;
  }

  .label-with-tooltip {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .slider-container {
    width: 240px;
  }

  .connect-btn {
    padding: 8px 16px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
    background-color: var(--accent-primary);
    color: white;
    border: none;
  }

  .connect-btn:hover {
    background-color: var(--accent-hover);
  }

  .connect-btn.connected {
    background: none;
    border: 1px solid var(--text-muted);
    color: var(--text-muted);
  }

  .connect-btn.connected:hover {
    border-color: var(--text-secondary);
    color: var(--text-secondary);
  }

  .clear-btn {
    padding: 8px 16px;
    border-radius: 8px;
    border: 1px solid #ff6b6b;
    background: none;
    color: #ff6b6b;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease;
  }

  .clear-btn:hover:not(:disabled) {
    background-color: rgba(255, 107, 107, 0.1);
  }

  .clear-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .folder-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    border-radius: 8px;
    border: 1px solid var(--text-muted);
    background: none;
    color: var(--text-secondary);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .folder-btn:hover {
    border-color: var(--text-primary);
    color: var(--text-primary);
    background-color: var(--bg-hover);
  }

  /* Last.fm styles */
  .lastfm-connected {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .lastfm-username {
    font-size: 12px;
    color: var(--accent-primary);
  }

  .lastfm-config {
    padding: 16px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
    margin-top: 8px;
  }

  .config-info {
    font-size: 13px;
    color: var(--text-muted);
    margin-bottom: 16px;
  }

  .config-info a {
    color: var(--accent-primary);
    text-decoration: none;
  }

  .config-info a:hover {
    text-decoration: underline;
  }

  .config-field {
    margin-bottom: 12px;
  }

  .config-field label {
    display: block;
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .config-field input {
    width: 100%;
    padding: 8px 12px;
    background-color: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 14px;
  }

  .config-field input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .config-field input::placeholder {
    color: var(--text-disabled);
  }

  .auth-info {
    font-size: 13px;
    color: var(--text-secondary);
    margin: 16px 0;
    padding: 12px;
    background-color: var(--bg-secondary);
    border-radius: 6px;
  }

  .auth-start-btn,
  .auth-complete-btn {
    width: 100%;
    padding: 10px 16px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .auth-start-btn {
    background-color: var(--accent-primary);
    color: white;
    border: none;
  }

  .auth-start-btn:hover:not(:disabled) {
    background-color: var(--accent-hover);
  }

  .auth-start-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .auth-complete-btn {
    background-color: #1db954;
    color: white;
    border: none;
  }

  .auth-complete-btn:hover:not(:disabled) {
    background-color: #1ed760;
  }

  .auth-complete-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .auth-cancel-btn {
    width: 100%;
    padding: 10px 16px;
    margin-top: 8px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
    background: none;
    border: 1px solid var(--text-muted);
    color: var(--text-muted);
  }

  .auth-cancel-btn:hover {
    border-color: var(--text-secondary);
    color: var(--text-secondary);
  }

  .connect-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* API Keys section styles */
  .api-keys-section {
    padding-bottom: 16px;
  }

  .section-title-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    background: none;
    border: none;
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    cursor: pointer;
    padding: 0;
    text-align: left;
  }

  .section-title-btn:hover {
    color: var(--accent-primary);
  }

  .keys-badge {
    font-size: 10px;
    font-weight: 500;
    padding: 2px 6px;
    border-radius: 4px;
    background-color: var(--accent-primary);
    color: white;
    margin-left: auto;
  }

  .api-keys-info {
    margin: 16px 0;
    padding: 12px;
    background-color: var(--bg-tertiary);
    border-radius: 8px;
  }

  .api-keys-info p {
    font-size: 13px;
    color: var(--text-muted);
    margin: 0;
    line-height: 1.5;
  }

  .api-key-group {
    padding: 16px 0;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .api-key-group.last {
    border-bottom: none;
    padding-bottom: 0;
  }

  .api-key-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  .api-key-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .status-badge {
    font-size: 10px;
    font-weight: 500;
    padding: 2px 6px;
    border-radius: 4px;
  }

  .status-badge.active {
    background-color: rgba(29, 185, 84, 0.2);
    color: #1db954;
  }

  .status-badge.custom {
    background-color: rgba(99, 102, 241, 0.2);
    color: #6366f1;
  }

  .status-badge.inactive {
    background-color: var(--bg-tertiary);
    color: var(--text-muted);
  }

  .api-key-desc {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0 0 12px 0;
  }

  .api-key-desc a {
    color: var(--accent-primary);
    text-decoration: none;
  }

  .api-key-desc a:hover {
    text-decoration: underline;
  }

  .api-key-fields {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .api-key-fields input {
    flex: 1;
    min-width: 150px;
    padding: 8px 12px;
    background-color: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
  }

  .api-key-fields input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .api-key-fields input::placeholder {
    color: var(--text-disabled);
  }

  .api-key-actions {
    display: flex;
    gap: 8px;
  }

  .save-btn {
    padding: 8px 16px;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
    background-color: var(--accent-primary);
    color: white;
    border: none;
  }

  .save-btn:hover:not(:disabled) {
    background-color: var(--accent-hover);
  }

  .save-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .clear-btn-small {
    padding: 8px 12px;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
    background: none;
    border: 1px solid var(--text-muted);
    color: var(--text-muted);
  }

  .clear-btn-small:hover:not(:disabled) {
    border-color: #ff6b6b;
    color: #ff6b6b;
  }

  .clear-btn-small:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Offline status indicator */
  .status-indicator {
    font-weight: 500;
    color: #4ade80;
  }

  .status-indicator.offline {
    color: #fbbf24;
  }

  /* Network check overlay */
  .network-check-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 9999;
  }

  .network-check-spinner {
    width: 48px;
    height: 48px;
    border: 4px solid rgba(255, 255, 255, 0.2);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>

<MigrationModal
  isOpen={showMigrationModal}
  onClose={closeMigrationModal}
  totalTracks={legacyTracksCount}
/>
