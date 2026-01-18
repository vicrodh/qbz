<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import Modal from './Modal.svelte';
  import { Heart, Star, Music, Folder, Disc, Library, Upload, GripVertical } from 'lucide-svelte';
  import type { FavoritesPreferences } from '../types';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onSave: (prefs: FavoritesPreferences) => void;
    initialPreferences: FavoritesPreferences;
  }

  let { isOpen, onClose, onSave, initialPreferences }: Props = $props();

  let customIconPath = $state<string | null>(initialPreferences.custom_icon_path || null);
  let customIconPreset = $state<string | null>(initialPreferences.custom_icon_preset || 'heart');
  let iconBackground = $state<string | null>(initialPreferences.icon_background || null);
  let tabOrder = $state<string[]>([...initialPreferences.tab_order]);
  let useCustomImage = $state(!!initialPreferences.custom_icon_path);
  let saving = $state(false);

  const presetIcons = [
    { id: 'heart', label: 'Heart', icon: Heart },
    { id: 'star', label: 'Star', icon: Star },
    { id: 'music', label: 'Music', icon: Music },
    { id: 'folder', label: 'Folder', icon: Folder },
    { id: 'disc', label: 'Disc', icon: Disc },
    { id: 'library', label: 'Library', icon: Library },
  ];

  const solidColors = [
    { id: 'accent', label: 'Accent', value: 'var(--accent-primary)' },
    { id: 'red', label: 'Red', value: '#ef4444' },
    { id: 'orange', label: 'Orange', value: '#f97316' },
    { id: 'amber', label: 'Amber', value: '#f59e0b' },
    { id: 'green', label: 'Green', value: '#10b981' },
    { id: 'blue', label: 'Blue', value: '#3b82f6' },
    { id: 'purple', label: 'Purple', value: '#a855f7' },
    { id: 'pink', label: 'Pink', value: '#ec4899' },
  ];

  const gradients = [
    { id: 'sunset', label: 'Sunset', value: 'linear-gradient(135deg, #667eea 0%, #764ba2 100%)' },
    { id: 'ocean', label: 'Ocean', value: 'linear-gradient(135deg, #2e3192 0%, #1bffff 100%)' },
    { id: 'fire', label: 'Fire', value: 'linear-gradient(135deg, #ff0844 0%, #ffb199 100%)' },
    { id: 'forest', label: 'Forest', value: 'linear-gradient(135deg, #0ba360 0%, #3cba92 100%)' },
    { id: 'aurora', label: 'Aurora', value: 'linear-gradient(135deg, #a8edea 0%, #fed6e3 100%)' },
    { id: 'midnight', label: 'Midnight', value: 'linear-gradient(135deg, #000428 0%, #004e92 100%)' },
  ];

  const tabLabels: Record<string, string> = {
    tracks: 'Tracks',
    albums: 'Albums',
    artists: 'Artists',
    playlists: 'Playlists',
  };

  function selectPreset(presetId: string) {
    customIconPreset = presetId;
    useCustomImage = false;
    customIconPath = null;
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
        customIconPreset = null;
      }
    } catch (err) {
      console.error('Failed to open file picker:', err);
    }
  }

  function moveUp(index: number) {
    if (index === 0) return;
    const temp = tabOrder[index];
    tabOrder[index] = tabOrder[index - 1];
    tabOrder[index - 1] = temp;
  }

  function moveDown(index: number) {
    if (index === tabOrder.length - 1) return;
    const temp = tabOrder[index];
    tabOrder[index] = tabOrder[index + 1];
    tabOrder[index + 1] = temp;
  }

  async function handleSave() {
    saving = true;
    try {
      const prefs: FavoritesPreferences = {
        custom_icon_path: useCustomImage ? customIconPath : null,
        custom_icon_preset: useCustomImage ? null : customIconPreset,
        icon_background: iconBackground,
        tab_order: tabOrder,
      };

      await invoke('save_favorites_preferences', { prefs });
      onSave(prefs);
      onClose();
    } catch (err) {
      console.error('Failed to save favorites preferences:', err);
    } finally {
      saving = false;
    }
  }

  function handleCancel() {
    customIconPath = initialPreferences.custom_icon_path || null;
    customIconPreset = initialPreferences.custom_icon_preset || 'heart';
    iconBackground = initialPreferences.icon_background || null;
    tabOrder = [...initialPreferences.tab_order];
    useCustomImage = !!initialPreferences.custom_icon_path;
    onClose();
  }
</script>

<Modal {isOpen} onClose={handleCancel} title="Favorites Settings" maxWidth="736px">
  {#snippet children()}
  <div class="modal-columns">
    <!-- Left Column: Icon Customization -->
    <div class="modal-column">
      <div class="modal-section">
        <h3>Icon</h3>
        <p class="section-description">Choose an icon for the Favorites page header</p>
        
        <div class="icon-grid">
          {#each presetIcons as preset}
            <button
              class="icon-preset-btn"
              class:active={!useCustomImage && customIconPreset === preset.id}
              onclick={() => selectPreset(preset.id)}
              title={preset.label}
            >
              <svelte:component this={preset.icon} size={24} />
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

      <div class="modal-section">
        <h3>Background</h3>
        <p class="section-description">Choose a background color or gradient</p>
        
        <div class="color-section">
          <h4>Solid Colors</h4>
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
          <h4>Gradients</h4>
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

    <!-- Right Column: Tab Order -->
    <div class="modal-column">
      <div class="modal-section">
        <h3>Tab Order</h3>
        <p class="section-description">Reorder tabs in the Favorites page navigation</p>
        
        <div class="tab-order-list">
          {#each tabOrder as tab, index}
            <div class="tab-order-item">
              <div class="tab-grip">
                <GripVertical size={16} />
              </div>
              <div class="tab-label">{tabLabels[tab] || tab}</div>
              <div class="tab-controls">
                <button
                  class="tab-move-btn"
                  onclick={() => moveUp(index)}
                  disabled={index === 0}
                  title="Move up"
                >
                  ↑
                </button>
                <button
                  class="tab-move-btn"
                  onclick={() => moveDown(index)}
                  disabled={index === tabOrder.length - 1}
                  title="Move down"
                >
                  ↓
                </button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    </div>
  </div>
  {/snippet}

  {#snippet footer()}
  <div class="modal-actions">
    <button class="btn btn-secondary" onclick={handleCancel} disabled={saving}>
      Cancel
    </button>
    <button class="btn btn-primary" onclick={handleSave} disabled={saving}>
      {saving ? 'Saving...' : 'Save'}
    </button>
  </div>
  {/snippet}
</Modal>

<style>
  .modal-columns {
    display: grid;
    grid-template-columns: 65fr 35fr;
    gap: 32px;
  }

  .modal-column {
    min-width: 0;
  }

  .modal-section {
    margin-bottom: 32px;
  }

  .modal-section:last-of-type {
    margin-bottom: 0;
  }

  .modal-section h3 {
    font-size: 15px;
    font-weight: 600;
    margin: 0 0 4px 0;
    color: var(--text-primary);
  }

  .modal-section h4 {
    font-size: 13px;
    font-weight: 600;
    margin: 0 0 8px 0;
    color: var(--text-secondary);
  }

  .section-description {
    font-size: 13px;
    color: var(--text-muted);
    margin: 0 0 16px 0;
  }

  .color-section {
    margin-bottom: 20px;
  }

  .color-section:last-of-type {
    margin-bottom: 0;
  }

  .icon-grid {
    display: grid;
    grid-template-columns: repeat(6, 1fr);
    gap: 12px;
    margin-bottom: 16px;
  }

  .icon-preset-btn {
    aspect-ratio: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-secondary);
    border: 2px solid transparent;
    border-radius: 8px;
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
    grid-template-columns: repeat(8, 1fr);
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

  .tab-order-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .tab-order-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
  }

  .tab-grip {
    color: var(--text-muted);
    cursor: grab;
  }

  .tab-label {
    flex: 1;
    font-size: 14px;
    color: var(--text-primary);
  }

  .tab-controls {
    display: flex;
    gap: 4px;
  }

  .tab-move-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: 14px;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .tab-move-btn:hover:not(:disabled) {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .tab-move-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    padding-top: 24px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .btn {
    padding: 8px 20px;
    border-radius: 6px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
    border: none;
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

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
