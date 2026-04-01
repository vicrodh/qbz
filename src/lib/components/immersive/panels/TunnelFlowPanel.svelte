<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import QualityBadge from '$lib/components/QualityBadge.svelte';
  import { getPanelFrameInterval } from '$lib/immersive/fpsConfig';

  interface Props {
    enabled?: boolean;
    artwork?: string;
    trackTitle?: string;
    artist?: string;
    album?: string;
    explicit?: boolean;
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
    originalBitDepth?: number;
    originalSamplingRate?: number;
    format?: string;
  }

  interface Point {
    x: number;
    y: number;
  }

  interface RgbColor {
    r: number;
    g: number;
    b: number;
  }

  let {
    enabled = true,
    artwork = '',
    trackTitle = '',
    artist = '',
    album = '',
    explicit = false,
    quality,
    bitDepth,
    samplingRate,
    originalBitDepth,
    originalSamplingRate,
    format
  }: Props = $props();

  let canvasRef: HTMLCanvasElement | null = $state(null);
  let ctx: CanvasRenderingContext2D | null = null;
  let animationFrame: number | null = null;
  let unlisten: UnlistenFn | null = null;
  let isInitialized = false;

  const NUM_BARS = 16;
  const SMOOTHING = 0.72;
  const FRAME_INTERVAL = getPanelFrameInterval('tunnel-flow');
  const BASE_RENDER_SCALE = FRAME_INTERVAL <= 20 ? 0.58 : FRAME_INTERVAL <= 34 ? 0.64 : 0.7;

  const RING_COUNT = 18;
  const STREAK_COUNT = 0;
  const DEFAULT_LINE_PALETTE: RgbColor[] = [
    { r: 255, g: 106, b: 106 },
    { r: 255, g: 205, b: 92 },
    { r: 104, g: 220, b: 170 },
    { r: 110, g: 176, b: 255 },
  ];

  const smoothedData = new Float32Array(NUM_BARS);

  let phase = 0;
  let lastRenderTime = 0;
  let kickPulse = 0;
  let previousBass = 0;
  let previousHigh = 0;
  let paletteRequestId = 0;
  let linePalette: RgbColor[] = $state(DEFAULT_LINE_PALETTE);

  function clamp01(value: number): number {
    return Math.max(0, Math.min(1, value));
  }

  function wrapHue(value: number): number {
    const hue = value % 360;
    return hue < 0 ? hue + 360 : hue;
  }

  function getLineColor(index: number): RgbColor {
    if (!linePalette.length) return DEFAULT_LINE_PALETTE[0];
    return linePalette[Math.abs(index) % linePalette.length];
  }

  function colorSaturation(red: number, green: number, blue: number): number {
    const max = Math.max(red, green, blue);
    const min = Math.min(red, green, blue);
    if (max <= 0) return 0;
    return (max - min) / max;
  }

  function colorDistance(colorA: RgbColor, colorB: RgbColor): number {
    const redDiff = colorA.r - colorB.r;
    const greenDiff = colorA.g - colorB.g;
    const blueDiff = colorA.b - colorB.b;
    return Math.hypot(redDiff, greenDiff, blueDiff);
  }

  async function extractLinePaletteFromArtwork(source: string): Promise<RgbColor[]> {
    if (typeof window === 'undefined' || !source) return DEFAULT_LINE_PALETTE;

    const image = new Image();
    image.decoding = 'async';
    image.crossOrigin = 'anonymous';

    const loadPromise = new Promise<void>((resolve, reject) => {
      image.onload = () => resolve();
      image.onerror = () => reject(new Error('Artwork palette load failed'));
    });

    image.src = source;
    await loadPromise;

    const sampleSize = 36;
    const sampleCanvas = document.createElement('canvas');
    sampleCanvas.width = sampleSize;
    sampleCanvas.height = sampleSize;
    const sampleCtx = sampleCanvas.getContext('2d', { willReadFrequently: true });
    if (!sampleCtx) return DEFAULT_LINE_PALETTE;

    sampleCtx.drawImage(image, 0, 0, sampleSize, sampleSize);

    let pixelData: Uint8ClampedArray;
    try {
      pixelData = sampleCtx.getImageData(0, 0, sampleSize, sampleSize).data;
    } catch {
      return DEFAULT_LINE_PALETTE;
    }

    type ColorBucket = { count: number; red: number; green: number; blue: number; satSum: number };
    const buckets = new Map<string, ColorBucket>();

    for (let pixelOffset = 0; pixelOffset < pixelData.length; pixelOffset += 4) {
      const alpha = pixelData[pixelOffset + 3];
      if (alpha < 120) continue;

      const red = pixelData[pixelOffset];
      const green = pixelData[pixelOffset + 1];
      const blue = pixelData[pixelOffset + 2];
      const luminance = (red * 0.2126 + green * 0.7152 + blue * 0.0722) / 255;
      if (luminance < 0.05 || luminance > 0.97) continue;

      const saturation = colorSaturation(red, green, blue);
      const quantizedRed = Math.floor(red / 32) * 32;
      const quantizedGreen = Math.floor(green / 32) * 32;
      const quantizedBlue = Math.floor(blue / 32) * 32;
      const bucketKey = `${quantizedRed}-${quantizedGreen}-${quantizedBlue}`;
      const existingBucket = buckets.get(bucketKey);

      if (existingBucket) {
        existingBucket.count += 1;
        existingBucket.red += red;
        existingBucket.green += green;
        existingBucket.blue += blue;
        existingBucket.satSum += saturation;
      } else {
        buckets.set(bucketKey, { count: 1, red, green, blue, satSum: saturation });
      }
    }

    if (!buckets.size) return DEFAULT_LINE_PALETTE;

    const rankedBuckets = [...buckets.values()].sort((bucketA, bucketB) => {
      const avgSatA = bucketA.satSum / bucketA.count;
      const avgSatB = bucketB.satSum / bucketB.count;
      const scoreA = bucketA.count * (0.72 + avgSatA * 1.28);
      const scoreB = bucketB.count * (0.72 + avgSatB * 1.28);
      return scoreB - scoreA;
    });

    const palette: RgbColor[] = [];
    for (const bucket of rankedBuckets) {
      const candidate: RgbColor = {
        r: Math.round(bucket.red / bucket.count),
        g: Math.round(bucket.green / bucket.count),
        b: Math.round(bucket.blue / bucket.count),
      };
      const candidateSat = colorSaturation(candidate.r, candidate.g, candidate.b);
      if (candidateSat < 0.12 && palette.length > 0) continue;
      if (palette.some((existingColor) => colorDistance(existingColor, candidate) < 44)) continue;
      palette.push(candidate);
      if (palette.length >= 4) break;
    }

    if (!palette.length) return DEFAULT_LINE_PALETTE;
    while (palette.length < 4) palette.push(palette[palette.length - 1]);
    return palette;
  }

  function getBassEnergy(): number {
    let sum = 0;
    for (let bandIndex = 0; bandIndex < 4; bandIndex++) sum += smoothedData[bandIndex];
    return sum / 4;
  }

  function getMidEnergy(): number {
    let sum = 0;
    for (let bandIndex = 4; bandIndex < 10; bandIndex++) sum += smoothedData[bandIndex];
    return sum / 6;
  }

  function getHighEnergy(): number {
    let sum = 0;
    for (let bandIndex = 10; bandIndex < NUM_BARS; bandIndex++) sum += smoothedData[bandIndex];
    return sum / 6;
  }

  function getSideVectors(side: number): { normalX: number; normalY: number; tangentX: number; tangentY: number } {
    if (side === 0) return { normalX: 0, normalY: -1, tangentX: 1, tangentY: 0 };
    if (side === 1) return { normalX: 1, normalY: 0, tangentX: 0, tangentY: 1 };
    if (side === 2) return { normalX: 0, normalY: 1, tangentX: -1, tangentY: 0 };
    return { normalX: -1, normalY: 0, tangentX: 0, tangentY: -1 };
  }

  function makeWarpedSquare(
    centerX: number,
    centerY: number,
    halfSize: number,
    timeMs: number,
    seed: number,
    warpAmount: number,
    scaleX = 1,
    scaleY = 1
  ): Point[] {
    const cornerVectors: Array<{ sx: number; sy: number }> = [
      { sx: -1, sy: -1 },
      { sx: 1, sy: -1 },
      { sx: 1, sy: 1 },
      { sx: -1, sy: 1 },
    ];

    const points: Point[] = [];
    const scaledHalfX = halfSize * scaleX;
    const scaledHalfY = halfSize * scaleY;
    const jitterScale = halfSize * warpAmount;

    for (let cornerIndex = 0; cornerIndex < cornerVectors.length; cornerIndex++) {
      const corner = cornerVectors[cornerIndex];
      const waveA = Math.sin(timeMs * 0.0015 + seed * 0.9 + cornerIndex * 1.7);
      const waveB = Math.cos(timeMs * 0.0012 - seed * 0.7 + cornerIndex * 1.3);
      const jitter = jitterScale * (waveA * 0.6 + waveB * 0.4);

      points.push({
        x: centerX + corner.sx * scaledHalfX + (corner.sx * 0.6 + corner.sy * 0.22) * jitter * scaleX,
        y: centerY + corner.sy * scaledHalfY + (corner.sy * 0.6 - corner.sx * 0.22) * jitter * scaleY,
      });
    }

    return points;
  }

  function drawFeedback(
    centerX: number,
    centerY: number,
    drawWidth: number,
    drawHeight: number,
    minDim: number,
    timeMs: number,
    bass: number,
    high: number
  ) {
    if (!ctx || !canvasRef) return;

    const driftRadius = minDim * (0.006 + bass * 0.016 + high * 0.01 + kickPulse * 0.012);
    const driftX = Math.cos(timeMs * 0.001 + phase * 0.18) * driftRadius;
    const driftY = Math.sin(timeMs * 0.0012 + phase * 0.16) * driftRadius;

    ctx.save();
    ctx.translate(centerX, centerY);
    ctx.rotate(Math.sin(timeMs * 0.0011) * 0.0022 + (high - bass) * 0.003);
    const zoom = 1.004 + bass * 0.014 + high * 0.01 + kickPulse * 0.011;
    ctx.scale(zoom, zoom);
    ctx.translate(-centerX + driftX, -centerY + driftY);
    ctx.globalAlpha = 0.74 + kickPulse * 0.08;
    ctx.drawImage(canvasRef, 0, 0, drawWidth, drawHeight);
    ctx.restore();

    ctx.globalAlpha = 1;
  }

  function drawBackground(
    drawWidth: number,
    drawHeight: number,
    centerX: number,
    centerY: number,
    minDim: number,
    timeMs: number,
    bass: number,
    high: number
  ) {
    if (!ctx) return;

    const centerWash = ctx.createRadialGradient(
      centerX,
      centerY,
      minDim * 0.06,
      centerX,
      centerY,
      minDim * 1.16
    );
    centerWash.addColorStop(0, `rgba(255, 255, 255, ${0.008 + bass * 0.008})`);
    centerWash.addColorStop(0.45, `rgba(180, 180, 180, ${0.006 + high * 0.006})`);
    centerWash.addColorStop(1, 'rgba(0, 0, 0, 0)');

    ctx.globalCompositeOperation = 'source-over';
    ctx.fillStyle = centerWash;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    const haze = ctx.createLinearGradient(0, 0, drawWidth, drawHeight);
    haze.addColorStop(0, `rgba(255, 255, 255, ${0.004 + high * 0.005})`);
    haze.addColorStop(0.5, 'rgba(0, 0, 0, 0)');
    haze.addColorStop(1, `rgba(180, 180, 180, ${0.005 + bass * 0.007})`);

    ctx.globalCompositeOperation = 'soft-light';
    ctx.fillStyle = haze;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    ctx.globalCompositeOperation = 'source-over';
  }

  function drawTunnelLayers(
    centerX: number,
    centerY: number,
    minDim: number,
    tunnelScaleX: number,
    tunnelScaleY: number,
    timeMs: number,
    bass: number,
    mid: number,
    high: number
  ) {
    if (!ctx) return;

    ctx.globalCompositeOperation = 'screen';
    const ringRecords: Array<{ innerSquare: Point[]; perspective: number; fade: number }> = [];
    let previousInnerSquare: Point[] | null = null;

    for (let ringIndex = 0; ringIndex < RING_COUNT; ringIndex++) {
      const travel = (ringIndex / RING_COUNT + phase * (1.28 + bass * 0.24 + kickPulse * 0.34)) % 1;
      const perspective = Math.pow(travel, 1.42);
      const spacingCurve = (Math.exp(perspective * 3.2) - 1) / (Math.exp(3.2) - 1);
      const appear = clamp01((perspective - 0.18) / 0.26);
      const vanish = 1 - clamp01((perspective - 0.9) / 0.1);
      const fade = Math.pow(appear * vanish, 0.82);

      const outerHalf = minDim * (0.084 + spacingCurve * 0.92);
      const thickness = minDim * (0.005 + (1 - spacingCurve) * (0.028 + bass * 0.016) + kickPulse * 0.0036);
      const innerHalf = Math.max(minDim * 0.028, outerHalf - thickness);
      const warp = 0.0025 + (1 - spacingCurve) * 0.013 + high * 0.004;
      const bendStrength = minDim * (0.018 + high * 0.01 + kickPulse * 0.008) * (1 - spacingCurve);
      const bendX = Math.sin(timeMs * 0.0011 + ringIndex * 0.31 + phase * 0.12) * bendStrength;
      const bendY = Math.cos(timeMs * 0.00086 + ringIndex * 0.24 + phase * 0.1) * bendStrength * 0.66;
      const ringCenterX = centerX + bendX;
      const ringCenterY = centerY + bendY;

      const outerSquare = makeWarpedSquare(
        ringCenterX,
        ringCenterY,
        outerHalf,
        timeMs,
        ringIndex + 0.3,
        warp,
        tunnelScaleX,
        tunnelScaleY
      );
      const innerSquare = makeWarpedSquare(
        ringCenterX,
        ringCenterY,
        innerHalf,
        timeMs,
        ringIndex + 1.7,
        warp * 0.72,
        tunnelScaleX,
        tunnelScaleY
      );

      for (let sideIndex = 0; sideIndex < 4; sideIndex++) {
        const nextIndex = (sideIndex + 1) % 4;

        const outerA = outerSquare[sideIndex];
        const outerB = outerSquare[nextIndex];
        const innerB = innerSquare[nextIndex];
        const innerA = innerSquare[sideIndex];

        const alpha = fade * (0.2 + bass * 0.08 + kickPulse * 0.06);

        const wallGradient = ctx.createLinearGradient(innerA.x, innerA.y, outerA.x, outerA.y);
        wallGradient.addColorStop(0, `rgba(210, 210, 210, ${alpha * 0.22})`);
        wallGradient.addColorStop(0.44, `rgba(255, 255, 255, ${alpha * 1.16})`);
        wallGradient.addColorStop(1, 'rgba(0, 0, 0, 0)');

        ctx.fillStyle = wallGradient;
        ctx.beginPath();
        ctx.moveTo(outerA.x, outerA.y);
        ctx.lineTo(outerB.x, outerB.y);
        ctx.lineTo(innerB.x, innerB.y);
        ctx.lineTo(innerA.x, innerA.y);
        ctx.closePath();
        ctx.fill();

        if ((ringIndex + sideIndex) % 3 === 0) {
          // Sparse wall stains to avoid heavy overdraw
          const sideVectors = getSideVectors(sideIndex);
          const lane = Math.sin(ringIndex * 1.13 + sideIndex * 1.7 + timeMs * 0.0012 + travel * 22);
          const radialX = outerHalf * tunnelScaleX;
          const radialY = outerHalf * tunnelScaleY;
          const blobCenterX =
            centerX +
            sideVectors.normalX * radialX * 0.82 +
            sideVectors.tangentX * radialX * 0.58 * lane;
          const blobCenterY =
            centerY +
            sideVectors.normalY * radialY * 0.82 +
            sideVectors.tangentY * radialY * 0.58 * lane;

          const blobSize = minDim * (0.008 + fade * (0.018 + bass * 0.01));
          const blobGradient = ctx.createRadialGradient(
            blobCenterX,
            blobCenterY,
            blobSize * 0.14,
            blobCenterX,
            blobCenterY,
            blobSize * 1.08
          );
          const particleCoreColor = getLineColor(ringIndex + sideIndex);
          const particleOuterColor = getLineColor(ringIndex + sideIndex + 1);
          blobGradient.addColorStop(
            0,
            `rgba(${particleCoreColor.r}, ${particleCoreColor.g}, ${particleCoreColor.b}, ${alpha * (0.56 + mid * 0.18)})`
          );
          blobGradient.addColorStop(
            0.62,
            `rgba(${particleOuterColor.r}, ${particleOuterColor.g}, ${particleOuterColor.b}, ${alpha * (0.3 + high * 0.12)})`
          );
          blobGradient.addColorStop(1, 'rgba(0, 0, 0, 0)');

          ctx.fillStyle = blobGradient;
          ctx.beginPath();
          ctx.arc(blobCenterX, blobCenterY, blobSize, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      const edgeAlpha = fade * (0.34 + bass * 0.12 + kickPulse * 0.08);
      const edgeColor = getLineColor(ringIndex);
      const edgeStroke = `rgba(${edgeColor.r}, ${edgeColor.g}, ${edgeColor.b}, ${edgeAlpha})`;
      const edgeWidth = 0.86 + fade * (1.78 + bass * 0.72 + kickPulse * 0.56);
      ctx.strokeStyle = edgeStroke;
      ctx.lineWidth = edgeWidth;
      ctx.beginPath();
      ctx.moveTo(innerSquare[0].x, innerSquare[0].y);
      ctx.lineTo(innerSquare[1].x, innerSquare[1].y);
      ctx.lineTo(innerSquare[2].x, innerSquare[2].y);
      ctx.lineTo(innerSquare[3].x, innerSquare[3].y);
      ctx.closePath();
      ctx.stroke();

      if (previousInnerSquare) {
        ctx.strokeStyle = edgeStroke;
        ctx.lineWidth = Math.max(0.74, edgeWidth * 0.88);
        for (let vertexIndex = 0; vertexIndex < 4; vertexIndex++) {
          ctx.beginPath();
          ctx.moveTo(previousInnerSquare[vertexIndex].x, previousInnerSquare[vertexIndex].y);
          ctx.lineTo(innerSquare[vertexIndex].x, innerSquare[vertexIndex].y);
          ctx.stroke();
        }
      }

      previousInnerSquare = innerSquare;
      ringRecords.push({ innerSquare, perspective, fade });
    }

    const orderedRings = [...ringRecords].sort((ringA, ringB) => ringA.perspective - ringB.perspective);
    if (orderedRings.length > 4) {
      const audioDrive = clamp01(bass * 0.38 + high * 0.28 + kickPulse * 1.04);
      const linesPerSide = 1 + Math.floor(audioDrive * 1.6); // 1..2
      const eventStepMs = Math.max(92, 172 - audioDrive * 84);
      const eventBucket = Math.floor(timeMs / eventStepMs);
      const laneSlots = [0.2, 0.5, 0.8];
      const previousLineCap = ctx.lineCap;
      ctx.lineCap = 'round';

      for (let sideIndex = 0; sideIndex < 4; sideIndex++) {
        const nextSide = (sideIndex + 1) % 4;

        for (let traceIndex = 0; traceIndex < linesPerSide; traceIndex++) {
          const seed = sideIndex * 11.37 + traceIndex * 3.83 + eventBucket * 0.41;
          const gate = (Math.sin(eventBucket * 0.47 + phase * 2.2 + seed) + 1) * 0.5;
          const threshold = 0.6 - audioDrive * 0.32;
          if (gate < threshold) continue;

          const span = gate > 0.76 ? 3 : 2;
          const maxStartIndex = orderedRings.length - span - 1;
          if (maxStartIndex < 0) continue;

          const startSelector = (Math.sin(timeMs * 0.0014 + phase * 1.8 + seed * 0.9) + 1) * 0.5;
          const startIndex = Math.min(maxStartIndex, Math.floor(startSelector * (maxStartIndex + 1)));
          const endIndex = Math.min(orderedRings.length - 1, startIndex + span);

          const laneSelector = clamp01((Math.sin(eventBucket * 0.73 + seed * 1.3) + 1) * 0.5);
          const lane = laneSlots[Math.min(2, Math.floor(laneSelector * laneSlots.length))];
          const startRing = orderedRings[startIndex];
          const endRing = orderedRings[endIndex];

          const startA = startRing.innerSquare[sideIndex];
          const startB = startRing.innerSquare[nextSide];
          const endA = endRing.innerSquare[sideIndex];
          const endB = endRing.innerSquare[nextSide];

          const startX = startA.x + (startB.x - startA.x) * lane;
          const startY = startA.y + (startB.y - startA.y) * lane;
          const endX = endA.x + (endB.x - endA.x) * lane;
          const endY = endA.y + (endB.y - endA.y) * lane;

          const lineFade = clamp01((startRing.fade + endRing.fade) * 0.7);
          const strutAlpha = (0.32 + audioDrive * 0.34) * lineFade;
          const strutColor = getLineColor(sideIndex * 3 + traceIndex + startIndex);

          ctx.strokeStyle = `rgba(${strutColor.r}, ${strutColor.g}, ${strutColor.b}, ${strutAlpha})`;
          ctx.lineWidth = 1.5 + audioDrive * 1.4;
          ctx.beginPath();
          ctx.moveTo(startX, startY);
          ctx.lineTo(endX, endY);
          ctx.stroke();
        }
      }

      ctx.lineCap = previousLineCap;
    }

    ctx.globalCompositeOperation = 'source-over';
  }

  function drawSpeedMarks(
    centerX: number,
    centerY: number,
    minDim: number,
    tunnelScaleX: number,
    tunnelScaleY: number,
    timeMs: number,
    high: number
  ) {
    if (!ctx) return;

    ctx.globalCompositeOperation = 'source-over';
    ctx.lineCap = 'butt';
    ctx.lineJoin = 'round';

    for (let streakIndex = 0; streakIndex < STREAK_COUNT; streakIndex++) {
      const sideIndex = streakIndex % 4;
      const sideVectors = getSideVectors(sideIndex);

      const travel = (streakIndex / STREAK_COUNT + phase * (2.18 + high * 0.44 + kickPulse * 0.62)) % 1;
      const perspective = Math.pow(travel, 1.18);
      const appear = clamp01((perspective - 0.16) / 0.26);
      const vanish = 1 - clamp01((perspective - 0.9) / 0.1);
      const fade = appear * vanish;
      if (fade <= 0.001) continue;

      const wallHalf = minDim * (0.14 + perspective * 0.82);
      const radialX = wallHalf * tunnelScaleX;
      const radialY = wallHalf * tunnelScaleY;

      const laneWave = Math.sin(streakIndex * 1.37 + timeMs * 0.0014 + perspective * 8.2);
      const laneJitter = Math.sin(streakIndex * 2.11 + timeMs * 0.0018) * 0.12;
      const lane = Math.max(-0.92, Math.min(0.92, laneWave * 0.78 + laneJitter));

      const startX =
        centerX +
        sideVectors.normalX * radialX * 0.92 +
        sideVectors.tangentX * radialX * 0.68 * lane;
      const startY =
        centerY +
        sideVectors.normalY * radialY * 0.92 +
        sideVectors.tangentY * radialY * 0.68 * lane;

      const length = minDim * (0.04 + perspective * (0.22 + kickPulse * 0.08));
      const bend = minDim * 0.01 * Math.sin(streakIndex * 0.93 + timeMs * 0.0024);

      const midX = startX + sideVectors.normalX * length * 0.54 + sideVectors.tangentX * bend;
      const midY = startY + sideVectors.normalY * length * 0.54 + sideVectors.tangentY * bend;
      const endX = startX + sideVectors.normalX * length + sideVectors.tangentX * bend * 1.55;
      const endY = startY + sideVectors.normalY * length + sideVectors.tangentY * bend * 1.55;

      const hue = sideIndex % 2 === 0
        ? wrapHue(8 + timeMs * 0.03 + high * 8 + streakIndex * 0.7)
        : wrapHue(196 + timeMs * 0.025 + high * 10 + streakIndex * 0.6);
      const alpha = fade * (0.3 + high * 0.14 + kickPulse * 0.12);

      const lineGradient = ctx.createLinearGradient(startX, startY, endX, endY);
      lineGradient.addColorStop(0, `hsla(${hue}, 70%, 48%, 0)`);
      lineGradient.addColorStop(0.46, `hsla(${wrapHue(hue + 6)}, 86%, 60%, ${alpha})`);
      lineGradient.addColorStop(1, `hsla(${wrapHue(hue + 12)}, 80%, 50%, ${alpha * 0.18})`);

      ctx.strokeStyle = lineGradient;
      ctx.lineWidth = 1 + perspective * (2.6 + high * 0.7 + kickPulse * 0.7);
      ctx.beginPath();
      ctx.moveTo(startX, startY);
      ctx.lineTo(midX, midY);
      ctx.lineTo(endX, endY);
      ctx.stroke();
    }

    ctx.globalCompositeOperation = 'source-over';
  }

  function drawPortal(
    centerX: number,
    centerY: number,
    minDim: number,
    tunnelScaleX: number,
    tunnelScaleY: number,
    timeMs: number,
    bass: number,
    high: number,
    lateralCurve: number
  ) {
    if (!ctx) return;

    const portalHalf = minDim * 0.057;
    const throatOuterHalf = portalHalf * 1.58;

    ctx.globalCompositeOperation = 'screen';

    // Throat section: square ring that aligns with tunnel walls
    const outerSquare = makeWarpedSquare(
      centerX,
      centerY,
      throatOuterHalf,
      timeMs,
      941,
      0.008 + high * 0.006,
      tunnelScaleX,
      tunnelScaleY
    );
    const innerSquare = makeWarpedSquare(
      centerX,
      centerY,
      portalHalf,
      timeMs,
      942,
      0.004 + high * 0.004,
      tunnelScaleX,
      tunnelScaleY
    );

    for (let sideIndex = 0; sideIndex < 4; sideIndex++) {
      const nextIndex = (sideIndex + 1) % 4;
      const outerA = outerSquare[sideIndex];
      const outerB = outerSquare[nextIndex];
      const innerB = innerSquare[nextIndex];
      const innerA = innerSquare[sideIndex];

      const sideAlpha = 0.032 + high * 0.015;

      const sideGradient = ctx.createLinearGradient(innerA.x, innerA.y, outerA.x, outerA.y);
      sideGradient.addColorStop(0, `rgba(255, 255, 255, ${sideAlpha})`);
      sideGradient.addColorStop(0.5, `rgba(185, 185, 185, ${sideAlpha * 0.24})`);
      sideGradient.addColorStop(1, 'rgba(0, 0, 0, 0)');

      ctx.fillStyle = sideGradient;
      ctx.beginPath();
      ctx.moveTo(outerA.x, outerA.y);
      ctx.lineTo(outerB.x, outerB.y);
      ctx.lineTo(innerB.x, innerB.y);
      ctx.lineTo(innerA.x, innerA.y);
      ctx.closePath();
      ctx.fill();
    }

    // Final square hole aligned to innerSquare vertices so it does not float
    const holeInset = 0.82;
    const holeSquare: Point[] = innerSquare.map((point) => ({
      x: centerX + (point.x - centerX) * holeInset,
      y: centerY + (point.y - centerY) * holeInset,
    }));
    ctx.globalCompositeOperation = 'source-over';
    ctx.fillStyle = 'rgba(0, 0, 0, 0.98)';
    ctx.beginPath();
    ctx.moveTo(holeSquare[0].x, holeSquare[0].y);
    ctx.lineTo(holeSquare[1].x, holeSquare[1].y);
    ctx.lineTo(holeSquare[2].x, holeSquare[2].y);
    ctx.lineTo(holeSquare[3].x, holeSquare[3].y);
    ctx.closePath();
    ctx.fill();

    // Dark diffuse halo around the portal to avoid gray smoke and keep contrast progression.
    const darkHalo = ctx.createRadialGradient(
      centerX,
      centerY,
      portalHalf * 0.6,
      centerX,
      centerY,
      portalHalf * 4.8
    );
    darkHalo.addColorStop(0, 'rgba(0, 0, 0, 0.72)');
    darkHalo.addColorStop(0.32, 'rgba(0, 0, 0, 0.48)');
    darkHalo.addColorStop(0.72, 'rgba(0, 0, 0, 0.18)');
    darkHalo.addColorStop(1, 'rgba(0, 0, 0, 0)');
    ctx.globalCompositeOperation = 'multiply';
    ctx.fillStyle = darkHalo;
    ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
    ctx.globalCompositeOperation = 'source-over';

    const portalEdgeColor = getLineColor(0);
    const portalEdgeStroke = `rgba(${portalEdgeColor.r}, ${portalEdgeColor.g}, ${portalEdgeColor.b}, ${0.18 + high * 0.06})`;
    const portalEdgeWidth = 0.96 + bass * 0.26;

    // Directional front shadow: appears only on the curve-leading side.
    const lateralAbs = Math.abs(lateralCurve);
    if (lateralAbs > 0.06) {
      const shadowStrength = clamp01((lateralAbs - 0.06) / 0.58);
      const sideIsRight = lateralCurve > 0;
      const topIndex = sideIsRight ? 1 : 0;
      const bottomIndex = sideIsRight ? 2 : 3;

      const holeTop = holeSquare[topIndex];
      const holeBottom = holeSquare[bottomIndex];
      const innerTop = innerSquare[topIndex];
      const innerBottom = innerSquare[bottomIndex];

      const midHoleX = (holeTop.x + holeBottom.x) * 0.5;
      const midHoleY = (holeTop.y + holeBottom.y) * 0.5;
      const midInnerX = (innerTop.x + innerBottom.x) * 0.5;
      const midInnerY = (innerTop.y + innerBottom.y) * 0.5;

      const shadowGradient = ctx.createLinearGradient(midHoleX, midHoleY, midInnerX, midInnerY);
      shadowGradient.addColorStop(0, `rgba(0, 0, 0, ${0.56 + shadowStrength * 0.22})`);
      shadowGradient.addColorStop(0.72, `rgba(0, 0, 0, ${0.22 + shadowStrength * 0.18})`);
      shadowGradient.addColorStop(1, 'rgba(0, 0, 0, 0)');

      ctx.globalCompositeOperation = 'multiply';
      ctx.fillStyle = shadowGradient;
      ctx.beginPath();
      ctx.moveTo(holeTop.x, holeTop.y);
      ctx.lineTo(holeBottom.x, holeBottom.y);
      ctx.lineTo(innerBottom.x, innerBottom.y);
      ctx.lineTo(innerTop.x, innerTop.y);
      ctx.closePath();
      ctx.fill();

      ctx.strokeStyle = `rgba(0, 0, 0, ${0.42 + shadowStrength * 0.22})`;
      ctx.lineWidth = Math.max(1, portalEdgeWidth * 1.2);
      ctx.beginPath();
      ctx.moveTo(holeTop.x, holeTop.y);
      ctx.lineTo(holeBottom.x, holeBottom.y);
      ctx.stroke();

      ctx.globalCompositeOperation = 'source-over';
    }

    // Portal edge highlight to lock perspective corners
    ctx.strokeStyle = portalEdgeStroke;
    ctx.lineWidth = portalEdgeWidth;
    ctx.beginPath();
    ctx.moveTo(innerSquare[0].x, innerSquare[0].y);
    ctx.lineTo(innerSquare[1].x, innerSquare[1].y);
    ctx.lineTo(innerSquare[2].x, innerSquare[2].y);
    ctx.lineTo(innerSquare[3].x, innerSquare[3].y);
    ctx.closePath();
    ctx.stroke();

    // Slight rim on the black hole for clearer origin contrast
    ctx.strokeStyle = `rgba(${portalEdgeColor.r}, ${portalEdgeColor.g}, ${portalEdgeColor.b}, ${0.14 + high * 0.04})`;
    ctx.lineWidth = Math.max(0.76, portalEdgeWidth * 0.8);
    ctx.beginPath();
    ctx.moveTo(holeSquare[0].x, holeSquare[0].y);
    ctx.lineTo(holeSquare[1].x, holeSquare[1].y);
    ctx.lineTo(holeSquare[2].x, holeSquare[2].y);
    ctx.lineTo(holeSquare[3].x, holeSquare[3].y);
    ctx.closePath();
    ctx.stroke();
  }

  function drawCornerVignette(drawWidth: number, drawHeight: number, minDim: number) {
    if (!ctx) return;

    const corners = [
      { x: 0, y: 0 },
      { x: drawWidth, y: 0 },
      { x: 0, y: drawHeight },
      { x: drawWidth, y: drawHeight },
    ];

    ctx.globalCompositeOperation = 'multiply';

    for (const corner of corners) {
      const gradient = ctx.createRadialGradient(
        corner.x,
        corner.y,
        0,
        corner.x,
        corner.y,
        minDim * 0.86
      );
      gradient.addColorStop(0, 'rgba(0, 0, 0, 0.72)');
      gradient.addColorStop(0.44, 'rgba(0, 0, 0, 0.38)');
      gradient.addColorStop(1, 'rgba(0, 0, 0, 0)');
      ctx.fillStyle = gradient;
      ctx.fillRect(0, 0, drawWidth, drawHeight);
    }

    ctx.globalCompositeOperation = 'source-over';
  }

  function render(timestamp: number = 0) {
    if (!ctx || !canvasRef) return;

    const delta = timestamp - lastRenderTime;
    if (delta < FRAME_INTERVAL) {
      animationFrame = requestAnimationFrame(render);
      return;
    }
    lastRenderTime = timestamp;

    const bounds = canvasRef.getBoundingClientRect();
    const cssWidth = Math.max(1, bounds.width);
    const cssHeight = Math.max(1, bounds.height);
    const drawWidth = Math.max(320, Math.floor(cssWidth * BASE_RENDER_SCALE));
    const drawHeight = Math.max(180, Math.floor(cssHeight * BASE_RENDER_SCALE));

    if (canvasRef.width !== drawWidth || canvasRef.height !== drawHeight) {
      canvasRef.width = drawWidth;
      canvasRef.height = drawHeight;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.imageSmoothingEnabled = true;
    }

    const centerX = drawWidth * 0.5;
    const centerY = drawHeight * 0.5;
    const minDim = Math.min(drawWidth, drawHeight);
    const viewportAspect = drawWidth / Math.max(1, drawHeight);
    const tunnelScaleX = Math.min(1.58, Math.max(1.22, 1.04 + viewportAspect * 0.25));
    const tunnelScaleY = Math.max(0.62, Math.min(0.82, 1.38 - tunnelScaleX * 0.45));

    const bass = getBassEnergy();
    const mid = getMidEnergy();
    const high = getHighEnergy();
    const curveBaseX = drawWidth * (0.14 + high * 0.05 + kickPulse * 0.04);
    const curveBaseY = minDim * (0.036 + bass * 0.018 + kickPulse * 0.014);
    const curvePhase = timestamp * 0.0006 + phase * 0.14;
    const tunnelCenterX =
      centerX +
      Math.sin(curvePhase) * curveBaseX +
      Math.sin(curvePhase * 2.08 + 1.2) * drawWidth * 0.042;
    const tunnelCenterY =
      centerY +
      Math.cos(curvePhase * 0.82 + 0.7) * curveBaseY +
      Math.cos(curvePhase * 1.7 + 0.4) * minDim * 0.012;
    const lateralCurve = Math.max(-1, Math.min(1, (tunnelCenterX - centerX) / Math.max(1, drawWidth * 0.25)));

    phase += 0.012 + bass * 0.026 + high * 0.014 + kickPulse * 0.018;
    kickPulse *= 0.9;

    drawFeedback(tunnelCenterX, tunnelCenterY, drawWidth, drawHeight, minDim, timestamp, bass, high);

    ctx.fillStyle = `rgba(3, 0, 5, ${0.28 + high * 0.08})`;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    drawBackground(drawWidth, drawHeight, tunnelCenterX, tunnelCenterY, minDim, timestamp, bass, high);
    drawTunnelLayers(tunnelCenterX, tunnelCenterY, minDim, tunnelScaleX, tunnelScaleY, timestamp, bass, mid, high);
    drawSpeedMarks(tunnelCenterX, tunnelCenterY, minDim, tunnelScaleX, tunnelScaleY, timestamp, high);
    drawPortal(
      tunnelCenterX,
      tunnelCenterY,
      minDim,
      tunnelScaleX,
      tunnelScaleY,
      timestamp,
      bass,
      high,
      lateralCurve
    );
    drawCornerVignette(drawWidth, drawHeight, minDim);

    animationFrame = requestAnimationFrame(render);
  }

  async function init() {
    if (!canvasRef || isInitialized) return;

    ctx = canvasRef.getContext('2d');
    if (!ctx) return;
    isInitialized = true;

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
    } catch (error) {
      console.error('[TunnelFlow] Failed to enable backend:', error);
    }

    unlisten = await listen<number[]>('viz:data', (event) => {
      const payload = event.payload;
      if (!Array.isArray(payload)) return;

      const bytes = new Uint8Array(payload);
      const floats = new Float32Array(bytes.buffer);
      if (floats.length !== NUM_BARS) return;

      for (let bandIndex = 0; bandIndex < NUM_BARS; bandIndex++) {
        smoothedData[bandIndex] = smoothedData[bandIndex] * SMOOTHING + floats[bandIndex] * (1 - SMOOTHING);
      }

      const bass = getBassEnergy();
      const high = getHighEnergy();
      const bassDelta = bass - previousBass;
      const highDelta = high - previousHigh;
      previousBass = bass;
      previousHigh = high;

      if (bassDelta > 0.05 || highDelta > 0.05 || bass > 0.7) {
        kickPulse = clamp01(0.24 + bass * 0.52 + high * 0.34 + Math.max(0, bassDelta) * 1.8);
      }
    });

    render(0);
  }

  async function cleanup() {
    if (animationFrame) {
      cancelAnimationFrame(animationFrame);
      animationFrame = null;
    }

    if (unlisten) {
      unlisten();
      unlisten = null;
    }

    smoothedData.fill(0);
    phase = 0;
    lastRenderTime = 0;
    kickPulse = 0;
    previousBass = 0;
    previousHigh = 0;

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: false });
    } catch (error) {
      console.error('[TunnelFlow] Failed to disable backend:', error);
    }

    isInitialized = false;
  }

  $effect(() => {
    const artworkSource = artwork?.trim();
    if (typeof window === 'undefined') return;
    if (!artworkSource) {
      linePalette = DEFAULT_LINE_PALETTE;
      return;
    }

    const requestId = ++paletteRequestId;
    void extractLinePaletteFromArtwork(artworkSource)
      .then((palette) => {
        if (requestId !== paletteRequestId) return;
        linePalette = palette;
      })
      .catch(() => {
        if (requestId !== paletteRequestId) return;
        linePalette = DEFAULT_LINE_PALETTE;
      });
  });

  onMount(() => {
    if (enabled) init();
    return cleanup;
  });

  $effect(() => {
    if (enabled && !isInitialized) {
      init();
    } else if (!enabled && isInitialized) {
      cleanup();
    }
  });
</script>

<div class="tunnel-flow-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="tunnel-flow-canvas"></canvas>

  <div class="bottom-info">
    <div class="track-meta">
      <span class="track-title">{trackTitle}</span>
      {#if explicit}
        <span class="explicit-badge" title="{ $t('library.explicit') }"></span>
      {/if}
      {#if album}
        <span class="track-album">{album}</span>
      {/if}
      <span class="track-artist">{artist}</span>
      <QualityBadge {quality} {bitDepth} {samplingRate} {originalBitDepth} {originalSamplingRate} {format} compact />
    </div>
    {#if artwork}
      <div class="artwork-thumb">
        <img src={artwork} alt={trackTitle} />
      </div>
    {/if}
  </div>
</div>

<style>
  .tunnel-flow-panel {
    position: absolute;
    inset: 0;
    opacity: 0;
    transition: opacity 240ms ease;
    z-index: 5;
    background: #000;
  }

  .tunnel-flow-panel.visible {
    opacity: 1;
  }

  .tunnel-flow-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .bottom-info {
    position: absolute;
    bottom: 24px;
    right: 24px;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .track-meta {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
  }

  .track-title {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary, white);
    text-shadow: 0 1px 6px rgba(0, 0, 0, 0.5);
    max-width: 400px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-album {
    font-size: 12px;
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    font-style: italic;
    max-width: 400px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .track-artist {
    font-size: 12px;
    color: var(--alpha-60, rgba(255, 255, 255, 0.62));
    max-width: 400px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .artwork-thumb {
    width: 72px;
    height: 72px;
    border-radius: 6px;
    overflow: hidden;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
    flex-shrink: 0;
  }

  .artwork-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .explicit-badge {
    display: inline-block;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    opacity: 0.45;
    background-color: var(--text-primary, white);
    -webkit-mask: url('/explicit.svg') center / contain no-repeat;
    mask: url('/explicit.svg') center / contain no-repeat;
  }

  @media (max-width: 768px) {
    .bottom-info {
      right: 16px;
      bottom: 16px;
    }

    .artwork-thumb {
      width: 56px;
      height: 56px;
    }

    .track-title {
      font-size: 13px;
      max-width: 220px;
    }
  }
</style>
