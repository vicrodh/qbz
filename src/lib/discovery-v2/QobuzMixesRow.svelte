<script lang="ts">
  import { t } from '$lib/i18n';

  interface Props {
    onDailyQ?: () => void;
    onWeeklyQ?: () => void;
    onFavQ?: () => void;
    onTopQ?: () => void;
  }

  let { onDailyQ, onWeeklyQ, onFavQ, onTopQ }: Props = $props();

  /**
   * Four-tile "Qobuz Mixes" entry point. Mirrors the legacy ForYouTab
   * mix-cards row (DailyQ / WeeklyQ / FavQ / TopQ) — pure navigation
   * entries to dedicated mix views, no per-tile data fetch. Each tile
   * is a gradient artwork + label; clicking navigates to the relevant
   * mix view via callbacks from +page.svelte.
   */

  const tiles = $derived([
    {
      labelKey: 'qobuzMixes.daily',
      gradient: 'mix-gradient-daily',
      badge: 'qobuz',
      name: 'DailyQ',
      descKey: 'qobuzMixes.cardDesc',
      onClick: onDailyQ,
    },
    {
      labelKey: 'qobuzMixes.weekly',
      gradient: 'mix-gradient-weekly',
      badge: 'qobuz',
      name: 'WeeklyQ',
      descKey: 'weeklyMixes.cardDesc',
      onClick: onWeeklyQ,
    },
    {
      labelKey: 'qobuzMixes.fav',
      gradient: 'mix-gradient-favq',
      badge: 'qbz',
      name: 'FavQ',
      descKey: 'favMixes.cardDesc',
      onClick: onFavQ,
    },
    {
      labelKey: 'qobuzMixes.top',
      gradient: 'mix-gradient-topq',
      badge: 'qbz',
      name: 'TopQ',
      descKey: 'topMixes.cardDesc',
      onClick: onTopQ,
    },
  ].filter((tile) => !!tile.onClick));
</script>

<div class="mixes-row">
  {#each tiles as tile (tile.name)}
    <button class="mix-card" type="button" onclick={tile.onClick}>
      <div class="mix-artwork {tile.gradient}">
        <span class="mix-badge">{tile.badge}</span>
        <span class="mix-name">{tile.name}</span>
      </div>
      <p class="mix-desc">{@html $t(tile.descKey)}</p>
    </button>
  {/each}
</div>

<style>
  /* Fixed 220px tiles to match AlbumCardLite — earlier auto-fit minmax
     made the mixes stretch across the row and visually outsize every
     other card in the home view. Now they sit alongside album cards
     at the same scale. */
  .mixes-row {
    display: flex;
    flex-wrap: wrap;
    gap: 32px;
  }

  .mix-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 220px;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    font-family: inherit;
  }

  .mix-artwork {
    position: relative;
    width: 220px;
    height: 220px;
    border-radius: 8px;
    overflow: hidden;
    display: flex;
    align-items: flex-end;
    padding: 16px;
  }

  .mix-badge {
    position: absolute;
    top: 12px;
    left: 12px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: rgba(255, 255, 255, 0.9);
    background: rgba(0, 0, 0, 0.35);
    border-radius: 3px;
    padding: 3px 7px;
  }

  .mix-name {
    font-size: 22px;
    font-weight: 700;
    color: #fff;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
  }

  .mix-desc {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  /* Gradient variants — match the legacy ForYouTab visual identity. */
  .mix-gradient-daily {
    background: linear-gradient(135deg, #1e3a8a 0%, #6366f1 50%, #c084fc 100%);
  }
  .mix-gradient-weekly {
    background: linear-gradient(135deg, #065f46 0%, #10b981 50%, #fbbf24 100%);
  }
  .mix-gradient-favq {
    background: linear-gradient(135deg, #7f1d1d 0%, #ef4444 50%, #fb923c 100%);
  }
  .mix-gradient-topq {
    background: linear-gradient(135deg, #1f2937 0%, #4b5563 50%, #fbbf24 100%);
  }
</style>
