/**
 * Genre filter store for Home page filtering
 * Persists selected genres to localStorage
 */

import { invoke } from '@tauri-apps/api/core';

export interface GenreInfo {
  id: number;
  name: string;
  color?: string;
  slug?: string;
}

interface GenreFilterState {
  availableGenres: GenreInfo[];
  selectedGenreIds: Set<number>;
  isLoading: boolean;
  rememberSelection: boolean;
}

const STORAGE_KEY = 'qbz_genre_filter';

let state: GenreFilterState = {
  availableGenres: [],
  selectedGenreIds: new Set(),
  isLoading: false,
  rememberSelection: true,
};

const listeners: Set<() => void> = new Set();

function notify() {
  listeners.forEach((fn) => fn());
}

function saveToStorage() {
  if (!state.rememberSelection) return;
  try {
    const data = {
      selectedGenreIds: Array.from(state.selectedGenreIds),
      rememberSelection: state.rememberSelection,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  } catch (e) {
    console.error('Failed to save genre filter state:', e);
  }
}

function loadFromStorage() {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const data = JSON.parse(stored);
      state.selectedGenreIds = new Set(data.selectedGenreIds || []);
      state.rememberSelection = data.rememberSelection ?? true;
    }
  } catch (e) {
    console.error('Failed to load genre filter state:', e);
  }
}

export async function loadGenres(): Promise<void> {
  if (state.availableGenres.length > 0) return; // Already loaded

  state.isLoading = true;
  notify();

  try {
    const genres = await invoke<GenreInfo[]>('get_genres', {});
    state.availableGenres = genres;

    // Load saved selection after genres are available
    loadFromStorage();

    // Validate saved selection against available genres
    const validIds = new Set(genres.map((g) => g.id));
    const validSelection = new Set<number>();
    state.selectedGenreIds.forEach((id) => {
      if (validIds.has(id)) {
        validSelection.add(id);
      }
    });
    state.selectedGenreIds = validSelection;
  } catch (e) {
    console.error('Failed to load genres:', e);
    state.availableGenres = [];
  } finally {
    state.isLoading = false;
    notify();
  }
}

export function getGenreFilterState(): GenreFilterState {
  return { ...state, selectedGenreIds: new Set(state.selectedGenreIds) };
}

export function getAvailableGenres(): GenreInfo[] {
  return state.availableGenres;
}

export function getSelectedGenreIds(): Set<number> {
  return new Set(state.selectedGenreIds);
}

export function getSelectedGenreId(): number | undefined {
  // For API calls that only support single genre_id
  const ids = Array.from(state.selectedGenreIds);
  return ids.length === 1 ? ids[0] : undefined;
}

export function getSelectedGenreName(): string | undefined {
  const id = getSelectedGenreId();
  if (!id) return undefined;
  const genre = state.availableGenres.find(g => g.id === id);
  return genre?.name;
}

export function getSelectedGenreNames(): string[] {
  const ids = Array.from(state.selectedGenreIds);
  return ids
    .map(id => state.availableGenres.find(g => g.id === id)?.name)
    .filter((name): name is string => !!name);
}

export function isGenreSelected(genreId: number): boolean {
  return state.selectedGenreIds.has(genreId);
}

export function hasActiveFilter(): boolean {
  return state.selectedGenreIds.size > 0;
}

export function toggleGenre(genreId: number): void {
  if (state.selectedGenreIds.has(genreId)) {
    state.selectedGenreIds.delete(genreId);
  } else {
    state.selectedGenreIds.add(genreId);
  }
  saveToStorage();
  notify();
}

export function selectGenre(genreId: number): void {
  state.selectedGenreIds.clear();
  state.selectedGenreIds.add(genreId);
  saveToStorage();
  notify();
}

export function clearSelection(): void {
  state.selectedGenreIds.clear();
  saveToStorage();
  notify();
}

export function setRememberSelection(remember: boolean): void {
  state.rememberSelection = remember;
  if (remember) {
    saveToStorage();
  } else {
    localStorage.removeItem(STORAGE_KEY);
  }
  notify();
}

export function subscribe(callback: () => void): () => void {
  listeners.add(callback);
  return () => listeners.delete(callback);
}

// Initialize by loading from storage
loadFromStorage();
