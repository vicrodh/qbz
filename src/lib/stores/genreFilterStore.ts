/**
 * Genre filter store with context support and lazy-loaded 3-level hierarchy
 * Each context (home, favorites) has independent persistence
 */

import { invoke } from '@tauri-apps/api/core';

export interface GenreInfo {
  id: number;
  name: string;
  color?: string;
  slug?: string;
  parentId?: number;
  childrenLoaded?: boolean;
}

export interface GenreTreeNode {
  genre: GenreInfo;
  children: GenreTreeNode[];
  loading?: boolean;
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
  genreTree: GenreTreeNode[];
  childrenByParent: Map<number, GenreInfo[]>;
  isLoading: boolean;
  contexts: Record<GenreFilterContext, ContextState>;
}

const STORAGE_KEYS: Record<GenreFilterContext, string> = {
  home: 'qbz_genre_filter_home',
  favorites: 'qbz_genre_filter_favorites',
};

const GENRE_CACHE_KEY = 'qbz_genre_cache';
const GENRE_CACHE_TTL_DAYS = 30;

interface GenreCache {
  timestamp: number;
  parentGenres: GenreInfo[];
  childrenByParent: Record<number, GenreInfo[]>;
}

function saveGenreCache() {
  try {
    const cache: GenreCache = {
      timestamp: Date.now(),
      parentGenres: state.parentGenres,
      childrenByParent: Object.fromEntries(state.childrenByParent),
    };
    localStorage.setItem(GENRE_CACHE_KEY, JSON.stringify(cache));
  } catch (e) {
    console.error('Failed to save genre cache:', e);
  }
}

function loadGenreCache(): GenreCache | null {
  try {
    const stored = localStorage.getItem(GENRE_CACHE_KEY);
    if (!stored) return null;

    const cache: GenreCache = JSON.parse(stored);
    const ageInDays = (Date.now() - cache.timestamp) / (1000 * 60 * 60 * 24);

    if (ageInDays > GENRE_CACHE_TTL_DAYS) {
      localStorage.removeItem(GENRE_CACHE_KEY);
      return null;
    }

    return cache;
  } catch (e) {
    console.error('Failed to load genre cache:', e);
    return null;
  }
}

let currentContext: GenreFilterContext = 'home';

const state: GenreFilterState = {
  parentGenres: [],
  allGenres: [],
  genreTree: [],
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

function notifyAll() {
  notify('home');
  notify('favorites');
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

/** Initial load: use cache if valid, otherwise fetch from API */
export async function loadGenres(): Promise<void> {
  if (state.parentGenres.length > 0) return;

  // Try loading from cache first
  const cache = loadGenreCache();
  if (cache) {
    state.parentGenres = cache.parentGenres;
    state.childrenByParent = new Map(Object.entries(cache.childrenByParent).map(([k, v]) => [Number(k), v]));
    state.allGenres = [...cache.parentGenres, ...Object.values(cache.childrenByParent).flat()];
    state.genreTree = buildTreeFromCache(cache);
    loadFromStorage('home');
    loadFromStorage('favorites');
    notifyAll();
    return;
  }

  // Fetch ALL 3 levels from API (first load or refresh)
  state.isLoading = true;
  notifyAll();

  try {
    const parentGenres = await invoke<GenreInfo[]>('get_genres', {});
    parentGenres.sort((a, b) => a.name.localeCompare(b.name));

    // Load all children and grandchildren in parallel
    const childrenByParent = new Map<number, GenreInfo[]>();
    const allGenresList: GenreInfo[] = [...parentGenres];

    // Fetch level 2 (children) for all parents in parallel
    const level2Results = await Promise.all(
      parentGenres.map(async (parent) => {
        const children = await invoke<GenreInfo[]>('get_genres', { parentId: parent.id });
        return { parentId: parent.id, children: children.map(c => ({ ...c, parentId: parent.id })) };
      })
    );

    for (const { parentId, children } of level2Results) {
      childrenByParent.set(parentId, children);
      allGenresList.push(...children);
    }

    // Fetch level 3 (grandchildren) for all children in parallel
    const allChildren = level2Results.flatMap(r => r.children);
    const level3Results = await Promise.all(
      allChildren.map(async (child) => {
        const grandchildren = await invoke<GenreInfo[]>('get_genres', { parentId: child.id });
        return { parentId: child.id, grandchildren: grandchildren.map(gc => ({ ...gc, parentId: child.id })) };
      })
    );

    for (const { parentId, grandchildren } of level3Results) {
      if (grandchildren.length > 0) {
        childrenByParent.set(parentId, grandchildren);
        allGenresList.push(...grandchildren);
      }
    }

    // Build tree
    const genreTree: GenreTreeNode[] = parentGenres.map(parent => {
      const children = childrenByParent.get(parent.id) || [];
      return {
        genre: { ...parent, childrenLoaded: true },
        children: children.map(child => {
          const grandchildren = childrenByParent.get(child.id) || [];
          return {
            genre: { ...child, childrenLoaded: true },
            children: grandchildren.map(gc => ({ genre: gc, children: [] })),
          };
        }),
      };
    });

    state.parentGenres = parentGenres;
    state.allGenres = allGenresList;
    state.genreTree = genreTree;
    state.childrenByParent = childrenByParent;

    saveGenreCache();
    loadFromStorage('home');
    loadFromStorage('favorites');
  } catch (e) {
    console.error('Failed to load genres:', e);
    state.parentGenres = [];
    state.allGenres = [];
    state.genreTree = [];
  } finally {
    state.isLoading = false;
    notifyAll();
  }
}

function buildTreeFromCache(cache: GenreCache): GenreTreeNode[] {
  return cache.parentGenres.map(parent => {
    const children = cache.childrenByParent[parent.id] || [];
    return {
      genre: { ...parent, childrenLoaded: children.length > 0 },
      children: children.map(child => {
        const grandchildren = cache.childrenByParent[child.id] || [];
        return {
          genre: { ...child, childrenLoaded: grandchildren.length > 0 },
          children: grandchildren.map(gc => ({ genre: gc, children: [] })),
        };
      }),
    };
  });
}

/** Force refresh genres from API (clears cache) */
export async function forceRefreshGenres(): Promise<void> {
  localStorage.removeItem(GENRE_CACHE_KEY);
  state.parentGenres = [];
  state.allGenres = [];
  state.genreTree = [];
  state.childrenByParent = new Map();
  await loadGenres();
}

/** Lazy load children for a specific genre (silent = don't notify) */
export async function loadChildren(genreId: number, silent = false): Promise<GenreInfo[]> {
  // Check if already loaded
  if (state.childrenByParent.has(genreId)) {
    return state.childrenByParent.get(genreId) || [];
  }

  try {
    const children = await invoke<GenreInfo[]>('get_genres', { parentId: genreId });
    const taggedChildren = children.map(c => ({ ...c, parentId: genreId, childrenLoaded: false }));

    state.childrenByParent.set(genreId, taggedChildren);

    // Add to allGenres if not already present
    for (const child of taggedChildren) {
      if (!state.allGenres.find(g => g.id === child.id)) {
        state.allGenres.push(child);
      }
    }

    // Update tree node
    updateTreeNode(genreId, taggedChildren);

    // Update cache
    saveGenreCache();

    if (!silent) {
      notifyAll();
    }
    return taggedChildren;
  } catch (e) {
    console.error(`Failed to load children for genre ${genreId}:`, e);
    state.childrenByParent.set(genreId, []);
    return [];
  }
}

function updateTreeNode(parentId: number, children: GenreInfo[]) {
  // Find and update the node in the tree
  function updateNode(nodes: GenreTreeNode[]): boolean {
    for (const node of nodes) {
      if (node.genre.id === parentId) {
        node.genre.childrenLoaded = true;
        node.children = children.map(c => ({
          genre: c,
          children: [],
        }));
        return true;
      }
      if (updateNode(node.children)) return true;
    }
    return false;
  }

  updateNode(state.genreTree);
}

/** Check if children are loaded for a genre */
export function areChildrenLoaded(genreId: number): boolean {
  return state.childrenByParent.has(genreId);
}

export function getGenreFilterState(context?: GenreFilterContext) {
  const ctx = getContextState(context);
  return {
    availableGenres: state.parentGenres,
    allGenres: state.allGenres,
    genreTree: state.genreTree,
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

export function getGenreTree(): GenreTreeNode[] {
  return state.genreTree;
}

export function getChildGenres(parentId: number): GenreInfo[] {
  return state.childrenByParent.get(parentId) || [];
}

/** Get all loaded descendant IDs for a genre */
export function getAllDescendantIds(genreId: number): number[] {
  const descendants: number[] = [];
  const children = state.childrenByParent.get(genreId) || [];

  for (const child of children) {
    descendants.push(child.id);
    const grandchildren = state.childrenByParent.get(child.id) || [];
    descendants.push(...grandchildren.map(gc => gc.id));
  }

  return descendants;
}

/** Count loaded descendants */
export function countDescendants(genreId: number): number {
  return getAllDescendantIds(genreId).length;
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
 * Get all genre names to filter by, including all loaded descendants.
 */
export function getFilterGenreNames(context?: GenreFilterContext): string[] {
  const selectedIds = Array.from(getContextState(context).selectedGenreIds);
  const names = new Set<string>();

  for (const id of selectedIds) {
    const genre = state.allGenres.find((g: GenreInfo) => g.id === id);
    if (genre?.name) {
      names.add(genre.name);
    }

    const descendantIds = getAllDescendantIds(id);
    for (const descId of descendantIds) {
      const descGenre = state.allGenres.find((g: GenreInfo) => g.id === descId);
      if (descGenre?.name) {
        names.add(descGenre.name);
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
    // Eagerly load all descendants for filtering
    loadAllDescendants(genreId);
  }
  saveToStorage(ctx);
  notify(ctx);
}

/** Load all descendants (children and grandchildren) for a genre - single notification */
async function loadAllDescendants(genreId: number): Promise<void> {
  let loaded = false;

  // Load children if not loaded (silent)
  if (!state.childrenByParent.has(genreId)) {
    const children = await loadChildren(genreId, true);
    loaded = true;
    // Load grandchildren for each child in parallel (silent)
    await Promise.all(
      children.map(child =>
        !state.childrenByParent.has(child.id)
          ? loadChildren(child.id, true)
          : Promise.resolve([])
      )
    );
  } else {
    // Children already loaded, check grandchildren
    const children = state.childrenByParent.get(genreId) || [];
    const grandchildPromises = children
      .filter(child => !state.childrenByParent.has(child.id))
      .map(child => loadChildren(child.id, true));

    if (grandchildPromises.length > 0) {
      await Promise.all(grandchildPromises);
      loaded = true;
    }
  }

  // Single notification after all loads complete
  if (loaded) {
    notifyAll();
  }
}

export function setGenresSelected(genreIds: number[], selected: boolean, context?: GenreFilterContext): void {
  const ctx = context ?? currentContext;
  const ctxState = getContextState(ctx);

  for (const id of genreIds) {
    if (selected) {
      ctxState.selectedGenreIds.add(id);
    } else {
      ctxState.selectedGenreIds.delete(id);
    }
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

loadFromStorage('home');
loadFromStorage('favorites');
