<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { X, HardDrive, Network, RefreshCw, Power, PowerOff, AlertTriangle, FolderOpen } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  // LibraryFolder type matching Rust backend
  interface LibraryFolder {
    id: number;
    path: string;
    alias: string | null;
    enabled: boolean;
    isNetwork: boolean;
    networkFsType: string | null;
    userOverrideNetwork: boolean;
    lastScan: number | null;
  }

  interface Props {
    isOpen: boolean;
    folder: LibraryFolder | null;
    onClose: () => void;
    onSave: (folder: LibraryFolder) => void;
    onScanFolder?: (folderId: number) => void;
  }

  let {
    isOpen,
    folder,
    onClose,
    onSave,
    onScanFolder
  }: Props = $props();

  // Form state
  let alias = $state('');
  let currentPath = $state('');
  let pathChanged = $state(false);
  let enabled = $state(true);
  let isNetwork = $state(false);
  let detectedIsNetwork = $state(false); // What the system detected
  let networkFsType = $state('');
  let userOverrideNetwork = $state(false);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let scanning = $state(false);
  let accessible = $state(true);
  let checkingAccessibility = $state(false);

  // Network filesystem options
  const networkFsOptions = [
    { value: '', label: $t('library.networkFs.auto') },
    { value: 'cifs', label: 'SMB/CIFS (Samba)' },
    { value: 'nfs', label: 'NFS' },
    { value: 'sshfs', label: 'SSHFS' },
    { value: 'rclone', label: 'rclone (Cloud)' },
    { value: 'webdav', label: 'WebDAV' },
    { value: 'glusterfs', label: 'GlusterFS' },
    { value: 'ceph', label: 'CephFS' },
    { value: 'other', label: $t('library.networkFs.other') }
  ];

  // Reset form when modal opens
  $effect(() => {
    if (isOpen && folder) {
      alias = folder.alias || '';
      currentPath = folder.path;
      pathChanged = false;
      enabled = folder.enabled;
      isNetwork = folder.isNetwork;
      networkFsType = folder.networkFsType || '';
      userOverrideNetwork = folder.userOverrideNetwork;
      error = null;
      loading = false;
      scanning = false;
      checkAccessibility();
      detectNetworkStatus();
    }
  });

  // Detect if the current path is on a network mount
  async function detectNetworkStatus() {
    try {
      const info = await invoke<{ isNetwork: boolean }>('check_network_path', { path: currentPath });
      detectedIsNetwork = info.isNetwork;
    } catch (err) {
      console.error('Failed to detect network status:', err);
      detectedIsNetwork = false;
    }
  }

  async function checkAccessibility() {
    if (!folder) return;

    checkingAccessibility = true;
    try {
      accessible = await invoke<boolean>('library_check_folder_accessible', { path: folder.path });
    } catch (err) {
      console.error('Failed to check folder accessibility:', err);
      accessible = false;
    } finally {
      checkingAccessibility = false;
    }
  }

  async function handleSave() {
    if (!folder) return;

    loading = true;
    error = null;

    try {
      // If path was changed, update it first
      if (pathChanged && currentPath !== folder.path) {
        await invoke('library_update_folder_path', {
          id: folder.id,
          newPath: currentPath
        });
      }

      const updatedFolder = await invoke<LibraryFolder>('library_update_folder_settings', {
        id: folder.id,
        alias: alias.trim() || null,
        enabled,
        isNetwork,
        networkFsType: isNetwork && networkFsType ? networkFsType : null,
        userOverrideNetwork
      });
      onSave(updatedFolder);
      onClose();
    } catch (err) {
      console.error('Failed to update folder settings:', err);
      error = String(err);
    } finally {
      loading = false;
    }
  }

  async function handleChangePath() {
    try {
      const result = await open({
        directory: true,
        multiple: false,
        defaultPath: currentPath,
        title: $t('library.selectFolder')
      });

      if (result && typeof result === 'string') {
        currentPath = result;
        pathChanged = true;
        // Re-check accessibility for the new path
        checkingAccessibility = true;
        try {
          accessible = await invoke<boolean>('library_check_folder_accessible', { path: currentPath });
        } catch (err) {
          accessible = false;
        } finally {
          checkingAccessibility = false;
        }
      }
    } catch (err) {
      console.error('Failed to open folder picker:', err);
      error = String(err);
    }
  }

  async function handleScanFolder() {
    if (!folder || !onScanFolder) return;

    scanning = true;
    try {
      onScanFolder(folder.id);
      onClose();
    } catch (err) {
      console.error('Failed to start folder scan:', err);
      error = String(err);
    } finally {
      scanning = false;
    }
  }

  function handleNetworkToggle() {
    isNetwork = !isNetwork;
    userOverrideNetwork = true;
    if (!isNetwork) {
      networkFsType = '';
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    } else if (e.key === 'Enter' && !e.shiftKey) {
      handleSave();
    }
  }

  function formatLastScan(timestamp: number | null): string {
    if (!timestamp) return $t('library.neverScanned');
    const date = new Date(timestamp * 1000);
    return date.toLocaleString();
  }

  // Get display name for folder
  function getDisplayName(): string {
    if (!folder) return '';
    if (alias) return alias;
    // Get last part of path
    const parts = currentPath.split('/').filter(Boolean);
    return parts[parts.length - 1] || currentPath;
  }
</script>

{#if isOpen && folder}
  <div
    class="modal-overlay"
    onclick={() => onClose()}
    onkeydown={handleKeydown}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h2>{$t('library.folderSettings')}</h2>
        <button class="close-btn" onclick={onClose}>
          <X size={20} />
        </button>
      </div>

      <div class="modal-body">
        {#if error}
          <div class="error-message">{error}</div>
        {/if}

        <!-- Folder path info -->
        <div class="folder-info">
          <div class="folder-icon">
            {#if isNetwork}
              <Network size={24} />
            {:else}
              <HardDrive size={24} />
            {/if}
          </div>
          <div class="folder-details">
            <span class="folder-name">{getDisplayName()}</span>
            <span class="folder-path" class:path-changed={pathChanged}>{currentPath}</span>
          </div>
          <button class="change-path-btn" onclick={handleChangePath} disabled={loading} title={$t('library.changeFolder')}>
            <FolderOpen size={16} />
          </button>
          <div class="folder-status">
            {#if checkingAccessibility}
              <span class="status checking">
                <RefreshCw size={14} class="spinning" />
              </span>
            {:else if accessible}
              <span class="status accessible" title={$t('library.folderAccessible')}>
                <Power size={14} />
              </span>
            {:else}
              <span class="status inaccessible" title={$t('library.folderInaccessible')}>
                <PowerOff size={14} />
              </span>
            {/if}
          </div>
        </div>

        <!-- Alias -->
        <div class="form-group">
          <label for="alias">{$t('library.folderAlias')}</label>
          <input
            type="text"
            id="alias"
            bind:value={alias}
            placeholder={$t('library.folderAliasPlaceholder')}
            disabled={loading}
          />
          <span class="form-hint">{$t('library.folderAliasHint')}</span>
        </div>

        <!-- Enabled toggle -->
        <div class="form-group checkbox">
          <label>
            <input
              type="checkbox"
              bind:checked={enabled}
              disabled={loading}
            />
            <span>{$t('library.folderEnabled')}</span>
          </label>
          <span class="form-hint">{$t('library.folderEnabledHint')}</span>
        </div>

        <!-- Network folder toggle -->
        <div class="form-group checkbox">
          <label>
            <input
              type="checkbox"
              checked={isNetwork}
              onchange={handleNetworkToggle}
              disabled={loading}
            />
            <span class="network-label">
              <Network size={14} />
              {$t('library.isNetworkFolder')}
            </span>
          </label>
          {#if userOverrideNetwork && isNetwork !== detectedIsNetwork}
            {#if isNetwork && !detectedIsNetwork}
              <!-- User marked as network, but not detected -->
              <span class="form-hint override-hint warning">
                <AlertTriangle size={12} />
                {$t('library.networkOverrideMarkedNetwork')}
              </span>
            {:else}
              <!-- User marked as local, but detected as network -->
              <span class="form-hint override-hint danger">
                <AlertTriangle size={12} />
                {$t('library.networkOverrideMarkedLocal')}
              </span>
            {/if}
          {:else if userOverrideNetwork}
            <span class="form-hint override-hint">
              <AlertTriangle size={12} />
              {$t('library.networkOverrideHint')}
            </span>
          {:else}
            <span class="form-hint">{$t('library.networkFolderHint')}</span>
          {/if}
        </div>

        <!-- Network filesystem type (only shown if network folder) -->
        {#if isNetwork}
          <div class="form-group network-fs-group">
            <label for="network-fs">{$t('library.networkFsType')}</label>
            <select
              id="network-fs"
              bind:value={networkFsType}
              disabled={loading}
            >
              {#each networkFsOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
            <span class="form-hint">{$t('library.networkFsTypeHint')}</span>
          </div>
        {/if}

        <!-- Last scan info -->
        <div class="info-row">
          <span class="info-label">{$t('library.lastScanned')}:</span>
          <span class="info-value">{formatLastScan(folder.lastScan)}</span>
        </div>
      </div>

      <div class="modal-footer">
        <button
          class="btn-scan"
          onclick={handleScanFolder}
          disabled={loading || scanning || !enabled || !accessible}
          title={!enabled ? $t('library.folderDisabledNoScan') : !accessible ? $t('library.folderInaccessibleNoScan') : ''}
        >
          <RefreshCw size={14} class={scanning ? 'spinning' : ''} />
          {scanning ? $t('library.scanning') : $t('library.scanThisFolder')}
        </button>
        <div class="footer-spacer"></div>
        <button class="btn-secondary" onclick={onClose} disabled={loading}>
          {$t('common.cancel')}
        </button>
        <button class="btn-primary" onclick={handleSave} disabled={loading}>
          {loading ? $t('common.saving') : $t('common.save')}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    width: 100%;
    max-width: 560px;
    max-height: 90vh;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
    border-radius: 16px;
    border: 1px solid var(--bg-tertiary);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.5);
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 20px 24px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .modal-header h2 {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    transition: color 150ms ease;
  }

  .close-btn:hover {
    color: var(--text-primary);
  }

  .modal-body {
    padding: 28px 32px;
    overflow-y: auto;
  }

  .error-message {
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: #ef4444;
    padding: 12px;
    border-radius: 8px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .folder-info {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px;
    background: var(--bg-primary);
    border-radius: 12px;
    margin-bottom: 20px;
  }

  .folder-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 48px;
    height: 48px;
    background: var(--bg-tertiary);
    border-radius: 10px;
    color: var(--text-secondary);
  }

  .folder-details {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .folder-name {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .folder-path {
    font-size: 12px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-path.path-changed {
    color: var(--accent-primary);
  }

  .change-path-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .change-path-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .change-path-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .folder-status {
    display: flex;
    align-items: center;
  }

  .status {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 6px;
  }

  .status.checking {
    color: var(--text-muted);
  }

  .status.accessible {
    background: rgba(34, 197, 94, 0.1);
    color: #22c55e;
  }

  .status.inaccessible {
    background: rgba(239, 68, 68, 0.1);
    color: #ef4444;
  }

  .form-group {
    margin-bottom: 16px;
  }

  .form-group label {
    display: block;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    margin-bottom: 8px;
  }

  .form-group input[type="text"],
  .form-group select {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    color: var(--text-primary);
    transition: border-color 150ms ease;
  }

  .form-group select {
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23888888' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'%3E%3C/polyline%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 12px center;
    padding-right: 36px;
    cursor: pointer;
  }

  .form-group input[type="text"]:focus,
  .form-group select:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .form-hint {
    display: block;
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 6px;
  }

  .override-hint {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--text-warning, #f59e0b);
  }

  .override-hint.warning {
    color: #f59e0b;
  }

  .override-hint.danger {
    color: #ef4444;
  }

  .form-group.checkbox label {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
  }

  .form-group.checkbox input[type="checkbox"] {
    width: 18px;
    height: 18px;
    accent-color: var(--accent-primary);
  }

  .form-group.checkbox span {
    font-size: 14px;
    color: var(--text-primary);
  }

  .network-label {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .network-fs-group {
    margin-left: 28px;
    padding-left: 16px;
    border-left: 2px solid var(--bg-tertiary);
  }

  .info-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 0;
    border-top: 1px solid var(--bg-tertiary);
    margin-top: 16px;
  }

  .info-label {
    font-size: 13px;
    color: var(--text-muted);
  }

  .info-value {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .modal-footer {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px 24px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .footer-spacer {
    flex: 1;
  }

  .btn-scan {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 16px;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .btn-scan:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .btn-scan:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary,
  .btn-primary {
    padding: 10px 20px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .btn-secondary {
    background: transparent;
    border: 1px solid var(--text-muted);
    color: var(--text-primary);
  }

  .btn-secondary:hover:not(:disabled) {
    border-color: var(--text-primary);
  }

  .btn-primary {
    background: var(--accent-primary);
    border: none;
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .btn-secondary:disabled,
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  :global(.spinning) {
    animation: spin 1s linear infinite;
  }
</style>
