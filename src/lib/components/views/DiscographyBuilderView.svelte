<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, LibraryBig } from 'lucide-svelte';
  import { t } from 'svelte-i18n';
  import {
    createCollection,
    addItem,
    type MixtapeCollection,
  } from '$lib/stores/mixtapeCollectionsStore';
  import SourceBadge from '../SourceBadge.svelte';
  import QualityBadge from '../QualityBadge.svelte';
  import { showToast } from '$lib/stores/toastStore';
  import type { PageArtistResponse, PageArtistRelease } from '$lib/types';

  interface Props {
    artistId: string;
    onBack?: () => void;
    onCreated?: (collection: MixtapeCollection) => void;
  }
  let { artistId, onBack, onCreated }: Props = $props();

  // ── Types ──────────────────────────────────────────────────────────────────

  /** One candidate per album from a single source. */
  interface Candidate {
    group_key: string;
    source: 'qobuz' | 'local';
    source_item_id: string;
    title: string;
    artist: string;
    year: number | null;
    artwork_url: string | null;
    track_count: number | null;
    max_bit_depth: number | null;
    max_sample_rate: number | null; // kHz
    format: string | null;
    is_compilation: boolean;
    quality_score: number;
  }

  interface Group {
    key: string;
    title: string;
    year: number | null;
    is_compilation: boolean;
    candidates: Candidate[];
    primary: Candidate;
    alternates: Candidate[];
  }

  interface LocalAlbum {
    id: string;
    title: string;
    artist: string;
    all_artists?: string;
    year?: number;
    artwork_path?: string;
    track_count: number;
    format: string;
    bit_depth?: number;
    sample_rate: number;
    source?: string;
  }

  // ── State ──────────────────────────────────────────────────────────────────

  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let artistName = $state('');
  let artistAvatarUrl = $state<string | null>(null);
  let groups = $state<Group[]>([]);
  let checked = $state<Record<string, Set<string>>>({});
  let orderBy = $state<'release_date' | 'title' | 'manual'>('release_date');
  let collectionName = $state('');
  let creating = $state(false);

  // ── Quality helpers ────────────────────────────────────────────────────────

  function qualityScore(c: Pick<Candidate, 'max_bit_depth' | 'max_sample_rate' | 'format'>): number {
    const bit = c.max_bit_depth ?? 16;
    const rateHz = Math.round((c.max_sample_rate ?? 44.1) * 1000);
    const fmtBonus =
      c.format === 'FLAC' || c.format === 'ALAC' ? 1000 :
      c.format === 'MP3' || c.format === 'AAC' ? 0 : 500;
    return bit * 10_000_000 + rateHz + fmtBonus;
  }

  function normalizeTitle(title: string): string {
    return title
      .toLowerCase()
      .replace(/\s*[\(\[]\s*(deluxe|remastered?|expanded|anniversary|collector'?s?|special|bonus|extended|definitive|20th|25th|30th|40th|50th|\d+th).*?[\)\]]\s*/gi, ' ')
      .replace(/\s+/g, ' ')
      .trim();
  }

  function isCompilation(title: string): boolean {
    return /\b(best of|greatest hits|anthology|the very best|essential|collection)\b/i.test(title);
  }

  // ── Data fetching ──────────────────────────────────────────────────────────

  async function fetchQobuzAlbums(): Promise<Candidate[]> {
    const response = await invoke<PageArtistResponse>('v2_get_artist_page', {
      artistId: Number(artistId),
    });

    artistName = response.name?.display ?? artistName;

    // Build artist avatar URL from portrait hash + format
    if (response.images?.portrait) {
      const { hash, format } = response.images.portrait;
      artistAvatarUrl = `https://static.qobuz.com/images/artists/covers/medium/${hash}.${format}`;
    }

    const candidates: Candidate[] = [];
    const releaseGroups = response.releases ?? [];

    for (const group of releaseGroups) {
      for (const rel of group.items) {
        const title = String(rel.title ?? '');
        const year = rel.dates?.original
          ? new Date(rel.dates.original).getFullYear()
          : null;
        const candidate: Candidate = {
          group_key: `${normalizeTitle(title)}|${year ?? ''}`,
          source: 'qobuz',
          source_item_id: String(rel.id),
          title,
          artist: rel.artist?.name?.display ?? artistName,
          year,
          artwork_url: rel.image?.large ?? rel.image?.small ?? rel.image?.thumbnail ?? null,
          track_count: rel.tracks_count ?? null,
          max_bit_depth: rel.audio_info?.maximum_bit_depth ?? null,
          max_sample_rate: rel.audio_info?.maximum_sampling_rate ?? null,
          format: 'FLAC',
          is_compilation: isCompilation(title) || group.type === 'compilation',
          quality_score: 0,
        };
        candidate.quality_score = qualityScore(candidate);
        candidates.push(candidate);
      }
    }

    return candidates;
  }

  async function fetchLocalAlbums(): Promise<Candidate[]> {
    try {
      const albums = await invoke<LocalAlbum[]>('v2_library_get_albums', {
        include_hidden: false,
        exclude_network_folders: false,
      });

      if (!albums || albums.length === 0) return [];

      const normalizedArtistName = artistName.toLowerCase().trim();

      // Filter albums that belong to this artist by name match
      const filtered = albums.filter((album) => {
        const normalizedAlbumArtist = (album.artist ?? '').toLowerCase().trim();
        if (normalizedAlbumArtist === normalizedArtistName) return true;
        // Check all_artists comma-separated list
        if (album.all_artists) {
          const parts = album.all_artists.split(',').map((s) => s.toLowerCase().trim());
          if (parts.includes(normalizedArtistName)) return true;
        }
        return false;
      });

      return filtered.map((album): Candidate => {
        const title = String(album.title ?? '');
        const year = album.year ?? null;
        const sampleRateKhz = album.sample_rate ? album.sample_rate / 1000 : 44.1;
        const candidate: Candidate = {
          group_key: `${normalizeTitle(title)}|${year ?? ''}`,
          source: 'local',
          source_item_id: String(album.id),
          title,
          artist: album.artist ?? artistName,
          year,
          artwork_url: null, // local album paths not usable as URLs directly
          track_count: album.track_count ?? null,
          max_bit_depth: album.bit_depth ?? null,
          max_sample_rate: sampleRateKhz,
          format: album.format ?? 'FLAC',
          is_compilation: isCompilation(title),
          quality_score: 0,
        };
        candidate.quality_score = qualityScore(candidate);
        return candidate;
      });
    } catch (err) {
      console.warn('[DiscographyBuilder] local fetch failed (non-fatal):', err);
      return [];
    }
  }

  function buildGroups(candidates: Candidate[]): Group[] {
    // Dedupe incoming candidates by (source, source_item_id) — the Qobuz
    // artist-page endpoint can return the same album under multiple release
    // groups ("albums" + "compilations"), and duplicates cause
    // each_key_duplicate when those entries land in the same group's
    // alternates list.
    const seen = new Set<string>();
    const unique = candidates.filter((c) => {
      const key = `${c.source}|${c.source_item_id}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });

    const byKey = new Map<string, Candidate[]>();
    for (const candidate of unique) {
      const existing = byKey.get(candidate.group_key) ?? [];
      existing.push(candidate);
      byKey.set(candidate.group_key, existing);
    }

    const result: Group[] = [];
    for (const [key, items] of byKey) {
      // Sort by quality descending; ties broken by source (local first = lower precedence, qobuz second)
      const sorted = [...items].sort((a, b) => {
        if (b.quality_score !== a.quality_score) return b.quality_score - a.quality_score;
        // qobuz preferred over local when equal quality
        if (a.source !== b.source) return a.source === 'qobuz' ? -1 : 1;
        return 0;
      });
      result.push({
        key,
        title: sorted[0].title,
        year: sorted[0].year,
        is_compilation: sorted.every((c) => c.is_compilation),
        candidates: sorted,
        primary: sorted[0],
        alternates: sorted.slice(1),
      });
    }
    return result;
  }

  // ── Ordering ───────────────────────────────────────────────────────────────

  const orderedGroups = $derived(
    orderBy === 'title'
      ? [...groups].sort((a, b) => a.title.localeCompare(b.title))
      : orderBy === 'manual'
        ? groups
        : [...groups].sort((a, b) => {
            if (a.year == null && b.year == null) return a.title.localeCompare(b.title);
            if (a.year == null) return 1;
            if (b.year == null) return -1;
            return a.year - b.year;
          })
  );

  const selectedCount = $derived(
    groups.reduce((n, grp) => n + (checked[grp.key]?.size ?? 0), 0),
  );

  // ── Checkbox helpers ───────────────────────────────────────────────────────

  function candidateKey(candidate: Candidate): string {
    return `${candidate.source}|${candidate.source_item_id}`;
  }

  function toggleChecked(grp: Group, candidate: Candidate) {
    const key = candidateKey(candidate);
    const existing = checked[grp.key] ?? new Set<string>();
    const copy = new Set(existing);
    if (copy.has(key)) copy.delete(key);
    else copy.add(key);
    checked = { ...checked, [grp.key]: copy };
  }

  function isChecked(grp: Group, candidate: Candidate): boolean {
    return !!checked[grp.key]?.has(candidateKey(candidate));
  }

  // ── Load ───────────────────────────────────────────────────────────────────

  async function loadData() {
    loading = true;
    loadError = null;
    try {
      const [qobuzAlbums, localAlbums] = await Promise.all([
        fetchQobuzAlbums(),
        fetchLocalAlbums(),
      ]);

      const builtGroups = buildGroups([...qobuzAlbums, ...localAlbums]);
      groups = builtGroups;

      // Default selection: primary of each non-compilation group
      const initialChecked: Record<string, Set<string>> = {};
      for (const grp of builtGroups) {
        initialChecked[grp.key] = new Set();
        if (!grp.is_compilation) {
          initialChecked[grp.key].add(candidateKey(grp.primary));
        }
      }
      checked = initialChecked;

      if (!collectionName) {
        collectionName = `${artistName || 'Artist'} — Complete Discography`;
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error('[DiscographyBuilder] load failed:', err);
      loadError = msg;
    } finally {
      loading = false;
    }
  }

  // ── Create ─────────────────────────────────────────────────────────────────

  async function handleCreate() {
    if (selectedCount === 0 || creating) return;
    creating = true;
    try {
      const collection = await createCollection(
        'artist_collection',
        collectionName.trim() || 'Artist Collection',
        null,
        'artist_discography',
        artistId,
      );

      for (const grp of orderedGroups) {
        const checkedSet = checked[grp.key];
        if (!checkedSet || checkedSet.size === 0) continue;
        const orderedCandidates = [grp.primary, ...grp.alternates];
        for (const candidate of orderedCandidates) {
          if (checkedSet.has(candidateKey(candidate))) {
            await addItem(collection.id, {
              item_type: 'album',
              source: candidate.source,
              source_item_id: candidate.source_item_id,
              title: candidate.title,
              subtitle: candidate.artist,
              artwork_url: candidate.artwork_url ?? undefined,
              year: candidate.year ?? undefined,
              track_count: candidate.track_count ?? undefined,
            });
          }
        }
      }

      showToast($t('toast.collectionCreated'), 'success');
      onCreated?.(collection);
    } catch (err) {
      console.error('[DiscographyBuilder] create failed:', err);
      showToast($t('toast.collectionCreateFailed'), 'error');
    } finally {
      creating = false;
    }
  }

  onMount(loadData);
</script>

<div class="builder-view">
  {#if onBack}
    <button class="back-btn" onclick={() => onBack?.()}>
      <ArrowLeft size={16} />
      <span>{$t('actions.back')}</span>
    </button>
  {/if}

  <header class="builder-header">
    {#if artistAvatarUrl}
      <img class="avatar" src={artistAvatarUrl} alt="" />
    {:else}
      <div class="avatar placeholder"></div>
    {/if}
    <div class="header-text">
      <span class="eyebrow">{$t('collections.buildFromArtist')}</span>
      <h1 class="page-title">{artistName || '—'}</h1>
    </div>
  </header>

  <label class="field">
    <span class="field-label">{$t('common.name')}</span>
    <input
      type="text"
      bind:value={collectionName}
      class="field-input"
      maxlength="120"
    />
  </label>

  <div class="order-row">
    <span class="field-label">{$t('discographyBuilder.orderBy')}</span>
    <div class="segmented" role="group">
      <button
        class="segment"
        class:active={orderBy === 'release_date'}
        onclick={() => (orderBy = 'release_date')}
      >
        {$t('discographyBuilder.orderByReleaseDate')}
      </button>
      <button
        class="segment"
        class:active={orderBy === 'title'}
        onclick={() => (orderBy = 'title')}
      >
        {$t('discographyBuilder.orderByTitle')}
      </button>
      <button
        class="segment"
        class:active={orderBy === 'manual'}
        onclick={() => (orderBy = 'manual')}
      >
        {$t('discographyBuilder.orderByManual')}
      </button>
    </div>
  </div>

  {#if loading}
    <div class="state-msg">{$t('actions.loading')}</div>
  {:else if loadError}
    <div class="state-msg error">{loadError}</div>
  {:else if groups.length === 0}
    <div class="state-msg">{$t('search.noResults')}</div>
  {:else}
    <div class="groups">
      {#each orderedGroups as grp (grp.key)}
        <div class="group" class:is-compilation={grp.is_compilation}>
          <!-- Primary row -->
          <div class="row primary-row">
            <input
              type="checkbox"
              class="row-check"
              checked={isChecked(grp, grp.primary)}
              onchange={() => toggleChecked(grp, grp.primary)}
            />
            <div class="year-col">{grp.year ?? '—'}</div>
            <div class="title-col">
              <span class="album-title">{grp.title}</span>
              {#if grp.is_compilation}
                <span class="tag">{$t('discographyBuilder.compilationLabel')}</span>
              {/if}
            </div>
            <div class="badge-col">
              <SourceBadge value={grp.primary.source === 'qobuz' ? 'qobuz_streaming' : 'user'} />
            </div>
            <div class="quality-col">
              <QualityBadge
                compact
                bitDepth={grp.primary.max_bit_depth ?? undefined}
                samplingRate={grp.primary.max_sample_rate ?? undefined}
                format={grp.primary.format ?? undefined}
              />
            </div>
          </div>

          <!-- Alternate rows -->
          {#each grp.alternates as alt (alt.source + alt.source_item_id)}
            <div class="row alternate-row">
              <div class="connector" aria-hidden="true">└</div>
              <input
                type="checkbox"
                class="row-check"
                checked={isChecked(grp, alt)}
                onchange={() => toggleChecked(grp, alt)}
              />
              <div class="year-col alt">{alt.year ?? '—'}</div>
              <div class="title-col alt">
                <span class="album-title">{alt.title}</span>
                <span class="tag">{$t('discographyBuilder.alternateLabel')}</span>
              </div>
              <div class="badge-col">
                <SourceBadge value={alt.source === 'qobuz' ? 'qobuz_streaming' : 'user'} />
              </div>
              <div class="quality-col">
                <QualityBadge
                  compact
                  bitDepth={alt.max_bit_depth ?? undefined}
                  samplingRate={alt.max_sample_rate ?? undefined}
                  format={alt.format ?? undefined}
                />
              </div>
            </div>
          {/each}
        </div>
      {/each}
    </div>
  {/if}

  <footer class="builder-footer">
    <div class="footer-count">
      {$t('discographyBuilder.selectedCount', {
        values: { selected: selectedCount, total: groups.length },
      })}
    </div>
    <div class="footer-actions">
      <button class="secondary-btn" onclick={() => onBack?.()}>
        {$t('actions.cancel')}
      </button>
      <button
        class="primary-btn"
        onclick={handleCreate}
        disabled={creating || selectedCount === 0 || !collectionName.trim()}
      >
        {#if creating}
          {$t('actions.loading')}
        {:else}
          {$t('discographyBuilder.createBtn')}
        {/if}
      </button>
    </div>
  </footer>
</div>

<style>
  .builder-view {
    padding: 24px 32px 120px;
    color: var(--text-primary);
  }

  /* Mirror of ArtistDetailView's .back-btn — borderless, icon + text, muted. */
  .back-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-muted);
    background: none;
    border: none;
    cursor: pointer;
    font-family: inherit;
    padding: 0;
    margin-top: 24px;
    margin-bottom: 24px;
    transition: color 150ms ease;
  }
  .back-btn:hover {
    color: var(--text-secondary);
  }

  /* ── Header ── */
  .builder-header {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 24px;
  }
  .avatar {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    object-fit: cover;
    background: var(--bg-tertiary);
    flex-shrink: 0;
  }
  .avatar.placeholder {
    background: var(--bg-tertiary);
  }
  .header-text {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .eyebrow {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.5px;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .page-title {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ── Name field ── */
  .field {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 16px;
    max-width: 540px;
  }
  .field-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.5px;
    text-transform: uppercase;
    color: var(--text-muted);
  }
  .field-input {
    padding: 10px 12px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 14px;
    font-family: inherit;
  }
  .field-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  /* ── Order row ── */
  .order-row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 20px;
  }

  /* ── Segmented control — NO pills (8px corners, not 999px) ── */
  .segmented {
    display: inline-flex;
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    overflow: hidden;
  }
  .segment {
    padding: 7px 14px;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    border: none;
    font-family: inherit;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
  }
  .segment + .segment {
    border-left: 1px solid var(--bg-tertiary);
  }
  .segment:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
  .segment.active {
    background: var(--accent-primary);
    color: #fff;
  }

  /* ── State messages ── */
  .state-msg {
    padding: 48px;
    text-align: center;
    color: var(--text-muted);
    font-size: 14px;
  }
  .state-msg.error {
    color: var(--error, #e57373);
  }

  /* ── Album groups ── */
  .groups {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--bg-tertiary);
    padding-top: 4px;
  }
  .group {
    display: flex;
    flex-direction: column;
  }
  .group.is-compilation .primary-row {
    opacity: 0.65;
  }
  .group.is-compilation .primary-row:hover {
    opacity: 1;
  }

  /* ── Rows ── */
  .row {
    display: grid;
    grid-template-columns: 28px 56px 1fr 76px 96px;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: 6px;
  }
  .row:hover {
    background: var(--bg-hover);
  }

  .alternate-row {
    grid-template-columns: 22px 28px 56px 1fr 76px 96px;
    opacity: 0.72;
  }
  .alternate-row:hover {
    opacity: 1;
    background: var(--bg-hover);
  }

  .connector {
    color: var(--text-muted);
    font-size: 13px;
    text-align: center;
    user-select: none;
  }

  .row-check {
    width: 15px;
    height: 15px;
    cursor: pointer;
    flex-shrink: 0;
    accent-color: var(--accent-primary);
  }

  .year-col {
    font-size: 13px;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .year-col.alt {
    font-size: 12px;
    color: var(--text-muted);
  }

  .title-col {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .title-col.alt {
    color: var(--text-muted);
  }
  .album-title {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 14px;
    color: var(--text-primary);
  }
  .title-col.alt .album-title {
    font-size: 13px;
    color: var(--text-muted);
  }
  .tag {
    font-size: 10px;
    font-weight: 500;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.8px;
    font-style: italic;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .badge-col {
    display: flex;
    justify-content: flex-end;
  }
  .quality-col {
    display: flex;
    justify-content: flex-end;
  }

  /* ── Sticky footer ── */
  .builder-footer {
    position: sticky;
    bottom: 0;
    background: var(--bg-primary);
    border-top: 1px solid var(--bg-tertiary);
    padding: 12px 32px;
    margin: 0 -32px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    z-index: 10;
  }
  .footer-count {
    font-size: 13px;
    color: var(--text-secondary);
  }
  .footer-actions {
    display: flex;
    gap: 8px;
  }

  .primary-btn {
    padding: 10px 20px;
    background: var(--accent-primary);
    color: #fff;
    border: none;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }
  .primary-btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .secondary-btn {
    padding: 10px 16px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
  }
  .secondary-btn:hover {
    background: var(--bg-hover);
  }
</style>
