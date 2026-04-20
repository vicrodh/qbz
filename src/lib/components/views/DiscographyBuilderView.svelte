<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowLeft, Check, LibraryBig, RotateCcw } from 'lucide-svelte';
  import { t } from 'svelte-i18n';
  import {
    createCollection,
    addItem,
    type MixtapeCollection,
  } from '$lib/stores/mixtapeCollectionsStore';
  import QualityBadge from '../QualityBadge.svelte';
  import { showToast } from '$lib/stores/toastStore';
  import { getUserItem } from '$lib/utils/userStorage';
  import {
    releaseTypeOverrides,
    loadReleaseTypeOverrides,
    setReleaseTypeOverride,
    clearReleaseTypeOverride,
    overrideKey as overrideKeyFn,
    RELEASE_TYPE_CHOICES,
    type ReleaseType as StoreReleaseType,
  } from '$lib/stores/releaseTypeOverridesStore';
  import type { PageArtistResponse, PageArtistRelease } from '$lib/types';

  interface Props {
    artistId: string;
    onBack?: () => void;
    onCreated?: (collection: MixtapeCollection) => void;
    onOpenAlbum?: (source: 'qobuz' | 'local' | 'plex', sourceItemId: string) => void;
  }
  let { artistId, onBack, onCreated, onOpenAlbum }: Props = $props();

  // ── Types ──────────────────────────────────────────────────────────────────

  type ReleaseType = 'album' | 'ep' | 'single' | 'live' | 'compilation';

  /** One candidate per album from a single source. */
  interface Candidate {
    group_key: string;
    source: 'qobuz' | 'local' | 'plex';
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
    release_type: ReleaseType;
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

  interface PlexCachedAlbum {
    id: string;
    title: string;
    artist: string;
    artworkPath?: string;
    trackCount: number;
    totalDurationSecs: number;
    format: string;
    bitDepth?: number;
    sampleRate: number;
    source: string;
    year?: number;
    genre?: string;
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

  /**
   * Classify a release as Album / EP / Single / Live / Compilation.
   * Uses the Qobuz release_type + group_type when available (authoritative)
   * and falls back to a title + track-count heuristic for local / Plex
   * albums where metadata rarely carries this distinction.
   */
  function classifyRelease(
    title: string,
    trackCount: number | null,
    qobuzReleaseType: string | null,
    qobuzGroupType: string | null,
    titleIsCompilation: boolean,
  ): ReleaseType {
    const normalizedQobuz = (qobuzReleaseType || qobuzGroupType || '').toLowerCase();
    if (normalizedQobuz.includes('compilation') || titleIsCompilation) return 'compilation';
    if (normalizedQobuz === 'live' || /\blive\b|\bconcert\b|\bunplugged\b/i.test(title)) return 'live';
    if (normalizedQobuz === 'ep' || /\bep\b/i.test(title)) return 'ep';
    if (normalizedQobuz === 'single') return 'single';
    if (normalizedQobuz === 'album') return 'album';
    // Heuristic fallback for local / Plex where track_count is our only signal.
    if (trackCount != null) {
      if (trackCount <= 3) return 'single';
      if (trackCount <= 6) return 'ep';
    }
    return 'album';
  }

  // ── Release-type user overrides (shared sidecar store) ────────────────────
  // The classifier gets the vast majority right, but some releases come out
  // mislabeled. The user can click the type cell to override — stored
  // locally per-user in $releaseTypeOverrides, shared with other views so an
  // override set here shows up in Collection detail and vice-versa.

  function overrideKey(source: Candidate['source'], sourceItemId: string): string {
    return overrideKeyFn(source, sourceItemId);
  }

  function applyOverride(candidate: Candidate): ReleaseType {
    const override = $releaseTypeOverrides[overrideKey(candidate.source, candidate.source_item_id)];
    return (override as ReleaseType | undefined) ?? candidate.release_type;
  }

  function hasOverride(candidate: Candidate): boolean {
    return overrideKey(candidate.source, candidate.source_item_id) in $releaseTypeOverrides;
  }

  function applyTypeChoice(candidate: Candidate, choice: StoreReleaseType) {
    setReleaseTypeOverride(candidate.source, candidate.source_item_id, choice);
  }

  function resetTypeOverride(candidate: Candidate) {
    clearReleaseTypeOverride(candidate.source, candidate.source_item_id);
  }

  // Type-cell popover state — tracks which row's menu is open by (source|id).
  let openTypeMenuKey = $state<string | null>(null);
  const TYPE_CHOICES: ReleaseType[] = RELEASE_TYPE_CHOICES as ReleaseType[];

  function closeTypeMenu() {
    openTypeMenuKey = null;
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
        const titleIsCompilation = isCompilation(title) || group.type === 'compilation';
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
          is_compilation: titleIsCompilation,
          release_type: classifyRelease(
            title,
            rel.tracks_count ?? null,
            rel.release_type ?? null,
            group.type ?? null,
            titleIsCompilation,
          ),
          quality_score: 0,
        };
        candidate.quality_score = qualityScore(candidate);
        candidates.push(candidate);
      }
    }

    return candidates;
  }

  function isPlexLibraryEnabled(): boolean {
    return getUserItem('qbz-plex-enabled') === 'true';
  }

  async function fetchPlexAlbumsFromCache(): Promise<PlexCachedAlbum[]> {
    try {
      return await invoke<PlexCachedAlbum[]>('v2_plex_cache_get_albums');
    } catch {
      return [];
    }
  }

  function matchesArtist(albumArtist: string, allArtists: string | undefined, target: string): boolean {
    const normalizedTarget = target.toLowerCase().trim();
    if (!normalizedTarget) return false;
    if ((albumArtist ?? '').toLowerCase().trim() === normalizedTarget) return true;
    if (allArtists) {
      const parts = allArtists.split(',').map((s) => s.toLowerCase().trim());
      if (parts.includes(normalizedTarget)) return true;
    }
    return false;
  }

  function localAlbumToCandidate(album: LocalAlbum): Candidate {
    const title = String(album.title ?? '');
    const year = album.year ?? null;
    const trackCount = album.track_count ?? null;
    const sampleRateKhz = album.sample_rate ? album.sample_rate / 1000 : 44.1;
    const candidateSource: 'local' | 'plex' = album.source === 'plex' ? 'plex' : 'local';
    const titleIsCompilation = isCompilation(title);
    const candidate: Candidate = {
      group_key: `${normalizeTitle(title)}|${year ?? ''}`,
      source: candidateSource,
      source_item_id: String(album.id),
      title,
      artist: album.artist ?? artistName,
      year,
      artwork_url: null,
      track_count: trackCount,
      max_bit_depth: album.bit_depth ?? null,
      max_sample_rate: sampleRateKhz,
      format: album.format ?? 'FLAC',
      is_compilation: titleIsCompilation,
      release_type: classifyRelease(title, trackCount, null, null, titleIsCompilation),
      quality_score: 0,
    };
    candidate.quality_score = qualityScore(candidate);
    return candidate;
  }

  function plexAlbumToCandidate(album: PlexCachedAlbum): Candidate {
    const title = String(album.title ?? '');
    const year = album.year ?? null;
    const trackCount = album.trackCount ?? null;
    const sampleRateKhz = album.sampleRate ? album.sampleRate / 1000 : 44.1;
    const titleIsCompilation = isCompilation(title);
    const candidate: Candidate = {
      group_key: `${normalizeTitle(title)}|${year ?? ''}`,
      source: 'plex',
      source_item_id: String(album.id),
      title,
      artist: album.artist ?? artistName,
      year,
      artwork_url: null,
      track_count: trackCount,
      max_bit_depth: album.bitDepth ?? null,
      max_sample_rate: sampleRateKhz,
      format: album.format ?? 'FLAC',
      is_compilation: titleIsCompilation,
      release_type: classifyRelease(title, trackCount, null, null, titleIsCompilation),
      quality_score: 0,
    };
    candidate.quality_score = qualityScore(candidate);
    return candidate;
  }

  async function fetchLocalAlbums(): Promise<Candidate[]> {
    const plexEnabled = isPlexLibraryEnabled();

    async function fetchOnce(): Promise<{ local: LocalAlbum[]; plex: PlexCachedAlbum[] }> {
      const [local, plex] = await Promise.all([
        invoke<LocalAlbum[]>('v2_library_get_albums', {
          includeHidden: false,
          excludeNetworkFolders: false,
        }).catch((err) => {
          console.warn('[DiscographyBuilder] local fetch failed (non-fatal):', err);
          return [] as LocalAlbum[];
        }),
        plexEnabled ? fetchPlexAlbumsFromCache() : Promise.resolve([] as PlexCachedAlbum[]),
      ]);
      return { local, plex };
    }

    let { local, plex } = await fetchOnce();

    // Plex cache may still be warming up on first app launch. If the user
    // has Plex enabled but we got zero Plex albums, give it a moment and
    // try once more before giving up — matches LocalLibraryView's expectation
    // that Plex data eventually appears after a cold start.
    if (plexEnabled && plex.length === 0) {
      await new Promise((resolve) => setTimeout(resolve, 2000));
      ({ local, plex } = await fetchOnce());
    }

    const filteredLocal = local.filter((album) =>
      matchesArtist(album.artist ?? '', album.all_artists, artistName),
    );
    const filteredPlex = plex.filter((album) =>
      matchesArtist(album.artist ?? '', undefined, artistName),
    );

    return [
      ...filteredLocal.map(localAlbumToCandidate),
      ...filteredPlex.map(plexAlbumToCandidate),
    ];
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

  // ── Select-all helpers ─────────────────────────────────────────────────────
  // "Select all" only toggles the primary candidate of each non-compilation
  // group — mirrors the default selection applied on load. Alternates and
  // compilations stay unchecked so users don't accidentally add duplicates.

  const allPrimariesChecked = $derived(
    groups.length > 0 &&
      groups.every((grp) =>
        grp.is_compilation ? true : isChecked(grp, grp.primary),
      ),
  );

  const somePrimariesChecked = $derived(
    !allPrimariesChecked &&
      groups.some((grp) => !grp.is_compilation && isChecked(grp, grp.primary)),
  );

  function toggleAllPrimaries() {
    const target = !allPrimariesChecked;
    const next: Record<string, Set<string>> = {};
    for (const grp of groups) {
      const existing = checked[grp.key] ?? new Set<string>();
      const copy = new Set(existing);
      if (!grp.is_compilation) {
        const key = candidateKey(grp.primary);
        if (target) copy.add(key);
        else copy.delete(key);
      }
      next[grp.key] = copy;
    }
    checked = next;
  }

  // ── Load ───────────────────────────────────────────────────────────────────

  async function loadData() {
    loading = true;
    loadError = null;
    try {
      // Sequential: fetchQobuzAlbums sets `artistName`, which fetchLocalAlbums
      // needs for its artist-name filter. Running them in parallel caused the
      // local filter to run against an empty name and drop every match.
      const qobuzAlbums = await fetchQobuzAlbums();
      const localAlbums = await fetchLocalAlbums();

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
            // Collapse 'plex' → 'local' when persisting: the mixtape model
            // stores albums under the LocalLibrary umbrella; the actual
            // source resolver re-detects plex-vs-file at enqueue time from
            // source_item_id.
            const storedSource = candidate.source === 'plex' ? 'local' : candidate.source;
            await addItem(collection.id, {
              item_type: 'album',
              source: storedSource,
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

  onMount(() => {
    loadReleaseTypeOverrides();
    loadData();
  });
</script>

<div class="builder-view">
  <div class="builder-scroll">
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
      <div class="row header-row">
        <input
          type="checkbox"
          class="row-check header-check"
          checked={allPrimariesChecked}
          indeterminate={somePrimariesChecked}
          onchange={toggleAllPrimaries}
          title={allPrimariesChecked
            ? $t('discographyBuilder.uncheckAll')
            : $t('discographyBuilder.checkAll')}
          aria-label={allPrimariesChecked
            ? $t('discographyBuilder.uncheckAll')
            : $t('discographyBuilder.checkAll')}
        />
        <span class="col-label start">{$t('discographyBuilder.colYear')}</span>
        <span class="col-label start">{$t('discographyBuilder.colType')}</span>
        <span class="col-label start">{$t('discographyBuilder.colAlbum')}</span>
        <span class="col-label center">{$t('discographyBuilder.colTracks')}</span>
        <span class="col-label center">{$t('discographyBuilder.colSource')}</span>
        <span class="col-label center">{$t('discographyBuilder.colQuality')}</span>
      </div>
      {#each orderedGroups as grp (grp.key)}
        {@const primType = applyOverride(grp.primary)}
        {@const primKey = overrideKey(grp.primary.source, grp.primary.source_item_id)}
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
            <div class="type-col type-col-wrap">
              <button
                type="button"
                class="type-btn"
                class:compilation={primType === 'compilation'}
                class:live={primType === 'live'}
                class:ep={primType === 'ep'}
                class:single={primType === 'single'}
                class:is-overridden={hasOverride(grp.primary)}
                title={$t('discographyBuilder.typeOverrideHint')}
                onclick={(e) => {
                  e.stopPropagation();
                  openTypeMenuKey = openTypeMenuKey === primKey ? null : primKey;
                }}
              >
                {$t(`discographyBuilder.releaseType.${primType}`)}
              </button>
              {#if openTypeMenuKey === primKey}
                <div class="type-menu-backdrop" onclick={closeTypeMenu} role="presentation"></div>
                <div class="type-menu" role="menu">
                  {#each TYPE_CHOICES as choice}
                    <button
                      type="button"
                      class="type-menu-item"
                      class:selected={primType === choice}
                      onclick={() => {
                        applyTypeChoice(grp.primary, choice);
                        closeTypeMenu();
                      }}
                    >
                      {#if primType === choice}
                        <Check size={12} />
                      {:else}
                        <span class="check-placeholder"></span>
                      {/if}
                      <span>{$t(`discographyBuilder.releaseType.${choice}`)}</span>
                    </button>
                  {/each}
                  {#if hasOverride(grp.primary)}
                    <div class="type-menu-divider"></div>
                    <button
                      type="button"
                      class="type-menu-item reset"
                      onclick={() => {
                        resetTypeOverride(grp.primary);
                        closeTypeMenu();
                      }}
                    >
                      <RotateCcw size={12} />
                      <span>{$t('discographyBuilder.typeOverrideReset')}</span>
                    </button>
                  {/if}
                </div>
              {/if}
            </div>
            <div class="title-col">
              <button
                type="button"
                class="album-title-btn"
                onclick={() => onOpenAlbum?.(grp.primary.source, grp.primary.source_item_id)}
                title={grp.title}
              >
                <span class="album-title">{grp.title}</span>
              </button>
              {#if grp.is_compilation}
                <span class="tag">{$t('discographyBuilder.compilationLabel')}</span>
              {/if}
            </div>
            <div class="tracks-col" title={$t('common.trackCount', { values: { count: grp.primary.track_count ?? 0 } })}>
              {grp.primary.track_count ?? '—'}
            </div>
            <div
              class="source-indicator"
              class:plex-source={grp.primary.source === 'plex'}
              class:qobuz-source={grp.primary.source === 'qobuz'}
              title={grp.primary.source === 'plex'
                ? $t('library.plexTrackIndicator')
                : grp.primary.source === 'qobuz'
                  ? $t('library.qobuzTrackIndicator')
                  : $t('library.localTrackIndicator')}
            >
              {#if grp.primary.source === 'plex'}
                <span class="plex-indicator-icon" aria-hidden="true"></span>
              {:else if grp.primary.source === 'qobuz'}
                <span class="qobuz-indicator-icon" aria-hidden="true"></span>
              {:else}
                <span class="local-indicator-icon" aria-hidden="true"></span>
              {/if}
            </div>
            <div class="quality-col">
              <QualityBadge
                bitDepth={grp.primary.max_bit_depth ?? undefined}
                samplingRate={grp.primary.max_sample_rate ?? undefined}
                format={grp.primary.format ?? undefined}
              />
            </div>
          </div>

          <!-- Alternate rows -->
          {#each grp.alternates as alt (alt.source + alt.source_item_id)}
            {@const altType = applyOverride(alt)}
            {@const altKey = overrideKey(alt.source, alt.source_item_id)}
            <div class="row alternate-row">
              <div class="check-cell alt-check-cell">
                <span class="connector" aria-hidden="true">↳</span>
                <input
                  type="checkbox"
                  class="row-check"
                  checked={isChecked(grp, alt)}
                  onchange={() => toggleChecked(grp, alt)}
                />
              </div>
              <div class="year-col alt">{alt.year ?? '—'}</div>
              <div class="type-col alt type-col-wrap">
                <button
                  type="button"
                  class="type-btn alt"
                  class:compilation={altType === 'compilation'}
                  class:live={altType === 'live'}
                  class:ep={altType === 'ep'}
                  class:single={altType === 'single'}
                  class:is-overridden={hasOverride(alt)}
                  title={$t('discographyBuilder.typeOverrideHint')}
                  onclick={(e) => {
                    e.stopPropagation();
                    openTypeMenuKey = openTypeMenuKey === altKey ? null : altKey;
                  }}
                >
                  {$t(`discographyBuilder.releaseType.${altType}`)}
                </button>
                {#if openTypeMenuKey === altKey}
                  <div class="type-menu-backdrop" onclick={closeTypeMenu} role="presentation"></div>
                  <div class="type-menu" role="menu">
                    {#each TYPE_CHOICES as choice}
                      <button
                        type="button"
                        class="type-menu-item"
                        class:selected={altType === choice}
                        onclick={() => {
                          applyTypeChoice(alt, choice);
                          closeTypeMenu();
                        }}
                      >
                        {#if altType === choice}
                          <Check size={12} />
                        {:else}
                          <span class="check-placeholder"></span>
                        {/if}
                        <span>{$t(`discographyBuilder.releaseType.${choice}`)}</span>
                      </button>
                    {/each}
                    {#if hasOverride(alt)}
                      <div class="type-menu-divider"></div>
                      <button
                        type="button"
                        class="type-menu-item reset"
                        onclick={() => {
                          resetTypeOverride(alt);
                          closeTypeMenu();
                        }}
                      >
                        <RotateCcw size={12} />
                        <span>{$t('discographyBuilder.typeOverrideReset')}</span>
                      </button>
                    {/if}
                  </div>
                {/if}
              </div>
              <div class="title-col alt">
                <button
                  type="button"
                  class="album-title-btn"
                  onclick={() => onOpenAlbum?.(alt.source, alt.source_item_id)}
                  title={alt.title}
                >
                  <span class="album-title">{alt.title}</span>
                </button>
                <span class="tag">{$t('discographyBuilder.alternateLabel')}</span>
              </div>
              <div class="tracks-col alt" title={$t('common.trackCount', { values: { count: alt.track_count ?? 0 } })}>
                {alt.track_count ?? '—'}
              </div>
              <div
                class="source-indicator"
                class:plex-source={alt.source === 'plex'}
                class:qobuz-source={alt.source === 'qobuz'}
                title={alt.source === 'plex'
                  ? $t('library.plexTrackIndicator')
                  : alt.source === 'qobuz'
                    ? $t('library.qobuzTrackIndicator')
                    : $t('library.localTrackIndicator')}
              >
                {#if alt.source === 'plex'}
                  <span class="plex-indicator-icon" aria-hidden="true"></span>
                {:else if alt.source === 'qobuz'}
                  <span class="qobuz-indicator-icon" aria-hidden="true"></span>
                {:else}
                  <span class="local-indicator-icon" aria-hidden="true"></span>
                {/if}
              </div>
              <div class="quality-col">
                <QualityBadge
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
  </div>

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
  /* Two-part layout: .builder-scroll holds everything that scrolls; the
     footer is a fixed-height sibling pinned at the bottom of the view so
     its background never overlaps content. This replaces the earlier
     sticky-inside-scroll approach, which let last rows peek past the
     footer on some content lengths. */
  .builder-view {
    width: 100%;
    height: 100%;
    color: var(--text-primary);
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    position: relative;
  }

  .builder-scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 24px 32px 24px;
    box-sizing: border-box;
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
  /* Unified grid for primary and alternate rows so everything aligns
     vertically: checkbox · year · title · source · quality. Alternates
     are indented via padding-left on the row itself. */
  .row {
    display: grid;
    grid-template-columns: 28px 56px 72px minmax(0, 1fr) 72px 60px 156px;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: 6px;
  }
  .row:hover {
    background: var(--bg-hover);
  }

  /* Header row — labels the columns so the bare track-count number has
     context ("TRACKS" above it). Follows MixtapeCollectionDetailView's
     .item-list-header convention. */
  .header-row {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 1.2px;
    text-transform: uppercase;
    color: var(--text-muted);
    border-bottom: 1px solid var(--bg-tertiary);
    padding-top: 10px;
    padding-bottom: 10px;
  }
  .header-row:hover {
    background: transparent;
  }
  .col-label {
    white-space: nowrap;
  }
  .col-label.start {
    text-align: left;
  }
  .col-label.center {
    text-align: center;
  }

  .alternate-row {
    opacity: 0.72;
  }
  .alternate-row:hover {
    opacity: 1;
    background: var(--bg-hover);
  }

  .row-check {
    width: 15px;
    height: 15px;
    cursor: pointer;
    flex-shrink: 0;
    accent-color: var(--accent-primary);
  }

  /* Alternate rows: tiny connector glyph packed next to the checkbox inside
     the first column (no indent, same column as primary rows). Signals
     parent/child without shifting the alternate's horizontal alignment. */
  .check-cell.alt-check-cell {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    min-width: 0;
  }
  .check-cell.alt-check-cell .connector {
    color: var(--text-muted);
    font-size: 12px;
    line-height: 1;
    user-select: none;
    flex-shrink: 0;
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

  /* Release type label: Album / EP / Single / Live / Compilation. Non-default
     types (everything except plain Album) get a subtle color tint so the
     user can spot EPs / Live records at a glance without reading every row. */
  .type-col {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.4px;
    text-transform: uppercase;
    color: var(--text-muted);
    white-space: nowrap;
  }
  .type-col-wrap {
    position: relative;
  }

  /* Clickable type label — sidecar override editor. No border by default,
     subtle hover, dashed underline when a user override is active. */
  .type-btn {
    background: none;
    border: none;
    padding: 2px 4px;
    margin: 0 -4px;
    font-family: inherit;
    font-size: inherit;
    font-weight: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    color: var(--text-muted);
    cursor: pointer;
    border-radius: 4px;
    transition: background 120ms ease;
  }
  .type-btn:hover {
    background: var(--alpha-6);
    color: var(--text-primary);
  }
  .type-btn.alt {
    font-size: 10px;
  }
  .type-btn.compilation {
    color: #f59e0b;
  }
  .type-btn.live {
    color: #e91e63;
  }
  .type-btn.ep {
    color: #60a5fa;
  }
  .type-btn.single {
    color: #a78bfa;
  }
  .type-btn.is-overridden {
    text-decoration: underline dotted;
    text-underline-offset: 3px;
  }

  .type-menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
  }
  .type-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 101;
    min-width: 160px;
    background: var(--bg-secondary);
    border: 1px solid var(--bg-tertiary);
    border-radius: 6px;
    padding: 4px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.35);
    display: flex;
    flex-direction: column;
  }
  .type-menu-item {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    background: none;
    border: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    border-radius: 4px;
  }
  .type-menu-item:hover {
    background: var(--bg-hover);
  }
  .type-menu-item.selected {
    color: var(--accent-primary);
  }
  .type-menu-item.reset {
    color: var(--text-muted);
  }
  .type-menu-item .check-placeholder {
    display: inline-block;
    width: 12px;
    height: 12px;
    flex-shrink: 0;
  }
  .type-menu-divider {
    height: 1px;
    background: var(--bg-tertiary);
    margin: 4px 0;
  }

  .tracks-col {
    font-size: 13px;
    color: var(--text-secondary);
    white-space: nowrap;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .tracks-col.alt {
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
  .album-title-btn {
    background: none;
    border: none;
    padding: 0;
    margin: 0;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
    min-width: 0;
    max-width: 100%;
    display: inline-flex;
    align-items: center;
    color: inherit;
  }
  .album-title-btn:hover .album-title {
    color: var(--accent-primary);
    text-decoration: underline;
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

  .quality-col {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 0;
  }

  /* Mirror of TrackRow's .local-indicator / .plex-indicator-icon pattern,
     so the album-row source indicator matches the track-row pattern used
     inside Local Library's album detail view. */
  .source-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    opacity: 0.9;
    justify-self: center;
  }
  .plex-indicator-icon {
    width: 14px;
    height: 14px;
    background-color: var(--accent-primary);
    -webkit-mask: url('/plex-mono.svg') center / contain no-repeat;
    mask: url('/plex-mono.svg') center / contain no-repeat;
  }
  /* Use /qobuz-logo.svg (monochrome wordmark-shape) rather than the filled
     coloured logo — the filled version has solid coloured fill boxes that
     collapse into a single blob when tinted through a CSS mask. */
  .qobuz-indicator-icon {
    width: 16px;
    height: 16px;
    background-color: var(--accent-primary);
    -webkit-mask: url('/qobuz-logo.svg') center / contain no-repeat;
    mask: url('/qobuz-logo.svg') center / contain no-repeat;
  }
  /* /hdd.svg is a filled hard-drive glyph — reads at the same visual weight
     as the Plex / Qobuz marks instead of the thin line-art HardDrive icon. */
  .local-indicator-icon {
    width: 14px;
    height: 14px;
    background-color: var(--accent-primary);
    -webkit-mask: url('/hdd.svg') center / contain no-repeat;
    mask: url('/hdd.svg') center / contain no-repeat;
  }

  /* ── Fixed footer ── */
  /* Flex child of .builder-view (not sticky). Sits flush against
     NowPlayingBar. Extends the full width of the view — the global
     back-to-top button (z-index 200) floats on top of the footer's right
     edge. No right margin avoids a visual gap in the footer border. */
  .builder-footer {
    flex-shrink: 0;
    background: var(--bg-primary);
    border-top: 1px solid var(--bg-tertiary);
    padding: 12px 88px 12px 32px;
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
