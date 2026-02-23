export const ZOOM_OPTIONS = ['80%', '85%', '90%', '95%', '100%', '110%', '125%', '150%', '175%'] as const;

export const ZOOM_LEVELS = ZOOM_OPTIONS.map((option) => {
  const parsed = Number.parseFloat(option);
  return Number.isFinite(parsed) ? parsed / 100 : 1;
});

export const ZOOM_MAP: Record<string, number> = Object.fromEntries(
  ZOOM_OPTIONS.map((option, index) => [option, ZOOM_LEVELS[index]])
);

export const ZOOM_MIN = ZOOM_LEVELS[0];
export const ZOOM_MAX = ZOOM_LEVELS[ZOOM_LEVELS.length - 1];

export function getZoomLevelFromOption(option: string): number {
  return ZOOM_MAP[option] ?? 1;
}

export function findZoomOption(level: number): string | null {
  const epsilon = 0.01;
  for (const option of ZOOM_OPTIONS) {
    if (Math.abs((ZOOM_MAP[option] ?? 1) - level) < epsilon) {
      return option;
    }
  }
  return null;
}

export function clampZoom(level: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, level));
}

export function getNextZoomLevel(level: number, direction: 'in' | 'out'): number {
  const clamped = clampZoom(level);
  const epsilon = 0.001;

  if (direction === 'in') {
    for (const value of ZOOM_LEVELS) {
      if (value > clamped + epsilon) {
        return value;
      }
    }
    return ZOOM_MAX;
  }

  for (let index = ZOOM_LEVELS.length - 1; index >= 0; index -= 1) {
    if (ZOOM_LEVELS[index] < clamped - epsilon) {
      return ZOOM_LEVELS[index];
    }
  }

  return ZOOM_MIN;
}
