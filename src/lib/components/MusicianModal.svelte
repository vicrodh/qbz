<script lang="ts">
  /**
   * Musician Informational Modal
   *
   * Displays contextual information about a musician when navigation
   * to a full Musician Page is not appropriate (confidence level 0-1).
   *
   * This modal:
   * - Is NOT a page, provides context only
   * - Returns user to previous view on close
   * - Does NOT show albums, tracks, or playlists
   * - Does NOT allow deep navigation
   */
  import { X, User, Music, Info } from 'lucide-svelte';
  import type { ResolvedMusician } from '$lib/types';

  interface Props {
    musician: ResolvedMusician;
    onClose: () => void;
    onNavigateToArtist?: (artistId: number) => void;
  }

  let { musician, onClose, onNavigateToArtist }: Props = $props();

  // Explanatory copy based on confidence level
  const explanatoryCopy = $derived(() => {
    switch (musician.confidence) {
      case 'weak':
        return 'This musician appears in album credits but has limited catalog information available in Qobuz.';
      case 'none':
        return 'Unable to verify this musician\'s catalog. The information shown is based on album credits.';
      default:
        return 'This musician appears in album credits.';
    }
  });

  // Show CTA only if we have a Qobuz artist ID (confidence = confirmed)
  const showArtistCta = $derived(
    musician.confidence === 'confirmed' && musician.qobuz_artist_id
  );

  function handleNavigateToArtist() {
    if (musician.qobuz_artist_id && onNavigateToArtist) {
      onNavigateToArtist(musician.qobuz_artist_id);
      onClose();
    }
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onClose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="modal-backdrop"
  onclick={handleBackdropClick}
  role="dialog"
  aria-modal="true"
  aria-labelledby="musician-modal-title"
>
  <div class="modal">
    <!-- Header -->
    <div class="modal-header">
      <div class="musician-info">
        <div class="musician-icon">
          <User size={24} />
        </div>
        <div class="musician-details">
          <h2 id="musician-modal-title">{musician.name}</h2>
          <span class="role">{musician.role}</span>
        </div>
      </div>
      <button class="close-btn" onclick={onClose} title="Close">
        <X size={18} />
      </button>
    </div>

    <!-- Content -->
    <div class="modal-content">
      <!-- Bands & Projects -->
      {#if musician.bands.length > 0}
        <div class="section">
          <h3>Known for</h3>
          <div class="bands-list">
            {#each musician.bands as band}
              <div class="band-item">
                <Music size={14} />
                <span>{band}</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Explanatory Copy -->
      <div class="section info-section">
        <div class="info-box">
          <Info size={16} />
          <p>{explanatoryCopy()}</p>
        </div>
      </div>

      <!-- CTA Button (only for confirmed matches) -->
      {#if showArtistCta}
        <div class="section cta-section">
          <button class="cta-btn" onclick={handleNavigateToArtist}>
            View Artist Page
          </button>
        </div>
      {/if}
    </div>
  </div>
</div>

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
    border-radius: 16px;
    width: 90%;
    max-width: 420px;
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
    align-items: flex-start;
    justify-content: space-between;
    padding: 24px 24px 16px;
    border-bottom: 1px solid var(--bg-tertiary);
  }

  .musician-info {
    display: flex;
    align-items: flex-start;
    gap: 16px;
  }

  .musician-icon {
    width: 48px;
    height: 48px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border-radius: 50%;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .musician-details {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .musician-details h2 {
    font-size: 20px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
    line-height: 1.2;
  }

  .role {
    font-size: 14px;
    color: var(--text-muted);
    text-transform: capitalize;
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
    flex-shrink: 0;
  }

  .close-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .modal-content {
    padding: 24px;
  }

  .section {
    margin-bottom: 20px;
  }

  .section:last-child {
    margin-bottom: 0;
  }

  .section h3 {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin: 0 0 12px;
  }

  .bands-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .band-item {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 14px;
    color: var(--text-primary);
  }

  .band-item :global(svg) {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .info-section {
    margin-top: 24px;
  }

  .info-box {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 16px;
    background: var(--bg-tertiary);
    border-radius: 12px;
  }

  .info-box :global(svg) {
    color: var(--text-muted);
    flex-shrink: 0;
    margin-top: 2px;
  }

  .info-box p {
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-secondary);
    margin: 0;
  }

  .cta-section {
    margin-top: 24px;
    padding-top: 20px;
    border-top: 1px solid var(--bg-tertiary);
  }

  .cta-btn {
    width: 100%;
    padding: 12px 20px;
    background: var(--accent-primary);
    border: none;
    border-radius: 10px;
    color: white;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .cta-btn:hover {
    filter: brightness(1.1);
  }
</style>
