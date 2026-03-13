export type PlaybackSource = 'qobuz' | 'local' | 'plex';

type SourceCandidate = {
  source?: string | null;
  is_local?: boolean | null;
  isLocal?: boolean | null;
};

export function resolvePlaybackSource(candidate: SourceCandidate): PlaybackSource {
  const source = (candidate.source ?? '').toLowerCase();

  if (source === 'plex') return 'plex';
  if (source === 'local' || source === 'qobuz_download') return 'local';
  if (source === 'qobuz' || source === 'qobuz_connect_remote') return 'qobuz';

  if (candidate.is_local === true || candidate.isLocal === true) {
    return 'local';
  }

  return 'qobuz';
}

export function isPlaybackSourceLocal(source: PlaybackSource, knownLocalTrack = false): boolean {
  if (source === 'local' || source === 'plex') {
    return true;
  }

  return knownLocalTrack;
}
