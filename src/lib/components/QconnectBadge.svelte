<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Monitor, Globe, Smartphone, Speaker, Info, Power } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { QconnectSessionSnapshot, QconnectRendererInfo } from '$lib/services/qconnectRuntime';

  interface Props {
    connected: boolean;
    sessionSnapshot: QconnectSessionSnapshot | null;
    onToggleConnection: () => void | Promise<void>;
    busy?: boolean;
  }

  let {
    connected,
    sessionSnapshot: sessionSnapshotProp = null,
    onToggleConnection,
    busy = false,
  }: Props = $props();

  // Local snapshot that can be refreshed independently of the prop
  let sessionSnapshot = $state<QconnectSessionSnapshot | null>(null);
  $effect(() => {
    sessionSnapshot = sessionSnapshotProp;
  });

  let isPopupOpen = $state(false);
  let isInfoHovering = $state(false);
  let settingRenderer = $state(false);

  async function togglePopup(): Promise<void> {
    isPopupOpen = !isPopupOpen;
    // Force a fresh session snapshot when opening — the session state
    // may have stale renderer list from before deferred_renderer_join
    if (isPopupOpen && connected) {
      try {
        const fresh = await invoke<QconnectSessionSnapshot>('v2_qconnect_session_snapshot');
        if (fresh && fresh.renderers.length > 0) {
          sessionSnapshot = fresh;
        }
      } catch {
        // non-fatal
      }
    }
  }

  function closePopup(): void {
    isPopupOpen = false;
  }

  async function setActiveRenderer(rendererId: number): Promise<void> {
    if (settingRenderer) return;
    settingRenderer = true;
    try {
      await invoke('v2_qconnect_set_active_renderer', { request: { renderer_id: rendererId } });
    } catch (err) {
      console.warn('[QconnectBadge] set_active_renderer failed:', err);
    } finally {
      settingRenderer = false;
    }
  }

  function resolveDeviceType(renderer: QconnectRendererInfo): 'computer' | 'web' | 'mobile' | 'speaker' {
    const dtype = renderer.device_type ?? 5;
    if (dtype === 6) return 'mobile';
    if (dtype === 5) {
      const name = (renderer.friendly_name ?? '').toLowerCase();
      if (name.includes('web player') || name.includes('browser')) return 'web';
      return 'computer';
    }
    // device_type 3 or 4 or other → speaker/receiver
    return 'speaker';
  }

  function rendererDisplayName(renderer: QconnectRendererInfo): string {
    if (renderer.friendly_name) return renderer.friendly_name;
    const parts = [renderer.brand, renderer.model].filter(Boolean);
    return parts.length > 0 ? parts.join(' ') : `Renderer ${renderer.renderer_id}`;
  }

  const activeRenderer = $derived(
    sessionSnapshot?.renderers.find(
      (r) => r.renderer_id === sessionSnapshot?.active_renderer_id
    ) ?? null
  );

  const activeDeviceType = $derived(
    activeRenderer ? resolveDeviceType(activeRenderer) : 'computer'
  );

  const isLocalRendererActive = $derived(
    sessionSnapshot?.active_renderer_id === sessionSnapshot?.local_renderer_id
  );
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="qconnect-badge-wrapper">
  <button
    class="qconnect-badge"
    class:active={connected}
    onclick={togglePopup}
    title={connected ? $t('qconnect.statusConnected') : $t('qconnect.statusDisconnected')}
  >
    <span class="badge-icon">
      {#if activeDeviceType === 'web'}
        <Globe size={16} />
      {:else if activeDeviceType === 'mobile'}
        <Smartphone size={16} />
      {:else if activeDeviceType === 'speaker'}
        <Speaker size={16} />
      {:else}
        <Monitor size={16} />
      {/if}
    </span>
    <span class="badge-text">
      <span class="badge-label">{ $t('platforms.qobuz') }</span>
      <span class="badge-label">Connect</span>
    </span>
  </button>

  {#if isPopupOpen}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="popup-backdrop" onclick={closePopup}></div>
    <div class="qconnect-popup">
      <div class="popup-header">
        <span class="popup-title">{$t('qconnect.title')}</span>
        <div class="popup-header-actions">
          <div
            class="info-trigger"
            onmouseenter={() => (isInfoHovering = true)}
            onmouseleave={() => (isInfoHovering = false)}
          >
            <Info size={14} />
            {#if isInfoHovering}
              <div class="info-tooltip">
                <p>{$t('qconnect.experimentalWarning')}</p>
                <p>{$t('qconnect.localLibraryWarning')}</p>
              </div>
            {/if}
          </div>
        </div>
      </div>

      {#if connected && sessionSnapshot}
        <div class="renderer-list">
          {#each sessionSnapshot.renderers as renderer (renderer.renderer_id)}
            {@const dtype = resolveDeviceType(renderer)}
            {@const isActive = renderer.renderer_id === sessionSnapshot.active_renderer_id}
            {@const isLocal = renderer.renderer_id === sessionSnapshot.local_renderer_id}
            <button
              class="renderer-item"
              class:active={isActive}
              onclick={() => setActiveRenderer(renderer.renderer_id)}
              disabled={isActive || settingRenderer}
            >
              <span class="renderer-icon" class:active={isActive}>
                {#if dtype === 'web'}
                  <Globe size={14} />
                {:else if dtype === 'mobile'}
                  <Smartphone size={14} />
                {:else if dtype === 'speaker'}
                  <Speaker size={14} />
                {:else}
                  <Monitor size={14} />
                {/if}
              </span>
              <span class="renderer-name">
                {rendererDisplayName(renderer)}
                {#if isLocal}
                  <span class="this-device">({$t('qconnect.thisDevice')})</span>
                {/if}
              </span>
            </button>
          {/each}
        </div>
      {:else}
        <div class="disconnected-message">
          <span>{$t('qconnect.statusDisconnected')}</span>
        </div>
      {/if}

      <div class="popup-footer">
        <button
          class="toggle-btn"
          class:on={connected}
          onclick={onToggleConnection}
          disabled={busy}
        >
          <Power size={12} />
          <span>{connected ? $t('qconnect.turnOff') : $t('qconnect.turnOn')}</span>
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .qconnect-badge-wrapper {
    position: relative;
    display: flex;
    align-items: stretch;
  }

  .qconnect-badge {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    padding: 4px 6px;
    border-radius: 3px;
    font-size: 7px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.3px;
    cursor: pointer;
    transition: color 200ms ease, background 200ms ease, border-color 200ms ease;
    background: transparent;
    color: var(--alpha-15);
    border: 1px solid var(--alpha-6);
    white-space: nowrap;
    height: 100%;
    line-height: 1;
  }

  .qconnect-badge.active {
    background: rgba(234, 179, 8, 0.2);
    color: #eab308;
    border-color: rgba(234, 179, 8, 0.4);
  }

  .qconnect-badge:hover {
    opacity: 0.85;
  }

  .badge-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
  }

  .badge-text {
    display: flex;
    flex-direction: column;
    align-items: center;
    flex: 1;
    justify-content: center;
    gap: 0;
  }

  .badge-label {
    line-height: 1.1;
  }

  /* Popup */
  .popup-backdrop {
    position: fixed;
    inset: 0;
    z-index: 9998;
  }

  .qconnect-popup {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    z-index: 9999;
    min-width: 240px;
    max-width: 300px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .popup-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .popup-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .popup-header-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .info-trigger {
    position: relative;
    display: flex;
    align-items: center;
    color: var(--text-muted);
    cursor: help;
  }

  .info-tooltip {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    width: 260px;
    padding: 10px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    z-index: 10000;
  }

  .info-tooltip p {
    margin: 0;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-secondary);
  }

  .info-tooltip p + p {
    margin-top: 8px;
  }

  /* Renderer list */
  .renderer-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .renderer-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--text-secondary);
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: background 150ms ease;
  }

  .renderer-item:hover:not(:disabled) {
    background: var(--alpha-6);
  }

  .renderer-item.active {
    background: rgba(234, 179, 8, 0.1);
    color: var(--text-primary);
    cursor: default;
  }

  .renderer-item:disabled {
    opacity: 0.7;
  }

  .renderer-icon {
    display: flex;
    align-items: center;
    color: var(--text-muted);
  }

  .renderer-icon.active {
    color: #eab308;
  }

  .renderer-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .this-device {
    font-size: 10px;
    color: var(--text-muted);
    font-style: italic;
  }

  .disconnected-message {
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
    padding: 8px 0;
  }

  /* Footer toggle */
  .popup-footer {
    border-top: 1px solid var(--alpha-6);
    padding-top: 8px;
  }

  .toggle-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 6px 10px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    border: 1px solid var(--border-subtle);
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    transition: background 150ms ease, color 150ms ease;
  }

  .toggle-btn:hover:not(:disabled) {
    background: var(--alpha-6);
  }

  .toggle-btn.on {
    color: #eab308;
    border-color: rgba(234, 179, 8, 0.3);
  }

  .toggle-btn:disabled {
    opacity: 0.5;
    cursor: wait;
  }
</style>
