import { writable } from 'svelte/store';
import { clampZoom } from '$lib/utils/zoom';

const DEFAULT_ZOOM = 1;

function readStoredZoom(): number {
  if (typeof localStorage === 'undefined') {
    return DEFAULT_ZOOM;
  }

  const savedZoom = localStorage.getItem('qbz-zoom-level');
  if (!savedZoom) return DEFAULT_ZOOM;

  const parsed = Number.parseFloat(savedZoom);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return DEFAULT_ZOOM;
  }

  const clamped = clampZoom(parsed);
  if (clamped !== parsed) {
    localStorage.setItem('qbz-zoom-level', String(clamped));
  }

  return clamped;
}

const zoomStore = writable<number>(readStoredZoom());

let currentZoom = DEFAULT_ZOOM;
zoomStore.subscribe((value) => {
  currentZoom = value;
});

export function subscribeZoom(listener: (zoom: number) => void): () => void {
  return zoomStore.subscribe(listener);
}

export function getZoom(): number {
  return currentZoom;
}

export function setZoom(value: number): number {
  const clamped = clampZoom(value);
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('qbz-zoom-level', String(clamped));
  }
  zoomStore.set(clamped);
  return clamped;
}
