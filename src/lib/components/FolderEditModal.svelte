<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import Modal from './Modal.svelte';
  import { Heart, Star, Music, Folder, Disc, Library, Headphones, Upload } from 'lucide-svelte';
  import type { PlaylistFolder } from '$lib/stores/playlistFoldersStore';

  interface Props {
    isOpen: boolean;
    folder: PlaylistFolder | null;
    onClose: () => void;
    onSave: (folder: PlaylistFolder | null, updates: {
      name: string;
      iconType: string;
      iconPreset: string;
      iconColor: string;
      customImagePath?: string;
    }) => void;
    onDelete?: (folder: PlaylistFolder) => void;
  }

  let { isOpen, folder, onClose, onSave, onDelete }: Props = $props();

  let folderName = $state('');
  let customIconPath = $state<string | null>(null);
  let customIconPreset = $state<string>('folder');
  let iconBackground = $state<string | null>(null);
  let useCustomImage = $state(false);
  let saving = $state(false);

  const presetIcons = [
    { id: 'heart', label: 'Heart', icon: Heart },
    { id: 'star', label: 'Star', icon: Star },
    { id: 'music', label: 'Music', icon: Music },
    { id: 'folder', label: 'Folder', icon: Folder },
    { id: 'disc', label: 'Disc', icon: Disc },
    { id: 'library', label: 'Library', icon: Library },
    { id: 'headphones', label: 'Headphones', icon: Headphones },
  ];

  const solidColors = [
    { id: 'accent', label: 'Accent', value: 'var(--accent-primary)' },
    { id: 'red', label: 'Red', value: '#ef4444' },
    { id: 'orange', label: 'Orange', value: '#f97316' },
    { id: 'amber', label: 'Amber', value: '#f59e0b' },
    { id: 'green', label: 'Green', value: '#10b981' },
    { id: 'cyan', label: 'Cyan', value: '#06b6d4' },
    { id: 'blue', label: 'Blue', value: '#3b82f6' },
    { id: 'purple', label: 'Purple', value: '#a855f7' },
    { id: 'pink', label: 'Pink', value: '#ec4899' },
    { id: 'rose', label: 'Rose', value: '#f43f5e' },
    { id: 'slate', label: 'Slate', value: '#64748b' },
  ];

  const gradients = [
    { id: 'sunset', label: 'Sunset', value: 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)' },
    { id: 'ocean', label: 'Ocean', value: 'linear-gradient(135deg, #2e3192 0%, #1bffff 100%)' },
    { id: 'fire', label: 'Fire', value: 'linear-gradient(135deg, #ff0844 0%, #ffb199 100%)' },
    { id: 'mint', label: 'Mint', value: 'linear-gradient(135deg, #00b09b 0%, #96c93d 100%)' },
    { id: 'candy', label: 'Candy', value: 'linear-gradient(135deg, #fc6767 0%, #ec008c 100%)' },
    { id: 'forest', label: 'Forest', value: 'linear-gradient(135deg, #134e5e 0%, #71b280 100%)' },
  ];

  function selectPreset(presetId: string) {
    customIconPreset = presetId;
    useCustomImage = false;
    customIconPath = null;
  }

  function syncFromFolder() {
    if (folder) {
      folderName = folder.name;
      customIconPath = folder.custom_image_path || null;
      customIconPreset = folder.icon_preset || 'folder';
      iconBackground = folder.icon_color || null;
      useCustomImage = folder.icon_type === 'custom' && !!folder.custom_image_path;
    } else {
      folderName = '';
      customIconPath = null;
      customIconPreset = 'folder';
      iconBackground = null;
      useCustomImage = false;
    }
  }

  async function handleUploadClick() {
    try {
      const selected = await open({
        multiple: false,
        filters: [{
          name: 'Images',
          extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif']
        }]
      });

      if (selected && typeof selected === 'string') {
        customIconPath = selected;
        useCustomImage = true;
        customIconPreset = 'folder';
      }
    } catch (err) {
      console.error('Failed to open file picker:', err);
    }
  }

  async function handleSave() {
    if (!folderName.trim()) return;

    saving = true;
    try {
      onSave(folder, {
        name: folderName.trim(),
        iconType: useCustomImage ? 'custom' : 'preset',
        iconPreset: customIconPreset,
        iconColor: iconBackground || '',
        customImagePath: useCustomImage ? customIconPath || undefined : undefined
      });
      onClose();
    } finally {
      saving = false;
    }
  }

  function handleDelete() {
    if (folder && onDelete) {
      onDelete(folder);
    }
  }

  function handleCancel() {
    syncFromFolder();
    onClose();
  }

  $effect(() => {
    if (isOpen) {
      syncFromFolder();
    }
  });
</script>

<Modal {isOpen} onClose={handleCancel} title={folder ? 'Edit Folder' : 'New Folder'} maxWidth="480px">
  {#snippet children()}
  <div class="folder-modal-content">
    <!-- Name input -->
    <div class="modal-section">
      <h3>Name</h3>
      <input
        type="text"
        class="folder-name-input"
        bind:value={folderName}
        placeholder="Enter folder name"
      />
    </div>

    <!-- Icon Section -->
    <div class="modal-section">
      <h3>Icon</h3>

      <div class="icon-grid">
        {#each presetIcons as preset}
          <button
            class="icon-preset-btn"
            class:active={!useCustomImage && customIconPreset === preset.id}
            onclick={() => selectPreset(preset.id)}
            title={preset.label}
          >
            <svelte:component this={preset.icon} size={17} />
          </button>
        {/each}
      </div>

      <div class="custom-upload">
        <button class="upload-btn" onclick={handleUploadClick}>
          <Upload size={16} />
          <span>Upload Custom Image</span>
        </button>
        {#if useCustomImage && customIconPath}
          <span class="upload-filename">{customIconPath.split('/').pop()}</span>
        {/if}
      </div>
    </div>

    <!-- Background Section -->
    <div class="modal-section">
      <h3>Background</h3>

      <div class="color-section">
        <div class="color-grid">
          <button
            class="color-btn"
            class:active={iconBackground === null}
            onclick={() => iconBackground = null}
            title="None"
          >
            <div class="color-swatch no-color"></div>
          </button>
          {#each solidColors as color}
            <button
              class="color-btn"
              class:active={iconBackground === color.value}
              onclick={() => iconBackground = color.value}
              title={color.label}
            >
              <div class="color-swatch" style="background: {color.value};"></div>
            </button>
          {/each}
        </div>
      </div>

      <div class="color-section">
        <div class="color-grid">
          {#each gradients as gradient}
            <button
              class="color-btn"
              class:active={iconBackground === gradient.value}
              onclick={() => iconBackground = gradient.value}
              title={gradient.label}
            >
              <div class="color-swatch" style="background: {gradient.value};"></div>
            </button>
          {/each}
        </div>
      </div>
    </div>
  </div>
  {/snippet}

  {#snippet footer()}
  <div class="modal-actions">
    {#if folder && onDelete}
      <button class="btn btn-danger" onclick={handleDelete} disabled={saving}>
        Delete Folder
      </button>
    {/if}
    <div class="modal-actions-right">
      <button class="btn btn-secondary" onclick={handleCancel} disabled={saving}>
        Cancel
      </button>
      <button class="btn btn-primary" onclick={handleSave} disabled={saving || !folderName.trim()}>
        {saving ? 'Saving...' : folder ? 'Save' : 'Create'}
      </button>
    </div>
  </div>
  {/snippet}
</Modal>

<style>
  .folder-modal-content {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .folder-name-input {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    color: var(--text-primary);
    outline: none;
    transition: border-color 150ms ease;
  }

  .folder-name-input:focus {
    border-color: var(--accent-primary);
  }

  .folder-name-input::placeholder {
    color: var(--text-muted);
  }

  .modal-section {
    margin-bottom: 0;
  }

  .modal-section h3 {
    font-size: 15px;
    font-weight: 600;
    margin: 0 0 4px 0;
    color: var(--text-primary);
  }

  .color-section {
    margin-bottom: 20px;
  }

  .color-section:last-of-type {
    margin-bottom: 0;
  }

  .icon-grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 8px;
    margin-bottom: 12px;
  }

  .icon-preset-btn {
    width: 42px;
    height: 42px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-secondary);
    border: 2px solid transparent;
    border-radius: 6px;
    cursor: pointer;
    transition: all 150ms ease;
    color: var(--text-secondary);
  }

  .icon-preset-btn:hover {
    background: var(--bg-tertiary);
    border-color: var(--accent-primary);
    color: var(--text-primary);
  }

  .icon-preset-btn.active {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: var(--bg-primary);
  }

  .color-grid {
    display: grid;
    grid-template-columns: repeat(12, 1fr);
    gap: 2px;
  }

  .color-btn {
    aspect-ratio: 1;
    width: 30px;
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    font-size: 0;
    border: 2px solid transparent;
    border-radius: 6px;
    cursor: pointer;
    transition: all 150ms ease;
    padding: 3px;
  }

  .color-btn:hover {
    border-color: var(--accent-primary);
  }

  .color-btn.active {
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 1px var(--accent-primary);
  }

  .color-swatch {
    width: 100%;
    height: 100%;
    border-radius: 4px;
    border: 1px solid var(--bg-tertiary);
  }

  .color-swatch.no-color {
    background: linear-gradient(
      45deg,
      var(--bg-tertiary) 25%,
      transparent 25%,
      transparent 75%,
      var(--bg-tertiary) 75%,
      var(--bg-tertiary)
    ),
    linear-gradient(
      45deg,
      var(--bg-tertiary) 25%,
      transparent 25%,
      transparent 75%,
      var(--bg-tertiary) 75%,
      var(--bg-tertiary)
    );
    background-size: 10px 10px;
    background-position: 0 0, 5px 5px;
  }

  .custom-upload {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .upload-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 13px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .upload-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .upload-filename {
    font-size: 13px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 12px;
    padding-top: 24px;
    width: 100%;
  }

  .modal-actions-right {
    display: flex;
    gap: 12px;
  }

  .btn {
    padding: 8px 20px;
    border-radius: 6px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
    border: 1px solid transparent;
  }

  .btn-secondary {
    background: var(--bg-secondary);
    color: var(--text-primary);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--bg-tertiary);
  }

  .btn-primary {
    background: var(--accent-primary);
    color: var(--bg-primary);
  }

  .btn-primary:hover:not(:disabled) {
    opacity: 0.9;
  }

  .btn-danger {
    margin-right: auto;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.3);
    color: #ef4444;
  }

  .btn-danger:hover:not(:disabled) {
    background: rgba(239, 68, 68, 0.2);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
