/**
 * Genre filter store with context support
 * Each context (home, favorites) has independent persistence
 */

import { invoke } from '@tauri-apps/api/core';

export interface GenreInfo {
  id: number;
  name: string;
  color?: string;
  slug?: string;
  parentId?: number;
}

export type GenreFilterContext = 'home' | 'favorites';

interface ContextState {
  selectedGenreIds: Set<number>;
  rememberSelection: boolean;
  listeners: Set<() => void>;
}

interface GenreFilterState {
  parentGenres: GenreInfo[];
  allGenres: GenreInfo[];
  childrenByParent: Map<number, GenreInfo[]>;
  isLoading: boolean;
  contexts: Record<GenreFilterContext, ContextState>;
}

const STORAGE_KEYS: Record<GenreFilterContext, string> = {
  home: 'qbz_genre_filter_home',
  favorites: 'qbz_genre_filter_favorites',
};

// Default context for backwards compatibility
let currentContext: GenreFilterContext = 'home';

const state: GenreFilterState = {
  parentGenres: [],
  allGenres: [],
  childrenByParent: new Map(),
  isLoading: false,
  contexts: {
    home: {
      selectedGenreIds: new Set(),
      rememberSelection: true,
      listeners: new Set(),
    },
    favorites: {
      selectedGenreIds: new Set(),
      rememberSelection: true,
      listeners: new Set(),
    },
  },
};

function getContextState(context?: GenreFilterContext): ContextState {
  return state.contexts[context ?? currentContext];
}

function notify(context?: GenreFilterContext) {
  const ctx = getContextState(context);
  ctx.listeners.forEach((fn) => fn());
}

function saveToStorage(context?: GenreFilterContext) {
  const ctx = context ?? currentContext;
  const ctxState = getContextState(ctx);
  if (!ctxState.rememberSelection) return;
  try {
    const data = {
      selectedGenreIds: Array.from(ctxState.selectedGenreIds),
      rememberSelection: ctxState.rememberSelection,
    };
    localStorage.setItem(STORAGE_KEYS[ctx], JSON.stringify(data));
  } catch (e) {
    console.error(`Failed to save genre filter state for ${ctx}:`, e);
  }
}

function loadFromStorage(context: GenreFilterContext) {
  try {
    const stored = localStorage.getItem(STORAGE_KEYS[context]);
    if (stored) {
      const data = JSON.parse(stored);
      state.contexts[context].selectedGenreIds = new Set(data.selectedGenreIds || []);
      state.contexts[context].rememberSelection = data.rememberSelection ?? true;
    }
  } catch (e) {
    console.error(`Failed to load genre filter state for ${context}:`, e);
  }
}

export function setContext(context: GenreFilterContext): void {
  currentContext = context;
}

export function getContext(): GenreFilterContext {
  return currentContext;
}

export async function loadGenres(): Promise<void> {
  if (state.parentGenres.length > 0) return; // Already loaded

  state.isLoading = true;
  // Notify all contexts
  notify('home');
  notify('favorites');

  try {
    // Fetch top-level genres first
    const parentGenres = await invoke<GenreInfo[]>('get_genres', {});

    // Fetch sub-genres for each parent in parallel
    const childrenByParent = new Map<number, GenreInfo[]>();
    const subGenrePromises = parentGenres.map(async (parent) => {
      try {
        const children = await invoke<GenreInfo[]>('get_genres', { parentId: parent.id });
        // Tag children with their parent ID
        const taggedChildren = children.map(c => ({ ...c, parentId: parent.id }));
        childrenByParent.set(parent.id, taggedChildren);
        return taggedChildren;
      } catch {
        childrenByParent.set(parent.id, []);
        return [];
      }
    });

    const subGenreResults = await Promise.all(subGenrePromises);
    const allSubGenres = subGenreResults.flat();

    // Combine all genres and remove duplicates by ID
    const allGenresRaw = [...parentGenres, ...allSubGenres];
    const allGenres = Array.from(
      new Map(allGenresRaw.map(g => [g.id, g])).values()
    );

    // Sort alphabetically by name
    allGenres.sort((a, b) => a.name.localeCompare(b.name));
    parentGenres.sort((a, b) => a.name.localeCompare(b.name));

    state.parentGenres = parentGenres;
    state.allGenres = allGenres;
    state.childrenByParent = childrenByParent;

    // Load saved selections for all contexts
    loadFromStorage('home');
    loadFromStorage('favorites');

    // Validate saved selections against available genres
    const validIds = new Set(allGenres.map((g) => g.id));
    for (const ctx of ['home', 'favorites'] as GenreFilterContext[]) {
      const ctxState = state.contexts[ctx];
      const validSelection = new Set<number>();
      ctxState.selectedGenreIds.forEach((id) => {
        if (validIds.has(id)) {
          validSelection.add(id);
        }
      });
      ctxState.selectedGenreIds = validSelection;
    }
  } catch (e) {
    console.error('Failed to load genres:', e);
    state.parentGenres = [];
    state.allGenres = [];
    state.childrenByParent = new Map();
  } finally {
    state.isLoading = false;
    notify('home');
    notify('favorites');
  }
}

export function getGenreFilterState(context?: GenreFilterContext) {
  const ctx = getContextState(context);
  return {
    availableGenres: state.parentGenres,
    allGenres: state.allGenres,
    selectedGenreIds: new Set(ctx.selectedGenreIds),
    isLoading: state.isLoading,
    rememberSelection: ctx.rememberSelection,
  };
}

export function getAvailableGenres(): GenreInfo[] {
  return state.parentGenres;
}

export function getAllGenres(): GenreInfo[] {
  return state.allGenres;
}

export function getChildGenres(parentId: number): GenreInfo[] {
  return state.childrenByParent.get(parentId) || [];
}

export function getSelectedGenreIds(context?: GenreFilterContext): Set<number> {
  return new Set(getContextState(context).selectedGenreIds);
}

export function getSelectedGenreId(context?: GenreFilterContext): number | undefined {
  const ids = Array.from(getContextState(context).selectedGenreIds);
  return ids.length === 1 ? ids[0] : undefined;
}

export function getSelectedGenreName(context?: GenreFilterContext): string | undefined {
  const id = getSelectedGenreId(context);
  if (!id) return undefined;
  const genre = state.allGenres.find((g: GenreInfo) => g.id === id);
  return genre?.name;
}

export function getSelectedGenreNames(context?: GenreFilterContext): string[] {
  const ids = Array.from(getContextState(context).selectedGenreIds);
  return ids
    .map(id => state.allGenres.find((g: GenreInfo) => g.id === id)?.name)
    .filter((name): name is string => !!name);
}

/**
 * Get all genre names to filter by, including children of selected parent genres.
 * Use this for client-side filtering to ensure albums with sub-genres are included.
 */
export function getFilterGenreNames(context?: GenreFilterContext): string[] {
  const selectedIds = Array.from(getContextState(context).selectedGenreIds);
  const names = new Set<string>();

  for (const id of selectedIds) {
    // Add the selected genre's name
    const genre = state.allGenres.find((g: GenreInfo) => g.id === id);
    if (genre?.name) {
      names.add(genre.name);
    }

    // If it's a parent genre, also add all its children's names
    const children = state.childrenByParent.get(id);
    if (children) {
      for (const child of children) {
        if (child.name) {
          names.add(child.name);
        }
      }
    }
  }

  return Array.from(names);
}

export function isGenreSelected(genreId: number, context?: GenreFilterContext): boolean {
  return getContextState(context).selectedGenreIds.has(genreId);
}

export function hasActiveFilter(context?: GenreFilterContext): boolean {
  return getContextState(context).selectedGenreIds.size > 0;
}

export function toggleGenre(genreId: number, context?: GenreFilterContext): void {
  const ctx = context ?? currentContext;
  const ctxState = getContextState(ctx);
  if (ctxState.selectedGenreIds.has(genreId)) {
    ctxState.selectedGenreIds.delete(genreId);
  } else {
    ctxState.selectedGenreIds.add(genreId);
  }
  saveToStorage(ctx);
  notify(ctx);
}

export function selectGenre(genreId: number, context?: GenreFilterContext): void {
  const ctx = context ?? currentContext;
  const ctxState = getContextState(ctx);
  ctxState.selectedGenreIds.clear();
  ctxState.selectedGenreIds.add(genreId);
  saveToStorage(ctx);
  notify(ctx);
}

export function clearSelection(context?: GenreFilterContext): void {
  const ctx = context ?? currentContext;
  const ctxState = getContextState(ctx);
  ctxState.selectedGenreIds.clear();
  saveToStorage(ctx);
  notify(ctx);
}

export function setRememberSelection(remember: boolean, context?: GenreFilterContext): void {
  const ctx = context ?? currentContext;
  const ctxState = getContextState(ctx);
  ctxState.rememberSelection = remember;
  if (remember) {
    saveToStorage(ctx);
  } else {
    localStorage.removeItem(STORAGE_KEYS[ctx]);
  }
  notify(ctx);
}

export function subscribe(callback: () => void, context?: GenreFilterContext): () => void {
  const ctxState = getContextState(context);
  ctxState.listeners.add(callback);
  return () => ctxState.listeners.delete(callback);
}

// Initialize by loading from storage for all contexts
loadFromStorage('home');
loadFromStorage('favorites');
