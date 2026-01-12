<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, FolderOpen, ChevronDown, ChevronRight } from 'lucide-svelte';
  import Toggle from '../Toggle.svelte';
  import Dropdown from '../Dropdown.svelte';
  import VolumeSlider from '../VolumeSlider.svelte';
  import Tooltip from '../Tooltip.svelte';
  import {
    getDownloadCacheStats,
    clearDownloadCache,
    openDownloadCacheFolder,
    setDownloadCacheLimit,
    type DownloadCacheStats
  } from '$lib/stores/downloadState';
  import { clearCache as clearLyricsCache } from '$lib/stores/lyricsStore';
  import { getDevicePrettyName } from '$lib/utils/audioDeviceNames';

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

  interface AudioDevice {
    name: string;
    is_default: boolean;
  }

  interface PipewireSink {
    name: string;
    description: string;
    volume: number | null;
    is_default: boolean;
  }

  let { onBack, onLogout, userName = 'User', userEmail = '', subscription = 'Qobuz' }: Props = $props();

  // Cache state (memory cache)
  let cacheStats = $state<CacheStats | null>(null);
  let isClearing = $state(false);

  // Download cache state (offline storage)
  let downloadStats = $state<DownloadCacheStats | null>(null);
  let isClearingDownloads = $state(false);
  let downloadCacheLimit = $state('2 GB'); // Default 2GB

  // Lyrics cache state
  let isClearingLyrics = $state(false);

  // Audio device state
  let audioDevices = $state<AudioDevice[]>([]);
  let pipewireSinks = $state<PipewireSink[]>([]);

  // Map of ALSA name -> PipeWire description (for pretty names)
  const pipewireNameMap = $derived.by(() => {
    const map = new Map<string, string>();
    for (const sink of pipewireSinks) {
      map.set(sink.name, sink.description);
    }
    return map;
  });

  // Get pretty name: prefer PipeWire description, fall back to heuristic
  function getPrettyName(alsaName: string): string {
    // Try PipeWire description first
    const pwDesc = pipewireNameMap.get(alsaName);
    if (pwDesc) return pwDesc;
    // Fall back to heuristic
    return getDevicePrettyName(alsaName);
  }

  // Map of pretty name -> raw name
  const deviceNameMap = $derived.by(() => {
    const map = new Map<string, string>();
    map.set('System Default', 'System Default');
    for (const d of audioDevices) {
      const pretty = getPrettyName(d.name);
      map.set(pretty, d.name);
    }
    return map;
  });

  // Options for dropdown (pretty names from PipeWire when available)
  let audioDeviceOptions = $derived(['System Default', ...audioDevices.map(d => getPrettyName(d.name))]);

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

  // Audio settings
  let streamingQuality = $state('Hi-Res');
  let preferHighest = $state(true);
  let outputDevice = $state('System Default');
  let exclusiveMode = $state(false);
  let dacPassthrough = $state(false);

  // Playback settings
  let gaplessPlayback = $state(true);
  let crossfade = $state(0);
  let normalizeVolume = $state(false);

  // Appearance settings
  let theme = $state('Dark');
  let compactMode = $state(false);
  let language = $state('Auto');

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

    // Load language setting
    const savedLanguage = localStorage.getItem('qbz-language');
    if (savedLanguage) {
      language = savedLanguage;
    }

    // Load cache stats
    loadCacheStats();

    // Load download cache stats
    loadDownloadStats();

    // Load audio devices first (includes PipeWire sinks), then settings
    loadAudioDevices().then(() => loadAudioSettings());

    // Load Last.fm state
    loadLastfmState();

    // Load API keys state
    loadApiKeysState();
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

  function handleLanguageChange(lang: string) {
    language = lang;
    localStorage.setItem('qbz-language', lang);
  }

  interface AudioSettings {
    output_device: string | null;
    exclusive_mode: boolean;
    dac_passthrough: boolean;
    preferred_sample_rate: number | null;
  }

  async function loadAudioDevices() {
    try {
      // Load both ALSA devices and PipeWire sinks in parallel
      const [devices, sinks] = await Promise.all([
        invoke<AudioDevice[]>('get_audio_devices'),
        invoke<PipewireSink[]>('get_pipewire_sinks').catch(() => [] as PipewireSink[])
      ]);

      audioDevices = devices;
      pipewireSinks = sinks;

      // Debug: log raw names and their pretty versions
      console.log('[Audio] ALSA devices:', devices.map(d => d.name));
      console.log('[Audio] PipeWire sinks:', sinks.map(s => ({ name: s.name, desc: s.description })));
      console.log('[Audio] Final device options:', devices.map(d => ({
        raw: d.name,
        pretty: getPrettyName(d.name)
      })));
    } catch (err) {
      console.error('Failed to load audio devices:', err);
    }
  }

  async function loadAudioSettings() {
    try {
      const settings = await invoke<AudioSettings>('get_audio_settings');
      // Convert raw name to pretty name for display (uses PipeWire when available)
      if (settings.output_device) {
        outputDevice = getPrettyName(settings.output_device);
      } else {
        outputDevice = 'System Default';
      }
      exclusiveMode = settings.exclusive_mode;
      dacPassthrough = settings.dac_passthrough;
    } catch (err) {
      console.error('Failed to load audio settings:', err);
    }
  }

  // Get current raw device name for reinit
  function getCurrentRawDevice(): string | null {
    const rawName = deviceNameMap.get(outputDevice) ?? outputDevice;
    return rawName === 'System Default' ? null : rawName;
  }

  async function handleOutputDeviceChange(prettyName: string) {
    outputDevice = prettyName;
    // Convert pretty name back to raw name for saving
    const rawName = deviceNameMap.get(prettyName) ?? prettyName;
    const deviceToUse = rawName === 'System Default' ? null : rawName;
    try {
      await invoke('set_audio_output_device', { device: deviceToUse });
      // Reinitialize audio to apply the change immediately
      await invoke('reinit_audio_device', { device: deviceToUse });
      console.log('Audio output device changed and reinitialized:', rawName, '(displayed as:', prettyName, ')');
    } catch (err) {
      console.error('Failed to change audio output device:', err);
    }
  }

  async function handleExclusiveModeChange(enabled: boolean) {
    exclusiveMode = enabled;
    try {
      await invoke('set_audio_exclusive_mode', { enabled });
      // Reinitialize audio to apply/release exclusive mode immediately
      await invoke('reinit_audio_device', { device: getCurrentRawDevice() });
      console.log('Exclusive mode changed and audio reinitialized:', enabled);
    } catch (err) {
      console.error('Failed to change exclusive mode:', err);
    }
  }

  async function handleDacPassthroughChange(enabled: boolean) {
    dacPassthrough = enabled;
    try {
      await invoke('set_audio_dac_passthrough', { enabled });
      // DAC passthrough may also require reinit for proper effect
      await invoke('reinit_audio_device', { device: getCurrentRawDevice() });
      console.log('DAC passthrough changed and audio reinitialized:', enabled);
    } catch (err) {
      console.error('Failed to change DAC passthrough:', err);
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
      // Set the limit dropdown to match current limit
      if (downloadStats.limitBytes) {
        const limitGb = downloadStats.limitBytes / (1024 * 1024 * 1024);
        if (limitGb <= 0.5) downloadCacheLimit = '500 MB';
        else if (limitGb <= 1) downloadCacheLimit = '1 GB';
        else if (limitGb <= 2) downloadCacheLimit = '2 GB';
        else if (limitGb <= 5) downloadCacheLimit = '5 GB';
        else if (limitGb <= 10) downloadCacheLimit = '10 GB';
        else downloadCacheLimit = 'Unlimited';
      } else {
        downloadCacheLimit = 'Unlimited';
      }
    } catch (err) {
      console.error('Failed to load download stats:', err);
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

  async function handleDownloadLimitChange(limit: string) {
    downloadCacheLimit = limit;
    let limitMb: number | null = null;

    switch (limit) {
      case '500 MB': limitMb = 500; break;
      case '1 GB': limitMb = 1024; break;
      case '2 GB': limitMb = 2048; break;
      case '5 GB': limitMb = 5120; break;
      case '10 GB': limitMb = 10240; break;
      case 'Unlimited': limitMb = null; break;
    }

    try {
      await setDownloadCacheLimit(limitMb);
      await loadDownloadStats();
    } catch (err) {
      console.error('Failed to set download limit:', err);
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
</script>

<div class="settings-view">
  <!-- Header -->
  <div class="header">
    {#if onBack}
      <button class="back-btn" onclick={onBack}>
        <ArrowLeft size={16} />
        <span>Back</span>
      </button>
    {/if}
    <h1 class="title">Settings</h1>
  </div>

  <!-- Account Section -->
  <section class="section">
    <h3 class="section-title">Account</h3>
    <div class="account-card">
      <div class="avatar">{userName.charAt(0).toUpperCase()}</div>
      <div class="account-info">
        <div class="username">{userName}</div>
        {#if userEmail}
          <div class="email">{userEmail}</div>
        {/if}
        <div class="subscription">{subscription}</div>
      </div>
      <button class="logout-btn" onclick={onLogout}>Logout</button>
    </div>
  </section>

  <!-- Audio Section -->
  <section class="section">
    <h3 class="section-title">Audio</h3>
    <div class="setting-row">
      <span class="setting-label">Streaming Quality</span>
      <Dropdown
        value={streamingQuality}
        options={['MP3', 'CD Quality', 'Hi-Res', 'Hi-Res+']}
        onchange={handleQualityChange}
      />
    </div>
    <div class="setting-row">
      <span class="setting-label">Prefer highest available</span>
      <Toggle enabled={preferHighest} onchange={handlePreferHighestChange} />
    </div>
    <div class="setting-row">
      <div class="label-with-tooltip">
        <span class="setting-label">Output Device</span>
        <Tooltip text="Select your preferred audio output device. Changes take effect on app restart." />
      </div>
      <Dropdown
        value={outputDevice}
        options={audioDeviceOptions.length > 1 ? audioDeviceOptions : ['System Default']}
        onchange={handleOutputDeviceChange}
        wide
        expandLeft
        compact
      />
    </div>
    <div class="setting-row">
      <div class="label-with-tooltip">
        <span class="setting-label">Exclusive Mode</span>
        <Tooltip text="Locks the audio device for exclusive use by QBZ for better quality" />
      </div>
      <Toggle enabled={exclusiveMode} onchange={handleExclusiveModeChange} />
    </div>
    <div class="setting-row">
      <div class="label-with-tooltip">
        <span class="setting-label">DAC Passthrough</span>
        <Tooltip text="Bypass the system audio mixer to send audio directly to your DAC at its native sample rate. Recommended for external DACs." />
      </div>
      <Toggle enabled={dacPassthrough} onchange={handleDacPassthroughChange} />
    </div>
    <div class="setting-row last">
      <span class="setting-label">Sample Rate</span>
      <span class="setting-value">192 kHz</span>
    </div>
  </section>

  <!-- Playback Section -->
  <section class="section">
    <h3 class="section-title">Playback</h3>
    <div class="setting-row">
      <span class="setting-label">Gapless Playback</span>
      <Toggle enabled={gaplessPlayback} onchange={(v) => (gaplessPlayback = v)} />
    </div>
    <div class="setting-row">
      <span class="setting-label">Crossfade</span>
      <div class="slider-container">
        <VolumeSlider value={crossfade} onchange={(v) => (crossfade = v)} max={12} showValue />
      </div>
    </div>
    <div class="setting-row last">
      <span class="setting-label">Normalize Volume</span>
      <Toggle enabled={normalizeVolume} onchange={(v) => (normalizeVolume = v)} />
    </div>
  </section>

  <!-- Appearance Section -->
  <section class="section">
    <h3 class="section-title">Appearance</h3>
    <div class="setting-row">
      <span class="setting-label">Theme</span>
      <Dropdown
        value={theme}
        options={['Dark', 'Light', 'OLED Black', 'Warm']}
        onchange={handleThemeChange}
      />
    </div>
    <div class="setting-row">
      <span class="setting-label">Language</span>
      <Dropdown
        value={language}
        options={['Auto', 'English', 'Español', 'Français', 'Deutsch', 'Italiano', 'Português']}
        onchange={handleLanguageChange}
      />
    </div>
    <div class="setting-row last">
      <span class="setting-label">Compact Mode</span>
      <Toggle enabled={compactMode} onchange={(v) => (compactMode = v)} />
    </div>
  </section>

  <!-- Integrations Section -->
  <section class="section">
    <h3 class="section-title">Integrations</h3>

    {#if lastfmConnected}
      <div class="setting-row">
        <div class="lastfm-connected">
          <span class="setting-label">Last.fm</span>
          <span class="lastfm-username">Connected as {lastfmUsername}</span>
        </div>
        <button
          class="connect-btn connected"
          onclick={handleLastfmDisconnect}
        >
          Disconnect
        </button>
      </div>
      <div class="setting-row last">
        <span class="setting-label">Scrobbling</span>
        <Toggle enabled={scrobbling} onchange={handleScrobblingChange} />
      </div>
    {:else}
      <div class="setting-row" class:last={!showLastfmConfig && !lastfmAuthToken}>
        <span class="setting-label">Last.fm</span>
        <button
          class="connect-btn"
          onclick={handleLastfmConnect}
          disabled={lastfmConnecting}
        >
          {lastfmConnecting ? 'Connecting...' : 'Connect'}
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
  <section class="section">
    <h3 class="section-title">Storage</h3>
    <div class="setting-row">
      <span class="setting-label">Cache Size</span>
      <span class="setting-value">
        {#if cacheStats}
          {formatBytes(cacheStats.current_size_bytes)} / {formatBytes(cacheStats.max_size_bytes)}
        {:else}
          Loading...
        {/if}
      </span>
    </div>
    <div class="setting-row">
      <span class="setting-label">Cached Tracks</span>
      <span class="setting-value">
        {#if cacheStats}
          {cacheStats.cached_tracks} tracks
        {:else}
          -
        {/if}
      </span>
    </div>
    <div class="setting-row last">
      <span class="setting-label">Clear Cache</span>
      <button
        class="clear-btn"
        onclick={handleClearCache}
        disabled={isClearing || !cacheStats || cacheStats.current_size_bytes === 0}
      >
        {isClearing ? 'Clearing...' : 'Clear'}
      </button>
    </div>
  </section>

  <!-- Downloads Section (Offline Storage) -->
  <section class="section">
    <h3 class="section-title">Downloads</h3>
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
      <span class="setting-label">Storage Limit</span>
      <Dropdown
        value={downloadCacheLimit}
        options={['500 MB', '1 GB', '2 GB', '5 GB', '10 GB', 'Unlimited']}
        onchange={handleDownloadLimitChange}
      />
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
        title="Open download cache folder"
      >
        <FolderOpen size={16} />
        <span>Open</span>
      </button>
    </div>
  </section>

  <!-- Lyrics Section -->
  <section class="section">
    <h3 class="section-title">Lyrics</h3>
    <div class="setting-row">
      <span class="setting-label">Provider</span>
      <span class="setting-value">LRCLIB / lyrics.ovh</span>
    </div>
    <div class="setting-row last">
      <span class="setting-label">Clear Lyrics Cache</span>
      <button
        class="clear-btn"
        onclick={handleClearLyricsCache}
        disabled={isClearingLyrics}
      >
        {isClearingLyrics ? 'Clearing...' : 'Clear'}
      </button>
    </div>
  </section>

  <!-- API Keys Section (collapsible) -->
  <section class="section api-keys-section">
    <button
      class="section-title-btn"
      onclick={() => apiKeysExpanded = !apiKeysExpanded}
    >
      {#if apiKeysExpanded}
        <ChevronDown size={18} />
      {:else}
        <ChevronRight size={18} />
      {/if}
      <span>API Keys</span>
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

<style>
  .settings-view {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: 24px 32px;
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
    font-size: 28px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .section {
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

  .setting-value {
    font-size: 14px;
    color: var(--text-muted);
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
</style>
