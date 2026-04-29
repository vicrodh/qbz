/**
 * Helpers for TrackMixModal. Pure functions, easy to unit-test.
 *
 * See spec: qbz-nix-docs/superpowers/specs/2026-04-25-track-shuffle-mix-design.md
 */

export type SizeOption = {
  /** How many tracks the user is asking for. */
  size: number;
  /** True for the "All (N)" entry shown last; false for fixed-step entries. */
  isAll: boolean;
};

/**
 * Builds the list of selectable queue sizes for the DJ-mix modal, given the
 * count of unique tracks available in the collection (after dedup).
 *
 * Rules:
 * - If `uniqueCount` is `null` or `<= 0`, returns an empty array (the modal
 *   shows a loading or empty state instead of options).
 * - If `uniqueCount < 50`, returns a single `{ size: uniqueCount, isAll: true }`
 *   so the user can mix everything they have.
 * - Otherwise, returns `[50, 100, 150, …]` up to (but excluding) `uniqueCount`,
 *   followed by a final `{ size: uniqueCount, isAll: true }` entry. If
 *   `uniqueCount` is itself a multiple of 50, the duplicate intermediate entry
 *   is omitted (no `100, 100` repeated).
 */
export function buildSizeOptions(uniqueCount: number | null): SizeOption[] {
  if (uniqueCount === null || uniqueCount <= 0) return [];
  if (uniqueCount < 50) return [{ size: uniqueCount, isAll: true }];

  const opts: SizeOption[] = [];
  for (let s = 50; s < uniqueCount; s += 50) {
    opts.push({ size: s, isAll: false });
  }
  opts.push({ size: uniqueCount, isAll: true });
  return opts;
}
