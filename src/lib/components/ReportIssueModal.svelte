<script lang="ts">
  import { t } from '$lib/i18n';
  import Modal from './Modal.svelte';
  import { FileText, ExternalLink } from 'lucide-svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    onGoToLogs: () => void;
    onCreateIssue: () => void;
  }

  let { isOpen, onClose, onGoToLogs, onCreateIssue }: Props = $props();

  const HIDE_KEY = 'qbz-hide-report-issue-modal';
  let hideNextTime = $state(false);

  function persistHidePreference() {
    try {
      if (hideNextTime) localStorage.setItem(HIDE_KEY, 'true');
      else localStorage.removeItem(HIDE_KEY);
    } catch { /* ignore */ }
  }

  function handleGoToLogs() {
    persistHidePreference();
    onGoToLogs();
  }

  function handleCreateIssue() {
    persistHidePreference();
    onCreateIssue();
  }
</script>

<Modal {isOpen} {onClose} title={$t('reportIssue.title')} maxWidth="520px">
  {#snippet children()}
    <p class="intro">{$t('reportIssue.intro')}</p>
  {/snippet}

  {#snippet footer()}
    <div class="footer">
      <label class="hide-row">
        <input type="checkbox" bind:checked={hideNextTime} />
        <span class="hide-label">{$t('reportIssue.hideNextTime')}</span>
      </label>
      <div class="actions">
        <button class="btn secondary" onclick={handleGoToLogs}>
          <FileText size={14} />
          {$t('reportIssue.goToLogs')}
        </button>
        <button class="btn primary" onclick={handleCreateIssue}>
          <ExternalLink size={14} />
          {$t('reportIssue.createIssue')}
        </button>
      </div>
    </div>
  {/snippet}
</Modal>

<style>
  .intro {
    margin: 0 0 8px;
    font-size: 14px;
    line-height: 1.55;
    color: var(--text-secondary);
  }

  .footer {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 100%;
  }

  .hide-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .hide-row input[type="checkbox"] {
    accent-color: var(--accent-primary);
  }

  .hide-label {
    flex: 1;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    font-size: 13px;
    border-radius: 6px;
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease, border-color 150ms ease;
  }

  .btn.secondary {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
  }

  .btn.secondary:hover {
    background: var(--bg-hover);
  }

  .btn.primary {
    background: var(--accent-primary);
    color: var(--text-primary);
    border: 1px solid var(--accent-primary);
  }

  .btn.primary:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }
</style>
