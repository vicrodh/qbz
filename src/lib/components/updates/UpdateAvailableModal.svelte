<script lang="ts">
  import Modal from '../Modal.svelte';

  interface Props {
    isOpen: boolean;
    currentVersion: string;
    newVersion: string;
    onClose: () => void;
    onVisitReleasePage: () => void;
  }

  let { isOpen, currentVersion, newVersion, onClose, onVisitReleasePage }: Props = $props();

  function handleVisit(): void {
    onVisitReleasePage();
  }
</script>

<Modal {isOpen} onClose={onClose} title="New release available" maxWidth="560px">
  <div class="update-modal">
    <p class="lead">A new version of QBZ has been released</p>

    <div class="version-row" aria-label="Version change">
      <span class="version-chip">v{currentVersion}</span>
      <span class="arrow">→</span>
      <span class="version-chip new">v{newVersion}</span>
    </div>

    <button class="download-btn" onclick={handleVisit} type="button">
      Download on GitHub
    </button>
  </div>

  {#snippet footer()}
    <div class="footer-actions">
      <button class="btn btn-ghost" type="button" onclick={onClose}>Close</button>
      <button class="btn btn-primary" type="button" onclick={handleVisit}>Visit release page</button>
    </div>
  {/snippet}
</Modal>

<style>
  .update-modal {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 16px;
    padding: 8px 0;
  }

  .lead {
    margin: 0;
    color: var(--text-primary);
    font-size: 16px;
    font-weight: 500;
  }

  .version-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 4px;
  }

  .version-chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 6px 12px;
    border-radius: 999px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-color, var(--bg-tertiary));
    color: var(--text-primary);
    font-weight: 600;
  }

  .version-chip.new {
    background: color-mix(in srgb, var(--accent-primary) 16%, var(--bg-tertiary));
    border-color: color-mix(in srgb, var(--accent-primary) 40%, var(--bg-tertiary));
  }

  .arrow {
    color: var(--text-muted);
    font-size: 18px;
  }

  .download-btn {
    margin-top: 8px;
    border: none;
    background: transparent;
    color: var(--accent-primary);
    font-weight: 600;
    cursor: pointer;
    padding: 8px 10px;
    border-radius: 8px;
  }

  .download-btn:hover {
    background: var(--bg-hover);
  }

  .footer-actions {
    display: flex;
    gap: 8px;
    width: 100%;
    justify-content: flex-end;
  }
</style>

