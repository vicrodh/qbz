<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { ArrowLeft, ChevronDown, ChevronRight, Loader2 } from 'lucide-svelte';
  import Toggle from '../Toggle.svelte';
  import Dropdown from '../Dropdown.svelte';
  import VolumeSlider from '../VolumeSlider.svelte';
  import {
    getOfflineCacheStats,
    clearOfflineCache,
    type OfflineCacheStats
  } from '$lib/stores/offlineCacheState';
  import { notifyDownloadSettingsChanged } from '$lib/stores/downloadSettingsStore';
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
  import { getDevicePrettyName } from '$lib/utils/audioDeviceNames';
  import { ZOOM_OPTIONS, findZoomOption, getZoomLevelFromOption } from '$lib/utils/zoom';
  import { getZoom, setZoom, subscribeZoom } from '$lib/stores/zoomStore';
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
  import {
    subscribe as subscribeTitleBar,
    getHideTitleBar,
    setHideTitleBar
  } from '$lib/stores/titleBarStore';
  import {
    getPlaybackPreferences,
    setAutoplayMode,
    setShowContextIcon,
    type AutoplayMode
  } from '$lib/stores/playbackPreferencesStore';

  interface Props {
    onBack?: () => void;
    onLogout?: () => void;
    userName?: string;
    userEmail?: string;
    subscription?: string;
    subscriptionValidUntil?: string | null;
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

  let {
    onBack,
    onLogout,
    userName = 'User',
    userEmail = '',
    subscription = 'Qobuz™',
    subscriptionValidUntil = null
  }: Props = $props();

  // Cache state (memory cache)
  let cacheStats = $state<CacheStats | null>(null);
  let isClearing = $state(false);

  // Download cache state (offline storage)
  let downloadStats = $state<OfflineCacheStats | null>(null);
  let isClearingDownloads = $state(false);
  let isRepairingDownloads = $state(false);

  // Lyrics cache state
  let isClearingLyrics = $state(false);
  let lyricsCacheStats = $state<{ entries: number; sizeBytes: number } | null>(null);

  // Migration state
  let showMigrationModal = $state(false);
  let legacyTracksCount = $state(0);

  // Offline mode state
  let offlineStatus = $state<OfflineStatus>(getOfflineStatus());
  let offlineSettings = $state<OfflineSettings>(getOfflineSettings());

  // Flatpak detection state
  let isFlatpak = $state(false);
  let flatpakHelpText = $state('');
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
  let activeSection = $state('audio');

  // Navigation section definitions (static, refs resolved at click/scroll time)
  const navSectionDefs = [
    { id: 'audio', label: 'Audio' },
    { id: 'playback', label: 'Playback' },
    { id: 'offline', label: 'Offline' },
    { id: 'appearance', label: 'Appearance' },
    { id: 'downloads', label: 'Offline Library' },
    { id: 'library', label: 'Library' },
    { id: 'integrations', label: 'Integrations' },
    { id: 'storage', label: 'Storage' },
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
    'Warm': 'warm',
    'Nord': 'nord',
    'Dracula': 'dracula',
    'Tokyo Night': 'tokyo-night',
    'Catppuccin Mocha': 'catppuccin-mocha',
    'Rose Pine Dawn': 'rose-pine-dawn',
    'Breeze Dark': 'breeze-dark',
    'Breeze Light': 'breeze-light',
    'Adwaita Dark': 'adwaita-dark',
    'Adwaita Light': 'adwaita-light'
  };

  const themeReverseMap: Record<string, string> = {
    '': 'Dark',
    'light': 'Light',
    'oled': 'OLED Black',
    'warm': 'Warm',
    'nord': 'Nord',
    'dracula': 'Dracula',
    'tokyo-night': 'Tokyo Night',
    'catppuccin-mocha': 'Catppuccin Mocha',
    'rose-pine-dawn': 'Rose Pine Dawn',
    'breeze-dark': 'Breeze Dark',
    'breeze-light': 'Breeze Light',
    'adwaita-dark': 'Adwaita Dark',
    'adwaita-light': 'Adwaita Light'
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
  let selectedAlsaPlugin = $state<string>('hw (Direct Hardware)');
  let alsaHardwareVolume = $state(false);

  // Backend system state
  let availableBackends = $state<BackendInfo[]>([]);
  let backendDevices = $state<AudioDevice[]>([]);
  let alsaPlugins = $state<AlsaPluginInfo[]>([]);
  let isLoadingDevices = $state(false);

  // Backend selector options (derived)
  // TEST: Re-enable ALSA Direct to verify if CPAL can actually open hw: devices
  let backendOptions = $derived(['Auto', ...availableBackends.filter(b => b.is_available).map(b => b.name)]);

  // Helper to check if a device name looks like raw ALSA (needs translation)
  function needsTranslation(name: string): boolean {
    // PipeWire device names start with "alsa_output." - those already have friendly names
    if (name.startsWith('alsa_output.')) {
      return false;
    }

    // Everything else from ALSA needs translation
    return true;
  }

  // Device options based on selected backend (derived)
  // For ALSA: use description from aplay -L if available, otherwise translate
  // For PipeWire/PulseAudio: names are already friendly
  let deviceOptions = $derived.by(() => {
    // First pass: generate display names
    const displayNames = backendDevices.map(d => {
      if (d.description && selectedBackend === 'ALSA Direct') {
        return d.description;
      }
      return needsTranslation(d.name) ? getDevicePrettyName(d.name) : d.name;
    });

    // Second pass: check for duplicates and make unique if needed
    const counts = new Map<string, number>();
    const uniqueNames = displayNames.map((name, idx) => {
      const count = counts.get(name) || 0;
      counts.set(name, count + 1);

      // If duplicate, append device ID to make it unique
      if (displayNames.filter(n => n === name).length > 1) {
        const device = backendDevices[idx];
        return `${name} [${device.name}]`;
      }
      return name;
    });

    return ['System Default', ...uniqueNames];
  });

  // Map display name -> device for lookup when user selects
  let deviceByDisplayName = $derived.by(() => {
    const map = new Map<string, AudioDevice>();

    // Use same logic as deviceOptions to generate unique names
    const displayNames = backendDevices.map(d => {
      if (d.description && selectedBackend === 'ALSA Direct') {
        return d.description;
      }
      return needsTranslation(d.name) ? getDevicePrettyName(d.name) : d.name;
    });

    backendDevices.forEach((device, idx) => {
      let displayName = displayNames[idx];

      // If duplicate, append device ID
      if (displayNames.filter(n => n === displayName).length > 1) {
        displayName = `${displayName} [${device.name}]`;
      }

      map.set(displayName, device);
    });

    return map;
  });

  // ALSA plugin options (derived)
  let alsaPluginOptions = $derived(alsaPlugins.map(p => p.name));

  // Show ALSA plugin selector only when ALSA backend is selected (derived)
  let showAlsaPluginSelector = $derived(selectedBackend === 'ALSA Direct');

  // Show hardware volume control only for ALSA Direct + Hw plugin (bit-perfect)
  let showAlsaHardwareVolume = $derived(
    selectedBackend === 'ALSA Direct' && selectedAlsaPlugin === 'hw (Direct Hardware)'
  );

  // Smart toggle states - auto-disable incompatible features
  let exclusiveModeDisabled = $derived(selectedBackend === 'PipeWire' || selectedBackend === 'Auto' || selectedBackend === 'PulseAudio');
  let exclusiveModeTooltipOverride = $derived(
    exclusiveModeDisabled
      ? 'Exclusive mode is only available with ALSA Direct backend. PipeWire and PulseAudio are multiplexed audio servers and cannot provide true exclusive access.'
      : null
  );
  let dacPassthroughDisabled = $derived(selectedBackend !== 'PipeWire');
  let dacPassthroughTooltipOverride = $derived(
    dacPassthroughDisabled
      ? 'DAC Passthrough uses pw-metadata to force PipeWire sample rates. It is only compatible with the PipeWire backend.'
      : null
  );
  let gaplessDisabled = $derived(dacPassthrough);
  let gaplessTooltipOverride = $derived(
    gaplessDisabled
      ? 'Gapless playback is disabled when DAC Passthrough is enabled. Bit-perfect audio requires recreating streams for each sample rate change.'
      : null
  );

  // Playback settings
  let autoplayMode = $state<AutoplayMode>('continue');
  let showContextIcon = $state(true);
  let gaplessPlayback = $state(true);
  let crossfade = $state(0);
  let normalizeVolume = $state(false);

  // UI scale settings
  const zoomOptions = [...ZOOM_OPTIONS];
  let zoomLevel = $state('100%');

  // Appearance settings
  let theme = $state('Dark');
  let toastsEnabled = $state(true);
  let systemNotificationsEnabled = $state(true);
  let language = $state('Auto');

  // Title bar settings
  let hideTitleBar = $state(getHideTitleBar());

  // Tray settings
  let enableTray = $state(true);
  let minimizeToTray = $state(false);
  let closeToTray = $state(false);

  // Library settings
  let fetchQobuzArtistImages = $state(true);
  let showQobuzDownloadsInLibrary = $state(false);

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

    const updateZoomLevel = (value: number) => {
      const match = findZoomOption(value);
      if (match) {
        zoomLevel = match;
      }
    };

    updateZoomLevel(getZoom());
    const unsubscribeZoom = subscribeZoom(updateZoomLevel);

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

    // Load lyrics cache stats
    loadLyricsCacheStats();

    // Load audio devices first (includes PipeWire sinks), then settings
    // Also load backends and ALSA plugins
    Promise.all([
      loadAudioDevices(),
      loadBackends(),
      loadAlsaPlugins()
    ]).then(() => loadAudioSettings());

    // Load Last.fm state
    loadLastfmState();

    // Load notification preferences
    loadToastsPreference();
    toastsEnabled = getToastsEnabled();
    loadSystemNotificationsPreference();
    systemNotificationsEnabled = getSystemNotificationsEnabled();

    // Load playback preferences
    loadPlaybackPreferences();

    // Load tray settings
    loadTraySettings();

    // Detect if running in Flatpak
    loadFlatpakStatus();

    // Check for legacy cached files
    checkLegacyCachedFiles();

    // Subscribe to offline state changes
    const unsubscribeOffline = subscribeOffline(() => {
      offlineStatus = getOfflineStatus();
      offlineSettings = getOfflineSettings();
    });

    // Subscribe to title bar state changes
    const unsubscribeTitleBar = subscribeTitleBar(() => {
      hideTitleBar = getHideTitleBar();
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
      unsubscribeZoom();
      unsubscribeTitleBar();
      settingsViewEl?.removeEventListener('scroll', handleScroll);
    };
  });

  // Reload playback preferences each time the view becomes visible
  // This ensures settings are fresh when navigating back to settings
  $effect(() => {
    // This effect runs whenever the component is rendered
    // We reload preferences to ensure they're in sync with backend
    console.log('[Settings] Component rendered, reloading playback preferences');
    loadPlaybackPreferences();
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

  async function handleShowDownloadsChange(enabled: boolean) {
    try {
      await invoke('set_show_downloads_in_library', { show: enabled });
      showQobuzDownloadsInLibrary = enabled;
      // Notify LocalLibraryView to refresh
      notifyDownloadSettingsChanged();
    } catch (e) {
      console.error('Failed to update show downloads setting:', e);
    }
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

  async function handleShowNetworkFoldersChange(enabled: boolean) {
    try {
      await setShowNetworkFoldersInManualOffline(enabled);
    } catch (error) {
      console.error('Failed to set show network folders in manual offline:', error);
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
    alsa_hardware_volume: boolean;
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
    isLoadingDevices = true;
    try {
      const devices = await invoke<AudioDevice[]>('get_devices_for_backend', { backendType });
      backendDevices = devices;
      console.log(`[Audio] Devices for ${backendType}:`, devices);
    } catch (err) {
      console.error(`Failed to load devices for ${backendType}:`, err);
      backendDevices = [];
    } finally {
      isLoadingDevices = false;
    }
  }

  async function loadFlatpakStatus() {
    try {
      isFlatpak = await invoke<boolean>('is_running_in_flatpak');
      if (isFlatpak) {
        flatpakHelpText = await invoke<string>('get_flatpak_help_text');
      }
    } catch (err) {
      console.error('Failed to check Flatpak status:', err);
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
        const backendName = backend?.name ?? 'Auto';

        // TEST: Allow ALSA Direct to load for testing
        selectedBackend = backendName;
        // Load devices for selected backend
        await loadBackendDevices(settings.backend_type);

        // Set selected device from backend devices
        if (settings.output_device) {
          const device = backendDevices.find(d => d.id === settings.output_device);
          if (device) {
            // Use description from aplay -L if available (ALSA), otherwise translate
            outputDevice = (device.description && settings.backend_type === 'Alsa')
              ? device.description
              : (needsTranslation(device.name) ? getDevicePrettyName(device.name) : device.name);
          } else {
            outputDevice = 'System Default';
          }
        }
      } else {
        selectedBackend = 'Auto';
        // Auto mode: always use System Default (no device selection)
        // Device names from one backend (e.g., PipeWire) don't work in another (e.g., ALSA)
        backendDevices = [];
        outputDevice = 'System Default';
      }

      if (settings.alsa_plugin) {
        const plugin = alsaPlugins.find(p => p.plugin === settings.alsa_plugin);
        selectedAlsaPlugin = plugin?.name ?? 'hw (Direct Hardware)';
      } else {
        selectedAlsaPlugin = 'hw (Direct Hardware)';
      }

      alsaHardwareVolume = settings.alsa_hardware_volume ?? false;

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

    // Auto-disable incompatible features
    // DAC Passthrough only works with PipeWire
    if (backendName !== 'PipeWire') {
      if (dacPassthrough) {
        dacPassthrough = false;
        await invoke('set_audio_dac_passthrough', { enabled: false });
        console.log('[Audio] Disabled DAC Passthrough (only compatible with PipeWire)');
      }
    }

    // Exclusive mode only works with ALSA Direct
    if (backendName !== 'ALSA Direct') {
      if (exclusiveMode) {
        exclusiveMode = false;
        await invoke('set_audio_exclusive_mode', { enabled: false });
        console.log('[Audio] Disabled exclusive mode (only compatible with ALSA Direct)');
      }
    }

    try {
      // Save backend preference
      await invoke('set_audio_backend_type', { backendType });
      console.log('[Audio] Backend changed:', backendName, '(type:', backendType ?? 'auto', ')');

      // Load devices for new backend
      if (backendType) {
        await loadBackendDevices(backendType);
      } else {
        // Auto mode: no device selection (System Default only)
        // Device IDs from different backends are incompatible
        backendDevices = [];
      }

      // Reset to default device when switching backends (always)
      outputDevice = 'System Default';
      await invoke('set_audio_output_device', { device: null });

      // Reinitialize audio - this will recreate the stream completely
      await invoke('reinit_audio_device', { device: null });

      // Stop current playback to prevent stuck/dead streams
      try {
        await invoke('stop_playback');
        console.log('[Audio] Stopped playback after backend change');
      } catch (err) {
        // Ignore error if nothing was playing
        console.log('[Audio] No playback to stop');
      }
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

  async function handleAlsaHardwareVolumeChange(enabled: boolean) {
    alsaHardwareVolume = enabled;
    try {
      await invoke('set_audio_alsa_hardware_volume', { enabled });
      console.log('[Audio] ALSA hardware volume changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change ALSA hardware volume:', err);
    }
  }

  async function handleBackendDeviceChange(deviceName: string) {
    outputDevice = deviceName;

    // Get device ID from backendDevices using display name mapping
    const device = deviceByDisplayName.get(deviceName);
    const deviceId = deviceName === 'System Default' ? null : device?.id ?? null;

    try {
      await invoke('set_audio_output_device', { device: deviceId });
      await invoke('reinit_audio_device', { device: deviceId });
      console.log('[Audio] Backend device changed:', deviceName, '(id:', deviceId ?? 'default', ')');

      // Stop current playback to prevent stuck/dead streams
      try {
        await invoke('stop_playback');
        console.log('[Audio] Stopped playback after device change');
      } catch (err) {
        console.log('[Audio] No playback to stop');
      }
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

  async function loadLyricsCacheStats() {
    try {
      const stats = await invoke<{ entries: number; sizeBytes: number }>('lyrics_get_cache_stats');
      lyricsCacheStats = stats;
    } catch (err) {
      console.error('Failed to load lyrics cache stats:', err);
      lyricsCacheStats = null;
    }
  }

  async function loadDownloadStats() {
    try {
      downloadStats = await getOfflineCacheStats();
    } catch (err) {
      console.error('Failed to load download stats:', err);
    }
  }

  async function loadDownloadSettings() {
    try {
      const settings = await invoke<{download_root: string, show_in_library: boolean}>('get_download_settings');
      showQobuzDownloadsInLibrary = settings.show_in_library;
    } catch (err) {
      console.error('Failed to load download settings:', err);
    }
  }

  async function loadPlaybackPreferences() {
    console.log('[Settings] Loading playback preferences...');
    try {
      const prefs = await getPlaybackPreferences();
      console.log('[Settings] Loaded preferences:', prefs);
      autoplayMode = prefs.autoplay_mode;
      showContextIcon = prefs.show_context_icon;
      console.log('[Settings] Set autoplayMode to:', autoplayMode);
      console.log('[Settings] Set showContextIcon to:', showContextIcon);
    } catch (err) {
      console.error('Failed to load playback preferences:', err);
    }
  }

  interface TraySettings {
    enable_tray: boolean;
    minimize_to_tray: boolean;
    close_to_tray: boolean;
  }

  async function loadTraySettings() {
    try {
      const settings = await invoke<TraySettings>('get_tray_settings');
      enableTray = settings.enable_tray;
      minimizeToTray = settings.minimize_to_tray;
      closeToTray = settings.close_to_tray;
    } catch (err) {
      console.error('Failed to load tray settings:', err);
    }
  }

  async function handleEnableTrayChange(value: boolean) {
    try {
      await invoke('set_enable_tray', { value });
      enableTray = value;
      showToast($t('settings.appearance.tray.enableTrayDesc'), 'info');
    } catch (err) {
      console.error('Failed to set enable tray:', err);
      showToast('Failed to save tray setting', 'error');
    }
  }

  async function handleMinimizeToTrayChange(value: boolean) {
    try {
      await invoke('set_minimize_to_tray', { value });
      minimizeToTray = value;
    } catch (err) {
      console.error('Failed to set minimize to tray:', err);
      showToast('Failed to save tray setting', 'error');
    }
  }

  async function handleCloseToTrayChange(value: boolean) {
    try {
      await invoke('set_close_to_tray', { value });
      closeToTray = value;
    } catch (err) {
      console.error('Failed to set close to tray:', err);
      showToast('Failed to save tray setting', 'error');
    }
  }

  async function handleAutoplayModeChange(mode: AutoplayMode) {
    console.log('[Settings] Changing autoplay mode to:', mode);
    try {
      await setAutoplayMode(mode);
      autoplayMode = mode;
      console.log('[Settings] Autoplay mode saved successfully');
    } catch (err) {
      console.error('[Settings] Failed to set autoplay mode:', err);
      showToast('Failed to save autoplay preference', 'error');
    }
  }

  async function handleShowContextIconChange(show: boolean) {
    console.log('[Settings] Changing show context icon to:', show);
    try {
      await setShowContextIcon(show);
      showContextIcon = show;
      console.log('[Settings] Show context icon saved successfully');
    } catch (err) {
      console.error('[Settings] Failed to set show context icon:', err);
      showToast('Failed to save icon visibility preference', 'error');
    }
  }

  async function checkLegacyCachedFiles() {
    try {
      const result = await invoke<{has_legacy_files: boolean, total_tracks: number}>('detect_legacy_cached_files');
      if (result.has_legacy_files && result.total_tracks > 0) {
        legacyTracksCount = result.total_tracks;
        showMigrationModal = true;
      }
    } catch (err) {
      console.error('Failed to check for legacy cached files:', err);
    }
  }

  function closeMigrationModal() {
    showMigrationModal = false;
    // Refresh stats after migration
    loadDownloadStats();
  }

  async function handleRepairDownloads() {
    if (isRepairingDownloads) return;
    isRepairingDownloads = true;
    try {
      const report = await invoke<{
        total_downloads: number;
        added_tracks: number;
        repaired_tracks: number;
        skipped_tracks: number;
        failed_tracks: string[];
      }>('library_backfill_downloads');

      const message = `Repair complete!\n\nAdded: ${report.added_tracks}\nRepaired: ${report.repaired_tracks}\nSkipped: ${report.skipped_tracks}\nFailed: ${report.failed_tracks.length}`;

      showToast(message, 'success');

      // Trigger library reload
      notifyDownloadSettingsChanged();
    } catch (err) {
      console.error('Failed to repair downloads:', err);
      showToast('Failed to repair offline library: ' + String(err), 'error');
    } finally {
      isRepairingDownloads = false;
    }
  }

  async function handleClearDownloads() {
    if (isClearingDownloads) return;
    isClearingDownloads = true;
    try {
      await clearOfflineCache();
      await loadDownloadStats();
    } catch (err) {
      console.error('Failed to clear download cache:', err);
    } finally {
      isClearingDownloads = false;
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
      await loadLyricsCacheStats();
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
    const zoom = setZoom(getZoomLevelFromOption(value));
    try {
      await getCurrentWebview().setZoom(zoom);
    } catch (err) {
      console.warn('Failed to set zoom:', err);
    }
  }
</script>

<div class="settings-view" bind:this={settingsViewEl}>
  <!-- Loading Overlay for Device Enumeration -->
  {#if isLoadingDevices}
    <div class="loading-overlay">
      <div class="loading-content">
        <Loader2 size={48} class="spinner" />
        <p>Loading audio devices...</p>
        <p class="loading-subtitle">Parsing hardware information</p>
      </div>
    </div>
  {/if}

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
        {#if subscriptionValidUntil}
          <div class="subscription-until">
            {$t('settings.account.validUntil', { values: { date: subscriptionValidUntil } })}
          </div>
        {/if}
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
      <div class="setting-info">
        <span class="setting-label">Audio Backend</span>
        <span class="setting-desc">Choose audio system: Auto (recommended), PipeWire (modern), ALSA Direct (bit-perfect, exclusive), or PulseAudio (legacy).</span>
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
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.outputDevice')}</span>
        <span class="setting-desc">Select your preferred audio output device. Devices shown are from the selected backend.</span>
      </div>
      {#if isLoadingDevices}
        <span class="loading-text">Loading devices...</span>
      {:else}
        <Dropdown
          value={outputDevice}
          options={deviceOptions}
          onchange={handleBackendDeviceChange}
          wide
          expandLeft
          compact
        />
      {/if}
    </div>
    {#if showAlsaPluginSelector}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">ALSA Plugin</span>
        <span class="setting-desc">hw: Bit-perfect, exclusive. plughw: Auto-convert. pcm: Most compatible.</span>
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
    {#if showAlsaHardwareVolume}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Enable Hardware Volume Control</span>
        <span class="setting-desc">Experimental: Controls DAC volume via ALSA mixer. Some DACs don't support this - disable for maximum compatibility. If it fails, playback continues normally.</span>
      </div>
      <Toggle enabled={alsaHardwareVolume} onchange={handleAlsaHardwareVolumeChange} />
    </div>
    {/if}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.exclusiveMode')}</span>
        <span class="setting-desc">{exclusiveModeTooltipOverride ?? $t('settings.audio.exclusiveModeDesc')}</span>
      </div>
      <Toggle enabled={exclusiveMode} onchange={handleExclusiveModeChange} disabled={exclusiveModeDisabled} />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.dacPassthrough')}</span>
        <span class="setting-desc">{dacPassthroughTooltipOverride ?? $t('settings.audio.dacPassthroughDesc')}</span>
      </div>
      <Toggle enabled={dacPassthrough} onchange={handleDacPassthroughChange} disabled={dacPassthroughDisabled} />
    </div>
    {#if isFlatpak && selectedBackend === 'PipeWire' && dacPassthrough}
    <div class="flatpak-warning">
      <div class="warning-icon">⚠️</div>
      <div class="warning-content">
        <strong>Flatpak Limitation:</strong> PipeWire cannot guarantee bit-perfect playback in sandboxed environments due to daemon access restrictions.
        <br />
        <strong>Recommended:</strong> Switch to ALSA Direct backend for true bit-perfect audio.
      </div>
    </div>
    {/if}
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
      <div class="setting-info">
        <span class="setting-label">{$t('settings.playback.autoplayBehavior')}</span>
        <span class="setting-desc">{$t('settings.playback.autoplayBehaviorDesc')}</span>
      </div>
      <Toggle enabled={autoplayMode === 'continue'} onchange={(enabled) => handleAutoplayModeChange(enabled ? 'continue' : 'track_only')} />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.playback.showContextIcon')}</span>
        <span class="setting-desc">{$t('settings.playback.showContextIconTooltip')}</span>
      </div>
      <Toggle enabled={showContextIcon} onchange={handleShowContextIconChange} />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.playback.gapless')}</span>
        {#if gaplessTooltipOverride}
          <span class="setting-desc">{gaplessTooltipOverride}</span>
        {/if}
      </div>
      <Toggle enabled={gaplessPlayback} onchange={handleGaplessPlaybackChange} disabled={gaplessDisabled} />
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
    <div class="setting-row" class:last={!offlineSettings.manualOfflineMode}>
      <div class="setting-info">
        <span class="setting-label">{$t('offline.enableManual')}</span>
        <span class="setting-desc">{$t('offline.enableManualDesc')}</span>
      </div>
      <Toggle enabled={offlineSettings.manualOfflineMode} onchange={handleManualOfflineChange} />
    </div>
    <!-- Temporarily disabled: Show Partial Playlists -->
    <!-- <div class="setting-row" class:last={!offlineSettings.manualOfflineMode}>
      <div class="setting-info">
        <span class="setting-label">{$t('offline.showPartialPlaylists')}</span>
        <span class="setting-desc">{$t('offline.showPartialPlaylistsDesc')}</span>
      </div>
      <Toggle enabled={offlineSettings.showPartialPlaylists} onchange={handleShowPartialPlaylistsChange} />
    </div> -->

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
        options={[
          'Dark',
          'Light',
          'OLED Black',
          'Warm',
          'Nord',
          'Dracula',
          'Tokyo Night',
          'Catppuccin Mocha',
          'Rose Pine Dawn',
          'Breeze Dark',
          'Breeze Light',
          'Adwaita Dark',
          'Adwaita Light'
        ]}
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
    <div class="setting-row">
      <span class="setting-label">{$t('settings.appearance.systemNotifications')}</span>
      <Toggle enabled={systemNotificationsEnabled} onchange={(v) => { systemNotificationsEnabled = v; setSystemNotificationsEnabled(v); }} />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.appearance.hideTitleBar')}</span>
        <span class="setting-desc">{$t('settings.appearance.hideTitleBarDesc')}</span>
      </div>
      <Toggle enabled={hideTitleBar} onchange={(v) => setHideTitleBar(v)} />
    </div>

    <!-- System Tray subsection -->
    <h4 class="subsection-title">{$t('settings.appearance.tray.title')}</h4>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.appearance.tray.enableTray')}</span>
        <span class="setting-desc">{$t('settings.appearance.tray.enableTrayDesc')}</span>
      </div>
      <Toggle enabled={enableTray} onchange={(v) => handleEnableTrayChange(v)} />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.appearance.tray.minimizeToTray')}</span>
        <span class="setting-desc">{$t('settings.appearance.tray.minimizeToTrayDesc')}</span>
      </div>
      <Toggle enabled={minimizeToTray} onchange={(v) => handleMinimizeToTrayChange(v)} disabled={!enableTray} />
    </div>
    <div class="setting-row last">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.appearance.tray.closeToTray')}</span>
        <span class="setting-desc">{$t('settings.appearance.tray.closeToTrayDesc')}</span>
      </div>
      <Toggle enabled={closeToTray} onchange={(v) => handleCloseToTrayChange(v)} disabled={!enableTray} />
    </div>
  </section>

  <!-- Offline Library Section -->
  <section class="section" bind:this={downloadsSection}>
    <h3 class="section-title">{$t('settings.offlineLibrary.title')}</h3>
    <p class="section-note">{$t('settings.offlineLibrary.disclaimer')}</p>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.offlineLibrary.cachedTracks')}</span>
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
        <span class="setting-description">Display offline Qobuz™ tracks in your Local Library</span>
      </div>
      <Toggle enabled={showQobuzDownloadsInLibrary} onchange={handleShowDownloadsChange} />
    </div>
    <div class="setting-row">
      <div class="setting-with-description">
        <span class="setting-label">Repair Offline Library</span>
        <span class="setting-description">Fix offline markers lost during library scans</span>
      </div>
      <button
        class="clear-btn"
        onclick={handleRepairDownloads}
        disabled={isRepairingDownloads || !downloadStats || downloadStats.readyTracks === 0}
      >
        {isRepairingDownloads ? 'Repairing...' : 'Repair'}
      </button>
    </div>
    <div class="setting-row last">
      <span class="setting-label">Clear Offline Library</span>
      <button
        class="clear-btn"
        onclick={handleClearDownloads}
        disabled={isClearingDownloads || !downloadStats || downloadStats.readyTracks === 0}
      >
        {isClearingDownloads ? 'Clearing...' : 'Clear All'}
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
    <p class="section-note">{$t('settings.storage.queueCacheNote')}</p>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.storage.clearCache')}</span>
        <small class="setting-note">
          {#if cacheStats}
            {$t('settings.storage.queueCacheStats', {
              values: {
                tracks: cacheStats.cached_tracks,
                used: formatBytes(cacheStats.current_size_bytes),
                max: formatBytes(cacheStats.max_size_bytes)
              }
            })}
          {:else}
            {$t('actions.loading')}
          {/if}
        </small>
      </div>
      <button
        class="clear-btn"
        onclick={handleClearCache}
        disabled={isClearing || !cacheStats || cacheStats.current_size_bytes === 0}
      >
        {isClearing ? $t('settings.storage.clearing') : $t('actions.clear')}
      </button>
    </div>
    <div class="setting-row last">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.lyrics.clearLyrics')}</span>
        <small class="setting-note">
          {#if lyricsCacheStats}
            {$t('settings.lyrics.cacheStats', {
              values: {
                entries: lyricsCacheStats.entries,
                size: formatBytes(lyricsCacheStats.sizeBytes)
              }
            })}
          {:else}
            -
          {/if}
        </small>
      </div>
      <button
        class="clear-btn"
        onclick={handleClearLyricsCache}
        disabled={isClearingLyrics}
      >
        {isClearingLyrics ? $t('settings.storage.clearing') : $t('actions.clear')}
      </button>
    </div>
  </section>

  <!-- Flatpak Section (only shown when running in Flatpak) -->
  {#if isFlatpak}
    <section class="section flatpak-section">
      <h3 class="section-title">Flatpak Sandbox</h3>
      <div class="flatpak-info">
        <p class="flatpak-intro">
          QBZ is running inside a Flatpak sandbox. For offline libraries on NAS, network mounts, or external disks, direct filesystem access is required.
        </p>
        <div class="flatpak-guide">
          <h4>Grant Filesystem Access</h4>
          <p>Use <strong>Flatseal</strong> (GUI) or run this command:</p>
          <pre class="code-block">flatpak override --user --filesystem=/path/to/music com.blitzfc.qbz</pre>
          <h4>Examples</h4>
          <pre class="code-block"># CIFS / Samba mount
flatpak override --user --filesystem=/mnt/nas com.blitzfc.qbz

# SSHFS mount
flatpak override --user --filesystem=$HOME/music-nas com.blitzfc.qbz</pre>
          <p class="flatpak-note">
            <strong>Note:</strong> This setting is persistent and survives reboots and updates.
          </p>
        </div>
      </div>
    </section>
  {/if}
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
    background: var(--alpha-15);
    border-radius: 4px;
  }

  .settings-view:hover::-webkit-scrollbar-thumb {
    background: var(--alpha-25);
  }

  .settings-view::-webkit-scrollbar-thumb:hover {
    background: var(--alpha-40);
  }

  .loading-text {
    color: var(--alpha-60);
    font-size: 14px;
    font-style: italic;
  }

  .loading-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 9999;
  }

  .loading-content {
    text-align: center;
    color: white;
  }

  .loading-content :global(.spinner) {
    animation: spin 1s linear infinite;
    margin: 0 auto 16px auto;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }

  .loading-content p {
    margin: 0;
    font-size: 18px;
    font-weight: 500;
  }

  .loading-subtitle {
    margin-top: 8px !important;
    font-size: 14px !important;
    opacity: 0.7;
    font-weight: 400 !important;
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
    padding: 12px 32px;
    margin: 0 -24px 24px -32px;
    width: calc(100% + 56px);
    background: var(--bg-primary);
    border-bottom: 1px solid var(--alpha-6);
    box-shadow: 0 4px 8px -4px rgba(0, 0, 0, 0.5);
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

  .subsection-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin: 20px 0 12px;
    padding-top: 16px;
    border-top: 1px solid var(--border-color);
  }

  .section-note {
    margin: -6px 0 16px;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.4;
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

  .subscription-until {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 2px;
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
    gap: 16px;
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
    flex: 0 1 60%;
    min-width: 0;
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
    max-width: 60%;
    min-width: 0;
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

  .setting-row:has(.setting-description) {
    height: auto;
    min-height: 48px;
    padding: 12px 0;
    align-items: flex-start;
  }

  .setting-row:has(.setting-desc) {
    height: auto;
    min-height: 48px;
    padding: 12px 0;
    align-items: flex-start;
  }

  .setting-row:has(.radio-group) {
    height: auto;
    min-height: 48px;
    padding: 12px 0;
    align-items: flex-start;
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

  .slider-container {
    width: 240px;
  }

  .radio-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .radio-option {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    padding: 8px 12px;
    border-radius: 6px;
    transition: background 0.15s;
  }

  .radio-option:hover {
    background: var(--alpha-5);
  }

  .radio-option input[type="radio"] {
    width: 18px;
    height: 18px;
    cursor: pointer;
    accent-color: var(--accent-color, #1db954);
  }

  .radio-option span {
    font-size: 14px;
    color: var(--text-primary);
    user-select: none;
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

  /* Harmonize button widths across settings rows */
  .connect-btn,
  .clear-btn,
  .folder-btn,
  .logout-btn {
    min-width: 140px;
    padding-top: 7px;
    padding-bottom: 7px;
  }

  .folder-btn {
    justify-content: center;
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
    border: 4px solid var(--alpha-20);
    border-top-color: var(--accent-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Flatpak section styles */
  .flatpak-section {
    background-color: var(--bg-tertiary);
    border: 1px solid rgba(99, 102, 241, 0.2);
    border-radius: 8px;
    padding: 20px;
  }

  .flatpak-info {
    color: var(--text-secondary);
  }

  .flatpak-intro {
    font-size: 14px;
    line-height: 1.6;
    margin-bottom: 20px;
    color: var(--text-primary);
  }

  .flatpak-guide h4 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 8px;
    margin-top: 16px;
  }

  .flatpak-guide h4:first-child {
    margin-top: 0;
  }

  .flatpak-guide p {
    font-size: 13px;
    line-height: 1.5;
    margin-bottom: 8px;
  }

  .code-block {
    background-color: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    padding: 12px;
    font-family: 'Fira Code', 'Courier New', monospace;
    font-size: 12px;
    color: var(--accent-primary);
    overflow-x: auto;
    margin: 8px 0 16px 0;
    white-space: pre;
  }

  .flatpak-note {
    font-size: 12px;
    color: var(--text-muted);
    font-style: italic;
    margin-top: 12px;
  }

  .flatpak-note strong {
    color: var(--text-secondary);
    font-weight: 600;
  }

  /* Flatpak warning banner */
  .flatpak-warning {
    display: flex;
    gap: 12px;
    background-color: rgba(251, 191, 36, 0.1);
    border: 1px solid rgba(251, 191, 36, 0.3);
    border-radius: 8px;
    padding: 16px;
    margin: 16px 0;
    align-items: flex-start;
  }

  .warning-icon {
    font-size: 20px;
    flex-shrink: 0;
    line-height: 1;
  }

  .warning-content {
    flex: 1;
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-primary);
  }

  .warning-content strong {
    color: rgb(251, 191, 36);
    font-weight: 600;
  }
</style>

<MigrationModal
  isOpen={showMigrationModal}
  onClose={closeMigrationModal}
  totalTracks={legacyTracksCount}
/>
