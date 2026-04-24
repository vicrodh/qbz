import { isInputTarget } from './keyboard';

export interface ShiftRangeApplyArgs<TId> {
  current: Set<TId>;
  ids: readonly TId[];
  lastIndex: number;
  currentIndex: number;
}

export function applyShiftRange<TId>({
  current,
  ids,
  lastIndex,
  currentIndex,
}: ShiftRangeApplyArgs<TId>): Set<TId> {
  const next = new Set(current);
  const [lo, hi] = lastIndex <= currentIndex ? [lastIndex, currentIndex] : [currentIndex, lastIndex];
  for (let i = lo; i <= hi; i++) {
    if (i >= 0 && i < ids.length) next.add(ids[i]);
  }
  return next;
}

export function isSelectAllShortcut(event: KeyboardEvent): boolean {
  if (isInputTarget(event)) return false;
  if (!(event.ctrlKey || event.metaKey)) return false;
  return event.key === 'a' || event.key === 'A';
}
