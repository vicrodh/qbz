<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import ViewTransition from '../ViewTransition.svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import { writeText as copyToClipboard } from '@tauri-apps/plugin-clipboard-manager';
  import { ask } from '@tauri-apps/plugin-dialog';
  import { ArrowLeft, ChevronRight, ChevronDown, ChevronUp, Loader2, Sun, Moon, SunMoon, Ban, AlertTriangle, RefreshCw } from 'lucide-svelte';
  import Toggle from '../Toggle.svelte';
  import Dropdown from '../Dropdown.svelte';
  import DeviceDropdown from '../DeviceDropdown.svelte';
  import DACSetupWizard from '../DACSetupWizard.svelte';
  import RemoteControlSetupGuide from '../RemoteControlSetupGuide.svelte';
  import LogsModal from '../LogsModal.svelte';
  import VolumeSlider from '../VolumeSlider.svelte';
  import UpdateCheckResultModal from '../updates/UpdateCheckResultModal.svelte';
  import WhatsNewModal from '../updates/WhatsNewModal.svelte';
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
  import { getIsPlaying } from '$lib/stores/playerStore';
  import { setLocale, locale, t } from '$lib/i18n';
  import { get } from 'svelte/store';
  import MigrationModal from '../MigrationModal.svelte';
  import { getDevicePrettyName } from '$lib/utils/audioDeviceNames';
  import { getUserItem, setUserItem, removeUserItem } from '$lib/utils/userStorage';
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
    enableVerboseCapture,
    disableVerboseCapture,
    isVerboseCaptureEnabled
  } from '$lib/stores/consoleLogStore';
  import {
    subscribe as subscribeTitleBar,
    getHideTitleBar,
    setHideTitleBar,
    getUseSystemTitleBar,
    setUseSystemTitleBar
  } from '$lib/stores/titleBarStore';
  import {
    getPlaybackPreferences,
    setAutoplayMode,
    setShowContextIcon,
    type AutoplayMode
  } from '$lib/stores/playbackPreferencesStore';
  import {
    subscribe as subscribeUpdates,
    checkForUpdates,
    fetchReleaseForVersion,
    getCurrentVersion as getUpdatesCurrentVersion,
    getPreferences as getUpdatePreferences,
    initUpdatesStore,
    setCheckOnLaunch,
    setShowWhatsNewOnLaunch,
    type ReleaseInfo,
    type UpdateCheckStatus,
    type UpdatePreferences
  } from '$lib/stores/updatesStore';
  import { openReleasePageAndAcknowledge } from '$lib/services/updatesService';
  import {
    getCount as getBlacklistCount,
    isEnabled as isBlacklistEnabled,
    subscribe as subscribeBlacklist
  } from '$lib/stores/artistBlacklistStore';
  import {
    getShowPurchases,
    setShowPurchases as setShowPurchasesStore
  } from '$lib/stores/purchasesStore';

  interface Props {
    onBack?: () => void;
    onLogout?: () => void;
    onBlacklistManagerClick?: () => void;
    onPurchasesToggle?: (enabled: boolean) => void;
    userName?: string;
    userEmail?: string;
    subscription?: string;
    subscriptionValidUntil?: string | null;
    showTitleBar?: boolean;
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

  interface RemoteControlStatus {
    enabled: boolean;
    running: boolean;
    port: number;
    localUrl: string;
    secure: boolean;
    certUrl?: string | null;
    token: string;
    lastError?: string | null;
  }

  interface RemoteControlQr {
    qrDataUrl: string;
    url: string;
  }

  interface PlexServerInfo {
    friendlyName?: string | null;
    version?: string | null;
    machineIdentifier?: string | null;
  }

  interface PlexMusicSection {
    key: string;
    title: string;
  }

  interface PlexPinStartResult {
    pinId: number;
    code: string;
    authUrl: string;
    expiresIn?: number | null;
  }

  interface PlexPinCheckResult {
    authorized: boolean;
    expired: boolean;
    authToken?: string | null;
    expiresIn?: number | null;
  }

  interface PlexTrack {
    ratingKey: string;
    title: string;
    artist?: string | null;
    album?: string | null;
    durationMs?: number | null;
    artworkPath?: string | null;
    bitDepth?: number | null;
    samplingRateHz?: number | null;
  }

  let {
    onBack,
    onLogout,
    onBlacklistManagerClick,
    onPurchasesToggle,
    userName = 'User',
    userEmail = '',
    subscription = 'Qobuz™',
    subscriptionValidUntil = null,
    showTitleBar = true
  }: Props = $props();

  // Purchases toggle
  let purchasesEnabled = $state(getShowPurchases());

  function handlePurchasesToggle(v: boolean) {
    purchasesEnabled = v;
    setShowPurchasesStore(v);
    onPurchasesToggle?.(v);
  }

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
  let isClearingMusicBrainz = $state(false);
  let musicBrainzCacheStats = $state<{ recordings: number; artists: number; releases: number; relations: number } | null>(null);
  let isClearingVectorStore = $state(false);
  let vectorStoreStats = $state<{ artist_count: number; vector_count: number; entry_count: number } | null>(null);

  // Artwork cache state (local library thumbnails)
  let isClearingArtwork = $state(false);
  let artworkCacheStats = $state<{ artwork_cache_bytes: number; thumbnails_cache_bytes: number; artwork_file_count: number; thumbnail_file_count: number } | null>(null);
  let isClearingAllCaches = $state(false);

  // Reset & factory reset state
  let isResettingAudio = $state(false);
  let factoryResetConfirmed = $state(false);
  let isFactoryResetting = $state(false);

  // Migration state
  let showMigrationModal = $state(false);
  let legacyTracksCount = $state(0);

  // ALSA Utils help modal

  // DAC Setup Wizard modal
  let showDACWizardModal = $state(false);

  // Offline mode state
  let offlineStatus = $state<OfflineStatus>(getOfflineStatus());
  let offlineSettings = $state<OfflineSettings>(getOfflineSettings());

  // Flatpak detection state
  let isFlatpak = $state(false);
  let flatpakHelpText = $state('');
  let isCheckingNetwork = $state(false);

  // Updates state
  let updatePreferences = $state<UpdatePreferences>(getUpdatePreferences());
  let updatesCurrentVersion = $state(getUpdatesCurrentVersion());
  let isCheckingUpdates = $state(false);
  let isUpdateResultOpen = $state(false);
  let updateResultStatus = $state<UpdateCheckStatus>('no_updates');
  let updateResultRelease = $state<ReleaseInfo | null>(null);
  let isSettingsWhatsNewOpen = $state(false);
  let settingsWhatsNewRelease = $state<ReleaseInfo | null>(null);
  let isFetchingChangelog = $state(false);

  // Blacklist state
  let blacklistCount = $state(getBlacklistCount());
  let blacklistEnabled = $state(isBlacklistEnabled());

  // Section navigation
  let settingsViewEl: HTMLDivElement;
  let audioSection: HTMLElement;
  let playbackSection: HTMLElement;
  let offlineModeSection: HTMLElement;
  let appearanceSection: HTMLElement;
  let downloadsSection: HTMLElement;
  let contentFilteringSection: HTMLElement;
  let integrationsSection: HTMLElement;
  let remoteControlSection: HTMLElement;
  let updatesSection: HTMLElement;
  let storageSection: HTMLElement;
  let flatpakSection = $state<HTMLElement | null>(null);
  let activeSection = $state('audio');

  // Collapsible sections state (closed by default)
  let offlineLibraryCollapsed = $state(true);
  let storageCollapsed = $state(true);
  let developerCollapsed = $state(true);
  let forceDmabuf = $state(false);
  let hardwareAcceleration = $state(true);
  let verboseLogCapture = $state(false);
  let forceX11 = $state(false);
  let gdkScale = $state('');
  let gdkDpiScale = $state('');
  let compositionCollapsed = $state(true);
  // Graphics startup status (for showing degraded mode warning)
  let graphicsUsingFallback = $state(false);
  let graphicsIsWayland = $state(false);
  let graphicsHasNvidia = $state(false);
  let graphicsHwAccelEnabled = $state(true);
  let showLogsModal = $state(false);

  type CompositionProfileId = 'nativeWayland' | 'x11Balanced' | 'x11Performance';

  type CompositionProfile = {
    id: CompositionProfileId;
    forceX11: boolean;
    gdkScale: string;
    gdkDpiScale: string;
    labelKey: string;
    descKey: string;
  };

  const compositionProfiles: CompositionProfile[] = [
    {
      id: 'nativeWayland',
      forceX11: false,
      gdkScale: '',
      gdkDpiScale: '',
      labelKey: 'settings.appearance.composition.profiles.nativeWaylandLabel',
      descKey: 'settings.appearance.composition.profiles.nativeWaylandDesc',
    },
    {
      id: 'x11Balanced',
      forceX11: true,
      gdkScale: '1',
      gdkDpiScale: '1.1',
      labelKey: 'settings.appearance.composition.profiles.x11BalancedLabel',
      descKey: 'settings.appearance.composition.profiles.x11BalancedDesc',
    },
    {
      id: 'x11Performance',
      forceX11: true,
      gdkScale: '1',
      gdkDpiScale: '1',
      labelKey: 'settings.appearance.composition.profiles.x11PerformanceLabel',
      descKey: 'settings.appearance.composition.profiles.x11PerformanceDesc',
    },
  ];

  // Navigation section IDs with translation keys
  const navSectionIds = [
    { id: 'audio', labelKey: 'settings.audio.title' },
    { id: 'playback', labelKey: 'settings.playback.title' },
    { id: 'offline', labelKey: 'offline.title' },
    { id: 'appearance', labelKey: 'settings.appearance.title' },
    { id: 'downloads', labelKey: 'settings.offlineLibrary.title' },
    { id: 'content-filtering', labelKey: 'settings.contentFiltering.title' },
    { id: 'integrations', labelKey: 'settings.integrations.title' },
    { id: 'updates', labelKey: 'nav.updates' },
    { id: 'remote-control', labelKey: 'settings.integrations.remoteControl' },
    { id: 'storage', labelKey: 'settings.storage.title' },
  ];

  // Navigation section definitions (dynamic: includes Flatpak only when running in Flatpak)
  // NOTE: If adding new sections, add them BEFORE Flatpak. Flatpak must always be last.
  const navSectionDefs = $derived(
    isFlatpak
      ? [...navSectionIds, { id: 'flatpak', labelKey: 'nav.flatpak' }]
      : navSectionIds
  );

  // Get section element by id (resolved at call time, not definition time)
  function getSectionEl(id: string): HTMLElement | null {
    switch (id) {
      case 'audio': return audioSection;
      case 'playback': return playbackSection;
      case 'offline': return offlineModeSection;
      case 'appearance': return appearanceSection;
      case 'downloads': return downloadsSection;
      case 'content-filtering': return contentFilteringSection;
      case 'integrations': return integrationsSection;
      case 'remote-control': return remoteControlSection;
      case 'updates': return updatesSection;
      case 'storage': return storageSection;
      case 'flatpak': return flatpakSection;
      default: return null;
    }
  }

  function scrollToSection(id: string) {
    const el = getSectionEl(id);
    if (!el) return;
    activeSection = id;
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  async function handleUpdateCheckOnLaunchToggle(enabled: boolean): Promise<void> {
    await setCheckOnLaunch(enabled);
  }

  async function handleShowWhatsNewToggle(enabled: boolean): Promise<void> {
    await setShowWhatsNewOnLaunch(enabled);
  }

  async function handleManualUpdateCheck(): Promise<void> {
    if (isCheckingUpdates) return;
    isCheckingUpdates = true;
    try {
      const result = await checkForUpdates('manual');
      updateResultStatus = result.status;
      updateResultRelease = result.release;
      isUpdateResultOpen = true;
    } finally {
      isCheckingUpdates = false;
    }
  }

  function handleCloseUpdateResult(): void {
    isUpdateResultOpen = false;
  }

  function handleVisitReleaseFromResult(): void {
    if (!updateResultRelease) return;
    void openReleasePageAndAcknowledge(updateResultRelease);
    isUpdateResultOpen = false;
  }

  async function handleShowCurrentChangelog(): Promise<void> {
    const version = updatesCurrentVersion || getUpdatesCurrentVersion();
    if (!version) {
      showToast($t('settings.updates.versionUnavailable'), 'error');
      return;
    }
    isFetchingChangelog = true;
    try {
      const release = await fetchReleaseForVersion(version);
      if (!release) {
        showToast($t('settings.updates.changelogUnavailable'), 'error');
        return;
      }
      settingsWhatsNewRelease = release;
      isSettingsWhatsNewOpen = true;
    } catch {
      showToast($t('settings.updates.changelogUnavailable'), 'error');
    } finally {
      isFetchingChangelog = false;
    }
  }

  function handleCloseSettingsWhatsNew(): void {
    isSettingsWhatsNewOpen = false;
    settingsWhatsNewRelease = null;
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

  // Theme metadata with type classification
  type ThemeType = 'dark' | 'light';
  interface ThemeInfo {
    value: string;      // data-theme value
    type: ThemeType;    // dark or light
  }

  const themes: Record<string, ThemeInfo> = {
    // Dark themes
    'Dark':              { value: '',                 type: 'dark' },
    'OLED Black':        { value: 'oled',             type: 'dark' },
    'Warm':              { value: 'warm',             type: 'dark' },
    'Nord':              { value: 'nord',             type: 'dark' },
    'Dracula':           { value: 'dracula',          type: 'dark' },
    'Tokyo Night':       { value: 'tokyo-night',      type: 'dark' },
    'Catppuccin Mocha':  { value: 'catppuccin-mocha', type: 'dark' },
    'Breeze Dark':       { value: 'breeze-dark',      type: 'dark' },
    'Adwaita Dark':      { value: 'adwaita-dark',     type: 'dark' },
    'Alucard':           { value: 'alucard',          type: 'light' },
    'Aurora':            { value: 'aurora',           type: 'dark' },
    'Ikari':             { value: 'ikari',            type: 'dark' },
    'Ayanami':           { value: 'ayanami',          type: 'dark' },
    'Iscariot':          { value: 'iscariot',         type: 'dark' },
    'Rumi':              { value: 'rumi',             type: 'dark' },
    'Zoey':              { value: 'zoey',             type: 'dark' },
    'Mira':              { value: 'mira',             type: 'dark' },
    // Light themes
    'Light':             { value: 'light',            type: 'light' },
    'Rose Pine Dawn':    { value: 'rose-pine-dawn',   type: 'light' },
    'Breeze Light':      { value: 'breeze-light',     type: 'light' },
    'Adwaita Light':     { value: 'adwaita-light',    type: 'light' },
    'Duotone Snow':      { value: 'duotone-snow',     type: 'light' },
    'Snow Storm':        { value: 'snow-storm',       type: 'light' },
    'Frost':             { value: 'frost',            type: 'light' },
    'Langley':           { value: 'langley',          type: 'light' },
    'Kurosaki':          { value: 'kurosaki',         type: 'light' },
  };

  // Generate maps from themes object for compatibility
  const themeMap: Record<string, string> = Object.fromEntries(
    Object.entries(themes).map(([name, info]) => [name, info.value])
  );

  const themeReverseMap: Record<string, string> = Object.fromEntries(
    Object.entries(themes).map(([name, info]) => [info.value, name])
  );

  // Theme filter state: 'all' | 'dark' | 'light'
  type ThemeFilter = 'all' | 'dark' | 'light';
  let themeFilter = $state<ThemeFilter>('all');

  // Filtered theme options based on current filter
  const filteredThemeOptions = $derived(
    themeFilter === 'all'
      ? Object.keys(themes)
      : Object.entries(themes)
          .filter(([_, info]) => info.type === themeFilter)
          .map(([name]) => name)
  );

  function cycleThemeFilter() {
    if (themeFilter === 'all') themeFilter = 'dark';
    else if (themeFilter === 'dark') themeFilter = 'light';
    else themeFilter = 'all';
  }

  // Language mapping: display name -> locale code
  const languageToLocale: Record<string, string | null> = {
    'Auto': null,
    'English': 'en',
    'Español': 'es',
    'Français': 'fr',
    'Deutsch': 'de'
  };

  const localeToLanguage: Record<string, string> = {
    'en': 'English',
    'es': 'Español',
    'fr': 'Français',
    'de': 'Deutsch'
  };

  // Available languages (only those with translations)
  const availableLanguages = ['Auto', 'English', 'Español', 'Français', 'Deutsch'];

  // Font family selection
  const fontFamilies: Record<string, string> = {
    'LINE Seed JP': '',                // default (no data-font attribute)
    'Montserrat': 'montserrat',
    'Noto Sans': 'noto-sans',
    'Source Sans 3': 'source-sans-3',
    'System': 'system',
  };

  const fontReverseMap: Record<string, string> = Object.fromEntries(
    Object.entries(fontFamilies).map(([name, val]) => [val, name])
  );

  const fontOptions = Object.keys(fontFamilies);

  let selectedFont = $state('LINE Seed JP');

  function applyFont(fontValue: string) {
    if (fontValue) {
      document.documentElement.setAttribute('data-font', fontValue);
    } else {
      document.documentElement.removeAttribute('data-font');
    }
  }

  function handleFontChange(newFont: string) {
    selectedFont = newFont;
    const fontValue = fontFamilies[newFont] || '';
    applyFont(fontValue);
    localStorage.setItem('qbz-font-family', fontValue);
  }

  // Audio settings
  let streamingQuality = $state('Hi-Res+');
  let outputDevice = $state('System Default');
  let exclusiveMode = $state(false);
  let dacPassthrough = $state(false);
  let pwForceBitperfect = $state(false);
  let selectedBackend = $state<string>('Auto');
  let selectedAlsaPlugin = $state<string>('hw (Direct Hardware)');
  let alsaHardwareVolume = $state(false);
  let streamFirstTrack = $state(false);
  let streamBufferSeconds = $state(3);
  let streamingOnly = $state(false);
  let limitQualityToDevice = $state(false);  // Re-enabled in 1.2.x with manual per-device config
  let deviceMaxSampleRate = $state<number | null>(null);  // Per-device max sample rate

  // Sample rate options for the dropdown
  const sampleRateOptions = [
    { value: 44100, label: '44.1 kHz (CD)' },
    { value: 48000, label: '48 kHz (DVD)' },
    { value: 96000, label: '96 kHz (Hi-Res)' },
    { value: 192000, label: '192 kHz (Hi-Res+)' },
    { value: 384000, label: '384 kHz (DSD)' },
  ];

  // Backend system state
  let availableBackends = $state<BackendInfo[]>([]);
  let backendDevices = $state<AudioDevice[]>([]);
  let alsaPlugins = $state<AlsaPluginInfo[]>([]);
  let isLoadingDevices = $state(false);
  let defaultDeviceName = $state<string | null>(null);

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
  // For ALSA: use backend-provided description if available, otherwise translate
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

  // Device options for grouped dropdown (works for both ALSA and PipeWire)
  let groupedDeviceOptions = $derived.by(() => {
    // System Default is always first
    const options: {
      value: string;
      id: string;
      isDefault?: boolean;
      sampleRates?: number[];
      deviceBus?: string;
      isHardware?: boolean;
    }[] = [
      { value: 'System Default', id: 'system-default', isDefault: true }
    ];

    // Generate unique display names (same logic as deviceOptions)
    const displayNames = backendDevices.map(d => {
      if (d.description && selectedBackend === 'ALSA Direct') {
        return d.description;
      }
      return needsTranslation(d.name) ? getDevicePrettyName(d.name) : d.name;
    });

    backendDevices.forEach((device, idx) => {
      // Skip 'default' device - we already added explicit "System Default" above
      if (device.id === 'default' || device.name === 'default') {
        return;
      }

      let displayName = displayNames[idx];

      // If duplicate, append device ID to make unique
      if (displayNames.filter(n => n === displayName).length > 1) {
        displayName = `${displayName} [${device.name}]`;
      }

      options.push({
        value: displayName,
        id: device.id,
        isDefault: device.is_default,
        sampleRates: device.supported_sample_rates ?? undefined,
        deviceBus: device.device_bus ?? undefined,
        isHardware: device.is_hardware
      });
    });

    return options;
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
  let dacPassthroughTooltipOverrideKey = $derived(
    dacPassthroughDisabled
      ? 'settings.audio.dacPassthroughDisabledDesc'
      : null
  );
  let gaplessDisabled = $derived(
    selectedBackend === 'ALSA Direct' || streamingOnly || dacPassthrough
  );
  let gaplessDisabledReasonKey = $derived(
    selectedBackend === 'ALSA Direct'
      ? 'settings.playback.gaplessDisabledAlsa'
      : dacPassthrough
        ? 'settings.playback.gaplessDisabledDac'
        : streamingOnly
          ? 'settings.playback.gaplessDisabledStreaming'
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
  let useSystemTitleBar = $state(getUseSystemTitleBar());

  // Immersive default view
  const IMMERSIVE_VIEW_KEYS = [
    'remember', 'coverflow', 'static', 'vinyl', 'visualizer',
    'lyrics-focus', 'queue-focus',
    'split-lyrics', 'split-trackInfo', 'split-suggestions', 'split-queue'
  ] as const;
  let immersiveDefaultView = $state(
    getUserItem('qbz-immersive-default-view') || 'remember'
  );

  function getImmersiveViewOptions(): string[] {
    return IMMERSIVE_VIEW_KEYS.map(key => $t(`settings.appearance.immersiveViews.${key}`));
  }

  function getImmersiveViewDisplayValue(): string {
    return $t(`settings.appearance.immersiveViews.${immersiveDefaultView}`);
  }

  function handleImmersiveViewChange(displayValue: string) {
    const options = getImmersiveViewOptions();
    const index = options.indexOf(displayValue);
    if (index >= 0) {
      const key = IMMERSIVE_VIEW_KEYS[index];
      immersiveDefaultView = key;
      setUserItem('qbz-immersive-default-view', key);
    }
  }

  // Startup page setting
  const STARTUP_PAGE_KEYS = ['home', 'last-view'] as const;
  let startupPage = $state(
    getUserItem('qbz-startup-page') || 'home'
  );

  function getStartupPageOptions(): string[] {
    return STARTUP_PAGE_KEYS.map(key => $t(`settings.appearance.startupPages.${key}`));
  }

  function getStartupPageDisplayValue(): string {
    return $t(`settings.appearance.startupPages.${startupPage}`);
  }

  function handleStartupPageChange(displayValue: string) {
    const options = getStartupPageOptions();
    const index = options.indexOf(displayValue);
    if (index >= 0) {
      const key = STARTUP_PAGE_KEYS[index];
      startupPage = key;
      setUserItem('qbz-startup-page', key);
    }
  }

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

  // MusicBrainz integration state
  let musicbrainzEnabled = $state(true);

  // ListenBrainz integration state
  let listenbrainzConnected = $state(false);
  let listenbrainzUsername = $state('');
  let listenbrainzEnabled = $state(true);
  let listenbrainzToken = $state('');
  let listenbrainzConnecting = $state(false);
  let showListenBrainzConfig = $state(false);

  // Remote control state
  let remoteControlStatus = $state<RemoteControlStatus | null>(null);
  let remoteControlEnabled = $state(false);
  let remoteControlPort = $state(8182);
  let remoteControlSecure = $state(false);
  let remoteControlToken = $state('');
  let remoteControlCertUrl = $state('');
  let remoteControlLoading = $state(false);
  let remoteControlQrOpen = $state(false);
  let remoteControlQrData = $state('');
  let remoteControlUrl = $state('');
  let showRemoteControlGuide = $state(false);
  let remoteControlCollapsed = $state(true);

  // Plex LAN POC state
  let plexEnabled = $state(getUserItem('qbz-plex-enabled') === 'true');
  let plexUiCollapsed = $state(getUserItem('qbz-plex-ui-collapsed') === 'true');
  let plexManualTokenMode = $state(false);
  let plexServerUrl = $state('http://127.0.0.1');
  let plexBaseUrl = $state(getUserItem('qbz-plex-poc-base-url') || 'http://127.0.0.1:32400');
  let plexToken = $state(getUserItem('qbz-plex-poc-token') || '');
  let plexMetadataWriteEnabled = $state(getUserItem('qbz-plex-poc-metadata-write-enabled') === 'true');
  let plexSections = $state<PlexMusicSection[]>([]);
  let plexTracks = $state<PlexTrack[]>([]);
  let plexSectionTrackCounts = $state<Record<string, number>>({});
  let plexSelectedSectionKeys = $state<string[]>([]);
  let plexStatusKey = $state('settings.integrations.plexStatusIdle');
  let plexStatusValues = $state<Record<string, string | number>>({});
  let plexBusy = $state(false);
  let plexLastError = $state('');
  let plexAuthBusy = $state(false);
  let plexAuthPinId = $state<number | null>(null);
  let plexAuthCode = $state('');
  let plexAuthUrl = $state('');
  let plexAuthClientId = $state(getUserItem('qbz-plex-poc-client-id') || '');
  let plexAuthPollTimer: ReturnType<typeof setInterval> | null = null;

  const PLEX_ENABLED_KEY = 'qbz-plex-enabled';
  const PLEX_UI_COLLAPSED_KEY = 'qbz-plex-ui-collapsed';
  const PLEX_CACHE_SELECTED_SECTIONS_KEY = 'qbz-plex-poc-selected-sections';
  const PLEX_CACHE_SELECTED_SECTION_KEY = 'qbz-plex-poc-selected-section';
  const PLEX_CACHE_SERVER_ID_KEY = 'qbz-plex-poc-machine-id';
  const PLEX_CLIENT_ID_KEY = 'qbz-plex-poc-client-id';
  const PLEX_METADATA_WRITE_KEY = 'qbz-plex-poc-metadata-write-enabled';

  // Qobuz Link Handler state
  let qobuzLinkHandlerEnabled = $state(false);
  let qobuzLinkHandlerBusy = $state(false);

  async function handleQobuzLinkHandlerToggle(enabled: boolean) {
    qobuzLinkHandlerBusy = true;
    try {
      if (enabled) {
        await invoke('v2_register_qobuzapp_handler');
      } else {
        await invoke('v2_deregister_qobuzapp_handler');
      }
      qobuzLinkHandlerEnabled = enabled;
    } catch (err) {
      console.error('Failed to toggle qobuzapp handler:', err);
      showToast(get(t)('settings.integrations.qobuzLinkHandlerError'), 'error');
    } finally {
      qobuzLinkHandlerBusy = false;
    }
  }

  // Load saved settings on mount
  onMount(() => {
    // Load theme
    const savedTheme = localStorage.getItem('qbz-theme') || '';
    theme = themeReverseMap[savedTheme] || 'Dark';
    applyTheme(savedTheme);

    // Load font
    const savedFont = localStorage.getItem('qbz-font-family') || '';
    selectedFont = fontReverseMap[savedFont] || 'LINE Seed JP';
    applyFont(savedFont);

    // Load streaming quality preference
    const savedQuality = getUserItem('qbz-streaming-quality');
    if (savedQuality) {
      streamingQuality = savedQuality;
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
    const savedFetchArtistImages = getUserItem('qbz-fetch-artist-images');
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

    // Load MusicBrainz cache stats
    loadMusicBrainzCacheStats();

    // Load vector store stats
    loadVectorStoreStats();

    // Load artwork cache stats
    loadArtworkCacheStats();

    // Load audio devices first (includes PipeWire sinks), then settings
    // Also load backends and ALSA plugins
    Promise.all([
      loadAudioDevices(),
      loadBackends(),
      loadAlsaPlugins()
    ]).then(() => loadAudioSettings());

    // Load Last.fm state
    loadLastfmState();

    // Load MusicBrainz state
    loadMusicBrainzState();

    // Load ListenBrainz state
    loadListenBrainzState();

    // Load remote control status
    loadRemoteControlStatus();

    // Check qobuzapp:// link handler registration
    invoke<boolean>('v2_check_qobuzapp_handler')
      .then((registered) => { qobuzLinkHandlerEnabled = registered; })
      .catch((err) => { console.warn('Could not check qobuzapp handler:', err); });

    // Warm-start Plex panel from local cache and refresh in background
    hydratePlexAddressFieldsFromBaseUrl();
    if (plexEnabled) {
      void loadPlexCachedState();
      void refreshPlexInBackground();
    }

    // Load notification preferences
    loadToastsPreference();
    toastsEnabled = getToastsEnabled();
    loadSystemNotificationsPreference();
    systemNotificationsEnabled = getSystemNotificationsEnabled();

    // Load playback preferences
    loadPlaybackPreferences();

    // Load tray settings
    loadTraySettings();

    // Initialize updates preferences/version state
    initUpdatesStore();
    const unsubscribeUpdates = subscribeUpdates(() => {
      updatePreferences = getUpdatePreferences();
      updatesCurrentVersion = getUpdatesCurrentVersion();
    });

    // Detect if running in Flatpak
    loadFlatpakStatus();

    // Check for legacy cached files
    checkLegacyCachedFiles();

    // Load developer settings
    invoke('v2_get_developer_settings').then((settings: any) => {
      forceDmabuf = settings.force_dmabuf;
    }).catch(() => {});

    // Initialize verbose log capture state (runtime only, not persisted)
    verboseLogCapture = isVerboseCaptureEnabled();

    // Load graphics settings
    invoke('v2_get_graphics_settings').then((settings: any) => {
      forceX11 = settings.force_x11;
      gdkScale = settings.gdk_scale || '';
      gdkDpiScale = settings.gdk_dpi_scale || '';
      hardwareAcceleration = settings.hardware_acceleration;
    }).catch(() => {});

    // Load graphics startup status (for fallback warning)
    invoke('v2_get_graphics_startup_status').then((status: any) => {
      graphicsUsingFallback = status.using_fallback;
      graphicsIsWayland = status.is_wayland;
      graphicsHasNvidia = status.has_nvidia;
      graphicsHwAccelEnabled = status.hardware_accel_enabled;
    }).catch(() => {});

    // Subscribe to offline state changes
    const unsubscribeOffline = subscribeOffline(() => {
      offlineStatus = getOfflineStatus();
      offlineSettings = getOfflineSettings();
    });

    // Subscribe to title bar state changes
    const unsubscribeTitleBar = subscribeTitleBar(() => {
      hideTitleBar = getHideTitleBar();
      useSystemTitleBar = getUseSystemTitleBar();
    });

    // Subscribe to blacklist state changes
    const unsubscribeBlacklist = subscribeBlacklist(() => {
      blacklistCount = getBlacklistCount();
      blacklistEnabled = isBlacklistEnabled();
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
      if (plexAuthPollTimer) {
        clearInterval(plexAuthPollTimer);
      }
      unsubscribeOffline();
      unsubscribeZoom();
      unsubscribeTitleBar();
      unsubscribeUpdates();
      unsubscribeBlacklist();
      settingsViewEl?.removeEventListener('scroll', handleScroll);
    };
  });

  
  async function loadLastfmState() {
    try {
      // Check if embedded (build-time) credentials are available
      hasEmbeddedCredentials = await invoke<boolean>('v2_lastfm_has_embedded_credentials');

      // Load saved credentials from localStorage (for user-provided keys)
      const savedApiKey = getUserItem('qbz-lastfm-api-key');
      const savedApiSecret = getUserItem('qbz-lastfm-api-secret');
      const savedSessionKey = getUserItem('qbz-lastfm-session-key');
      const savedUsername = getUserItem('qbz-lastfm-username');
      const savedScrobbling = getUserItem('qbz-lastfm-scrobbling');

      // If we have user-provided credentials, set them
      if (savedApiKey && savedApiSecret) {
        lastfmApiKey = savedApiKey;
        lastfmApiSecret = savedApiSecret;
        await invoke('v2_lastfm_set_credentials', {
          apiKey: savedApiKey,
          apiSecret: savedApiSecret
        });
      }

      // Restore session if available
      if (savedSessionKey && savedUsername) {
        await invoke('v2_lastfm_set_session', { sessionKey: savedSessionKey });
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
        setUserItem('qbz-lastfm-api-key', lastfmApiKey);
        setUserItem('qbz-lastfm-api-secret', lastfmApiSecret);
        await invoke('v2_lastfm_set_credentials', {
          apiKey: lastfmApiKey,
          apiSecret: lastfmApiSecret
        });
      }

      // Get auth URL (V2 stores token internally)
      const url = await invoke<string>('v2_lastfm_get_auth_url');
      lastfmAuthToken = 'pending'; // V2 stores token internally, just mark as pending

      // Open browser for authorization using Tauri's native opener
      try {
        await invoke('v2_lastfm_open_auth_url', { url });
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
      // V2 uses internally stored token
      const session = await invoke<{ name: string; key: string }>('v2_lastfm_complete_auth');

      lastfmConnected = true;
      lastfmUsername = session.name;
      showLastfmConfig = false;
      lastfmAuthToken = '';

      // Save session
      setUserItem('qbz-lastfm-session-key', session.key);
      setUserItem('qbz-lastfm-username', session.name);
    } catch (err) {
      console.error('Failed to complete Last.fm auth:', err);
      alert(`Authorization failed: ${err}`);
    } finally {
      lastfmConnecting = false;
    }
  }

  async function handleLastfmDisconnect() {
    try {
      await invoke('v2_lastfm_disconnect');
      lastfmConnected = false;
      lastfmUsername = '';

      // Clear saved session
      removeUserItem('qbz-lastfm-session-key');
      removeUserItem('qbz-lastfm-username');
    } catch (err) {
      console.error('Failed to disconnect Last.fm:', err);
    }
  }

  function handleScrobblingChange(enabled: boolean) {
    scrobbling = enabled;
    setUserItem('qbz-lastfm-scrobbling', String(enabled));
  }

  async function loadMusicBrainzState() {
    try {
      musicbrainzEnabled = await invoke<boolean>('v2_musicbrainz_is_enabled');
    } catch (err) {
      console.error('Failed to load MusicBrainz state:', err);
    }
  }

  async function handleMusicBrainzChange(enabled: boolean) {
    try {
      await invoke('v2_musicbrainz_set_enabled', { enabled });
      musicbrainzEnabled = enabled;
    } catch (err) {
      console.error('Failed to update MusicBrainz setting:', err);
    }
  }

  // ListenBrainz functions
  async function loadListenBrainzState() {
    try {
      const status = await invoke<{
        connected: boolean;
        userName: string | null;
        enabled: boolean;
      }>('v2_listenbrainz_get_status');
      listenbrainzConnected = status.connected;
      listenbrainzUsername = status.userName || '';
      listenbrainzEnabled = status.enabled;
    } catch (err) {
      console.error('Failed to load ListenBrainz state:', err);
    }
  }

  async function handleListenBrainzConnect() {
    if (!listenbrainzToken.trim()) {
      showListenBrainzConfig = true;
      return;
    }

    listenbrainzConnecting = true;
    try {
      const userInfo = await invoke<{ user_name: string }>('v2_listenbrainz_connect', {
        token: listenbrainzToken.trim()
      });
      listenbrainzConnected = true;
      listenbrainzUsername = userInfo.user_name;
      listenbrainzToken = '';
      showListenBrainzConfig = false;
    } catch (err) {
      console.error('Failed to connect to ListenBrainz:', err);
      alert(`ListenBrainz error: ${err}`);
    } finally {
      listenbrainzConnecting = false;
    }
  }

  async function handleListenBrainzDisconnect() {
    try {
      await invoke('v2_listenbrainz_disconnect');
      listenbrainzConnected = false;
      listenbrainzUsername = '';
    } catch (err) {
      console.error('Failed to disconnect ListenBrainz:', err);
    }
  }

  async function handleListenBrainzEnabledChange(enabled: boolean) {
    try {
      await invoke('v2_listenbrainz_set_enabled', { enabled });
      listenbrainzEnabled = enabled;
    } catch (err) {
      console.error('Failed to update ListenBrainz setting:', err);
    }
  }

  function normalizePlexServerUrl(value: string): string {
    const trimmed = value.trim();
    if (!trimmed) return '';
    return /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`;
  }

  function isPrivateIpv4(hostname: string): boolean {
    if (!/^\d+\.\d+\.\d+\.\d+$/.test(hostname)) return false;
    const octets = hostname.split('.').map(Number);
    if (octets.some((octet) => Number.isNaN(octet) || octet < 0 || octet > 255)) return false;
    if (octets[0] === 10) return true;
    if (octets[0] === 127) return true;
    if (octets[0] === 192 && octets[1] === 168) return true;
    if (octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31) return true;
    return false;
  }

  function isLocalPlexAddress(urlInput: string): boolean {
    const normalized = normalizePlexServerUrl(urlInput);
    if (!normalized) return false;
    try {
      const parsed = new URL(normalized);
      const host = parsed.hostname.toLowerCase();
      if (host === 'localhost' || host === '::1') return true;
      if (host.endsWith('.local') || host.endsWith('.lan')) return true;
      if (!host.includes('.')) return true;
      return isPrivateIpv4(host);
    } catch {
      return false;
    }
  }

  function resolvePlexBaseUrl(): string {
    const normalizedUrl = normalizePlexServerUrl(plexServerUrl);
    if (!normalizedUrl) return '';
    try {
      const parsed = new URL(normalizedUrl);
      if (!['http:', 'https:'].includes(parsed.protocol)) return '';
      if (!parsed.port) {
        parsed.port = '32400';
      }
      return `${parsed.protocol}//${parsed.host}`;
    } catch {
      return '';
    }
  }

  function hydratePlexAddressFieldsFromBaseUrl() {
    try {
      const parsed = new URL(plexBaseUrl || 'http://127.0.0.1:32400');
      plexServerUrl = `${parsed.protocol}//${parsed.host}`;
    } catch {
      plexServerUrl = 'http://127.0.0.1';
    }
  }

  function canUsePlexRequests(): boolean {
    return plexEnabled && isLocalPlexAddress(plexServerUrl) && !!resolvePlexBaseUrl() && !!plexToken.trim();
  }

  function persistPlexConfig() {
    plexBaseUrl = resolvePlexBaseUrl();
    setUserItem('qbz-plex-poc-base-url', plexBaseUrl);
    setUserItem('qbz-plex-poc-token', plexToken.trim());
  }

  function persistPlexSelectedSections() {
    setUserItem(PLEX_CACHE_SELECTED_SECTIONS_KEY, JSON.stringify(plexSelectedSectionKeys));
    if (plexSelectedSectionKeys.length === 1) {
      setUserItem(PLEX_CACHE_SELECTED_SECTION_KEY, plexSelectedSectionKeys[0]);
    } else {
      removeUserItem(PLEX_CACHE_SELECTED_SECTION_KEY);
    }
  }

  function readPersistedPlexSelectedSections(): string[] {
    const raw = getUserItem(PLEX_CACHE_SELECTED_SECTIONS_KEY);
    if (raw) {
      try {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
          return parsed.filter((item): item is string => typeof item === 'string' && item.trim().length > 0);
        }
      } catch (error) {
        console.warn('Failed parsing persisted Plex section keys:', error);
      }
    }
    const legacySingle = getUserItem(PLEX_CACHE_SELECTED_SECTION_KEY);
    return legacySingle ? [legacySingle] : [];
  }

  function handlePlexEnabledToggle(enabled: boolean) {
    plexEnabled = enabled;
    setUserItem(PLEX_ENABLED_KEY, enabled ? 'true' : 'false');
    if (!enabled) {
      plexStatusKey = 'settings.integrations.plexStatusDisabled';
      plexStatusValues = {};
      return;
    }
    plexStatusKey = 'settings.integrations.plexStatusIdle';
    plexStatusValues = {};
    void loadPlexCachedState();
    void refreshPlexInBackground();
  }

  function togglePlexUiCollapsed() {
    plexUiCollapsed = !plexUiCollapsed;
    setUserItem(PLEX_UI_COLLAPSED_KEY, plexUiCollapsed ? 'true' : 'false');
  }

  function handlePlexMetadataWriteToggle(enabled: boolean) {
    plexMetadataWriteEnabled = enabled;
    setUserItem(PLEX_METADATA_WRITE_KEY, enabled ? 'true' : 'false');
  }

  function ensurePlexClientId(): string {
    if (plexAuthClientId) return plexAuthClientId;
    const generated = (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function')
      ? `qbz-${crypto.randomUUID()}`
      : `qbz-${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`;
    plexAuthClientId = generated;
    setUserItem(PLEX_CLIENT_ID_KEY, generated);
    return generated;
  }

  async function handlePlexConnectEasy() {
    if (!plexEnabled || plexAuthBusy || !isLocalPlexAddress(plexServerUrl) || !resolvePlexBaseUrl()) return;
    plexAuthBusy = true;
    plexLastError = '';
    persistPlexConfig();
    try {
      const clientIdentifier = ensurePlexClientId();
      const pin = await invoke<PlexPinStartResult>('v2_plex_auth_pin_start', { clientIdentifier });
      plexAuthPinId = pin.pinId;
      plexAuthCode = pin.code;
      plexAuthUrl = pin.authUrl;
      plexStatusKey = 'settings.integrations.plexStatusAuthPending';
      plexStatusValues = { code: pin.code };

      if (plexAuthPollTimer) {
        clearInterval(plexAuthPollTimer);
      }

      plexAuthPollTimer = setInterval(async () => {
        if (!plexAuthPinId) return;
        try {
          const check = await invoke<PlexPinCheckResult>('v2_plex_auth_pin_check', {
            clientIdentifier,
            pinId: plexAuthPinId,
            code: plexAuthCode || null
          });

          if (check.authorized && check.authToken) {
            plexToken = check.authToken;
            persistPlexConfig();
            plexStatusKey = 'settings.integrations.plexStatusAuthConnected';
            plexStatusValues = {};
            if (plexAuthPollTimer) {
              clearInterval(plexAuthPollTimer);
              plexAuthPollTimer = null;
            }
            plexAuthPinId = null;
            plexAuthCode = '';
            void runPlexAutoSetup();
          } else if (check.expired) {
            plexStatusKey = 'settings.integrations.plexStatusAuthExpired';
            plexStatusValues = {};
            if (plexAuthPollTimer) {
              clearInterval(plexAuthPollTimer);
              plexAuthPollTimer = null;
            }
            plexAuthPinId = null;
            plexAuthCode = '';
          }
        } catch (error) {
          console.warn('Plex auth polling failed:', error);
        }
      }, 2500);
    } catch (error) {
      console.error('Failed starting Plex easy connect:', error);
      setPlexError(error);
    } finally {
      plexAuthBusy = false;
    }
  }

  function handleOpenPlexAuthUrl() {
    if (!plexAuthUrl) return;
    invoke('v2_plex_open_auth_url', { url: plexAuthUrl }).catch((error) => {
      console.error('Failed opening Plex auth URL:', error);
      setPlexError(error);
    });
  }

  async function handleCopyPlexCode() {
    if (!plexAuthCode) return;
    try {
      await copyToClipboard(plexAuthCode);
      showToast(get(t)('settings.integrations.plexCodeCopied'), 'success');
    } catch (error) {
      console.error('Failed copying Plex code:', error);
      showToast(get(t)('settings.integrations.plexCodeCopyFailed'), 'error');
    }
  }

  async function handlePlexTokenBlur() {
    persistPlexConfig();
    if (canUsePlexRequests()) {
      await runPlexAutoSetup();
    }
  }

  function setPlexError(error: unknown) {
    const message = String(error);
    plexLastError = message;
    plexStatusKey = 'settings.integrations.plexStatusError';
    plexStatusValues = { error: message };
  }

  async function handlePlexDisconnect() {
    const confirmed = await ask(get(t)('settings.integrations.plexDisconnectConfirmDesc'), {
      title: get(t)('settings.integrations.plexDisconnectConfirmTitle'),
      kind: 'warning',
      okLabel: get(t)('settings.integrations.plexDisconnectConfirmOk'),
      cancelLabel: get(t)('actions.cancel')
    });
    if (!confirmed) return;

    if (plexAuthPollTimer) {
      clearInterval(plexAuthPollTimer);
      plexAuthPollTimer = null;
    }

    plexAuthPinId = null;
    plexAuthCode = '';
    plexAuthUrl = '';
    plexAuthBusy = false;
    plexToken = '';
    plexSections = [];
    plexTracks = [];
    plexSelectedSectionKeys = [];
    plexStatusKey = 'settings.integrations.plexStatusDisconnected';
    plexStatusValues = {};
    plexLastError = '';

    removeUserItem('qbz-plex-poc-token');
    removeUserItem(PLEX_CACHE_SELECTED_SECTIONS_KEY);
    removeUserItem(PLEX_CACHE_SELECTED_SECTION_KEY);
    removeUserItem(PLEX_CACHE_SERVER_ID_KEY);

    try {
      await invoke('v2_plex_cache_clear');
    } catch (error) {
      console.warn('Failed clearing Plex cache:', error);
    }
  }

  async function handlePlexClearCache() {
    const confirmed = await ask(get(t)('settings.integrations.plexClearCacheConfirmDesc'), {
      title: get(t)('settings.integrations.plexClearCacheConfirmTitle'),
      kind: 'warning',
      okLabel: get(t)('settings.integrations.plexClearCacheConfirmOk'),
      cancelLabel: get(t)('actions.cancel')
    });
    if (!confirmed) return;

    try {
      await invoke('v2_plex_cache_clear');
      plexTracks = [];
      plexStatusKey = 'settings.integrations.plexStatusCacheCleared';
      plexStatusValues = {};
      plexLastError = '';
      removeUserItem(PLEX_CACHE_SERVER_ID_KEY);
    } catch (error) {
      console.error('Failed clearing Plex cache:', error);
      setPlexError(error);
    }
  }

  async function loadPlexCachedState() {
    if (!plexEnabled) return;
    try {
      const cachedSections = await invoke<PlexMusicSection[]>('v2_plex_cache_get_sections');
      if (Array.isArray(cachedSections) && cachedSections.length > 0) {
        plexSections = cachedSections;
      }

      const persistedSections = readPersistedPlexSelectedSections();
      if (persistedSections.length > 0) {
        plexSelectedSectionKeys = persistedSections;
      }

      const cachedTracks = await invoke<PlexTrack[]>('v2_plex_cache_get_tracks', {
        sectionKey: null
      });
      if (Array.isArray(cachedTracks) && cachedTracks.length > 0) {
        plexTracks = cachedTracks;
        plexStatusKey = 'settings.integrations.plexStatusCacheLoaded';
        plexStatusValues = { count: cachedTracks.length };
      }
    } catch (error) {
      console.warn('Failed to load Plex cached state:', error);
    }
  }

  async function refreshPlexInBackground() {
    if (!canUsePlexRequests()) return;
    await runPlexAutoSetup();
  }

  async function handlePlexPing(): Promise<boolean> {
    if (!canUsePlexRequests()) return false;
    plexBusy = true;
    plexLastError = '';
    persistPlexConfig();
    try {
      const info = await invoke<PlexServerInfo>('v2_plex_ping', {
        baseUrl: plexBaseUrl.trim(),
        token: plexToken.trim()
      });
      if (info.machineIdentifier) {
        setUserItem(PLEX_CACHE_SERVER_ID_KEY, info.machineIdentifier);
      }
      plexStatusKey = 'settings.integrations.plexStatusConnected';
      plexStatusValues = {
        server: info.friendlyName || info.machineIdentifier || 'Plex',
        version: info.version || '?'
      };
      return true;
    } catch (error) {
      console.error('Failed Plex ping:', error);
      setPlexError(error);
      return false;
    } finally {
      plexBusy = false;
    }
  }

  async function syncSelectedPlexLibraries() {
    if (!canUsePlexRequests()) return;
    plexBusy = true;
    plexLastError = '';
    try {
      const serverId = getUserItem(PLEX_CACHE_SERVER_ID_KEY) || null;
      await invoke('v2_plex_cache_clear');

      if (plexSections.length > 0) {
        await invoke<number>('v2_plex_cache_save_sections', {
          serverId,
          sections: plexSections
        });
      }

      if (plexSelectedSectionKeys.length === 0) {
        plexTracks = [];
        plexStatusKey = 'settings.integrations.plexStatusTracksLoaded';
        plexStatusValues = { count: 0 };
        return;
      }

      let totalCount = 0;
      const sectionCounts: Record<string, number> = {};
      for (const sectionKey of plexSelectedSectionKeys) {
        const sectionTracks = await invoke<PlexTrack[]>('v2_plex_get_section_tracks', {
          baseUrl: plexBaseUrl.trim(),
          token: plexToken.trim(),
          sectionKey
        });
        const count = sectionTracks.length;
        sectionCounts[sectionKey] = count;
        totalCount += count;
        await invoke<number>('v2_plex_cache_save_tracks', {
          serverId,
          sectionKey,
          tracks: sectionTracks
        });
      }

      plexSectionTrackCounts = { ...plexSectionTrackCounts, ...sectionCounts };
      plexTracks = await invoke<PlexTrack[]>('v2_plex_cache_get_tracks', { sectionKey: null });
      plexStatusKey = 'settings.integrations.plexStatusTracksLoaded';
      plexStatusValues = { count: totalCount };
    } catch (error) {
      console.error('Failed syncing selected Plex libraries:', error);
      setPlexError(error);
    } finally {
      plexBusy = false;
    }
  }

  async function handlePlexLoadSections(options: { autoSyncSelected?: boolean } = {}) {
    if (!canUsePlexRequests()) return;
    plexBusy = true;
    plexLastError = '';
    persistPlexConfig();
    try {
      const sections = await invoke<PlexMusicSection[]>('v2_plex_get_music_sections', {
        baseUrl: plexBaseUrl.trim(),
        token: plexToken.trim()
      });
      plexSections = sections;
      await invoke<number>('v2_plex_cache_save_sections', {
        serverId: getUserItem(PLEX_CACHE_SERVER_ID_KEY) || null,
        sections
      });

      const available = new Set(sections.map((section) => section.key));
      const persisted = readPersistedPlexSelectedSections().filter((key) => available.has(key));
      plexSelectedSectionKeys = persisted.length > 0 ? persisted : sections.map((section) => section.key);
      persistPlexSelectedSections();

      plexStatusKey = 'settings.integrations.plexStatusSectionsLoaded';
      plexStatusValues = { count: sections.length };

      if (options.autoSyncSelected !== false) {
        await syncSelectedPlexLibraries();
      }
    } catch (error) {
      console.error('Failed loading Plex sections:', error);
      setPlexError(error);
    } finally {
      plexBusy = false;
    }
  }

  async function runPlexAutoSetup() {
    const connected = await handlePlexPing();
    if (!connected) return;
    await handlePlexLoadSections({ autoSyncSelected: true });
  }

  async function handlePlexLibraryToggle(sectionKey: string, checked: boolean) {
    const current = new Set(plexSelectedSectionKeys);
    if (checked) {
      current.add(sectionKey);
    } else {
      current.delete(sectionKey);
      const nextCounts = { ...plexSectionTrackCounts };
      delete nextCounts[sectionKey];
      plexSectionTrackCounts = nextCounts;
    }
    plexSelectedSectionKeys = plexSections
      .map((section) => section.key)
      .filter((key) => current.has(key));
    persistPlexSelectedSections();
    await syncSelectedPlexLibraries();
  }

  async function loadRemoteControlStatus() {
    try {
      const status = await invoke<RemoteControlStatus>('v2_remote_control_get_status');
      remoteControlStatus = status;
      remoteControlEnabled = status.enabled;
      remoteControlPort = status.port;
      remoteControlSecure = status.secure;
      remoteControlUrl = status.localUrl;
      remoteControlToken = status.token;
      remoteControlCertUrl = status.certUrl ?? '';
    } catch (err) {
      console.error('Failed to load remote control status:', err);
    }
  }

  async function handleRemoteControlToggle(enabled: boolean) {
    remoteControlLoading = true;
    try {
      const status = await invoke<RemoteControlStatus>('v2_remote_control_set_enabled', { enabled });
      remoteControlStatus = status;
      remoteControlEnabled = status.enabled;
      remoteControlPort = status.port;
      remoteControlSecure = status.secure;
      remoteControlUrl = status.localUrl;
      remoteControlToken = status.token;
      remoteControlCertUrl = status.certUrl ?? '';
      if (!enabled) {
        remoteControlQrOpen = false;
      }
    } catch (err) {
      console.error('Failed to update remote control enabled state:', err);
    } finally {
      remoteControlLoading = false;
    }
  }

  async function handleRemoteControlPortChange(value: number) {
    if (!Number.isFinite(value)) return;
    remoteControlLoading = true;
    try {
      const status = await invoke<RemoteControlStatus>('v2_remote_control_set_port', { port: value });
      remoteControlStatus = status;
      remoteControlEnabled = status.enabled;
      remoteControlPort = status.port;
      remoteControlSecure = status.secure;
      remoteControlUrl = status.localUrl;
      remoteControlToken = status.token;
      remoteControlCertUrl = status.certUrl ?? '';
      if (remoteControlQrOpen) {
        await handleRemoteControlQrToggle(true);
      }
    } catch (err) {
      console.error('Failed to update remote control port:', err);
    } finally {
      remoteControlLoading = false;
    }
  }

  async function handleRemoteControlQrToggle(forceOpen = false) {
    if (remoteControlQrOpen && !forceOpen) {
      remoteControlQrOpen = false;
      return;
    }
    remoteControlLoading = true;
    try {
      const qr = await invoke<RemoteControlQr>('v2_remote_control_get_pairing_qr');
      remoteControlQrData = qr.qrDataUrl;
      remoteControlUrl = qr.url;
      remoteControlQrOpen = true;
    } catch (err) {
      console.error('Failed to load remote control QR:', err);
    } finally {
      remoteControlLoading = false;
    }
  }

  async function handleRemoteControlRegenerateToken() {
    const confirmed = await ask(
      get(t)('settings.integrations.remoteControlRegenerateDesc'),
      {
        title: get(t)('settings.integrations.remoteControlRegenerateTitle'),
        kind: 'warning',
        okLabel: get(t)('settings.integrations.remoteControlRegenerateConfirm'),
        cancelLabel: get(t)('actions.cancel')
      }
    );

    if (!confirmed) return;

    remoteControlLoading = true;
    try {
      const qr = await invoke<RemoteControlQr>('v2_remote_control_regenerate_token');
      remoteControlQrData = qr.qrDataUrl;
      remoteControlUrl = qr.url;
      remoteControlQrOpen = true;
      const status = await invoke<RemoteControlStatus>('v2_remote_control_get_status');
      remoteControlStatus = status;
      remoteControlEnabled = status.enabled;
      remoteControlPort = status.port;
      remoteControlSecure = status.secure;
      remoteControlToken = status.token;
      remoteControlCertUrl = status.certUrl ?? '';
    } catch (err) {
      console.error('Failed to regenerate remote control token:', err);
    } finally {
      remoteControlLoading = false;
    }
  }

  async function handleRemoteControlCopyToken() {
    if (!remoteControlToken) return;
    try {
      await copyToClipboard(remoteControlToken);
      showToast(get(t)('toast.copied'), 'success');
    } catch (err) {
      console.error('Failed to copy token:', err);
    }
  }

  async function handleRemoteControlCopyCert() {
    if (!remoteControlCertUrl) return;
    try {
      await copyToClipboard(remoteControlCertUrl);
      showToast(get(t)('toast.copied'), 'success');
    } catch (err) {
      console.error('Failed to copy certificate URL:', err);
    }
  }

  async function handleRemoteControlSecureChange(secure: boolean) {
    remoteControlLoading = true;
    try {
      const status = await invoke<RemoteControlStatus>('v2_remote_control_set_secure', { secure });
      remoteControlStatus = status;
      remoteControlEnabled = status.enabled;
      remoteControlPort = status.port;
      remoteControlSecure = status.secure;
      remoteControlUrl = status.localUrl;
      remoteControlToken = status.token;
      remoteControlCertUrl = status.certUrl ?? '';
      if (remoteControlQrOpen) {
        await handleRemoteControlQrToggle(true);
      }
    } catch (err) {
      console.error('Failed to update remote control secure mode:', err);
    } finally {
      remoteControlLoading = false;
    }
  }

  async function handleShowDownloadsChange(enabled: boolean) {
    try {
      await invoke('v2_set_show_downloads_in_library', { show: enabled });
      showQobuzDownloadsInLibrary = enabled;
      // Notify LocalLibraryView to refresh
      notifyDownloadSettingsChanged();
    } catch (e) {
      console.error('Failed to update show downloads setting:', e);
    }
  }

  async function handleQualityChange(quality: string) {
    const previousQuality = streamingQuality;
    streamingQuality = quality;
    setUserItem('qbz-streaming-quality', quality);

    // Clear playback cache when quality changes (issue #34)
    // This ensures cached tracks don't play at wrong quality
    // Important for users with hardware sample rate limitations
    if (previousQuality !== quality) {
      try {
        await invoke('v2_clear_cache');
        await loadCacheStats();
        showToast($t('settings.audio.qualityChangedCacheCleared'), 'success');
      } catch (err) {
        console.error('Failed to clear cache after quality change:', err);
      }
    }
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
        await invoke('v2_clear_artist_cache');
        console.log('Artist cache cleared after language change');
      } catch (error) {
        console.error('Failed to clear artist cache:', error);
      }
    } else {
      // 'Auto' - use browser locale, defaulting to 'en'
      const browserLocale = navigator.language.split('-')[0];
      const supportedLocale = ['en', 'es', 'fr', 'de'].includes(browserLocale) ? browserLocale : 'en';
      await setLocale(supportedLocale);
      // Clear the stored locale so it uses browser detection on next load
      localStorage.removeItem('qbz-locale');
      // Also clear artist cache
      try {
        await invoke('v2_clear_artist_cache');
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
    stream_first_track: boolean;
    stream_buffer_seconds: number;
    streaming_only: boolean;
    limit_quality_to_device: boolean;
    device_max_sample_rate: number | null;
    gapless_enabled: boolean;
    pw_force_bitperfect: boolean;
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
    supported_sample_rates: number[] | null;
    device_bus: string | null;  // "usb", "pci", "bluetooth", etc.
    is_hardware: boolean;
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

  /**
   * Reinitialize audio device and auto-resume playback if it was active.
   * The backend preserves playback position across reinit, so resume
   * will seek back to where the user was.
   */
  async function reinitAndResume(device: string | null): Promise<void> {
    const wasPlaying = getIsPlaying();
    // Reinit audio device (V2 only)
    await invoke('v2_reinit_audio_device', { device });
    if (wasPlaying) {
      // Small delay to let the new stream initialize
      await new Promise(r => setTimeout(r, 150));
      await invoke('v2_resume_playback');
    }
  }

  async function loadAudioDevices() {
    try {
      // Load PipeWire sinks - these have friendly descriptions already
      const sinks = await invoke<PipewireSink[]>('get_pipewire_sinks').catch(() => [] as PipewireSink[]);
      pipewireSinks = sinks;

      // Load hardware audio status
      const hwStatus = await invoke<HardwareAudioStatus>('v2_get_hardware_audio_status').catch(() => null);
      hardwareStatus = hwStatus;

      console.log('[Audio] PipeWire sinks:', sinks.map(s => ({ name: s.name, desc: s.description })));
      console.log('[Audio] Hardware status:', hwStatus);
    } catch (err) {
      console.error('Failed to load audio devices:', err);
    }
  }

  async function loadBackends() {
    try {
      const backends = await invoke<BackendInfo[]>('v2_get_available_backends');
      availableBackends = backends;
      console.log('[Audio] Available backends:', backends);
    } catch (err) {
      console.error('Failed to load backends:', err);
    }
  }

  async function loadAlsaPlugins() {
    try {
      const plugins = await invoke<AlsaPluginInfo[]>('v2_get_alsa_plugins');
      alsaPlugins = plugins;
      console.log('[Audio] ALSA plugins:', plugins);
    } catch (err) {
      console.error('Failed to load ALSA plugins:', err);
    }
  }

  async function loadBackendDevices(backendType: 'PipeWire' | 'Alsa' | 'Pulse') {
    isLoadingDevices = true;
    try {
      const devices = await invoke<AudioDevice[]>('v2_get_devices_for_backend', { backendType });
      backendDevices = devices;
      console.log(`[Audio] Devices for ${backendType}:`, devices);

      // Fetch the name of the current default device for this backend
      try {
        const name = await invoke<string | null>('v2_get_default_device_name', { backendType });
        defaultDeviceName = name;
      } catch {
        defaultDeviceName = null;
      }
    } catch (err) {
      console.error(`Failed to load devices for ${backendType}:`, err);
      backendDevices = [];
      defaultDeviceName = null;
    } finally {
      isLoadingDevices = false;
    }
  }

  async function handleRefreshDevices() {
    if (isLoadingDevices) return;
    const backendType = selectedBackend === 'ALSA Direct' ? 'Alsa' :
                        selectedBackend === 'PipeWire' ? 'PipeWire' :
                        selectedBackend === 'PulseAudio' ? 'Pulse' : null;
    if (backendType) {
      await loadBackendDevices(backendType);
    }
  }

  async function loadFlatpakStatus() {
    try {
      isFlatpak = await invoke<boolean>('v2_is_running_in_flatpak');
      if (isFlatpak) {
        flatpakHelpText = await invoke<string>('get_flatpak_help_text');
      }
    } catch (err) {
      console.error('Failed to check Flatpak status:', err);
    }
  }

  async function loadAudioSettings() {
    try {
      const settings = await invoke<AudioSettings>('v2_get_audio_settings');
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
      pwForceBitperfect = settings.pw_force_bitperfect;

      // Load backend and plugin settings
      if (settings.backend_type) {
        const backend = availableBackends.find(b => b.backend_type === settings.backend_type);
        const backendName = backend?.name ?? 'Auto';

        // TEST: Allow ALSA Direct to load for testing
        selectedBackend = backendName;
        // Load devices for selected backend
        await loadBackendDevices(settings.backend_type);

        // Set selected device from backend devices
        // IMPORTANT: Validate that saved device still exists in current enumeration
        // Device IDs like hw:X,0 can change between boots when USB devices are connected/disconnected
        if (settings.output_device) {
          const device = backendDevices.find(d => d.id === settings.output_device);
          if (device) {
            // Use backend-provided description if available (ALSA), otherwise translate
            outputDevice = (device.description && settings.backend_type === 'Alsa')
              ? device.description
              : (needsTranslation(device.name) ? getDevicePrettyName(device.name) : device.name);
          } else {
            // Saved device no longer exists - clear it from DB to prevent sync issues
            console.warn(`[Audio] Saved device '${settings.output_device}' not found in current enumeration. Resetting to System Default.`);
            outputDevice = 'System Default';
            try {
              await invoke('v2_set_audio_output_device', { device: null });
              console.log('[Audio] Cleared stale device from database');
            } catch (err) {
              console.error('[Audio] Failed to clear stale device:', err);
            }
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
      streamFirstTrack = settings.stream_first_track ?? false;
      streamBufferSeconds = settings.stream_buffer_seconds ?? 3;
      streamingOnly = settings.streaming_only ?? false;
      limitQualityToDevice = settings.limit_quality_to_device ?? false;
      gaplessPlayback = settings.gapless_enabled ?? true;

      // Load per-device sample rate limit
      const deviceId = settings.output_device ?? 'default';
      await loadDeviceSampleRateLimit(deviceId);
    } catch (err) {
      console.error('Failed to load audio settings:', err);
    }
  }

  async function handleOutputDeviceChange(description: string) {
    outputDevice = description;

    // Convert description back to device name for storage
    const deviceName = sinkDescriptionToName.get(description);
    const deviceToStore = description === 'System Default' ? null : (deviceName ?? null);

    // Try to find max sample rate from backendDevices if available
    // This enables quality limiting for PipeWire mode when possible
    const matchingDevice = backendDevices.find(d =>
      d.name === deviceName || d.description === description
    );
    const maxSampleRate = matchingDevice?.max_sample_rate ?? null;

    try {
      // Save the preference
      await invoke('v2_set_audio_output_device', { device: deviceToStore });
      // Store device's max sample rate for quality limiting
      await invoke('v2_set_audio_device_max_sample_rate', { rate: maxSampleRate });

      // Reinitialize audio with the selected device
      // CRITICAL: Pass the actual CPAL device name, not null
      // CPAL can now find this device because we're using CPAL names
      await reinitAndResume(deviceToStore);

      console.log('[Audio] Output device changed:', description, '(device:', deviceName ?? 'default', ', max_rate:', maxSampleRate ?? 'unknown', ')');
    } catch (err) {
      console.error('[Audio] Failed to change audio output device:', err);
    }
  }

  async function handleExclusiveModeChange(enabled: boolean) {
    exclusiveMode = enabled;
    try {
      await invoke('v2_set_audio_exclusive_mode', { enabled });

      // Reinitialize audio with currently selected device
      const deviceName = getCurrentDeviceSinkName();
      await reinitAndResume(deviceName);
      console.log('[Audio] Exclusive mode changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change exclusive mode:', err);
    }
  }

  async function handleDacPassthroughChange(enabled: boolean) {
    dacPassthrough = enabled;

    // Gapless not compatible with DAC Passthrough
    if (enabled && gaplessPlayback) {
      gaplessPlayback = false;
      await invoke('v2_set_audio_gapless_enabled', { enabled: false });
      console.log('[Audio] Disabled gapless playback (not compatible with DAC Passthrough)');
    }

    // Disabling DAC Passthrough also disables PW force bit-perfect
    if (!enabled && pwForceBitperfect) {
      pwForceBitperfect = false;
      await invoke('v2_set_audio_pw_force_bitperfect', { enabled: false });
      console.log('[Audio] Disabled PW force bit-perfect (requires DAC Passthrough)');
    }

    try {
      await invoke('v2_set_audio_dac_passthrough', { enabled });

      // Reinitialize audio with currently selected device
      const deviceName = getCurrentDeviceSinkName();
      await reinitAndResume(deviceName);
      console.log('[Audio] DAC passthrough changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change DAC passthrough:', err);
    }
  }

  async function handlePwForceBitperfectChange(enabled: boolean) {
    pwForceBitperfect = enabled;

    // Auto-enable DAC Passthrough when turning on bit-perfect
    if (enabled && !dacPassthrough) {
      await handleDacPassthroughChange(true);
    }

    try {
      await invoke('v2_set_audio_pw_force_bitperfect', { enabled });

      const deviceName = getCurrentDeviceSinkName();
      await reinitAndResume(deviceName);
      console.log('[Audio] PW force bit-perfect changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change PW force bit-perfect:', err);
    }
  }

  async function handleBackendChange(backendName: string) {
    selectedBackend = backendName;

    // Map UI name to backend type
    const backend = availableBackends.find(b => b.name === backendName);
    const backendType = backendName === 'Auto' ? null : backend?.backend_type ?? null;

    // Auto-disable incompatible features
    // DAC Passthrough and PW force bit-perfect only work with PipeWire
    if (backendName !== 'PipeWire') {
      if (pwForceBitperfect) {
        pwForceBitperfect = false;
        await invoke('v2_set_audio_pw_force_bitperfect', { enabled: false });
        console.log('[Audio] Disabled PW force bit-perfect (only compatible with PipeWire)');
      }
      if (dacPassthrough) {
        dacPassthrough = false;
        await invoke('v2_set_audio_dac_passthrough', { enabled: false });
        console.log('[Audio] Disabled DAC Passthrough (only compatible with PipeWire)');
      }
    }

    // Exclusive mode only works with ALSA Direct
    if (backendName !== 'ALSA Direct') {
      if (exclusiveMode) {
        exclusiveMode = false;
        await invoke('v2_set_audio_exclusive_mode', { enabled: false });
        console.log('[Audio] Disabled exclusive mode (only compatible with ALSA Direct)');
      }
    }

    // Gapless not compatible with ALSA Direct
    if (backendName === 'ALSA Direct') {
      if (gaplessPlayback) {
        gaplessPlayback = false;
        await invoke('v2_set_audio_gapless_enabled', { enabled: false });
        console.log('[Audio] Disabled gapless playback (not compatible with ALSA Direct)');
      }
    }

    try {
      // Save backend preference
      await invoke('v2_set_audio_backend_type', { backendType });
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
      await invoke('v2_set_audio_output_device', { device: null });

      // Reinitialize audio - recreates stream with new backend.
      // Position and audio data are preserved so the user can resume.
      await reinitAndResume(null);
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
      await invoke('v2_set_audio_alsa_plugin', { plugin });
      console.log('[Audio] ALSA plugin changed:', pluginName, '(type:', plugin ?? 'none', ')');

      // Reinitialize audio if ALSA backend is active
      if (selectedBackend === 'ALSA Direct') {
        const deviceName = getCurrentDeviceSinkName();
        await reinitAndResume(deviceName);
      }
    } catch (err) {
      console.error('[Audio] Failed to change ALSA plugin:', err);
    }
  }

  async function handleAlsaHardwareVolumeChange(enabled: boolean) {
    alsaHardwareVolume = enabled;
    try {
      await invoke('v2_set_audio_alsa_hardware_volume', { enabled });
      console.log('[Audio] ALSA hardware volume changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change ALSA hardware volume:', err);
    }
  }

  async function handleStreamFirstTrackChange(enabled: boolean) {
    streamFirstTrack = enabled;
    try {
      await invoke('v2_set_audio_stream_first_track', { enabled });
      console.log('[Audio] Stream first track changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change stream first track:', err);
    }
  }

  async function handleStreamBufferSecondsChange(seconds: number) {
    // Clamp to valid range
    const clamped = Math.max(1, Math.min(10, Math.round(seconds)));
    streamBufferSeconds = clamped;
    try {
      await invoke('v2_set_audio_stream_buffer_seconds', { seconds: clamped });
      console.log('[Audio] Stream buffer seconds changed:', clamped);
    } catch (err) {
      console.error('[Audio] Failed to change stream buffer seconds:', err);
    }
  }

  async function handleStreamingOnlyChange(enabled: boolean) {
    streamingOnly = enabled;

    // Gapless not compatible with streaming-only
    if (enabled && gaplessPlayback) {
      gaplessPlayback = false;
      await invoke('v2_set_audio_gapless_enabled', { enabled: false });
      console.log('[Audio] Disabled gapless playback (not compatible with streaming-only)');
    }

    try {
      await invoke('v2_set_audio_streaming_only', { enabled });
      console.log('[Audio] Streaming-only mode changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change streaming-only mode:', err);
    }
  }

  async function handleLimitQualityToDeviceChange(enabled: boolean) {
    limitQualityToDevice = enabled;
    try {
      await invoke('v2_set_audio_limit_quality_to_device', { enabled });
      console.log('[Audio] Limit quality to device changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change limit quality to device:', err);
    }
  }

  async function handleDeviceMaxSampleRateChange(rate: number | null) {
    deviceMaxSampleRate = rate;
    // Get current device ID
    const device = deviceByDisplayName.get(outputDevice);
    const deviceId = outputDevice === 'System Default' ? 'default' : device?.id ?? 'default';

    try {
      await invoke('v2_set_device_sample_rate_limit', { deviceId, rate });
      console.log('[Audio] Device max sample rate changed:', deviceId, rate);
    } catch (err) {
      console.error('[Audio] Failed to change device max sample rate:', err);
    }
  }

  async function loadDeviceSampleRateLimit(deviceId: string) {
    try {
      const rate = await invoke<number | null>('v2_get_device_sample_rate_limit', { deviceId });
      deviceMaxSampleRate = rate;
      console.log('[Audio] Loaded sample rate limit for', deviceId, ':', rate);
    } catch (err) {
      console.error('[Audio] Failed to load device sample rate limit:', err);
      deviceMaxSampleRate = null;
    }
  }

  async function handleBackendDeviceChange(deviceName: string) {
    outputDevice = deviceName;

    // Get device ID from backendDevices using display name mapping
    const device = deviceByDisplayName.get(deviceName);
    const deviceId = deviceName === 'System Default' ? null : device?.id ?? null;
    const maxSampleRate = device?.max_sample_rate ?? null;

    // Warn if device lookup failed (possible sync issue between UI and deviceByDisplayName)
    if (deviceName !== 'System Default' && !device) {
      console.warn(`[Audio] Device lookup failed for '${deviceName}'. Available keys:`, Array.from(deviceByDisplayName.keys()));
    }

    try {
      await invoke('v2_set_audio_output_device', { device: deviceId });
      // Store device's max sample rate for quality limiting
      await invoke('v2_set_audio_device_max_sample_rate', { rate: maxSampleRate });
      // Load per-device sample rate limit for the new device
      await loadDeviceSampleRateLimit(deviceId ?? 'default');
      // Reinitialize audio - position and audio data preserved for resume.
      await reinitAndResume(deviceId);
      console.log('[Audio] Backend device changed:', deviceName, '(id:', deviceId ?? 'default', ', max_rate:', maxSampleRate ?? 'unknown', ')');
    } catch (err) {
      console.error('[Audio] Failed to change backend device:', err);
    }
  }

  async function handleGaplessPlaybackChange(enabled: boolean) {
    gaplessPlayback = enabled;
    try {
      await invoke('v2_set_audio_gapless_enabled', { enabled });
      console.log('[Audio] Gapless playback changed:', enabled);
    } catch (err) {
      console.error('[Audio] Failed to change gapless playback:', err);
    }
  }

  async function handleCrossfadeChange(value: number) {
    crossfade = value;

    // Auto-disable DAC passthrough if crossfade > 0
    if (value > 0 && dacPassthrough) {
      dacPassthrough = false;
      console.log('[Audio] Crossfade enabled: disabled DAC passthrough');
      try {
        await invoke('v2_set_audio_dac_passthrough', { enabled: false });

        // Reinitialize audio with currently selected device
        const deviceName = getCurrentDeviceSinkName();
        await reinitAndResume(deviceName);
      } catch (err) {
        console.error('[Audio] Failed to disable DAC passthrough:', err);
      }
    }
  }

  async function loadCacheStats() {
    try {
      cacheStats = await invoke<CacheStats>('v2_get_cache_stats');
    } catch (err) {
      console.error('Failed to load cache stats:', err);
    }
  }

  async function loadLyricsCacheStats() {
    try {
      const stats = await invoke<{ entries: number; sizeBytes: number }>('v2_lyrics_get_cache_stats');
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
      const settings = await invoke<{download_root: string, show_in_library: boolean}>('v2_get_download_settings');
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
      const settings = await invoke<TraySettings>('v2_get_tray_settings');
      enableTray = settings.enable_tray;
      minimizeToTray = settings.minimize_to_tray;
      closeToTray = settings.close_to_tray;
    } catch (err) {
      console.error('Failed to load tray settings:', err);
    }
  }

  async function handleEnableTrayChange(value: boolean) {
    try {
      await invoke('v2_set_enable_tray', { value });
      enableTray = value;
      showToast($t('settings.appearance.tray.enableTrayDesc'), 'info');
    } catch (err) {
      console.error('Failed to set enable tray:', err);
      showToast($t('toast.failedSaveTray'), 'error');
    }
  }

  async function handleMinimizeToTrayChange(value: boolean) {
    try {
      await invoke('v2_set_minimize_to_tray', { value });
      minimizeToTray = value;
    } catch (err) {
      console.error('Failed to set minimize to tray:', err);
      showToast($t('toast.failedSaveTray'), 'error');
    }
  }

  async function handleCloseToTrayChange(value: boolean) {
    try {
      await invoke('v2_set_close_to_tray', { value });
      closeToTray = value;
    } catch (err) {
      console.error('Failed to set close to tray:', err);
      showToast($t('toast.failedSaveTray'), 'error');
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
      showToast($t('toast.failedSaveAutoplay'), 'error');
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
      showToast($t('toast.failedSaveIconVisibility'), 'error');
    }
  }

  async function checkLegacyCachedFiles() {
    try {
      const result = await invoke<{has_legacy_files: boolean, total_tracks: number}>('v2_detect_legacy_cached_files');
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
      }>('v2_library_backfill_downloads');

      const message = `Repair complete!\n\nAdded: ${report.added_tracks}\nRepaired: ${report.repaired_tracks}\nSkipped: ${report.skipped_tracks}\nFailed: ${report.failed_tracks.length}`;

      showToast(message, 'success');

      // Trigger library reload
      notifyDownloadSettingsChanged();
    } catch (err) {
      console.error('Failed to repair downloads:', err);
      showToast($t('toast.failedRepairOffline', { values: { error: String(err) } }), 'error');
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

  async function handleOpenCacheFolder() {
    try {
      await invoke('v2_open_offline_cache_folder');
    } catch (err) {
      console.error('Failed to open cache folder:', err);
      showToast($t('toast.failedOpenCacheFolder'), 'error');
    }
  }

  async function handleClearCache() {
    if (isClearing) return;
    isClearing = true;
    try {
      await invoke('v2_clear_cache');
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

  async function handleClearMusicBrainzCache() {
    if (isClearingMusicBrainz) return;
    isClearingMusicBrainz = true;
    try {
      await invoke('v2_musicbrainz_clear_cache');
      console.log('MusicBrainz cache cleared');
      await loadMusicBrainzCacheStats();
    } catch (err) {
      console.error('Failed to clear MusicBrainz cache:', err);
    } finally {
      isClearingMusicBrainz = false;
    }
  }

  async function loadMusicBrainzCacheStats() {
    try {
      musicBrainzCacheStats = await invoke('v2_musicbrainz_get_cache_stats');
    } catch (err) {
      console.error('Failed to load MusicBrainz cache stats:', err);
      musicBrainzCacheStats = null;
    }
  }

  async function loadVectorStoreStats() {
    try {
      vectorStoreStats = await invoke('v2_get_vector_store_stats');
    } catch (err) {
      console.error('Failed to load vector store stats:', err);
      vectorStoreStats = null;
    }
  }

  async function handleClearVectorStore() {
    if (isClearingVectorStore) return;
    isClearingVectorStore = true;
    try {
      await invoke('v2_clear_vector_store');
      console.log('Artist vector store cleared');
      await loadVectorStoreStats();
    } catch (err) {
      console.error('Failed to clear vector store:', err);
    } finally {
      isClearingVectorStore = false;
    }
  }

  async function loadArtworkCacheStats() {
    try {
      artworkCacheStats = await invoke('v2_library_get_cache_stats');
    } catch (err) {
      console.error('Failed to load artwork cache stats:', err);
      artworkCacheStats = null;
    }
  }

  async function handleClearArtworkCache() {
    if (isClearingArtwork) return;
    isClearingArtwork = true;
    try {
      // Clear both legacy artwork cache and new thumbnails cache
      await invoke('v2_library_clear_artwork_cache');
      await invoke('v2_library_clear_thumbnails_cache');
      console.log('Artwork caches cleared');
      await loadArtworkCacheStats();
    } catch (err) {
      console.error('Failed to clear artwork cache:', err);
    } finally {
      isClearingArtwork = false;
    }
  }

  async function handleClearAllCaches() {
    if (isClearingAllCaches) return;
    isClearingAllCaches = true;
    try {
      // Clear all caches in parallel
      await Promise.all([
        invoke('v2_clear_cache'),
        clearLyricsCache(),
        invoke('v2_musicbrainz_clear_cache'),
        invoke('v2_clear_vector_store'),
        invoke('v2_library_clear_artwork_cache'),
        invoke('v2_library_clear_thumbnails_cache')
      ]);
      console.log('All caches cleared');
      // Reload all stats
      await Promise.all([
        loadCacheStats(),
        loadLyricsCacheStats(),
        loadMusicBrainzCacheStats(),
        loadVectorStoreStats(),
        loadArtworkCacheStats()
      ]);
    } catch (err) {
      console.error('Failed to clear all caches:', err);
    } finally {
      isClearingAllCaches = false;
    }
  }

  async function handleResetAudioSettings() {
    if (isResettingAudio) return;
    const confirmed = await ask($t('settings.audio.resetConfirmDesc'), {
      title: $t('settings.audio.resetConfirmTitle'),
      kind: 'warning',
    });
    if (!confirmed) return;
    isResettingAudio = true;
    try {
      await invoke('v2_stop_playback');
      await invoke('v2_reset_audio_settings');
      // Reinit audio device (V2 only)
      await invoke('v2_reinit_audio_device', { device: null });
      // Reset all audio UI state to defaults
      outputDevice = 'System Default';
      exclusiveMode = false;
      dacPassthrough = false;
      pwForceBitperfect = false;
      selectedBackend = 'Auto';
      selectedAlsaPlugin = 'hw (Direct Hardware)';
      alsaHardwareVolume = false;
      streamFirstTrack = false;
      streamBufferSeconds = 3;
      streamingOnly = false;
      limitQualityToDevice = false;
      // Reset playback UI state to defaults
      autoplayMode = 'continue';
      showContextIcon = false;
      gaplessPlayback = false;
      showToast($t('settings.audio.resetSuccess'), 'success');
    } catch (err) {
      console.error('Failed to reset audio settings:', err);
      showToast($t('settings.audio.resetError', { values: { error: String(err) } }), 'error');
    } finally {
      isResettingAudio = false;
    }
  }

  async function handleFactoryReset() {
    if (isFactoryResetting) return;
    const confirmed = await ask($t('settings.storage.factoryResetFinalConfirm'), {
      title: $t('settings.storage.factoryResetTitle'),
      kind: 'warning',
    });
    if (!confirmed) return;
    isFactoryResetting = true;
    try {
      await invoke('v2_factory_reset');
      onLogout?.();
    } catch (err) {
      console.error('Factory reset failed:', err);
      showToast($t('settings.storage.factoryResetError', { values: { error: String(err) } }), 'error');
      isFactoryResetting = false;
    }
  }

  async function handleForceX11Change(enabled: boolean) {
    try {
      await invoke('v2_set_force_x11', { enabled });
      forceX11 = enabled;
      showToast($t('settings.developer.restartRequired'), 'info');
    } catch (err) {
      console.error('Failed to set force_x11:', err);
      showToast(String(err), 'error');
    }
  }

  async function handleGdkScaleChange() {
    try {
      const value = gdkScale.trim() || null;
      await invoke('v2_set_gdk_scale', { value });
      showToast($t('settings.developer.restartRequired'), 'info');
    } catch (err) {
      console.error('Failed to set gdk_scale:', err);
      showToast(String(err), 'error');
    }
  }

  async function handleGdkDpiScaleChange() {
    try {
      const value = gdkDpiScale.trim() || null;
      await invoke('v2_set_gdk_dpi_scale', { value });
      showToast($t('settings.developer.restartRequired'), 'info');
    } catch (err) {
      console.error('Failed to set gdk_dpi_scale:', err);
      showToast(String(err), 'error');
    }
  }

  async function handleHardwareAccelerationChange(enabled: boolean) {
    try {
      await invoke('v2_set_hardware_acceleration', { enabled });
      hardwareAcceleration = enabled;
      showToast($t('settings.developer.restartRequired'), 'info');
    } catch (err) {
      console.error('Failed to set hardware_acceleration:', err);
      showToast(String(err), 'error');
    }
  }

  function normalizeScaleValue(value: string): string {
    return value.trim() || '';
  }

  function getActiveCompositionProfileId(): CompositionProfileId | null {
    const currentScale = normalizeScaleValue(gdkScale);
    const currentDpiScale = normalizeScaleValue(gdkDpiScale);
    for (const profile of compositionProfiles) {
      if (
        profile.forceX11 === forceX11
        && normalizeScaleValue(profile.gdkScale) === currentScale
        && normalizeScaleValue(profile.gdkDpiScale) === currentDpiScale
      ) {
        return profile.id;
      }
    }
    return null;
  }

  const activeCompositionProfileId = $derived(getActiveCompositionProfileId());

  async function applyCompositionProfile(profileId: CompositionProfileId) {
    const profile = compositionProfiles.find((candidate) => candidate.id === profileId);
    if (!profile) return;

    const previousForceX11 = forceX11;
    const previousGdkScale = gdkScale;
    const previousGdkDpiScale = gdkDpiScale;

    // Optimistic UI update so toggles/inputs reflect the selected profile immediately
    forceX11 = profile.forceX11;
    gdkScale = profile.gdkScale;
    gdkDpiScale = profile.gdkDpiScale;

    try {
      await invoke('v2_set_force_x11', { enabled: profile.forceX11 });
      await invoke('v2_set_gdk_scale', { value: profile.gdkScale || null });
      await invoke('v2_set_gdk_dpi_scale', { value: profile.gdkDpiScale || null });

      showToast(
        $t('settings.appearance.composition.profiles.applied', { values: { profile: $t(profile.labelKey) } }),
        'info'
      );
      showToast($t('settings.developer.restartRequired'), 'info');
    } catch (err) {
      // Roll back UI state if persistence fails
      forceX11 = previousForceX11;
      gdkScale = previousGdkScale;
      gdkDpiScale = previousGdkDpiScale;
      console.error('Failed to apply composition profile:', err);
      showToast(String(err), 'error');
    }
  }

  async function handleForceDmabufChange(enabled: boolean) {
    try {
      await invoke('v2_set_developer_force_dmabuf', { enabled });
      forceDmabuf = enabled;
      showToast($t('settings.developer.restartRequired'), 'info');
    } catch (err) {
      console.error('Failed to set force_dmabuf:', err);
      showToast(String(err), 'error');
    }
  }

  function handleVerboseLogCaptureChange(enabled: boolean) {
    if (enabled) {
      enableVerboseCapture();
    } else {
      disableVerboseCapture();
    }
    verboseLogCapture = enabled;
    showToast(
      enabled
        ? $t('settings.developer.verboseLogEnabled')
        : $t('settings.developer.verboseLogDisabled'),
      'info'
    );
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

  // Flatpak copyable command state
  let copiedCommands = $state<Record<string, boolean>>({});

  async function copyCommand(key: string, command: string) {
    try {
      await copyToClipboard(command);
      copiedCommands[key] = true;
      setTimeout(() => { copiedCommands[key] = false; }, 1200);
    } catch {
      try {
        await navigator.clipboard.writeText(command);
        copiedCommands[key] = true;
        setTimeout(() => { copiedCommands[key] = false; }, 1200);
      } catch {}
    }
  }
</script>

<ViewTransition duration={200} distance={12} direction="up">
<div class="settings-view" bind:this={settingsViewEl}>
  <!-- Loading Overlay for Device Enumeration -->
  {#if isLoadingDevices}
    <div class="loading-overlay">
      <div class="loading-content">
        <Loader2 size={48} class="spinner" />
        <p>{$t('settings.audio.loadingAudioDevices')}</p>
        <p class="loading-subtitle">{$t('settings.audio.parsingHardware')}</p>
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

  <!-- Account Section (compact) -->
  <section class="section account-section">
    <div class="account-card-compact">
      <div class="avatar-small">{userName.charAt(0).toUpperCase()}</div>
      <div class="account-info-compact">
        <span class="username-compact">{userName}</span>
        <span class="separator">·</span>
        <span class="subscription-text">{subscription}</span>
      </div>
      <button class="logout-btn-compact" onclick={onLogout}>{$t('settings.account.logout')}</button>
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
        {$t(section.labelKey)}
      </button>
    {/each}
  </nav>

  <!-- Audio Section -->
  <section class="section" bind:this={audioSection}>
    <h3 class="section-title">{$t('settings.audio.title')}</h3>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.streamingQuality')}</span>
        <span class="setting-desc">{$t('settings.audio.streamingQualityDesc')}</span>
      </div>
      <Dropdown
        value={streamingQuality}
        options={['MP3', 'CD Quality', 'Hi-Res', 'Hi-Res+']}
        onchange={handleQualityChange}
      />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.limitQualityToDevice')}</span>
        <span class="setting-desc">{$t('settings.audio.limitQualityToDeviceDesc')}</span>
      </div>
      <Toggle enabled={limitQualityToDevice} onchange={handleLimitQualityToDeviceChange} />
    </div>
    {#if limitQualityToDevice}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.maxSampleRate')}</span>
        <span class="setting-desc">{$t('settings.audio.maxSampleRateDesc')}</span>
      </div>
      <Dropdown
        value={deviceMaxSampleRate ? sampleRateOptions.find(o => o.value === deviceMaxSampleRate)?.label ?? 'No limit' : 'No limit'}
        options={['No limit', ...sampleRateOptions.map(o => o.label)]}
        onchange={(label) => {
          if (label === 'No limit') {
            handleDeviceMaxSampleRateChange(null);
          } else {
            const option = sampleRateOptions.find(o => o.label === label);
            if (option) handleDeviceMaxSampleRateChange(option.value);
          }
        }}
        wide
        expandLeft
        compact
      />
    </div>
    {/if}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.audioBackend')}</span>
        <span class="setting-desc">{$t('settings.audio.audioBackendDesc')}</span>
      </div>
      <div class="backend-selector-row">
        <Dropdown
          value={selectedBackend}
          options={backendOptions}
          onchange={handleBackendChange}
          wide
          expandLeft
          compact
        />
        {#if selectedBackend === 'PipeWire'}
          <div class="dac-setup-wrapper">
            <button
              class="dac-setup-btn"
              onclick={() => showDACWizardModal = true}
            >
              <img src="/gandalf.svg" alt="DAC Setup" class="gandalf-icon" />
            </button>
            <div class="dac-tooltip">
              <img src="/gandalf.svg" alt="" class="tooltip-gandalf" />
              <div class="tooltip-content">
                <span class="tooltip-title">{$t('dacWizard.tooltip.title')}</span>
                <span class="tooltip-desc">{$t('dacWizard.tooltip.desc')}</span>
              </div>
            </div>
          </div>
        {/if}
      </div>
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.outputDevice')}</span>
        <span class="setting-desc">{$t('settings.audio.outputDeviceDesc')}</span>
        {#if defaultDeviceName}
          <span class="setting-desc-secondary">
            {$t('settings.audio.systemDefaultIs', { values: { device: defaultDeviceName } })}
          </span>
        {/if}
      </div>
      {#if isLoadingDevices}
        <span class="loading-text">{$t('settings.audio.loadingDevices')}</span>
      {:else if selectedBackend === 'ALSA Direct'}
        <div class="dropdown-with-help">
          <DeviceDropdown
            value={outputDevice}
            devices={groupedDeviceOptions}
            onchange={handleBackendDeviceChange}
            backend="alsa"
            wide
            expandLeft
          />
          <button
            class="help-icon-btn"
            onclick={handleRefreshDevices}
            title={$t('settings.audio.refreshDevices')}
          >
            <RefreshCw size={16} />
          </button>
        </div>
      {:else if selectedBackend === 'PipeWire'}
        <div class="dropdown-with-help">
          <DeviceDropdown
            value={outputDevice}
            devices={groupedDeviceOptions}
            onchange={handleBackendDeviceChange}
            backend="pipewire"
            wide
            expandLeft
          />
          <button
            class="help-icon-btn"
            onclick={handleRefreshDevices}
            title={$t('settings.audio.refreshDevices')}
          >
            <RefreshCw size={16} />
          </button>
        </div>
      {:else}
        <div class="dropdown-with-help">
          <Dropdown
            value={outputDevice}
            options={deviceOptions}
            onchange={handleBackendDeviceChange}
            wide
            expandLeft
            compact
          />
          <button
            class="help-icon-btn"
            onclick={handleRefreshDevices}
            title={$t('settings.audio.refreshDevices')}
          >
            <RefreshCw size={16} />
          </button>
        </div>
      {/if}
    </div>
    {#if showAlsaPluginSelector}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.alsaPlugin')}</span>
        <span class="setting-desc">{$t('settings.audio.alsaPluginDesc')}</span>
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
        <span class="setting-label">{$t('settings.audio.hardwareVolume')}</span>
        <span class="setting-desc">{$t('settings.audio.hardwareVolumeDesc')}</span>
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
        <span class="setting-desc">{dacPassthroughTooltipOverrideKey ? $t(dacPassthroughTooltipOverrideKey) : $t('settings.audio.dacPassthroughDesc')}</span>
      </div>
      <Toggle enabled={dacPassthrough} onchange={handleDacPassthroughChange} disabled={dacPassthroughDisabled} />
    </div>
    {#if dacPassthrough}
    <small class="setting-note">{$t('settings.audio.dacPassthroughNote')}</small>
    {/if}
    {#if isFlatpak && selectedBackend === 'PipeWire' && dacPassthrough}
    <div class="flatpak-warning">
      <div class="warning-icon">⚠️</div>
      <div class="warning-content">
        <strong>{$t('settings.audio.flatpakWarningTitle')}</strong> {$t('settings.audio.flatpakWarningDesc')}
        <br />
        <strong>{$t('settings.audio.flatpakRecommended')}</strong> {$t('settings.audio.flatpakRecommendedDesc')}
      </div>
    </div>
    {/if}
    {#if dacPassthrough && selectedBackend === 'PipeWire'}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.pwForceBitperfect')}</span>
        <span class="setting-desc">{$t('settings.audio.pwForceBitperfectDesc')}</span>
      </div>
      <Toggle enabled={pwForceBitperfect} onchange={handlePwForceBitperfectChange} />
    </div>
    {#if pwForceBitperfect}
    <small class="setting-note">{$t('settings.audio.pwForceBitperfectNote')}</small>
    {/if}
    {/if}
    <div class="setting-row">
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
    <div class="setting-row last">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.audio.resetTitle')}</span>
        <span class="setting-desc">{$t('settings.audio.resetDesc')}</span>
      </div>
      <button
        class="reset-btn"
        onclick={handleResetAudioSettings}
        disabled={isResettingAudio}
      >
        {isResettingAudio ? $t('settings.storage.clearing') : $t('settings.audio.resetButton')}
      </button>
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
        <span class="setting-label">{$t('settings.playback.gapless')} <span class="experimental-inline">{$t('settings.playback.experimental')}</span></span>
        <span class="setting-desc">{gaplessDisabledReasonKey ? $t(gaplessDisabledReasonKey) : $t('settings.playback.gaplessDesc')}</span>
      </div>
      <Toggle enabled={gaplessPlayback} onchange={handleGaplessPlaybackChange} disabled={gaplessDisabled} />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.playback.streamUncached')}</span>
        <span class="setting-desc">{$t('settings.playback.streamUncachedDesc')}</span>
      </div>
      <Toggle enabled={streamFirstTrack} onchange={handleStreamFirstTrackChange} />
    </div>
    {#if streamFirstTrack}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.playback.initialBuffer')}</span>
        <span class="setting-desc">{$t('settings.playback.initialBufferDesc', { values: { seconds: streamBufferSeconds } })}</span>
      </div>
      <input
        type="range"
        min="1"
        max="10"
        step="1"
        value={streamBufferSeconds}
        oninput={(e) => handleStreamBufferSecondsChange(parseInt(e.currentTarget.value))}
        class="buffer-slider"
      />
    </div>
    {/if}
    <div class="setting-row last">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.playback.streamingOnly')}</span>
        <span class="setting-desc">{$t('settings.playback.streamingOnlyDesc')}</span>
      </div>
      <Toggle enabled={streamingOnly} onchange={handleStreamingOnlyChange} />
    </div>
    <!-- Crossfade, Normalize Volume hidden until properly implemented (see issue #29) -->
    <!-- <div class="setting-row">
      <span class="setting-label">{$t('settings.playback.crossfade')}</span>
      <div class="slider-container">
        <VolumeSlider value={crossfade} onchange={handleCrossfadeChange} max={12} showValue />
      </div>
    </div>
    <div class="setting-row last">
      <span class="setting-label">{$t('settings.playback.normalizeVolume')}</span>
      <Toggle enabled={normalizeVolume} onchange={(v) => (normalizeVolume = v)} />
    </div> -->
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
      <div class="theme-selector">
        <button
          class="theme-filter-btn"
          onclick={cycleThemeFilter}
          title={themeFilter === 'all' ? 'All themes' : themeFilter === 'dark' ? 'Dark themes' : 'Light themes'}
        >
          {#if themeFilter === 'all'}
            <SunMoon size={16} />
          {:else if themeFilter === 'dark'}
            <Moon size={16} />
          {:else}
            <Sun size={16} />
          {/if}
        </button>
        <Dropdown
          value={theme}
          options={filteredThemeOptions}
          onchange={handleThemeChange}
        />
      </div>
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
      <span class="setting-label">{$t('settings.appearance.fontFamily')}</span>
      <Dropdown
        value={selectedFont}
        options={fontOptions}
        onchange={handleFontChange}
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
        <span class="setting-label">{$t('settings.appearance.useSystemTitleBar')}</span>
        <span class="setting-desc">{$t('settings.appearance.useSystemTitleBarDesc')}</span>
      </div>
      <Toggle enabled={useSystemTitleBar} onchange={(v) => setUseSystemTitleBar(v)} />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.appearance.hideTitleBar')}</span>
        <span class="setting-desc">{$t('settings.appearance.hideTitleBarDesc')}</span>
      </div>
      <Toggle enabled={hideTitleBar} onchange={(v) => setHideTitleBar(v)} disabled={useSystemTitleBar} />
    </div>
    <div class="setting-row">
      <span class="setting-label">{$t('settings.appearance.immersiveDefaultView')}</span>
      <Dropdown
        value={getImmersiveViewDisplayValue()}
        options={getImmersiveViewOptions()}
        onchange={handleImmersiveViewChange}
      />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.appearance.startupPage')}</span>
        <span class="setting-desc">{$t('settings.appearance.startupPageDesc')}</span>
      </div>
      <Dropdown
        value={getStartupPageDisplayValue()}
        options={getStartupPageOptions()}
        onchange={handleStartupPageChange}
      />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.appearance.showPurchases')}</span>
        <span class="setting-desc">{$t('settings.appearance.showPurchasesDesc')}</span>
      </div>
      <Toggle enabled={purchasesEnabled} onchange={handlePurchasesToggle} />
    </div>

    <!-- Composition subsection (collapsible) -->
    <div class="collapsible-section composition-subsection">
      <button class="section-title-btn" onclick={() => compositionCollapsed = !compositionCollapsed}>
        <span class="section-title composition-title">{$t('settings.appearance.composition.title')}</span>
        <span class="section-summary">{$t('settings.appearance.composition.summary')}</span>
        {#if compositionCollapsed}
          <ChevronDown size={16} />
        {:else}
          <ChevronUp size={16} />
        {/if}
      </button>
      {#if !compositionCollapsed}
        <p class="section-note">{$t('settings.appearance.composition.helpText')}</p>

        {#if graphicsUsingFallback}
          <div class="composition-warning fallback-warning">
            <AlertTriangle size={14} />
            <div>
              <span class="fallback-title">{$t('settings.appearance.composition.fallbackWarning')}</span>
              <span class="fallback-desc">{$t('settings.appearance.composition.fallbackDesc')}</span>
              <code class="recovery-cmd">qbz --reset-graphics</code>
            </div>
          </div>
        {/if}

        <div class="composition-warning">
          <AlertTriangle size={14} />
          <div>
            <span>{$t('settings.appearance.composition.recoveryNote')}</span>
            <code class="recovery-cmd">{$t('settings.appearance.composition.recoveryCmd')}</code>
          </div>
        </div>

        <div class="composition-profile-section">
          <span class="composition-profile-title">{$t('settings.appearance.composition.profiles.title')}</span>
          <p class="section-note">{$t('settings.appearance.composition.profiles.helpText')}</p>
          <div class="composition-profile-grid">
            {#each compositionProfiles as profile (profile.id)}
              <button
                class="composition-profile-card"
                class:active={activeCompositionProfileId === profile.id}
                type="button"
                onclick={() => applyCompositionProfile(profile.id)}
              >
                <span class="profile-label">{$t(profile.labelKey)}</span>
                <span class="profile-desc">{$t(profile.descKey)}</span>
              </button>
            {/each}
          </div>
        </div>

        <div class="setting-row">
          <div class="setting-info">
            <span class="setting-label">{$t('settings.appearance.composition.hardwareAcceleration')}</span>
            <span class="setting-desc">{$t('settings.appearance.composition.hardwareAccelerationDesc')}</span>
          </div>
          <Toggle enabled={hardwareAcceleration} onchange={(v) => handleHardwareAccelerationChange(v)} />
        </div>

        <div class="setting-row">
          <div class="setting-info">
            <span class="setting-label">{$t('settings.appearance.composition.forceDmabuf')}</span>
            <span class="setting-desc">{$t('settings.appearance.composition.forceDmabufDesc')}</span>
          </div>
          <Toggle enabled={forceDmabuf} onchange={(v) => handleForceDmabufChange(v)} />
        </div>

        <div class="setting-row">
          <div class="setting-info">
            <span class="setting-label">{$t('settings.appearance.composition.forceX11')}</span>
            <span class="setting-desc">{$t('settings.appearance.composition.forceX11Desc')}</span>
          </div>
          <Toggle enabled={forceX11} onchange={(v) => handleForceX11Change(v)} />
        </div>

        {#if forceX11}
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-label">{$t('settings.appearance.composition.gdkScale')}</span>
              <span class="setting-desc">{$t('settings.appearance.composition.gdkScaleDesc')}</span>
            </div>
            <input
              class="composition-input"
              type="text"
              placeholder="auto"
              bind:value={gdkScale}
              onblur={handleGdkScaleChange}
            />
          </div>
          <div class="setting-row">
            <div class="setting-info">
              <span class="setting-label">{$t('settings.appearance.composition.gdkDpiScale')}</span>
              <span class="setting-desc">{$t('settings.appearance.composition.gdkDpiScaleDesc')}</span>
            </div>
            <input
              class="composition-input"
              type="text"
              placeholder="auto"
              bind:value={gdkDpiScale}
              onblur={handleGdkDpiScaleChange}
            />
          </div>
        {/if}

        <div class="composition-env-section">
          <span class="composition-env-title">{$t('settings.appearance.composition.envVarsTitle')}</span>
          <p class="section-note">{$t('settings.appearance.composition.envVarsDesc')}</p>
          <div class="env-vars-list">
            <div class="env-var-row">
              <code>QBZ_HARDWARE_ACCEL=0</code>
              <span>{$t('settings.appearance.composition.envVarHwAccel0')}</span>
            </div>
            <div class="env-var-row">
              <code>QBZ_HARDWARE_ACCEL=1</code>
              <span>{$t('settings.appearance.composition.envVarHwAccel1')}</span>
            </div>
            <div class="env-var-row">
              <code>QBZ_FORCE_X11=1</code>
              <span>{$t('settings.appearance.composition.envVarForceX11')}</span>
            </div>
            <div class="env-var-row">
              <code>QBZ_SOFTWARE_RENDER=1</code>
              <span>{$t('settings.appearance.composition.envVarSoftwareRender')}</span>
            </div>
            <div class="env-var-row">
              <code>QBZ_FORCE_DMABUF=1</code>
              <span>{$t('settings.appearance.composition.envVarForceDmabuf')}</span>
            </div>
            <div class="env-var-row">
              <code>QBZ_DISABLE_DMABUF=1</code>
              <span>{$t('settings.appearance.composition.envVarDisableDmabuf')}</span>
            </div>
          </div>
        </div>
      {/if}
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
  <section class="section collapsible-section" bind:this={downloadsSection}>
    <button class="section-title-btn" onclick={() => offlineLibraryCollapsed = !offlineLibraryCollapsed}>
      <h3 class="section-title">{$t('settings.offlineLibrary.title')}</h3>
      <span class="section-summary">{$t('settings.offlineLibrary.sectionSummary')}</span>
      {#if offlineLibraryCollapsed}
        <ChevronDown size={16} />
      {:else}
        <ChevronUp size={16} />
      {/if}
    </button>
    {#if !offlineLibraryCollapsed}
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
          <span class="setting-label">{$t('settings.offlineLibrary.showInLibrary')}</span>
          <span class="setting-description">{$t('settings.offlineLibrary.showInLibraryDesc')}</span>
        </div>
        <Toggle enabled={showQobuzDownloadsInLibrary} onchange={handleShowDownloadsChange} />
      </div>
      <div class="setting-row">
        <div class="setting-with-description">
          <span class="setting-label">{$t('settings.offlineLibrary.repair')}</span>
          <span class="setting-description">{$t('settings.offlineLibrary.repairDesc')}</span>
        </div>
        <button
          class="clear-btn"
          onclick={handleRepairDownloads}
          disabled={isRepairingDownloads || !downloadStats || downloadStats.readyTracks === 0}
        >
          {isRepairingDownloads ? $t('settings.offlineLibrary.repairing') : $t('actions.repair')}
        </button>
      </div>
      <div class="setting-row">
        <span class="setting-label">{$t('settings.offlineLibrary.clearCache')}</span>
        <button
          class="clear-btn"
          onclick={handleClearDownloads}
          disabled={isClearingDownloads || !downloadStats || downloadStats.readyTracks === 0}
        >
          {isClearingDownloads ? $t('settings.storage.clearing') : $t('settings.offlineLibrary.clearCache')}
        </button>
      </div>
      <div class="setting-row">
        <div class="setting-with-description">
          <span class="setting-label">{$t('settings.offlineLibrary.manageCache')}</span>
          <span class="setting-description">{$t('settings.offlineLibrary.manageCacheDesc')}</span>
        </div>
        <button
          class="clear-btn"
          onclick={handleOpenCacheFolder}
        >
          {$t('settings.offlineLibrary.openFolder')}
        </button>
      </div>
      <div class="setting-row last">
        <div class="setting-with-description">
          <span class="setting-label">{$t('settings.library.fetchArtistImages')}</span>
          <span class="setting-description">{$t('settings.library.fetchArtistImagesDesc')}</span>
        </div>
        <Toggle enabled={fetchQobuzArtistImages} onchange={(v) => {
          fetchQobuzArtistImages = v;
          setUserItem('qbz-fetch-artist-images', String(v));
        }} />
      </div>
    {/if}
  </section>

  <!-- Content Filtering Section -->
  <section class="section" bind:this={contentFilteringSection}>
    <h3 class="section-title">{$t('settings.contentFiltering.title')}</h3>
    <div class="setting-row last">
      <div class="setting-info">
        <div class="setting-with-icon">
          <Ban size={18} class="setting-icon" />
          <div class="setting-with-description">
            <span class="setting-label">{$t('settings.contentFiltering.artistBlacklist')}</span>
            <span class="setting-description">
              {blacklistCount} {blacklistCount === 1 ? 'artist' : 'artists'} blocked
              {#if !blacklistEnabled}
                <span class="status-disabled">({$t('settings.contentFiltering.disabled')})</span>
              {/if}
            </span>
          </div>
        </div>
      </div>
      <button class="link-btn" onclick={onBlacklistManagerClick}>
        {$t('settings.contentFiltering.manage')}
        <ChevronRight size={16} />
      </button>
    </div>
  </section>

  <!-- Integrations Section -->
  <section class="section" bind:this={integrationsSection}>
    <h3 class="section-title">{$t('settings.integrations.title')}</h3>

    <!-- Qobuz Link Handler -->
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.integrations.qobuzLinkHandler')}</span>
        <small class="setting-note">{$t('settings.integrations.qobuzLinkHandlerDesc')}</small>
      </div>
      <Toggle enabled={qobuzLinkHandlerEnabled} onchange={handleQobuzLinkHandlerToggle} disabled={qobuzLinkHandlerBusy} />
    </div>

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
            <label for="lastfm-api-key">{$t('settings.integrations.apiKey')}</label>
            <input
              id="lastfm-api-key"
              type="text"
              bind:value={lastfmApiKey}
              placeholder={$t('placeholders.enterApiKey')}
            />
          </div>
          <div class="config-field">
            <label for="lastfm-api-secret">{$t('settings.integrations.sharedSecret')}</label>
            <input
              id="lastfm-api-secret"
              type="password"
              bind:value={lastfmApiSecret}
              placeholder={$t('placeholders.enterSharedSecret')}
            />
          </div>
          <button
            class="auth-start-btn"
            onclick={handleLastfmConnect}
            disabled={!lastfmApiKey || !lastfmApiSecret || lastfmConnecting}
          >
            {lastfmConnecting ? $t('settings.integrations.opening') : $t('settings.integrations.authorizeLastfm')}
          </button>
        </div>
      {/if}
    {/if}

    <!-- ListenBrainz Integration -->
    {#if listenbrainzConnected}
      <div class="setting-row">
        <div class="lastfm-connected">
          <span class="setting-label">ListenBrainz</span>
          <span class="lastfm-username">{$t('settings.integrations.connectedAs', { values: { username: listenbrainzUsername }})}</span>
        </div>
        <button
          class="connect-btn connected"
          onclick={handleListenBrainzDisconnect}
        >
          {$t('settings.integrations.disconnect')}
        </button>
      </div>
      <div class="setting-row">
        <span class="setting-label">{$t('settings.integrations.scrobbling')}</span>
        <Toggle enabled={listenbrainzEnabled} onchange={handleListenBrainzEnabledChange} />
      </div>
    {:else}
      <div class="setting-row" class:last={!showListenBrainzConfig}>
        <span class="setting-label">{$t('settings.integrations.listenbrainz')}</span>
        <button
          class="connect-btn"
          onclick={() => showListenBrainzConfig = !showListenBrainzConfig}
          disabled={listenbrainzConnecting}
        >
          {listenbrainzConnecting ? $t('settings.integrations.connecting') : $t('settings.integrations.connect')}
        </button>
      </div>

      {#if showListenBrainzConfig}
        <div class="lastfm-config">
          <p class="config-info">
            {$t('settings.integrations.listenbrainzTokenHint')}
            <a href="https://listenbrainz.org/settings/" target="_blank" rel="noopener">
              listenbrainz.org/settings
            </a>
          </p>
          <div class="config-field">
            <label for="listenbrainz-token">{$t('settings.integrations.userToken')}</label>
            <input
              id="listenbrainz-token"
              type="password"
              bind:value={listenbrainzToken}
              placeholder={$t('placeholders.pasteToken')}
            />
          </div>
          <button
            class="auth-start-btn"
            onclick={handleListenBrainzConnect}
            disabled={!listenbrainzToken.trim() || listenbrainzConnecting}
          >
            {listenbrainzConnecting ? 'Connecting...' : 'Connect'}
          </button>
        </div>
      {/if}
    {/if}

    <!-- MusicBrainz Integration -->
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.integrations.musicbrainz')}</span>
        <small class="setting-note">
          {$t('settings.integrations.musicbrainzDesc')}
        </small>
      </div>
      <Toggle enabled={musicbrainzEnabled} onchange={handleMusicBrainzChange} />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.integrations.plexConnection')}</span>
        <small class="setting-note">{$t('settings.integrations.plexConnectionLore')}</small>
      </div>
      <div class="setting-row-controls">
        <Toggle enabled={plexEnabled} onchange={handlePlexEnabledToggle} />
        <button
          class="setting-link-button section-collapse-btn"
          onclick={togglePlexUiCollapsed}
          title={$t('settings.integrations.plexCollapseHint')}
        >
          {#if plexUiCollapsed}
            <ChevronDown size={16} />
          {:else}
            <ChevronUp size={16} />
          {/if}
        </button>
      </div>
    </div>

    {#if plexEnabled && !plexUiCollapsed}
      <div class="setting-row plex-two-column-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.plexServerUrl')}:</span>
          <small class="setting-note">{$t('settings.integrations.plexServerUrlHelp')}</small>
        </div>
        <input
          id="plex-server-url"
          class="remote-control-input plex-server-url-input"
          type="text"
          bind:value={plexServerUrl}
          placeholder={$t('settings.integrations.plexServerUrlPlaceholder')}
          oninput={() => persistPlexConfig()}
          onblur={() => persistPlexConfig()}
        />
      </div>

      <div class="setting-row plex-two-column-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.plexAuthorizeLabel')}</span>
          <small class="setting-note">{$t('settings.integrations.plexAuthorizeGenerateHelp')}</small>
        </div>
        <button
          class="connect-btn plex-action-btn"
          onclick={handlePlexConnectEasy}
          disabled={plexBusy || plexAuthBusy || !isLocalPlexAddress(plexServerUrl) || !resolvePlexBaseUrl()}
        >
          {plexAuthBusy ? $t('actions.loading') : $t('settings.integrations.plexActionGenerateCode')}
        </button>
      </div>

      {#if plexAuthCode}
        <div class="plex-divider"></div>

        <div class="setting-row plex-two-column-row">
          <div class="setting-info">
            <span class="setting-label">{$t('settings.integrations.plexLinkWithPlex')}</span>
            <small class="setting-note">{$t('settings.integrations.plexLinkWithPlexHelp')}</small>
          </div>
          <div class="plex-code-row">
            <input
              class="remote-control-input plex-code-input"
              type="text"
              readonly
              value={plexAuthCode}
              title={$t('settings.integrations.plexCodeTooltip')}
            />
            <button
              class="connect-btn plex-action-btn"
              onclick={handleCopyPlexCode}
              title={$t('settings.integrations.plexCodeTooltip')}
            >
              {$t('settings.integrations.plexActionCopyCode')}
            </button>
          </div>
        </div>
        <div class="setting-row plex-two-column-row">
          <div class="setting-info">
            <span class="setting-label">{$t('settings.integrations.plexAuthorizeUsingCode')}</span>
            <small class="setting-note">{$t('settings.integrations.plexAuthorizeHelp')}</small>
          </div>
          <button
            class="connect-btn plex-action-btn"
            onclick={handleOpenPlexAuthUrl}
            disabled={!plexAuthUrl}
          >
            {$t('settings.integrations.plexActionOpenAuth')}
          </button>
        </div>
      {/if}

      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.plexManualTokenToggle')}</span>
          <small class="setting-note">{$t('settings.integrations.plexManualTokenHelp')}</small>
        </div>
        <Toggle enabled={plexManualTokenMode} onchange={(enabled) => plexManualTokenMode = enabled} />
      </div>

      {#if plexManualTokenMode}
        <div class="setting-row plex-two-column-row">
          <span class="setting-label">{$t('settings.integrations.plexToken')}</span>
          <input
            class="remote-control-input"
            type="password"
            bind:value={plexToken}
            placeholder={$t('settings.integrations.plexTokenPlaceholder')}
            onblur={handlePlexTokenBlur}
          />
        </div>
      {/if}

      <div class="setting-row plex-two-column-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.plexConnectionCheckLabel')}</span>
          {#if plexStatusKey === 'settings.integrations.plexStatusConnected'}
            <small class="setting-note plex-connected-note">{$t(plexStatusKey, { values: plexStatusValues })}</small>
          {:else}
            <small class="setting-note">{$t(plexStatusKey, { values: plexStatusValues })}</small>
          {/if}
        </div>
        <button
          class="connect-btn plex-action-btn"
          onclick={() => handlePlexPing()}
          disabled={plexBusy || !canUsePlexRequests()}
        >
          {plexBusy ? $t('actions.loading') : $t('settings.integrations.plexActionPing')}
        </button>
      </div>

      <div class="setting-row plex-two-column-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.plexGetLibrariesLabel')}</span>
          <small class="setting-note">{$t('settings.integrations.plexGetLibrariesHelp')}</small>
        </div>
        <button
          class="connect-btn plex-action-btn"
          onclick={() => handlePlexLoadSections({ autoSyncSelected: true })}
          disabled={plexBusy || !canUsePlexRequests()}
        >
          {plexBusy ? $t('actions.loading') : $t('settings.integrations.plexActionLoadSections')}
        </button>
      </div>

      <div class="setting-row plex-libraries-block">
        <span class="setting-label">{$t('settings.integrations.plexMusicLibraries')}</span>
        <div class="plex-libraries-grid">
          {#each plexSections as plexSection}
            <label class="plex-library-item">
              <input
                type="checkbox"
                checked={plexSelectedSectionKeys.includes(plexSection.key)}
                disabled={plexBusy || !canUsePlexRequests()}
                onchange={(event) => handlePlexLibraryToggle(plexSection.key, (event.currentTarget as HTMLInputElement).checked)}
              />
              <span class="plex-library-name">{plexSection.title}</span>
              {#if plexSectionTrackCounts[plexSection.key] !== undefined}
                <span class="plex-library-count">({plexSectionTrackCounts[plexSection.key]} {$t('settings.integrations.plexTracksShort')})</span>
              {/if}
            </label>
          {/each}
        </div>
        {#if plexLastError}
          <small class="setting-note plex-error-note">{plexLastError}</small>
        {/if}
      </div>

      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.plexMetadataWrite')}</span>
          <small class="setting-note">{$t('settings.integrations.plexMetadataWriteDesc')}</small>
        </div>
        <Toggle enabled={plexMetadataWriteEnabled} onchange={handlePlexMetadataWriteToggle} />
      </div>

      <div class="setting-row plex-two-column-row">
        <span class="setting-label">{$t('settings.integrations.plexDisconnectRowLabel')}</span>
        <button
          class="connect-btn plex-action-btn"
          onclick={handlePlexDisconnect}
          disabled={plexBusy || plexAuthBusy || !plexToken.trim()}
        >
          {$t('settings.integrations.plexActionDisconnect')}
        </button>
      </div>

      <div class="setting-row last plex-two-column-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.plexClearCacheRowLabel')}</span>
          <small class="setting-note">{$t('settings.integrations.plexClearCacheRowHelp')}</small>
        </div>
        <button
          class="connect-btn plex-action-btn"
          onclick={handlePlexClearCache}
          disabled={plexBusy || plexAuthBusy}
        >
          {$t('settings.integrations.plexClearButton')}
        </button>
      </div>
    {/if}
  </section>

  <!-- Updates Section -->
  <section class="section" bind:this={updatesSection}>
    <h3 class="section-title">{$t('settings.updates.title')}</h3>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.updates.checkOnLaunch')}</span>
      </div>
      <Toggle
        enabled={updatePreferences.checkOnLaunch}
        onchange={handleUpdateCheckOnLaunchToggle}
      />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.updates.checkNow')}</span>
      </div>
      <button
        class="connect-btn updates-check-btn"
        onclick={handleManualUpdateCheck}
        disabled={isCheckingUpdates}
        type="button"
      >
        {#if isCheckingUpdates}
          <Loader2 size={14} class="spin" />
          <span>{$t('settings.updates.checking')}</span>
        {:else}
          <span>{$t('settings.updates.check')}</span>
        {/if}
      </button>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.updates.showWhatsNew')}</span>
      </div>
      <Toggle
        enabled={updatePreferences.showWhatsNewOnLaunch}
        onchange={handleShowWhatsNewToggle}
      />
    </div>

    <div class="setting-row last">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.updates.showChangelog')}</span>
        {#if updatesCurrentVersion}
          <small class="setting-note">{$t('settings.updates.currentVersion', { values: { version: updatesCurrentVersion } })}</small>
        {/if}
      </div>
      <button
        class="connect-btn"
        onclick={handleShowCurrentChangelog}
        type="button"
        disabled={isFetchingChangelog}
      >
        {isFetchingChangelog ? $t('actions.loading') : $t('actions.show')}
      </button>
    </div>
  </section>

  <!-- Remote Control Section (collapsible) -->
  <section class="section collapsible-section" id="remote-control" bind:this={remoteControlSection}>
    <button class="section-title-btn" onclick={() => remoteControlCollapsed = !remoteControlCollapsed}>
      <h3 class="section-title">{$t('settings.integrations.remoteControl')}</h3>
      <span class="experimental-badge">{$t('settings.integrations.remoteControlExperimental')}</span>
      {#if remoteControlCollapsed}
        <ChevronDown size={16} />
      {:else}
        <ChevronUp size={16} />
      {/if}
    </button>
    {#if !remoteControlCollapsed}
    <!-- TODO: Re-enable when setup guide content is complete
    <div class="section-header-actions">
      <button class="setup-guide-btn" onclick={() => showRemoteControlGuide = true}>
        {$t('settings.integrations.remoteControlSetupGuide')}
      </button>
    </div>
    -->

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.integrations.remoteControlEnable')}</span>
        <small class="setting-note">
          {$t('settings.integrations.remoteControlDesc')}
        </small>
      </div>
      <Toggle
        enabled={remoteControlEnabled}
        onchange={handleRemoteControlToggle}
        disabled={remoteControlLoading}
      />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.integrations.remoteControlPort')}</span>
        <small class="setting-note">
          {$t('settings.integrations.remoteControlPortDesc')}
        </small>
      </div>
      <input
        class="remote-control-input"
        type="number"
        min="1024"
        max="65535"
        bind:value={remoteControlPort}
        disabled={!remoteControlEnabled || remoteControlLoading}
        onchange={(e) => handleRemoteControlPortChange(Number((e.target as HTMLInputElement).value))}
      />
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.integrations.remoteControlSecure')}</span>
        <small class="setting-note">
          {$t('settings.integrations.remoteControlSecureDesc')}
        </small>
      </div>
      <Toggle
        enabled={remoteControlSecure}
        onchange={handleRemoteControlSecureChange}
        disabled={!remoteControlEnabled || remoteControlLoading}
      />
    </div>

    {#if remoteControlEnabled}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.remoteControlToken')}</span>
          <small class="setting-note">
            {$t('settings.integrations.remoteControlTokenDesc')}
          </small>
        </div>
        <div class="remote-control-actions">
          <input
            class="remote-control-input"
            type="text"
            readonly
            value={remoteControlToken}
            disabled={!remoteControlEnabled || remoteControlLoading}
          />
          <button
            class="connect-btn connected"
            onclick={handleRemoteControlCopyToken}
            disabled={!remoteControlEnabled || remoteControlLoading || !remoteControlToken}
          >
            {$t('actions.copy')}
          </button>
        </div>
      </div>
    {/if}

    {#if remoteControlEnabled && remoteControlSecure}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">{$t('settings.integrations.remoteControlCert')}</span>
          <small class="setting-note">
            {$t('settings.integrations.remoteControlCertDesc')}
          </small>
        </div>
        <div class="remote-control-actions">
          <input
            class="remote-control-input"
            type="text"
            readonly
            value={remoteControlCertUrl}
            disabled={!remoteControlEnabled || remoteControlLoading}
          />
          <button
            class="connect-btn connected"
            onclick={handleRemoteControlCopyCert}
            disabled={!remoteControlEnabled || remoteControlLoading || !remoteControlCertUrl}
          >
            {$t('actions.copy')}
          </button>
        </div>
      </div>
    {/if}

    <div class="setting-row" class:last={!remoteControlQrOpen}>
      <div class="setting-info">
        <span class="setting-label">{$t('settings.integrations.remoteControlStatus')}</span>
        <small class="setting-note">
          {#if remoteControlStatus?.running}
            {$t('settings.integrations.remoteControlStatusRunning')}
          {:else}
            {$t('settings.integrations.remoteControlStatusStopped')}
          {/if}
          {#if remoteControlUrl}
            <span class="remote-control-url">{$t('settings.integrations.remoteControlUrlLabel')}: {remoteControlUrl}</span>
          {/if}
        </small>
      </div>
      <div class="remote-control-actions">
        <button
          class="connect-btn connected"
          onclick={handleRemoteControlRegenerateToken}
          disabled={!remoteControlEnabled || remoteControlLoading}
        >
          {$t('settings.integrations.remoteControlRegenerate')}
        </button>
        <button
          class="connect-btn"
          onclick={() => handleRemoteControlQrToggle()}
          disabled={!remoteControlEnabled || remoteControlLoading}
        >
          {remoteControlQrOpen
            ? $t('settings.integrations.remoteControlHideQr')
            : $t('settings.integrations.remoteControlShowQr')}
        </button>
      </div>
    </div>

    {#if remoteControlQrOpen}
      <div class="remote-control-qr">
        <img src={remoteControlQrData} alt={$t('settings.integrations.remoteControlQrAlt')} />
        <div class="remote-control-qr-meta">
          <p class="remote-control-qr-help">{$t('settings.integrations.remoteControlQrHelp')}</p>
        </div>
      </div>
    {/if}
    {/if}
  </section>

  {#if isUpdateResultOpen}
    <UpdateCheckResultModal
      isOpen={isUpdateResultOpen}
      status={updateResultStatus}
      newVersion={updateResultRelease?.version ?? ''}
      onClose={handleCloseUpdateResult}
      onVisitReleasePage={handleVisitReleaseFromResult}
    />
  {/if}

  {#if settingsWhatsNewRelease}
    <WhatsNewModal
      isOpen={isSettingsWhatsNewOpen}
      release={settingsWhatsNewRelease}
      showTitleBar={showTitleBar}
      onClose={handleCloseSettingsWhatsNew}
    />
  {/if}

  <!-- Storage Section (Memory Cache) -->
  <section class="section collapsible-section" bind:this={storageSection}>
    <button class="section-title-btn" onclick={() => storageCollapsed = !storageCollapsed}>
      <h3 class="section-title">{$t('settings.storage.title')}</h3>
      <span class="section-summary">{$t('settings.storage.sectionSummary')}</span>
      {#if storageCollapsed}
        <ChevronDown size={16} />
      {:else}
        <ChevronUp size={16} />
      {/if}
    </button>
    {#if !storageCollapsed}
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
    <div class="setting-row">
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
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.integrations.musicbrainzCache')}</span>
        <small class="setting-note">
          {#if musicBrainzCacheStats}
            {musicBrainzCacheStats.artists} artists, {musicBrainzCacheStats.relations} relations, {musicBrainzCacheStats.recordings} recordings
          {:else}
            -
          {/if}
        </small>
      </div>
      <button
        class="clear-btn"
        onclick={handleClearMusicBrainzCache}
        disabled={isClearingMusicBrainz || !musicBrainzCacheStats || (musicBrainzCacheStats.artists === 0 && musicBrainzCacheStats.relations === 0 && musicBrainzCacheStats.recordings === 0)}
      >
        {isClearingMusicBrainz ? $t('settings.storage.clearing') : $t('actions.clear')}
      </button>
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Artist Vectors (Suggestions)</span>
        <small class="setting-note">
          {#if vectorStoreStats}
            {vectorStoreStats.artist_count} artists, {vectorStoreStats.entry_count} relations
          {:else}
            -
          {/if}
        </small>
      </div>
      <button
        class="clear-btn"
        onclick={handleClearVectorStore}
        disabled={isClearingVectorStore || !vectorStoreStats || vectorStoreStats.entry_count === 0}
      >
        {isClearingVectorStore ? $t('settings.storage.clearing') : $t('actions.clear')}
      </button>
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Artwork Thumbnails</span>
        <small class="setting-note">
          {#if artworkCacheStats}
            {@const thumbCount = artworkCacheStats.thumbnail_file_count ?? 0}
            {@const thumbBytes = artworkCacheStats.thumbnails_cache_bytes ?? 0}
            {@const legacyCount = artworkCacheStats.artwork_file_count ?? 0}
            {@const legacyBytes = artworkCacheStats.artwork_cache_bytes ?? 0}
            {#if thumbCount > 0 || legacyCount > 0}
              {#if thumbCount > 0}{thumbCount} thumbnails ({formatBytes(thumbBytes)}){/if}{#if thumbCount > 0 && legacyCount > 0}, {/if}{#if legacyCount > 0}{legacyCount} legacy files ({formatBytes(legacyBytes)}){/if}
            {:else}
              No cached artwork
            {/if}
          {:else}
            -
          {/if}
        </small>
      </div>
      <button
        class="clear-btn"
        onclick={handleClearArtworkCache}
        disabled={isClearingArtwork || !artworkCacheStats || ((artworkCacheStats.thumbnails_cache_bytes ?? 0) === 0 && (artworkCacheStats.artwork_cache_bytes ?? 0) === 0)}
      >
        {isClearingArtwork ? $t('settings.storage.clearing') : $t('actions.clear')}
      </button>
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Clear All Caches</span>
        <small class="setting-note">
          Clears all cached data above
        </small>
      </div>
      <button
        class="clear-btn"
        onclick={handleClearAllCaches}
        disabled={isClearingAllCaches}
      >
        {isClearingAllCaches ? $t('settings.storage.clearing') : $t('actions.clearAll')}
      </button>
    </div>
    <div class="setting-row last">
      <div class="danger-zone">
        <div class="danger-zone-header">
          <span class="setting-label danger-label">{$t('settings.storage.factoryResetTitle')}</span>
          <span class="setting-desc">{$t('settings.storage.factoryResetDesc')}</span>
        </div>
        <div class="factory-reset-controls">
          <label class="checkbox-label">
            <input type="checkbox" bind:checked={factoryResetConfirmed} />
            <span>{$t('settings.storage.factoryResetCheckbox')}</span>
          </label>
          <button
            class="factory-reset-btn"
            onclick={handleFactoryReset}
            disabled={!factoryResetConfirmed || isFactoryResetting}
          >
            {isFactoryResetting ? $t('settings.storage.clearing') : $t('settings.storage.factoryResetButton')}
          </button>
        </div>
      </div>
    </div>
    {/if}
  </section>

  <!-- Developer Mode Section (not in jump-nav, collapsed by default) -->
  <section class="section collapsible-section">
    <button class="section-title-btn" onclick={() => developerCollapsed = !developerCollapsed}>
      <h3 class="section-title">{$t('settings.developer.title')}</h3>
      <span class="section-summary">{$t('settings.developer.summary')}</span>
      {#if developerCollapsed}
        <ChevronDown size={16} />
      {:else}
        <ChevronUp size={16} />
      {/if}
    </button>
    {#if !developerCollapsed}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.developer.verboseLogCapture')}</span>
        <small class="setting-note">{$t('settings.developer.verboseLogCaptureDesc')}</small>
      </div>
      <Toggle enabled={verboseLogCapture} onchange={handleVerboseLogCaptureChange} />
    </div>
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">{$t('settings.developer.viewLogs')}</span>
        <small class="setting-note">{$t('settings.developer.viewLogsDesc')}</small>
      </div>
      <button class="clear-btn" onclick={() => showLogsModal = true}>
        {$t('settings.developer.viewLogs')}
      </button>
    </div>
    {/if}
  </section>

  <LogsModal isOpen={showLogsModal} onClose={() => showLogsModal = false} />

  <!-- Flatpak Section (only shown when running in Flatpak) -->
  <!-- NOTE: Keep this section LAST. If adding new settings sections, add them BEFORE this one. -->
  {#if isFlatpak}
    <section class="section flatpak-section" id="flatpak" bind:this={flatpakSection}>
      <h3 class="section-title">Flatpak Sandbox</h3>
      <div class="flatpak-info">
        <p class="flatpak-intro">
          QBZ is running inside a Flatpak sandbox. For offline libraries on NAS, network mounts, or external disks, direct filesystem access is required.
        </p>
        <div class="flatpak-guide">
          <h4>Grant Filesystem Access</h4>
          <p>Use <strong>Flatseal</strong> (GUI) or run this command for each folder you want to add:</p>
          <div class="copyable-command">
            <pre class="code-block">flatpak override --user --filesystem=/path/to/your/music com.blitzfc.qbz</pre>
            <button class="copy-btn" onclick={() => copyCommand('fs-basic', 'flatpak override --user --filesystem=/path/to/your/music com.blitzfc.qbz')}>
              {copiedCommands['fs-basic'] ? 'Copied!' : 'Copy'}
            </button>
          </div>
          <h4>Examples</h4>
          <div class="copyable-command">
            <pre class="code-block"># CIFS / Samba mount
flatpak override --user --filesystem=/mnt/nas com.blitzfc.qbz

# SSHFS mount
flatpak override --user --filesystem=$HOME/music-nas com.blitzfc.qbz

# Custom folder (edit as needed)
flatpak override --user --filesystem=/home/USUARIO/Música com.blitzfc.qbz</pre>
            <button class="copy-btn" onclick={() => copyCommand('fs-examples', `# CIFS / Samba mount\nflatpak override --user --filesystem=/mnt/nas com.blitzfc.qbz\n\n# SSHFS mount\nflatpak override --user --filesystem=$HOME/music-nas com.blitzfc.qbz\n\n# Custom folder (edit as needed)\nflatpak override --user --filesystem=/home/USUARIO/Música com.blitzfc.qbz`)}>
              {copiedCommands['fs-examples'] ? 'Copied!' : 'Copy'}
            </button>
          </div>
          <p class="flatpak-note">
            <strong>Note:</strong> This setting is persistent and survives reboots and updates.<br />
            <strong>Tip:</strong> You can repeat the command for as many folders as you need.
          </p>
        </div>
        <div class="flatpak-guide" style="margin-top:2em;">
          <h4>Chromecast &amp; DLNA Device Discovery</h4>
          <p>
            To detect Chromecast and DLNA devices on your network, you must grant network sharing permissions to the app:
          </p>
          <div class="copyable-command">
            <pre class="code-block">flatpak override --user --share=network com.blitzfc.qbz</pre>
            <button class="copy-btn" onclick={() => copyCommand('network', 'flatpak override --user --share=network com.blitzfc.qbz')}>
              {copiedCommands['network'] ? 'Copied!' : 'Copy'}
            </button>
          </div>
          <p class="flatpak-note">
            <strong>Note:</strong> Without this, device discovery will not work.<br />
            You only need to do this once.
          </p>
        </div>
      </div>
    </section>
  {/if}
</div>
</ViewTransition>

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
    padding: 0 32px 24px 18px;
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

  .dropdown-with-help {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .help-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .help-icon-btn:hover {
    background: var(--bg-hover);
    color: var(--accent);
  }

  .backend-selector-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .dac-setup-wrapper {
    position: relative;
  }

  .dac-setup-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    background: var(--accent-primary);
    border: none;
    border-radius: 6px;
    color: white;
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .dac-setup-btn:hover {
    opacity: 0.9;
    transform: scale(1.05);
  }

  .dac-setup-btn .gandalf-icon {
    width: 24px;
    height: 24px;
    filter: invert(1);
  }

  .dac-tooltip {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    padding: 12px 14px;
    display: flex;
    align-items: center;
    gap: 12px;
    opacity: 0;
    visibility: hidden;
    transform: translateY(-4px);
    transition: all 150ms ease;
    z-index: 100;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
    white-space: nowrap;
    pointer-events: none;
  }

  .dac-setup-wrapper:hover .dac-tooltip {
    opacity: 1;
    visibility: visible;
    transform: translateY(0);
  }

  .tooltip-gandalf {
    width: 36px;
    height: 36px;
    opacity: 0.9;
  }

  .tooltip-content {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .tooltip-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .tooltip-desc {
    font-size: 12px;
    color: var(--text-secondary);
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

  .experimental-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    padding: 3px 8px;
    border-radius: 4px;
    background: rgba(239, 68, 68, 0.15);
    color: #ef4444;
    border: 1px solid rgba(239, 68, 68, 0.3);
  }

  .experimental-inline {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.3px;
    color: var(--text-muted);
    opacity: 0.7;
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

  /* Compact Account Section */
  .account-section {
    padding: 16px 24px;
  }

  .account-card-compact {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 12px;
  }

  .avatar-small {
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background-color: var(--accent-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    color: white;
    font-size: 14px;
    font-weight: 600;
    flex-shrink: 0;
    align-self: center;
  }

  .account-info-compact {
    flex: 1;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 8px;
    min-width: 0;
    align-self: center;
  }

  .username-compact {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .account-info-compact .separator {
    color: var(--text-muted);
    font-size: 14px;
  }

  .subscription-text {
    font-size: 14px;
    font-weight: 400;
    color: var(--accent-primary);
  }

  .logout-btn-compact {
    padding: 6px 16px;
    border-radius: 6px;
    border: 1px solid var(--danger);
    background: none;
    color: var(--danger);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease;
    flex-shrink: 0;
    align-self: center;
  }

  .logout-btn-compact:hover {
    background-color: var(--danger-bg);
  }

  /* Collapsible sections */
  .collapsible-section .section-title-btn {
    display: flex;
    align-items: baseline;
    gap: 12px;
    width: 100%;
    padding: 0;
    margin-bottom: 8px;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-muted);
  }

  .collapsible-section .section-title-btn .section-title {
    margin-bottom: 0;
    flex-shrink: 0;
  }

  .section-summary {
    flex: 1;
    font-size: 12px;
    color: var(--text-muted);
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .collapsible-section .section-title-btn :global(svg) {
    flex-shrink: 0;
    color: var(--text-muted);
    transition: color 150ms ease;
  }

  .collapsible-section .section-title-btn:hover :global(svg) {
    color: var(--text-primary);
  }

  .collapsible-section .section-title-btn .experimental-badge {
    flex-shrink: 0;
    margin-left: -4px;
  }

  /* Composition subsection (inside Appearance) */
  .composition-subsection {
    padding-top: 16px;
    margin-top: 8px;
    border-top: 1px solid var(--border-color);
  }

  .composition-title {
    font-size: 14px !important;
  }

  .composition-warning {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px 12px;
    margin-bottom: 16px;
    border-radius: 8px;
    background: rgba(245, 158, 11, 0.08);
    border: 1px solid rgba(245, 158, 11, 0.2);
    font-size: 12px;
    color: var(--text-secondary);
  }

  .composition-warning :global(svg) {
    flex-shrink: 0;
    color: #f59e0b;
    margin-top: 1px;
  }

  /* Fallback warning (more prominent - red/error style) */
  .composition-warning.fallback-warning {
    background: rgba(239, 68, 68, 0.1);
    border-color: rgba(239, 68, 68, 0.3);
  }

  .composition-warning.fallback-warning :global(svg) {
    color: #ef4444;
  }

  .fallback-title {
    display: block;
    font-weight: 600;
    color: #ef4444;
    margin-bottom: 2px;
  }

  .fallback-desc {
    display: block;
    color: var(--text-secondary);
  }

  .composition-profile-section {
    margin-bottom: 16px;
  }

  .composition-profile-title {
    display: block;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 6px;
  }

  .composition-profile-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 10px;
  }

  .composition-profile-card {
    text-align: left;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    transition: border-color 140ms ease, background 140ms ease;
  }

  .composition-profile-card:hover {
    border-color: var(--accent-primary);
    background: color-mix(in srgb, var(--bg-secondary) 92%, var(--accent-primary) 8%);
  }

  .composition-profile-card.active {
    border-color: var(--accent-primary);
    background: color-mix(in srgb, var(--bg-secondary) 88%, var(--accent-primary) 12%);
  }

  .profile-label {
    font-size: 12px;
    font-weight: 600;
    line-height: 1.3;
  }

  .profile-desc {
    font-size: 11px;
    color: var(--text-secondary);
    line-height: 1.35;
  }

  .recovery-cmd {
    display: block;
    margin-top: 4px;
    padding: 4px 8px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    font-family: monospace;
    font-size: 12px;
    color: var(--text-primary);
    user-select: all;
  }

  .composition-input {
    width: 80px;
    padding: 6px 8px;
    border-radius: 8px;
    border: 1px solid var(--bg-tertiary);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 12px;
    text-align: center;
  }

  .composition-env-section {
    margin-top: 16px;
    padding-top: 16px;
    border-top: 1px solid var(--border-color);
  }

  .composition-env-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .env-vars-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .env-var-row {
    display: flex;
    align-items: baseline;
    gap: 12px;
    font-size: 12px;
  }

  .env-var-row code {
    flex-shrink: 0;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--bg-tertiary);
    font-family: monospace;
    font-size: 11px;
    color: var(--text-primary);
  }

  .env-var-row span {
    color: var(--text-muted);
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

  .setting-desc-secondary {
    font-size: 11px;
    color: var(--text-muted);
    opacity: 0.7;
    font-style: italic;
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

  .updates-check-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-width: 112px;
  }

  .updates-check-btn :global(.spin) {
    animation: updates-spin 1s linear infinite;
  }

  @keyframes updates-spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
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

  .reset-btn {
    padding: 8px 16px;
    border-radius: 8px;
    border: 1px solid var(--text-muted);
    background: none;
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease;
    white-space: nowrap;
  }

  .reset-btn:hover:not(:disabled) {
    background-color: rgba(255, 107, 107, 0.1);
    border-color: #ff6b6b;
    color: #ff6b6b;
  }

  .reset-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .danger-zone {
    width: 100%;
    border: 1px solid rgba(255, 107, 107, 0.3);
    border-radius: 8px;
    padding: 16px;
    background: rgba(255, 107, 107, 0.05);
  }

  .danger-zone-header {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 12px;
  }

  .danger-label {
    color: #ff6b6b;
  }

  .factory-reset-controls {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .checkbox-label {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    cursor: pointer;
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.4;
  }

  .checkbox-label input[type="checkbox"] {
    margin-top: 2px;
    flex-shrink: 0;
    accent-color: #ff6b6b;
  }

  .factory-reset-btn {
    padding: 8px 16px;
    border-radius: 8px;
    border: 1px solid #ff6b6b;
    background: rgba(255, 107, 107, 0.1);
    color: #ff6b6b;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color 150ms ease;
    align-self: flex-start;
  }

  .factory-reset-btn:hover:not(:disabled) {
    background-color: rgba(255, 107, 107, 0.2);
  }

  .factory-reset-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Harmonize button widths across settings rows */
  .connect-btn,
  .clear-btn {
    min-width: 140px;
    padding-top: 7px;
    padding-bottom: 7px;
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

  /* Buffer slider styling */
  .buffer-slider {
    width: 120px;
    height: 4px;
    -webkit-appearance: none;
    appearance: none;
    background: var(--alpha-20);
    border-radius: 2px;
    cursor: pointer;
  }

  .buffer-slider::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    background: var(--accent-color, #3b82f6);
    border-radius: 50%;
    cursor: pointer;
    transition: transform 0.1s ease;
  }

  .buffer-slider::-webkit-slider-thumb:hover {
    transform: scale(1.1);
  }

  .buffer-slider::-moz-range-thumb {
    width: 14px;
    height: 14px;
    background: var(--accent-color, #3b82f6);
    border-radius: 50%;
    cursor: pointer;
    border: none;
  }

  /* Flatpak copyable command styling */
  .copyable-command {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-bottom: 8px;
  }

  .copyable-command .code-block {
    margin: 0;
    font-size: 13px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    padding: 8px 12px;
    user-select: all;
    min-width: 0;
    flex: 1;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .copy-btn {
    background: var(--accent-primary);
    color: white;
    border: none;
    border-radius: 6px;
    padding: 6px 14px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background 0.15s;
    flex-shrink: 0;
  }

  .copy-btn:hover {
    background: var(--accent-secondary);
  }

  /* Theme selector with filter button */
  .theme-selector {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .theme-filter-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 8px;
    color: var(--text-muted);
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .theme-filter-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* Content Filtering Section */
  .setting-with-icon {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }

  .setting-with-icon :global(.setting-icon) {
    color: var(--text-muted);
    margin-top: 2px;
    flex-shrink: 0;
  }

  .link-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    transition: background-color 150ms ease;
    flex-shrink: 0;
  }

  .link-btn:hover {
    background: var(--bg-hover);
  }

  .status-disabled {
    color: #fbbf24;
    font-size: 12px;
  }

  .remote-control-input {
    width: 120px;
    padding: 6px 8px;
    border-radius: 8px;
    border: 1px solid var(--bg-tertiary);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 12px;
  }

  .remote-control-actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .remote-control-qr {
    display: flex;
    gap: 16px;
    padding: 12px 0;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .remote-control-qr img {
    width: 160px;
    height: 160px;
    background: white;
    border-radius: 10px;
    padding: 8px;
  }

  .remote-control-qr-meta {
    display: flex;
    flex-direction: column;
    justify-content: center;
  }

  .remote-control-qr-help {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .remote-control-url {
    display: block;
    margin-top: 6px;
    font-size: 11px;
    color: var(--text-muted);
    word-break: break-all;
  }

  .setting-row-controls {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    align-self: center;
  }

  .section-collapse-btn {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    line-height: 1;
  }

  .plex-server-url-input {
    width: 210px;
    max-width: 210px;
  }

  .plex-two-column-row {
    align-items: center;
  }

  .plex-divider {
    width: 100%;
    height: 1px;
    background: var(--border-subtle);
    margin: 2px 0;
  }

  .plex-code-row {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-end;
  }

  .plex-code-input {
    flex: 1;
    font-family: 'JetBrains Mono', 'Fira Code', monospace;
    letter-spacing: 0.06em;
  }

  .plex-action-btn {
    min-width: 170px;
    background: none;
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
  }

  .plex-action-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
    background: var(--bg-hover);
  }

  .plex-libraries-block {
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: start;
    width: 100%;
    height: auto;
    min-height: unset;
  }

  .setting-row.plex-libraries-block {
    height: auto;
    min-height: 48px;
    align-items: stretch;
    padding-bottom: 14px;
  }

  .plex-libraries-grid {
    border: 1px solid var(--border-subtle);
    border-radius: 10px;
    background: var(--bg-tertiary);
    padding: 10px;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px 16px;
    width: 100%;
    margin: 0 auto;
    max-height: 130px;
    overflow-y: auto;
  }

  .plex-library-item {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    color: var(--text-primary);
  }

  .plex-library-name {
    font-size: 13px;
    font-weight: 500;
  }

  .plex-library-count {
    font-size: 11px;
    color: var(--text-muted);
  }

  .plex-connected-note {
    color: #86efac;
  }

  .plex-error-note {
    color: #fca5a5;
    word-break: break-word;
  }

</style>

<MigrationModal
  isOpen={showMigrationModal}
  onClose={closeMigrationModal}
  totalTracks={legacyTracksCount}
/>

<DACSetupWizard
  isOpen={showDACWizardModal}
  onClose={() => showDACWizardModal = false}
/>

<RemoteControlSetupGuide
  isOpen={showRemoteControlGuide}
  onClose={() => showRemoteControlGuide = false}
/>
