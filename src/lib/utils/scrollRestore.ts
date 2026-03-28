import { tick } from 'svelte';
import { getNavigationState, getSavedScrollPosition } from '../stores/navigationStore';

export function restoreScrollOnBackForward(
  containerEl: HTMLElement | null,
  setScrollTop?: (value: number) => void
) {
  const navState = getNavigationState();

  if (navState.isBackForward) {
    const saved = getSavedScrollPosition(navState.activeView, navState.activeItemId);
    setScrollTop?.(saved);

    tick().then(() => {
      if (containerEl) {
          containerEl.scrollTop = saved;
      }
    });
  }
}