import { describe, it, expect } from 'vitest';
import { buildSizeOptions } from './trackMixModal.helpers';

describe('buildSizeOptions', () => {
  it('returns empty array for null', () => {
    expect(buildSizeOptions(null)).toEqual([]);
  });

  it('returns empty array for zero', () => {
    expect(buildSizeOptions(0)).toEqual([]);
  });

  it('returns single "All (N)" option when uniqueCount < 50', () => {
    expect(buildSizeOptions(12)).toEqual([{ size: 12, isAll: true }]);
    expect(buildSizeOptions(1)).toEqual([{ size: 1, isAll: true }]);
    expect(buildSizeOptions(49)).toEqual([{ size: 49, isAll: true }]);
  });

  it('returns single "All (50)" when uniqueCount is exactly 50 (no duplicate)', () => {
    expect(buildSizeOptions(50)).toEqual([{ size: 50, isAll: true }]);
  });

  it('returns 50, 100, 150, "All (200)" for clean multiple of 50', () => {
    expect(buildSizeOptions(200)).toEqual([
      { size: 50, isAll: false },
      { size: 100, isAll: false },
      { size: 150, isAll: false },
      { size: 200, isAll: true },
    ]);
  });

  it('returns 50, 100, 150, "All (187)" for irregular count', () => {
    expect(buildSizeOptions(187)).toEqual([
      { size: 50, isAll: false },
      { size: 100, isAll: false },
      { size: 150, isAll: false },
      { size: 187, isAll: true },
    ]);
  });

  it('returns 50, "All (51)" when count is just past a step', () => {
    expect(buildSizeOptions(51)).toEqual([
      { size: 50, isAll: false },
      { size: 51, isAll: true },
    ]);
  });

  it('returns empty array for negative input (defensive)', () => {
    expect(buildSizeOptions(-5)).toEqual([]);
  });
});
