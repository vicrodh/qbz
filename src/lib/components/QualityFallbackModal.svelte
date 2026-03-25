<script lang="ts">
  import { t } from '$lib/i18n';
  import { X } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';

  interface Props {
    isOpen: boolean;
    trackTitle: string;
    onTryLower: () => void;
    onSkip: () => void;
    onClose: () => void;
  }

  let { isOpen, trackTitle, onTryLower, onSkip, onClose }: Props = $props();
  let rememberChoice = $state(false);

  async function handleTryLower() {
    if (rememberChoice) {
      try {
        await invoke('v2_set_quality_fallback_behavior', { behavior: 'always_fallback' });
      } catch (err) {
        console.error('Failed to persist fallback behavior:', err);
      }
    }
    onTryLower();
  }

  async function handleSkip() {
    if (rememberChoice) {
      try {
        await invoke('v2_set_quality_fallback_behavior', { behavior: 'always_skip' });
      } catch (err) {
        console.error('Failed to persist fallback behavior:', err);
      }
    }
    onSkip();
  }
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="modal-backdrop" onclick={onClose} role="presentation">
    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      <div class="modal-header">
        <h3>{$t('qualityFallback.title')}</h3>
        <button class="close-btn" onclick={onClose}>
          <X size={16} />
        </button>
      </div>
      <div class="modal-body">
        <p>{$t('qualityFallback.body', { values: { track: trackTitle } })}</p>
      </div>
      <div class="modal-actions">
        <label class="remember-choice">
          <input type="checkbox" bind:checked={rememberChoice} />
          <span>{$t('qualityFallback.rememberChoice')}</span>
        </label>
        <div class="buttons">
          <button class="btn secondary" onclick={handleSkip}>
            {$t('qualityFallback.skip')}
          </button>
          <button class="btn primary" onclick={handleTryLower}>
            {$t('qualityFallback.tryLower')}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
    animation: fadeIn 150ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .modal {
    background: var(--bg-secondary);
    border-radius: 12px;
    width: 90%;
    max-width: 420px;
    box-shadow: 0 24px 48px rgba(0, 0, 0, 0.4);
    animation: slideUp 200ms ease;
  }

  @keyframes slideUp {
    from {
      opacity: 0;
      transform: translateY(20px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 20px 12px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .modal-header h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .close-btn {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 150ms ease, background-color 150ms ease;
  }

  .close-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .modal-body {
    padding: 16px 20px;
  }

  .modal-body p {
    font-size: 14px;
    line-height: 1.5;
    color: var(--text-secondary);
    margin: 0;
  }

  .modal-actions {
    padding: 12px 20px 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .remember-choice {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-size: 13px;
    color: var(--text-muted);
  }

  .remember-choice input[type="checkbox"] {
    accent-color: var(--accent-primary);
    width: 16px;
    height: 16px;
    cursor: pointer;
  }

  .buttons {
    display: flex;
    gap: 10px;
    justify-content: flex-end;
  }

  .btn {
    padding: 8px 18px;
    border: none;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 150ms ease, color 150ms ease;
  }

  .btn.secondary {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .btn.secondary:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .btn.primary {
    background: var(--accent-primary);
    color: white;
  }

  .btn.primary:hover {
    filter: brightness(1.1);
  }
</style>
