/**
 * Format a track's display title with its Qobuz subtitle/edition info
 * (the `version` field) appended in parentheses when present.
 *
 * Qobuz returns a separate `version` field for tracks belonging to
 * remix albums, reissues, deluxe editions, etc. (e.g. "Player's Ball
 * Mix", "Nine Inch Noize Version", "Remastered 2024"). The mobile app
 * displays it parenthesized after the title; qbz now mirrors that so
 * remix albums are distinguishable from originals.
 *
 * Issue: https://github.com/vicrodh/qbz/issues/360
 *
 * @example
 *   formatTrackTitle({ title: "Player's Ball" })
 *   // => "Player's Ball"
 *
 *   formatTrackTitle({ title: "Player's Ball", version: "Player's Ball Mix" })
 *   // => "Player's Ball (Player's Ball Mix)"
 */
export function formatTrackTitle(
  track: { title?: string | null; version?: string | null } | null | undefined
): string {
  if (!track) {
    return '';
  }
  const title = (track.title ?? '').trim();
  const version = (track.version ?? '').trim();
  if (!version) {
    return title;
  }
  return `${title} (${version})`;
}
