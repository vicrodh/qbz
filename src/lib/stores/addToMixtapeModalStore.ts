import { writable } from 'svelte/store';
import type { AddToMixtapeItem } from '$lib/components/AddToMixtapeModal.svelte';

interface State {
  open: boolean;
  /**
   * List of items to add. Single-item callers pass a 1-element array
   * (or a single item via the overloaded helper below); bulk callers
   * (BulkActionBar) pass many. The modal loops over all entries when the
   * user picks a target collection.
   */
  items: AddToMixtapeItem[];
}

export const addToMixtapeModal = writable<State>({ open: false, items: [] });

export function openAddToMixtape(
  input: AddToMixtapeItem | AddToMixtapeItem[],
): void {
  const items = Array.isArray(input) ? input : [input];
  if (items.length === 0) return;
  addToMixtapeModal.set({ open: true, items });
}

export function closeAddToMixtape(): void {
  addToMixtapeModal.set({ open: false, items: [] });
}
