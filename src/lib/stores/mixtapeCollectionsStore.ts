/**
 * Mixtape & Collection store.
 *
 * Wraps the v2_* backend commands registered in src-tauri. The store
 * holds the flat list of collections (all kinds) so the Sidebar /
 * MyQBZ grid views can subscribe; calls that return a single
 * collection (get / enqueue / add item) do NOT modify the store
 * shape — consumers call loadCollections() to refresh when needed.
 *
 * Typed in snake_case to match the backend JSON envelope.
 */

import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

export type CollectionKind = 'mixtape' | 'collection' | 'artist_collection';
export type CollectionSourceType = 'manual' | 'artist_discography';
export type CollectionPlayMode = 'in_order' | 'album_shuffle';
export type ItemType = 'album' | 'track' | 'playlist';
export type AlbumSource = 'qobuz' | 'local';
export type EnqueueMode = 'replace' | 'append' | 'play_next';

export interface MixtapeCollectionItem {
  collection_id: string;
  position: number;
  item_type: ItemType;
  source: AlbumSource;
  source_item_id: string;
  title: string;
  subtitle: string | null;
  artwork_url: string | null;
  year: number | null;
  track_count: number | null;
  added_at: number;
}

export interface MixtapeCollection {
  id: string;
  kind: CollectionKind;
  name: string;
  description: string | null;
  source_type: CollectionSourceType;
  source_ref: string | null;
  play_mode: CollectionPlayMode;
  custom_artwork_path: string | null;
  position: number;
  hidden: boolean;
  last_played_at: number | null;
  play_count: number;
  last_synced_at: number | null;
  created_at: number;
  updated_at: number;
  items: MixtapeCollectionItem[];
}

export interface NewItemInput {
  item_type: ItemType;
  source: AlbumSource;
  source_item_id: string;
  title: string;
  subtitle?: string;
  artwork_url?: string;
  year?: number;
  track_count?: number;
}

export const collectionsStore = writable<MixtapeCollection[]>([]);

// ──────────────────────────── Collection ops ────────────────────────────

export async function loadCollections(kind?: CollectionKind): Promise<void> {
  const list = await invoke<MixtapeCollection[]>('v2_list_mixtape_collections', {
    kind: kind ?? null,
  });
  collectionsStore.set(list);
}

export async function getCollection(id: string): Promise<MixtapeCollection | null> {
  const result = await invoke<MixtapeCollection | null>('v2_get_mixtape_collection', { id });
  return result ?? null;
}

export async function createCollection(
  kind: CollectionKind,
  name: string,
  description: string | null = null,
  source_type: CollectionSourceType = 'manual',
  source_ref: string | null = null,
): Promise<MixtapeCollection> {
  const created = await invoke<MixtapeCollection>('v2_create_mixtape_collection', {
    kind,
    name,
    description,
    sourceType: source_type,
    sourceRef: source_ref,
  });
  collectionsStore.update((cs) => [...cs, created]);
  return created;
}

export async function renameCollection(id: string, newName: string): Promise<void> {
  await invoke('v2_rename_mixtape_collection', { id, newName });
  collectionsStore.update((cs) =>
    cs.map((c) => (c.id === id ? { ...c, name: newName } : c)),
  );
}

export async function setDescription(id: string, description: string | null): Promise<void> {
  await invoke('v2_set_mixtape_description', { id, description });
  collectionsStore.update((cs) =>
    cs.map((c) => (c.id === id ? { ...c, description } : c)),
  );
}

export async function setPlayMode(id: string, mode: CollectionPlayMode): Promise<void> {
  await invoke('v2_set_mixtape_play_mode', { id, mode });
  collectionsStore.update((cs) =>
    cs.map((c) => (c.id === id ? { ...c, play_mode: mode } : c)),
  );
}

export async function setKind(id: string, kind: CollectionKind): Promise<void> {
  await invoke('v2_set_mixtape_kind', { id, kind });
  collectionsStore.update((cs) =>
    cs.map((c) => (c.id === id ? { ...c, kind } : c)),
  );
}

export async function setCustomArtwork(id: string, path: string | null): Promise<void> {
  await invoke('v2_set_mixtape_custom_artwork', { id, path });
  collectionsStore.update((cs) =>
    cs.map((c) => (c.id === id ? { ...c, custom_artwork_path: path } : c)),
  );
}

export async function deleteCollection(id: string): Promise<void> {
  await invoke('v2_delete_mixtape_collection', { id });
  collectionsStore.update((cs) => cs.filter((c) => c.id !== id));
}

// ──────────────────────────── Item ops ────────────────────────────

/**
 * Returns true if the item was added, false if the backend rejected it
 * as a dedup (exact source+source_item_id already in this collection).
 */
export async function addItem(collectionId: string, item: NewItemInput): Promise<boolean> {
  return await invoke<boolean>('v2_add_mixtape_item', {
    collectionId,
    itemType: item.item_type,
    source: item.source,
    sourceItemId: item.source_item_id,
    title: item.title,
    subtitle: item.subtitle ?? null,
    artworkUrl: item.artwork_url ?? null,
    year: item.year ?? null,
    trackCount: item.track_count ?? null,
  });
}

export async function removeItem(collectionId: string, position: number): Promise<void> {
  await invoke('v2_remove_mixtape_item', { collectionId, position });
}

export async function reorderItems(collectionId: string, newOrder: number[]): Promise<void> {
  await invoke('v2_reorder_mixtape_items', { collectionId, newOrder });
}

// ──────────────────────────── Playback ────────────────────────────

export async function enqueueCollection(
  collectionId: string,
  mode: EnqueueMode = 'replace',
): Promise<void> {
  await invoke('v2_enqueue_collection', { collectionId, mode });
}
