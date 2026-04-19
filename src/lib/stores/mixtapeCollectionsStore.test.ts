import { describe, it, expect, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';

// Mock @tauri-apps/api/core before importing the store under test
// vi.hoisted() is required in vitest 4.x so the variable is available
// when vi.mock() factory runs (vi.mock is hoisted to top of file).
const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}));

import {
  collectionsStore,
  loadCollections,
  getCollection,
  createCollection,
  renameCollection,
  setDescription,
  setPlayMode,
  setKind,
  setCustomArtwork,
  deleteCollection,
  addItem,
  removeItem,
  reorderItems,
  enqueueCollection,
  type MixtapeCollection,
} from './mixtapeCollectionsStore';

function makeCollection(overrides: Partial<MixtapeCollection> = {}): MixtapeCollection {
  return {
    id: overrides.id ?? 'col-1',
    kind: overrides.kind ?? 'mixtape',
    name: overrides.name ?? '90s Cassettes',
    description: overrides.description ?? null,
    source_type: overrides.source_type ?? 'manual',
    source_ref: overrides.source_ref ?? null,
    play_mode: overrides.play_mode ?? 'in_order',
    custom_artwork_path: overrides.custom_artwork_path ?? null,
    position: overrides.position ?? 0,
    hidden: overrides.hidden ?? false,
    last_played_at: overrides.last_played_at ?? null,
    play_count: overrides.play_count ?? 0,
    last_synced_at: overrides.last_synced_at ?? null,
    created_at: overrides.created_at ?? 1700000000000,
    updated_at: overrides.updated_at ?? 1700000000000,
    items: overrides.items ?? [],
  };
}

beforeEach(() => {
  mockInvoke.mockReset();
  collectionsStore.set([]);
});

describe('mixtapeCollectionsStore', () => {
  it('loadCollections(undefined) invokes v2_list_mixtape_collections with kind=null and populates the store', async () => {
    const data = [makeCollection({ id: 'a' }), makeCollection({ id: 'b', kind: 'collection' })];
    mockInvoke.mockResolvedValueOnce(data);

    await loadCollections();

    expect(mockInvoke).toHaveBeenCalledWith('v2_list_mixtape_collections', { kind: null });
    expect(get(collectionsStore)).toEqual(data);
  });

  it('loadCollections("mixtape") filters by kind', async () => {
    const data = [makeCollection({ id: 'a' })];
    mockInvoke.mockResolvedValueOnce(data);

    await loadCollections('mixtape');

    expect(mockInvoke).toHaveBeenCalledWith('v2_list_mixtape_collections', { kind: 'mixtape' });
    expect(get(collectionsStore)).toEqual(data);
  });

  it('getCollection returns the collection without touching the store', async () => {
    const c = makeCollection({ id: 'x' });
    mockInvoke.mockResolvedValueOnce(c);

    const result = await getCollection('x');

    expect(mockInvoke).toHaveBeenCalledWith('v2_get_mixtape_collection', { id: 'x' });
    expect(result).toEqual(c);
    expect(get(collectionsStore)).toEqual([]); // store untouched
  });

  it('createCollection appends to the store on success', async () => {
    const created = makeCollection({ id: 'new', name: 'Sunday Drive' });
    mockInvoke.mockResolvedValueOnce(created);

    const result = await createCollection('mixtape', 'Sunday Drive');

    expect(mockInvoke).toHaveBeenCalledWith('v2_create_mixtape_collection', {
      kind: 'mixtape',
      name: 'Sunday Drive',
      description: null,
      sourceType: 'manual',
      sourceRef: null,
    });
    expect(result).toEqual(created);
    expect(get(collectionsStore)).toEqual([created]);
  });

  it('createCollection passes through description and artist_discography source_ref', async () => {
    const created = makeCollection({
      id: 'art-1',
      kind: 'artist_collection',
      source_type: 'artist_discography',
      source_ref: 'artist-42',
    });
    mockInvoke.mockResolvedValueOnce(created);

    await createCollection(
      'artist_collection',
      'George Harrison',
      'Complete discography',
      'artist_discography',
      'artist-42',
    );

    expect(mockInvoke).toHaveBeenCalledWith('v2_create_mixtape_collection', {
      kind: 'artist_collection',
      name: 'George Harrison',
      description: 'Complete discography',
      sourceType: 'artist_discography',
      sourceRef: 'artist-42',
    });
  });

  it('renameCollection updates the store entry', async () => {
    const c = makeCollection({ id: 'a', name: 'Old Name' });
    collectionsStore.set([c]);
    mockInvoke.mockResolvedValueOnce(undefined);

    await renameCollection('a', 'New Name');

    expect(mockInvoke).toHaveBeenCalledWith('v2_rename_mixtape_collection', {
      id: 'a', newName: 'New Name',
    });
    expect(get(collectionsStore)[0].name).toBe('New Name');
  });

  it('setPlayMode updates the store entry', async () => {
    const c = makeCollection({ id: 'a', play_mode: 'in_order' });
    collectionsStore.set([c]);
    mockInvoke.mockResolvedValueOnce(undefined);

    await setPlayMode('a', 'album_shuffle');

    expect(mockInvoke).toHaveBeenCalledWith('v2_set_mixtape_play_mode', {
      id: 'a', mode: 'album_shuffle',
    });
    expect(get(collectionsStore)[0].play_mode).toBe('album_shuffle');
  });

  it('setKind updates the store entry', async () => {
    const c = makeCollection({ id: 'a', kind: 'mixtape' });
    collectionsStore.set([c]);
    mockInvoke.mockResolvedValueOnce(undefined);

    await setKind('a', 'collection');

    expect(mockInvoke).toHaveBeenCalledWith('v2_set_mixtape_kind', {
      id: 'a', kind: 'collection',
    });
    expect(get(collectionsStore)[0].kind).toBe('collection');
  });

  it('setDescription and setCustomArtwork pass null through', async () => {
    const c = makeCollection({ id: 'a', description: 'old', custom_artwork_path: '/x.png' });
    collectionsStore.set([c]);

    mockInvoke.mockResolvedValueOnce(undefined);
    await setDescription('a', null);
    expect(mockInvoke).toHaveBeenLastCalledWith('v2_set_mixtape_description', {
      id: 'a', description: null,
    });
    expect(get(collectionsStore)[0].description).toBeNull();

    mockInvoke.mockResolvedValueOnce(undefined);
    await setCustomArtwork('a', null);
    expect(mockInvoke).toHaveBeenLastCalledWith('v2_set_mixtape_custom_artwork', {
      id: 'a', path: null,
    });
    expect(get(collectionsStore)[0].custom_artwork_path).toBeNull();
  });

  it('deleteCollection removes from the store', async () => {
    const a = makeCollection({ id: 'a' });
    const b = makeCollection({ id: 'b' });
    collectionsStore.set([a, b]);
    mockInvoke.mockResolvedValueOnce(undefined);

    await deleteCollection('a');

    expect(mockInvoke).toHaveBeenCalledWith('v2_delete_mixtape_collection', { id: 'a' });
    expect(get(collectionsStore)).toEqual([b]);
  });

  it('addItem returns the dedup boolean from the backend', async () => {
    mockInvoke.mockResolvedValueOnce(true);
    const ok1 = await addItem('col-1', {
      item_type: 'album', source: 'qobuz', source_item_id: 'a-1',
      title: 'Dookie', subtitle: 'Green Day',
    });
    expect(ok1).toBe(true);
    expect(mockInvoke).toHaveBeenCalledWith('v2_add_mixtape_item', {
      collectionId: 'col-1',
      itemType: 'album',
      source: 'qobuz',
      sourceItemId: 'a-1',
      title: 'Dookie',
      subtitle: 'Green Day',
      artworkUrl: null,
      year: null,
      trackCount: null,
    });

    mockInvoke.mockResolvedValueOnce(false);
    const ok2 = await addItem('col-1', {
      item_type: 'album', source: 'qobuz', source_item_id: 'a-1',
      title: 'Dookie',
    });
    expect(ok2).toBe(false);
  });

  it('removeItem and reorderItems pass through without store manipulation', async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await removeItem('col-1', 2);
    expect(mockInvoke).toHaveBeenLastCalledWith('v2_remove_mixtape_item', {
      collectionId: 'col-1', position: 2,
    });

    mockInvoke.mockResolvedValueOnce(undefined);
    await reorderItems('col-1', [2, 0, 1]);
    expect(mockInvoke).toHaveBeenLastCalledWith('v2_reorder_mixtape_items', {
      collectionId: 'col-1', newOrder: [2, 0, 1],
    });
  });

  it('enqueueCollection defaults mode to "replace"', async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await enqueueCollection('col-1');
    expect(mockInvoke).toHaveBeenLastCalledWith('v2_enqueue_collection', {
      collectionId: 'col-1', mode: 'replace',
    });

    mockInvoke.mockResolvedValueOnce(undefined);
    await enqueueCollection('col-1', 'append');
    expect(mockInvoke).toHaveBeenLastCalledWith('v2_enqueue_collection', {
      collectionId: 'col-1', mode: 'append',
    });
  });
});
