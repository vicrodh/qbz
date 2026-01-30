<script lang="ts">
  import { X, Maximize2, Minimize2, Music2, Info, Radio, BarChart3, ListMusic } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  export type ImmersiveTab = 'lyrics' | 'credits' | 'suggestions' | 'visualizer' | 'queue';

  interface Props {
    activeTab: ImmersiveTab;
    onTabChange: (tab: ImmersiveTab) => void;
    onClose: () => void;
    visible?: boolean;
    hasLyrics?: boolean;
    hasCredits?: boolean;
    hasSuggestions?: boolean;
    hasVisualizer?: boolean;
  }

  let {
    activeTab,
    onTabChange,
    onClose,
    visible = true,
    hasLyrics = true,
    hasCredits = true,
    hasSuggestions = true,
    hasVisualizer = false
  }: Props = $props();

  const tabs = $derived([
    { id: 'lyrics' as const, label: $t('player.lyrics'), icon: Music2, enabled: hasLyrics },
    { id: 'credits' as const, label: $t('player.credits') || 'Credits', icon: Info, enabled: hasCredits },
    { id: 'suggestions' as const, label: $t('player.suggestions') || 'Suggestions', icon: Radio, enabled: hasSuggestions },
    { id: 'visualizer' as const, label: $t('player.visualizer') || 'Visualizer', icon: BarChart3, enabled: hasVisualizer },
    { id: 'queue' as const, label: $t('player.queue') || 'Queue', icon: ListMusic, enabled: true },
  ].filter(tab => tab.enabled));
</script>

<header class="immersive-header" class:visible>
  <!-- Left: Logo/Brand (optional) -->
  <div class="header-left">
    <!-- Reserved for future use -->
  </div>

  <!-- Center/Right: Tabs -->
  <nav class="tabs">
    {#each tabs as tab (tab.id)}
      <button
        class="tab"
        class:active={activeTab === tab.id}
        onclick={() => onTabChange(tab.id)}
      >
        <tab.icon size={16} />
        <span class="tab-label">{tab.label}</span>
      </button>
    {/each}
  </nav>

  <!-- Right: Actions -->
  <div class="header-actions">
    <button class="action-btn" onclick={onClose} title={$t('actions.close') + ' (Esc)'}>
      <X size={20} />
    </button>
  </div>
</header>

<style>
  .immersive-header {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 20;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    opacity: 0;
    transform: translateY(-8px);
    transition: opacity 250ms ease, transform 250ms ease;
    pointer-events: none;
  }

  .immersive-header.visible {
    opacity: 1;
    transform: translateY(0);
    pointer-events: auto;
  }

  .header-left {
    flex: 1;
    min-width: 100px;
  }

  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px;
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    border-radius: 12px;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: none;
    border: none;
    border-radius: 8px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .tab:hover {
    color: var(--alpha-90, rgba(255, 255, 255, 0.9));
    background: var(--alpha-10, rgba(255, 255, 255, 0.1));
  }

  .tab.active {
    color: var(--text-primary, white);
    background: var(--alpha-15, rgba(255, 255, 255, 0.15));
  }


  .header-actions {
    flex: 1;
    min-width: 100px;
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid var(--alpha-10, rgba(255, 255, 255, 0.1));
    border-radius: 50%;
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    cursor: pointer;
    transition: all 150ms ease;
  }

  .action-btn:hover {
    color: var(--text-primary, white);
    background: rgba(0, 0, 0, 0.5);
  }

  /* Responsive */
  @media (max-width: 900px) {
    .tabs {
      padding: 3px;
    }

    .tab {
      padding: 8px 12px;
    }

    .tab-label {
      display: none;
    }
  }

  @media (max-width: 600px) {
    .immersive-header {
      padding: 12px 16px;
    }

    .header-left {
      display: none;
    }

    .action-btn {
      width: 36px;
      height: 36px;
    }
  }
</style>
