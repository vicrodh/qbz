import { Vibrant } from 'node-vibrant/browser';
import { getCachedImageUrl } from '$lib/services/imageCacheService';

export interface ArtworkSwatch {
  hex: string;
  rgb: [number, number, number];
  population: number;
  /** Perceived luminance in 0..1 (Rec. 709). */
  luminance: number;
}

export interface ArtworkPalette {
  /** Most populous swatch — closest to a "background" pick. */
  dominant: ArtworkSwatch | null;
  vibrant: ArtworkSwatch | null;
  muted: ArtworkSwatch | null;
  lightVibrant: ArtworkSwatch | null;
  darkVibrant: ArtworkSwatch | null;
  lightMuted: ArtworkSwatch | null;
  darkMuted: ArtworkSwatch | null;
}

const cache = new Map<string, Promise<ArtworkPalette>>();

function relativeLuminance(r: number, g: number, b: number): number {
  const channel = (c: number) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function toSwatch(s: { rgb: [number, number, number]; population: number; hex: string } | null | undefined): ArtworkSwatch | null {
  if (!s) return null;
  const [r, g, b] = s.rgb;
  return {
    hex: s.hex,
    rgb: [r, g, b],
    population: s.population,
    luminance: relativeLuminance(r, g, b),
  };
}

export async function extractPalette(url: string | null | undefined): Promise<ArtworkPalette> {
  if (!url) return emptyPalette();
  const cached = cache.get(url);
  if (cached) return cached;

  const promise = (async () => {
    try {
      // Resolve through the local image cache so Vibrant works on a
      // file:// / asset:// URL instead of going to the remote CDN.
      // Qobuz CDN sends Access-Control-Allow-Origin: * for album covers
      // but NOT for artist images, which taints the canvas Vibrant uses
      // and makes pixel reads fail. Routing through the cache also
      // primes the on-disk store for subsequent visits.
      let resolvedUrl = url;
      if (
        url.startsWith('http://') ||
        url.startsWith('https://')
      ) {
        try {
          resolvedUrl = await getCachedImageUrl(url);
        } catch {
          resolvedUrl = url;
        }
      }
      const palette = await Vibrant.from(resolvedUrl).getPalette();
      const all = [
        palette.Vibrant,
        palette.Muted,
        palette.LightVibrant,
        palette.DarkVibrant,
        palette.LightMuted,
        palette.DarkMuted,
      ].filter((s): s is NonNullable<typeof s> => !!s);
      const dominantRaw = all.length > 0
        ? all.reduce((a, b) => (a.population >= b.population ? a : b))
        : null;
      return {
        dominant: toSwatch(dominantRaw),
        vibrant: toSwatch(palette.Vibrant),
        muted: toSwatch(palette.Muted),
        lightVibrant: toSwatch(palette.LightVibrant),
        darkVibrant: toSwatch(palette.DarkVibrant),
        lightMuted: toSwatch(palette.LightMuted),
        darkMuted: toSwatch(palette.DarkMuted),
      } satisfies ArtworkPalette;
    } catch (err) {
      console.warn('[artworkPalette] extract failed for', url, err);
      return emptyPalette();
    }
  })();

  cache.set(url, promise);
  return promise;
}

/**
 * Pick the swatch most appropriate for an album header backdrop:
 * darkVibrant when it carries enough weight (≥ 10% of total swatch population),
 * else darkMuted, else dominant. Returns null when the palette is empty.
 */
export function pickHeaderColor(palette: ArtworkPalette | null): ArtworkSwatch | null {
  if (!palette) return null;
  const swatches = [
    palette.vibrant,
    palette.muted,
    palette.lightVibrant,
    palette.darkVibrant,
    palette.lightMuted,
    palette.darkMuted,
  ];
  const totalPop = swatches.reduce((sum, s) => sum + (s?.population ?? 0), 0);
  if (palette.darkVibrant && totalPop > 0 && palette.darkVibrant.population / totalPop >= 0.1) {
    return palette.darkVibrant;
  }
  if (palette.darkMuted) return palette.darkMuted;
  return palette.dominant;
}

function emptyPalette(): ArtworkPalette {
  return {
    dominant: null,
    vibrant: null,
    muted: null,
    lightVibrant: null,
    darkVibrant: null,
    lightMuted: null,
    darkMuted: null,
  };
}
