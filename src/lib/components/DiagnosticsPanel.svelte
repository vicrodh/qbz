<script lang="ts">
  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { writeText as copyToClipboard } from '@tauri-apps/plugin-clipboard-manager';
  import { t } from '$lib/i18n';
  import { RefreshCw, Copy, Check, ChevronDown, ChevronRight, Radio, LoaderCircle } from 'lucide-svelte';
  import {
    getCurrentTrack,
    getIsPlaying,
    getCurrentTime,
    getDuration,
    getVolume
  } from '$lib/stores/playerStore';
  import type { QconnectConnectionStatus, QconnectSessionSnapshot } from '$lib/services/qconnectRuntime';

  interface RuntimeDiagnostics {
    audioOutputDevice: string | null;
    audioBackendType: string | null;
    audioExclusiveMode: boolean;
    audioDacPassthrough: boolean;
    audioPreferredSampleRate: number | null;
    audioAlsaPlugin: string | null;
    audioAlsaHardwareVolume: boolean;
    audioNormalizationEnabled: boolean;
    audioNormalizationTargetLufs: number;
    audioGaplessEnabled: boolean;
    audioPwForceBitperfect: boolean;
    audioStreamBufferSeconds: number;
    audioStreamingOnly: boolean;
    gfxHardwareAcceleration: boolean;
    gfxForceX11: boolean;
    gfxGdkScale: string | null;
    gfxGdkDpiScale: string | null;
    gfxGskRenderer: string | null;
    runtimeUsingFallback: boolean;
    runtimeIsWayland: boolean;
    runtimeHasNvidia: boolean;
    runtimeHasAmd: boolean;
    runtimeHasIntel: boolean;
    runtimeIsVm: boolean;
    runtimeHwAccelEnabled: boolean;
    runtimeForceX11Active: boolean;
    devForceDmabuf: boolean;
    envWebkitDisableDmabuf: string | null;
    envWebkitDisableCompositing: string | null;
    envGdkBackend: string | null;
    envGskRenderer: string | null;
    envLibglAlwaysSoftware: string | null;
    envWaylandDisplay: string | null;
    envXdgSessionType: string | null;
    appVersion: string;
  }

  interface DiagRow {
    label: string;
    saved: string;
    runtime: string;
    status: 'match' | 'mismatch' | 'info';
  }

  interface SystemInfo {
    os: string;
    arch: string;
    kernelVersion: string | null;
    distroId: string | null;
    distroVersionId: string | null;
    distroPrettyName: string | null;
    installMethod: string;
    flatpakRuntime: string | null;
    flatpakRuntimeVersion: string | null;
    webkit2gtkVersion: string | null;
    gtkVersion: string | null;
    glibcVersion: string | null;
    alsaVersion: string | null;
    pipewireVersion: string | null;
    pulseaudioVersion: string | null;
  }

  interface PlaybackSnapshot {
    isPlaying: boolean;
    volumePercent: number;
    positionSecs: number;
    durationSecs: number;
    hasTrack: boolean;
    trackTitle: string | null;
    trackArtist: string | null;
    trackAlbum: string | null;
    trackQuality: string | null;
    trackFormat: string | null;
    trackBitDepth: number | null;
    trackSamplingRate: number | null;
    trackIsLocal: boolean | null;
    trackSource: string | null;
  }

  interface QconnectDiag {
    running: boolean;
    transport_connected: boolean;
    hasEndpoint: boolean;
    lastError: string | null;
    role: 'none' | 'controller' | 'local-renderer' | 'observer';
    activeRendererName: string | null;
    activeRendererBrand: string | null;
    activeRendererModel: string | null;
    rendererCount: number;
  }

  interface CastDeviceBrief {
    name: string;
    protocol: string;
  }

  interface CastScanResult {
    chromecastCount: number;
    dlnaCount: number;
    devices: CastDeviceBrief[];
    durationMs: number;
    error: string | null;
  }

  let diagnostics = $state<RuntimeDiagnostics | null>(null);
  let systemInfo = $state<SystemInfo | null>(null);
  let playback = $state<PlaybackSnapshot | null>(null);
  let qconnect = $state<QconnectDiag | null>(null);
  let castScan = $state<CastScanResult | null>(null);
  let castScanning = $state(false);
  let loading = $state(false);
  let copied = $state(false);
  let error = $state<string | null>(null);
  let panelOpen = $state(false);

  let audioOpen = $state(true);
  let graphicsOpen = $state(true);
  let envOpen = $state(false);
  let playbackOpen = $state(true);
  let qconnectOpen = $state(true);
  let castOpen = $state(true);
  let systemOpen = $state(true);

  const CAST_SCAN_DURATION_MS = 10000;

  // Redact things that look like UUIDs / hex IDs / tokens (defensive: keep paste
  // logs free of anything CodeQL or paste-site filters might flag as secrets).
  function redactIdLike(value: string | null | undefined): string | null {
    if (!value) return null;
    // Strip 32+ char hex strings and UUID shapes.
    return value
      .replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, '<uuid>')
      .replace(/\b[0-9a-f]{32,}\b/gi, '<hex>');
  }

  function snapshotPlayback(): PlaybackSnapshot {
    const track = getCurrentTrack();
    return {
      isPlaying: getIsPlaying(),
      volumePercent: Math.round(getVolume()),
      positionSecs: Math.round(getCurrentTime()),
      durationSecs: Math.round(getDuration()),
      hasTrack: !!track,
      trackTitle: track?.title ?? null,
      trackArtist: track?.artist ?? null,
      trackAlbum: track?.album ?? null,
      trackQuality: track?.quality ?? null,
      trackFormat: track?.format ?? null,
      trackBitDepth: track?.bitDepth ?? null,
      trackSamplingRate: track?.samplingRate ?? null,
      trackIsLocal: track?.isLocal ?? null,
      trackSource: track?.source ?? null,
    };
  }

  async function snapshotQconnect(): Promise<QconnectDiag> {
    const empty: QconnectDiag = {
      running: false,
      transport_connected: false,
      hasEndpoint: false,
      lastError: null,
      role: 'none',
      activeRendererName: null,
      activeRendererBrand: null,
      activeRendererModel: null,
      rendererCount: 0,
    };
    try {
      const status = await invoke<QconnectConnectionStatus>('v2_qconnect_get_status');
      const session = await invoke<QconnectSessionSnapshot>('v2_qconnect_session_snapshot');
      const out: QconnectDiag = { ...empty };
      out.running = !!status.running;
      out.transport_connected = !!status.transport_connected;
      out.hasEndpoint = !!status.endpoint_url;
      out.lastError = status.last_error ? redactIdLike(String(status.last_error)) : null;
      out.rendererCount = session?.renderers?.length ?? 0;
      const activeId = session?.active_renderer_id ?? null;
      const localId = session?.local_renderer_id ?? null;
      if (activeId !== null && activeId !== undefined) {
        const active = session?.renderers?.find((r) => r.renderer_id === activeId);
        out.activeRendererName = active?.friendly_name ?? null;
        out.activeRendererBrand = active?.brand ?? null;
        out.activeRendererModel = active?.model ?? null;
        if (activeId === localId) {
          out.role = 'local-renderer';
        } else {
          out.role = 'controller';
        }
      } else if (out.running || out.transport_connected) {
        out.role = 'observer';
      }
      return out;
    } catch {
      return empty;
    }
  }

  async function runCastScan(): Promise<void> {
    if (castScanning) return;
    castScanning = true;
    const started = Date.now();
    let startError: string | null = null;
    try {
      await Promise.allSettled([
        invoke('v2_cast_start_discovery'),
        invoke('v2_dlna_start_discovery'),
      ]);
    } catch (err) {
      startError = String(err);
    }
    await new Promise((resolve) => setTimeout(resolve, CAST_SCAN_DURATION_MS));
    let chromecastDevices: { name?: string }[] = [];
    let dlnaDevices: { name?: string }[] = [];
    try {
      const [cc, dlna] = await Promise.allSettled([
        invoke<{ name?: string }[]>('v2_cast_get_devices'),
        invoke<{ name?: string }[]>('v2_dlna_get_devices'),
      ]);
      if (cc.status === 'fulfilled') chromecastDevices = cc.value ?? [];
      if (dlna.status === 'fulfilled') dlnaDevices = dlna.value ?? [];
    } catch {
      // ignore
    }
    try {
      await Promise.allSettled([
        invoke('v2_cast_stop_discovery'),
        invoke('v2_dlna_stop_discovery'),
      ]);
    } catch {
      // ignore
    }
    const devices: CastDeviceBrief[] = [
      ...chromecastDevices.map((d) => ({ name: d.name ?? '<unnamed>', protocol: 'chromecast' })),
      ...dlnaDevices.map((d) => ({ name: d.name ?? '<unnamed>', protocol: 'dlna' })),
    ];
    castScan = {
      chromecastCount: chromecastDevices.length,
      dlnaCount: dlnaDevices.length,
      devices,
      durationMs: Date.now() - started,
      error: startError,
    };
    castScanning = false;
  }

  function bool(val: boolean): string {
    return val ? 'ON' : 'OFF';
  }

  function str(val: string | null | undefined): string {
    return val ?? '—';
  }

  function matchStatus(saved: string, runtime: string): 'match' | 'mismatch' | 'info' {
    if (saved === '—' || runtime === '—') return 'info';
    return saved === runtime ? 'match' : 'mismatch';
  }

  function getAudioRows(diag: RuntimeDiagnostics): DiagRow[] {
    return [
      { label: 'Output Device', saved: str(diag.audioOutputDevice), runtime: '—', status: 'info' },
      { label: 'Backend', saved: str(diag.audioBackendType), runtime: '—', status: 'info' },
      { label: 'Exclusive Mode', saved: bool(diag.audioExclusiveMode), runtime: '—', status: 'info' },
      { label: 'DAC Passthrough', saved: bool(diag.audioDacPassthrough), runtime: '—', status: 'info' },
      { label: 'Preferred Sample Rate', saved: diag.audioPreferredSampleRate ? `${diag.audioPreferredSampleRate} Hz` : 'Auto', runtime: '—', status: 'info' },
      { label: 'ALSA Plugin', saved: str(diag.audioAlsaPlugin), runtime: '—', status: 'info' },
      { label: 'ALSA HW Volume', saved: bool(diag.audioAlsaHardwareVolume), runtime: '—', status: 'info' },
      { label: 'Normalization', saved: bool(diag.audioNormalizationEnabled), runtime: '—', status: 'info' },
      { label: 'Normalization Target', saved: `${diag.audioNormalizationTargetLufs} LUFS`, runtime: '—', status: 'info' },
      { label: 'Gapless', saved: bool(diag.audioGaplessEnabled), runtime: '—', status: 'info' },
      { label: 'PW Force Bitperfect', saved: bool(diag.audioPwForceBitperfect), runtime: '—', status: 'info' },
      { label: 'Stream Buffer', saved: `${diag.audioStreamBufferSeconds}s`, runtime: '—', status: 'info' },
      { label: 'Streaming Only', saved: bool(diag.audioStreamingOnly), runtime: '—', status: 'info' },
    ];
  }

  function getGraphicsRows(diag: RuntimeDiagnostics): DiagRow[] {
    const hwSaved = bool(diag.gfxHardwareAcceleration);
    const hwRuntime = bool(diag.runtimeHwAccelEnabled);
    const x11Saved = bool(diag.gfxForceX11);
    const x11Runtime = bool(diag.runtimeForceX11Active);
    const compositing = diag.envWebkitDisableCompositing === '1' ? 'DISABLED' : 'ENABLED';
    const dmabuf = diag.envWebkitDisableDmabuf === '1' ? 'DISABLED' : 'ENABLED';

    return [
      { label: 'Hardware Acceleration', saved: hwSaved, runtime: hwRuntime, status: matchStatus(hwSaved, hwRuntime) },
      { label: 'Force DMA-BUF', saved: bool(diag.devForceDmabuf), runtime: dmabuf, status: diag.devForceDmabuf === (dmabuf === 'ENABLED') ? 'match' : 'mismatch' },
      { label: 'Force X11', saved: x11Saved, runtime: x11Runtime, status: matchStatus(x11Saved, x11Runtime) },
      { label: 'GSK Renderer', saved: str(diag.gfxGskRenderer), runtime: str(diag.envGskRenderer), status: matchStatus(str(diag.gfxGskRenderer), str(diag.envGskRenderer)) },
      { label: 'GDK Scale', saved: str(diag.gfxGdkScale), runtime: '—', status: 'info' },
      { label: 'GDK DPI Scale', saved: str(diag.gfxGdkDpiScale), runtime: '—', status: 'info' },
      { label: 'Compositing Mode', saved: '—', runtime: compositing, status: 'info' },
      { label: 'GPU: NVIDIA', saved: '—', runtime: diag.runtimeHasNvidia ? 'Detected' : 'No', status: 'info' },
      { label: 'GPU: Intel', saved: '—', runtime: diag.runtimeHasIntel ? 'Detected' : 'No', status: 'info' },
      { label: 'GPU: AMD', saved: '—', runtime: diag.runtimeHasAmd ? 'Detected' : 'No', status: 'info' },
      { label: 'Wayland', saved: '—', runtime: diag.runtimeIsWayland ? 'Yes' : 'No (X11)', status: 'info' },
      { label: 'VM', saved: '—', runtime: diag.runtimeIsVm ? 'Yes' : 'No', status: 'info' },
      { label: 'Using Fallback', saved: '—', runtime: bool(diag.runtimeUsingFallback), status: diag.runtimeUsingFallback ? 'mismatch' : 'info' },
    ];
  }

  function getEnvRows(diag: RuntimeDiagnostics): DiagRow[] {
    return [
      { label: 'WEBKIT_DISABLE_DMABUF_RENDERER', saved: '—', runtime: str(diag.envWebkitDisableDmabuf), status: 'info' },
      { label: 'WEBKIT_DISABLE_COMPOSITING_MODE', saved: '—', runtime: str(diag.envWebkitDisableCompositing), status: 'info' },
      { label: 'GDK_BACKEND', saved: '—', runtime: str(diag.envGdkBackend), status: 'info' },
      { label: 'GSK_RENDERER', saved: '—', runtime: str(diag.envGskRenderer), status: 'info' },
      { label: 'LIBGL_ALWAYS_SOFTWARE', saved: '—', runtime: str(diag.envLibglAlwaysSoftware), status: 'info' },
      { label: 'WAYLAND_DISPLAY', saved: '—', runtime: str(diag.envWaylandDisplay), status: 'info' },
      { label: 'XDG_SESSION_TYPE', saved: '—', runtime: str(diag.envXdgSessionType), status: 'info' },
    ];
  }

  async function loadDiagnostics() {
    loading = true;
    error = null;
    try {
      const [diagRes, sysRes, qcRes] = await Promise.allSettled([
        invoke<RuntimeDiagnostics>('v2_get_runtime_diagnostics'),
        invoke<SystemInfo>('v2_get_system_info'),
        snapshotQconnect(),
      ]);
      if (diagRes.status === 'fulfilled') {
        diagnostics = diagRes.value;
      } else {
        error = String(diagRes.reason);
      }
      if (sysRes.status === 'fulfilled') {
        systemInfo = sysRes.value;
      }
      if (qcRes.status === 'fulfilled') {
        qconnect = qcRes.value;
      }
      playback = snapshotPlayback();
    } finally {
      loading = false;
    }
  }

  async function exportToClipboard() {
    if (!diagnostics) return;
    try {
      const exportData = {
        ...diagnostics,
        systemInfo,
        playback,
        qconnect,
        castScan,
        exportedAt: new Date().toISOString(),
      };
      await copyToClipboard(JSON.stringify(exportData, null, 2));
      copied = true;
      setTimeout(() => { copied = false; }, 1500);
    } catch {
      try {
        await navigator.clipboard.writeText(
          JSON.stringify({ ...diagnostics, systemInfo, playback, qconnect, castScan }, null, 2)
        );
        copied = true;
        setTimeout(() => { copied = false; }, 1500);
      } catch { /* ignore */ }
    }
  }

  function togglePanel() {
    panelOpen = !panelOpen;
    if (panelOpen && !diagnostics) {
      loadDiagnostics();
    }
  }

  onDestroy(() => {
    // (intentional) don't leave any subscriptions behind
  });

  function systemRows(s: SystemInfo): DiagRow[] {
    const rows: DiagRow[] = [
      { label: 'OS', saved: '—', runtime: s.os, status: 'info' },
      { label: 'Arch', saved: '—', runtime: s.arch, status: 'info' },
      { label: 'Kernel', saved: '—', runtime: str(s.kernelVersion), status: 'info' },
      { label: 'Distro', saved: '—', runtime: str(s.distroPrettyName), status: 'info' },
      { label: 'Distro ID', saved: '—', runtime: str(s.distroId), status: 'info' },
      { label: 'Distro Version', saved: '—', runtime: str(s.distroVersionId), status: 'info' },
      { label: 'Install Method', saved: '—', runtime: s.installMethod, status: 'info' },
    ];
    if (s.flatpakRuntime) {
      rows.push({ label: 'Flatpak Runtime', saved: '—', runtime: `${s.flatpakRuntime} ${str(s.flatpakRuntimeVersion)}`, status: 'info' });
    }
    rows.push(
      { label: 'WebKit2GTK', saved: '—', runtime: str(s.webkit2gtkVersion), status: 'info' },
      { label: 'GTK', saved: '—', runtime: str(s.gtkVersion), status: 'info' },
      { label: 'glibc', saved: '—', runtime: str(s.glibcVersion), status: 'info' },
      { label: 'ALSA', saved: '—', runtime: str(s.alsaVersion), status: 'info' },
      { label: 'PipeWire', saved: '—', runtime: str(s.pipewireVersion), status: 'info' },
      { label: 'PulseAudio', saved: '—', runtime: str(s.pulseaudioVersion), status: 'info' },
    );
    return rows;
  }

  function playbackRows(p: PlaybackSnapshot): DiagRow[] {
    return [
      { label: 'Playing', saved: '—', runtime: bool(p.isPlaying), status: 'info' },
      { label: 'Volume', saved: '—', runtime: `${p.volumePercent}%`, status: 'info' },
      { label: 'Position / Duration', saved: '—', runtime: `${p.positionSecs}s / ${p.durationSecs}s`, status: 'info' },
      { label: 'Has Track', saved: '—', runtime: bool(p.hasTrack), status: 'info' },
      { label: 'Track Title', saved: '—', runtime: str(p.trackTitle), status: 'info' },
      { label: 'Track Artist', saved: '—', runtime: str(p.trackArtist), status: 'info' },
      { label: 'Track Album', saved: '—', runtime: str(p.trackAlbum), status: 'info' },
      { label: 'Track Source', saved: '—', runtime: str(p.trackSource), status: 'info' },
      { label: 'Track Is Local', saved: '—', runtime: p.trackIsLocal === null ? '—' : bool(p.trackIsLocal), status: 'info' },
      { label: 'Track Quality', saved: '—', runtime: str(p.trackQuality), status: 'info' },
      { label: 'Track Format', saved: '—', runtime: str(p.trackFormat), status: 'info' },
      { label: 'Track Bit Depth', saved: '—', runtime: p.trackBitDepth ? `${p.trackBitDepth}-bit` : '—', status: 'info' },
      { label: 'Track Sample Rate', saved: '—', runtime: p.trackSamplingRate ? `${p.trackSamplingRate} Hz` : '—', status: 'info' },
    ];
  }

  function qconnectRows(q: QconnectDiag): DiagRow[] {
    return [
      { label: 'Running', saved: '—', runtime: bool(q.running), status: 'info' },
      { label: 'Transport Connected', saved: '—', runtime: bool(q.transport_connected), status: 'info' },
      { label: 'Has Endpoint', saved: '—', runtime: bool(q.hasEndpoint), status: 'info' },
      { label: 'Role', saved: '—', runtime: q.role, status: 'info' },
      { label: 'Active Renderer', saved: '—', runtime: str(q.activeRendererName), status: 'info' },
      { label: 'Renderer Brand', saved: '—', runtime: str(q.activeRendererBrand), status: 'info' },
      { label: 'Renderer Model', saved: '—', runtime: str(q.activeRendererModel), status: 'info' },
      { label: 'Visible Renderers', saved: '—', runtime: String(q.rendererCount), status: 'info' },
      { label: 'Last Error', saved: '—', runtime: str(q.lastError), status: 'info' },
    ];
  }

  function castRows(c: CastScanResult): DiagRow[] {
    const rows: DiagRow[] = [
      { label: 'Chromecast devices', saved: '—', runtime: String(c.chromecastCount), status: 'info' },
      { label: 'DLNA devices', saved: '—', runtime: String(c.dlnaCount), status: 'info' },
      { label: 'Scan duration', saved: '—', runtime: `${Math.round(c.durationMs / 1000)}s`, status: 'info' },
    ];
    if (c.error) {
      rows.push({ label: 'Scan error', saved: '—', runtime: c.error, status: 'mismatch' });
    }
    for (const d of c.devices) {
      rows.push({ label: `• ${d.protocol}`, saved: '—', runtime: d.name, status: 'info' });
    }
    return rows;
  }
</script>

<div class="diagnostics-panel">
  <button class="section-toggle panel-toggle" onclick={togglePanel}>
    {#if panelOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
    <h4 class="subsection-title" style="margin:0">{$t('settings.developer.diagnostics.title')}</h4>
    <small class="setting-note" style="margin:0">{$t('settings.developer.diagnostics.description')}</small>
  </button>

  {#if panelOpen}
  <div class="diag-body">
  <div class="diag-actions">
    <button class="diag-btn" onclick={loadDiagnostics} disabled={loading}>
      <RefreshCw size={14} class={loading ? 'spinning' : ''} />
      {$t('settings.developer.diagnostics.refresh')}
    </button>
    <button class="diag-btn" onclick={exportToClipboard} disabled={!diagnostics}>
      {#if copied}
        <Check size={14} />
        {$t('settings.developer.diagnostics.exported')}
      {:else}
        <Copy size={14} />
        {$t('settings.developer.diagnostics.export')}
      {/if}
    </button>
  </div>

  {#if error}
    <div class="diag-error">{error}</div>
  {/if}

  {#if diagnostics}
    <div class="diag-version">QBZ v{diagnostics.appVersion}</div>

    <!-- System Info Section -->
    {#if systemInfo}
      <button class="section-toggle" onclick={() => systemOpen = !systemOpen}>
        {#if systemOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
        {$t('settings.developer.diagnostics.sectionSystem')}
      </button>
      {#if systemOpen}
        <table class="diag-table">
          <thead>
            <tr>
              <th>{$t('settings.developer.diagnostics.colSetting')}</th>
              <th colspan="2">{$t('settings.developer.diagnostics.colRuntime')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each systemRows(systemInfo) as row (row.label)}
              <tr>
                <td class="label-cell">{row.label}</td>
                <td class="value-cell" colspan="2">{row.runtime}</td>
                <td class="status-cell info">·</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}

    <!-- Playback Section -->
    {#if playback}
      <button class="section-toggle" onclick={() => playbackOpen = !playbackOpen}>
        {#if playbackOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
        {$t('settings.developer.diagnostics.sectionPlayback')}
      </button>
      {#if playbackOpen}
        <table class="diag-table">
          <thead>
            <tr>
              <th>{$t('settings.developer.diagnostics.colSetting')}</th>
              <th colspan="2">{$t('settings.developer.diagnostics.colRuntime')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each playbackRows(playback) as row (row.label)}
              <tr>
                <td class="label-cell">{row.label}</td>
                <td class="value-cell" colspan="2">{row.runtime}</td>
                <td class="status-cell info">·</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}

    <!-- Qobuz Connect Section -->
    {#if qconnect}
      <button class="section-toggle" onclick={() => qconnectOpen = !qconnectOpen}>
        {#if qconnectOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
        {$t('settings.developer.diagnostics.sectionQconnect')}
      </button>
      {#if qconnectOpen}
        <table class="diag-table">
          <thead>
            <tr>
              <th>{$t('settings.developer.diagnostics.colSetting')}</th>
              <th colspan="2">{$t('settings.developer.diagnostics.colRuntime')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each qconnectRows(qconnect) as row (row.label)}
              <tr>
                <td class="label-cell">{row.label}</td>
                <td class="value-cell" colspan="2">{row.runtime}</td>
                <td class="status-cell info">·</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}

    <!-- Cast Discovery Section -->
    <button class="section-toggle" onclick={() => castOpen = !castOpen}>
      {#if castOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
      {$t('settings.developer.diagnostics.sectionCast')}
    </button>
    {#if castOpen}
      <div class="diag-actions" style="margin: 8px 0 12px;">
        <button class="diag-btn" onclick={runCastScan} disabled={castScanning}>
          <Radio size={14} class={castScanning ? 'spinning' : ''} />
          {castScanning
            ? $t('settings.developer.diagnostics.castScanning')
            : $t('settings.developer.diagnostics.castScan')}
        </button>
      </div>
      {#if castScanning && !castScan}
        <div class="cast-scan-placeholder">
          <LoaderCircle size={20} class="spinning" />
          <span>{$t('settings.developer.diagnostics.castScanning')}</span>
        </div>
      {/if}
      {#if castScan}
        <table class="diag-table">
          <thead>
            <tr>
              <th>{$t('settings.developer.diagnostics.colSetting')}</th>
              <th colspan="2">{$t('settings.developer.diagnostics.colRuntime')}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each castRows(castScan) as row (row.label + row.runtime)}
              <tr>
                <td class="label-cell">{row.label}</td>
                <td class="value-cell" colspan="2">{row.runtime}</td>
                <td class="status-cell {row.status}">{row.status === 'mismatch' ? '✗' : '·'}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
    {/if}

    <!-- Audio Section -->
    <button class="section-toggle" onclick={() => audioOpen = !audioOpen}>
      {#if audioOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
      {$t('settings.developer.diagnostics.sectionAudio')}
    </button>
    {#if audioOpen}
      <table class="diag-table">
        <thead>
          <tr>
            <th>{$t('settings.developer.diagnostics.colSetting')}</th>
            <th>{$t('settings.developer.diagnostics.colSaved')}</th>
            <th>{$t('settings.developer.diagnostics.colRuntime')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each getAudioRows(diagnostics) as row (row.label)}
            <tr>
              <td class="label-cell">{row.label}</td>
              <td class="value-cell">{row.saved}</td>
              <td class="value-cell">{row.runtime}</td>
              <td class="status-cell {row.status}">{row.status === 'match' ? '✓' : row.status === 'mismatch' ? '✗' : '·'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

    <!-- Graphics Section -->
    <button class="section-toggle" onclick={() => graphicsOpen = !graphicsOpen}>
      {#if graphicsOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
      {$t('settings.developer.diagnostics.sectionGraphics')}
    </button>
    {#if graphicsOpen}
      <table class="diag-table">
        <thead>
          <tr>
            <th>{$t('settings.developer.diagnostics.colSetting')}</th>
            <th>{$t('settings.developer.diagnostics.colSaved')}</th>
            <th>{$t('settings.developer.diagnostics.colRuntime')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each getGraphicsRows(diagnostics) as row (row.label)}
            <tr>
              <td class="label-cell">{row.label}</td>
              <td class="value-cell">{row.saved}</td>
              <td class="value-cell">{row.runtime}</td>
              <td class="status-cell {row.status}">{row.status === 'match' ? '✓' : row.status === 'mismatch' ? '✗' : '·'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

    <!-- Environment Section -->
    <button class="section-toggle" onclick={() => envOpen = !envOpen}>
      {#if envOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
      {$t('settings.developer.diagnostics.sectionEnv')}
    </button>
    {#if envOpen}
      <table class="diag-table">
        <thead>
          <tr>
            <th>{$t('settings.developer.diagnostics.colSetting')}</th>
            <th colspan="2">{$t('settings.developer.diagnostics.colRuntime')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each getEnvRows(diagnostics) as row (row.label)}
            <tr>
              <td class="label-cell mono">{row.label}</td>
              <td class="value-cell mono" colspan="2">{row.runtime}</td>
              <td class="status-cell info">·</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {/if}
  </div>
  {/if}
</div>

<style>
  .diagnostics-panel {
    margin-top: 16px;
  }

  .diag-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: 12px;
  }

  .diag-actions {
    display: flex;
    gap: 8px;
    flex-shrink: 0;
  }

  .diag-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    font-size: 12px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    border: 1px solid var(--alpha-8);
    border-radius: 6px;
    cursor: pointer;
    transition: background 150ms ease;
  }

  .diag-btn:hover {
    background: var(--bg-hover);
  }

  .diag-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .diag-error {
    padding: 8px 12px;
    background: rgba(220, 50, 50, 0.15);
    color: var(--text-error, #f44);
    border-radius: 6px;
    font-size: 12px;
    margin-bottom: 12px;
  }

  .diag-version {
    font-size: 11px;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .section-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    background: none;
    border: none;
    cursor: pointer;
    width: 100%;
    text-align: left;
  }

  .section-toggle:hover {
    color: var(--accent-primary);
  }

  .diag-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
    margin-bottom: 16px;
  }

  .diag-table th {
    text-align: left;
    padding: 6px 10px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    border-bottom: 1px solid var(--alpha-8);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .diag-table td {
    padding: 5px 10px;
    border-bottom: 1px solid var(--alpha-4);
  }

  .label-cell {
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .value-cell {
    color: var(--text-primary);
    font-family: 'JetBrains Mono', 'Fira Code', monospace;
    font-size: 11px;
  }

  .mono {
    font-family: 'JetBrains Mono', 'Fira Code', monospace;
    font-size: 11px;
  }

  .status-cell {
    width: 24px;
    text-align: center;
    font-weight: 700;
  }

  .status-cell.match {
    color: #4caf50;
  }

  .status-cell.mismatch {
    color: #f44336;
  }

  .status-cell.info {
    color: var(--text-muted);
  }

  :global(.spinning) {
    animation: spin 1s linear infinite;
  }

  .cast-scan-placeholder {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    color: var(--text-muted);
    font-size: 12px;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
