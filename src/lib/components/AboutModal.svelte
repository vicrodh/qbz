<script lang="ts">
  import { X, Github, Globe, ExternalLink } from 'lucide-svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  const APP_VERSION = '0.1.0';
  const BUILD_DATE = new Date().toISOString().split('T')[0];

  function handleOpenUrl(url: string) {
    openUrl(url).catch(err => console.error('Failed to open URL:', err));
  }
</script>

{#if isOpen}
  <div class="modal-backdrop" onclick={onClose}>
    <div class="modal" onclick={(e) => e.stopPropagation()}>
      <!-- Header -->
      <div class="modal-header">
        <div class="app-branding">
          <img src="/icons/AppIcons/android/96x96.png" alt="QBZ" class="app-icon" />
          <div class="app-title">
            <h2>QBZ</h2>
            <span class="version">v{APP_VERSION}</span>
          </div>
        </div>
        <button class="close-btn" onclick={onClose}>
          <X size={18} />
        </button>
      </div>

      <!-- Content -->
      <div class="modal-content">
        <!-- Description -->
        <p class="description">
          A native Qobuz client for Linux, designed for audiophiles who need
          bit-perfect Hi-Fi playback without browser sample rate limitations.
        </p>

        <!-- Links -->
        <div class="links">
          <button class="link-btn" onclick={() => handleOpenUrl('https://github.com/vicrodh/qbz')}>
            <Github size={16} />
            <span>GitHub</span>
            <ExternalLink size={12} />
          </button>
          <button class="link-btn" onclick={() => handleOpenUrl('https://qbz.lol')}>
            <Globe size={16} />
            <span>Website</span>
            <ExternalLink size={12} />
          </button>
        </div>

        <!-- Build Info -->
        <div class="info-section">
          <h3>Build Info</h3>
          <div class="info-grid">
            <span class="label">Version</span>
            <span class="value">{APP_VERSION}</span>
            <span class="label">License</span>
            <span class="value">MIT</span>
            <span class="label">Platform</span>
            <span class="value">Linux (Tauri 2.0)</span>
            <span class="label">Build</span>
            <span class="value">{BUILD_DATE}</span>
          </div>
        </div>

        <!-- Attributions -->
        <div class="info-section">
          <h3>Acknowledgments</h3>
          <div class="attributions">
            <div class="attribution">
              <strong>Qobuz</strong> — Music streaming service and API
            </div>
            <div class="attribution">
              <strong>Tauri</strong> — Application framework (MIT/Apache-2.0)
            </div>
            <div class="attribution">
              <strong>Svelte</strong> — UI framework (MIT)
            </div>
            <div class="attribution">
              <strong>Rodio + Symphonia</strong> — Audio playback engine
            </div>
            <div class="attribution">
              <strong>Lucide</strong> — Icon library (ISC)
            </div>
            <div class="attribution">
              <strong>Song.link/Odesli</strong> — Music link aggregation API
            </div>
          </div>
        </div>

        <!-- Author -->
        <div class="info-section author-section">
          <h3>Author</h3>
          <button class="author-link" onclick={() => handleOpenUrl('https://github.com/vicrodh')}>
            Victor RH
            <ExternalLink size={12} />
          </button>
        </div>

        <!-- Signature -->
        <div class="signature">
          <p>
            Made with <span class="strikethrough">love in <img src="/mexico-flag.svg" alt="México" class="inline-icon flag" /></span> hatred.
          </p>
          <p class="signature-detail">
            Hatred towards all those companies that discriminate against the Linux community
            and don't provide us with a decent client for their product.
            <img src="/Tux.svg" alt="Tux" class="inline-icon tux" />
          </p>
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
    z-index: 1000;
    animation: fadeIn 150ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .modal {
    background: var(--bg-secondary);
    border-radius: 16px;
    width: 90%;
    max-width: 672px;
    max-height: 85vh;
    overflow-y: auto;
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
    padding: 24px 24px 16px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .app-branding {
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .app-icon {
    width: 56px;
    height: 56px;
    border-radius: 12px;
  }

  .app-title h2 {
    font-size: 24px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
  }

  .version {
    font-size: 13px;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .close-btn {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 8px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
  }

  .close-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .modal-content {
    padding: 24px;
  }

  .description {
    font-size: 14px;
    line-height: 1.6;
    color: var(--text-secondary);
    margin: 0 0 24px;
  }

  .links {
    display: flex;
    gap: 12px;
    margin-bottom: 24px;
  }

  .link-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    background: var(--bg-tertiary);
    border-radius: 8px;
    color: var(--text-primary);
    text-decoration: none;
    font-size: 13px;
    font-weight: 500;
    transition: all 150ms ease;
  }

  .link-btn:hover {
    background: var(--accent-primary);
    color: white;
  }

  .link-btn :global(svg:last-child) {
    opacity: 0.5;
  }

  .info-section {
    margin-bottom: 20px;
  }

  .info-section h3 {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin: 0 0 12px;
  }

  .info-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 16px;
    font-size: 13px;
  }

  .label {
    color: var(--text-muted);
  }

  .value {
    color: var(--text-primary);
    font-family: var(--font-mono);
  }

  .attributions {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .attribution {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .attribution strong {
    color: var(--text-primary);
    font-weight: 500;
  }

  .author-link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 14px;
    color: var(--text-primary);
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    transition: color 150ms ease;
  }

  .author-link:hover {
    color: var(--accent-primary);
  }

  .author-link :global(svg) {
    opacity: 0.5;
  }

  .signature {
    margin-top: 24px;
    padding-top: 20px;
    border-top: 1px solid var(--bg-tertiary);
    text-align: center;
  }

  .signature p {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0 0 8px;
    line-height: 1.6;
  }

  .strikethrough {
    text-decoration: line-through;
    opacity: 0.6;
  }

  .signature-detail {
    font-size: 11px !important;
    max-width: 360px;
    margin: 0 auto !important;
  }

  .inline-icon {
    display: inline-block;
    vertical-align: middle;
    height: 1.3em;
    width: auto;
  }

  .inline-icon.flag {
    margin: 0 2px;
    height: 1.1em;
  }

  .inline-icon.tux {
    margin-left: 4px;
    height: 1.4em;
  }
</style>
