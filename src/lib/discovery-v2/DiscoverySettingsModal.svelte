<script lang="ts">
  import { t } from '$lib/i18n';
  import Modal from './ModalLite.svelte';
  import { ChevronUp, ChevronDown, RotateCcw, TriangleAlert } from 'lucide-svelte';
  import {
    sectionPrefs,
    toggleSection,
    moveSection,
    resetToDefaults,
    type DiscoverySectionId,
    type DiscoveryTab,
  } from './sectionPrefs';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    /** Which tab's section prefs this modal edits. The gear button in
     *  the toolbar always passes the currently-active tab. */
    tab: DiscoveryTab;
  }

  let { isOpen, onClose, tab }: Props = $props();

  /** Maps each section id to the existing i18n key for its display label.
   *  All keys already exist in the five locale files. */
  const labelKeys: Record<DiscoverySectionId, string> = {
    newReleases: 'home.newReleases',
    pressAwards: 'home.pressAwards',
    qobuzPlaylists: 'home.qobuzPlaylists',
    recentlyPlayedAlbums: 'home.recentlyPlayed',
    continueListening: 'home.continueListening',
    idealDiscography: 'discover.idealDiscography',
    mostStreamed: 'home.mostStreamed',
    releaseWatch: 'home.releaseWatch',
    editorPicks: 'home.editorPicks',
    qobuzissimes: 'home.qobuzissimes',
    topArtists: 'home.yourTopArtists',
    favoriteAlbums: 'home.favoriteAlbums',
    qobuzMixes: 'home.qobuzMixes',
    radioStations: 'home.radioStations',
    similarAlbums: 'discovery.similarAlbums',
    rediscoverLibrary: 'discovery.rediscoverLibrary',
    essentialsByGenre: 'discovery.essentialsByGenre',
    artistsToFollow: 'discovery.artistsToFollow',
    artistSpotlight: 'discovery.artistSpotlight',
  };

  const tabPrefs = $derived($sectionPrefs[tab]);
  const enabledCount = $derived(tabPrefs.filter((p) => p.enabled).length);
  const totalCount = $derived(tabPrefs.length);
</script>

<Modal {isOpen} {onClose} title={$t('discovery.customize')} maxWidth="520px">
  {#snippet children()}
    <div class="warning">
      <TriangleAlert size={16} />
      <p>{$t('discovery.perfWarning')}</p>
    </div>

    <p class="count">
      {$t('discovery.enabledCount', { values: { count: enabledCount, total: totalCount } })}
    </p>

    <ul class="list">
      {#each tabPrefs as pref, idx (pref.id)}
        <li class="row">
          <label class="check">
            <input
              type="checkbox"
              checked={pref.enabled}
              onchange={() => toggleSection(tab, pref.id)}
            />
            <span class="label">{$t(labelKeys[pref.id])}</span>
          </label>
          <div class="row-actions">
            <button
              type="button"
              class="move-btn"
              aria-label={$t('discovery.moveUp')}
              disabled={idx === 0}
              onclick={() => moveSection(tab, pref.id, -1)}
            >
              <ChevronUp size={16} />
            </button>
            <button
              type="button"
              class="move-btn"
              aria-label={$t('discovery.moveDown')}
              disabled={idx === tabPrefs.length - 1}
              onclick={() => moveSection(tab, pref.id, 1)}
            >
              <ChevronDown size={16} />
            </button>
          </div>
        </li>
      {/each}
    </ul>
  {/snippet}

  {#snippet footer()}
    <button type="button" class="reset-btn" onclick={() => resetToDefaults(tab)}>
      <RotateCcw size={14} />
      {$t('discovery.resetDefaults')}
    </button>
  {/snippet}
</Modal>

<style>
  /* Discovery V2 settings — zero effects beyond standard form controls.
     Inherits backdrop / portal / ESC handling from shared Modal.svelte. */
  .warning {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    margin-bottom: 12px;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.4;
  }

  .warning p {
    margin: 0;
  }

  .count {
    margin: 0 0 12px 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 4px;
  }

  .row:hover {
    background: var(--bg-tertiary);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    flex: 1;
    min-width: 0;
  }

  .check input {
    width: 16px;
    height: 16px;
    cursor: pointer;
  }

  .label {
    font-size: 14px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .move-btn {
    width: 26px;
    height: 26px;
    border-radius: 4px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .move-btn:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .move-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .reset-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    padding: 6px 8px;
    font-family: inherit;
  }

  .reset-btn:hover {
    color: var(--text-primary);
  }
</style>
