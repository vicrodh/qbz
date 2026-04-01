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

  interface TunnelCloudSeed {
    angle: number;
    offset: number;
    speed: number;
    scale: number;
    hue: number;
    tilt: number;
  }

  interface TunnelWisp {
    depth: number;
    angle: number;
    speed: number;
    width: number;
    alpha: number;
    hue: number;
    spin: number;
    curve: number;
    phase: number;
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
  const SMOOTHING = 0.52;
  const FRAME_INTERVAL = getPanelFrameInterval('comet-flow');
  const BASE_RENDER_SCALE = FRAME_INTERVAL <= 20 ? 0.52 : FRAME_INTERVAL <= 34 ? 0.58 : 0.64;
  const smoothedData = new Float32Array(NUM_BARS);

  const MAX_WISPS = 28;
  const TARGET_WISPS = 10;
  const CLOUD_COUNT = 12;
  const cloudSeeds: TunnelCloudSeed[] = [];
  const wisps: TunnelWisp[] = [];

  let phase = 0;
  let lastRenderTime = 0;
  let previousBass = 0;
  let previousHigh = 0;
  let beatCooldown = 0;

  // Album art color extraction
  const DEFAULT_PALETTE = [356, 196, 280, 30]; // warm red, cyan, purple, orange
  let artPalette: number[] = [...DEFAULT_PALETTE];
  let paletteTarget: number[] = [...DEFAULT_PALETTE];
  let currentArtwork = '';

  // Darkest color from artwork for corner vignette
  let darkColor = { r: 8, g: 2, b: 12 };
  let darkColorTarget = { r: 8, g: 2, b: 12 };

  function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b);
    const l = (max + min) / 2;
    if (max === min) return [0, 0, l];
    const d = max - min;
    const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    let h = 0;
    if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
    else if (max === g) h = ((b - r) / d + 2) / 6;
    else h = ((r - g) / d + 4) / 6;
    return [h * 360, s, l];
  }

  function extractPalette(img: HTMLImageElement) {
    const size = 32;
    const sampleCanvas = document.createElement('canvas');
    sampleCanvas.width = size;
    sampleCanvas.height = size;
    const sCtx = sampleCanvas.getContext('2d');
    if (!sCtx) return;

    sCtx.drawImage(img, 0, 0, size, size);
    const data = sCtx.getImageData(0, 0, size, size).data;

    // Bucket hues into 12 sectors (30° each), weighted by saturation and away from gray
    const buckets = new Float32Array(12);
    const bucketHueSum = new Float32Array(12);
    const bucketCount = new Float32Array(12);

    for (let i = 0; i < data.length; i += 4) {
      const [h, s, l] = rgbToHsl(data[i], data[i + 1], data[i + 2]);
      if (s < 0.12 || l < 0.08 || l > 0.92) continue; // skip grays
      const bucket = Math.floor(h / 30) % 12;
      const weight = s * (0.5 + Math.abs(l - 0.5)); // prefer saturated, non-mid colors
      buckets[bucket] += weight;
      bucketHueSum[bucket] += h * weight;
      bucketCount[bucket] += weight;
    }

    // Pick top 4 buckets
    const indexed = Array.from(buckets).map((w, i) => ({ w, i }));
    indexed.sort((a, b) => b.w - a.w);

    const extracted: number[] = [];
    for (let j = 0; j < Math.min(4, indexed.length); j++) {
      const idx = indexed[j].i;
      if (bucketCount[idx] > 0) {
        extracted.push(Math.round(bucketHueSum[idx] / bucketCount[idx]));
      }
    }

    // Fill remaining slots with defaults if needed
    while (extracted.length < 4) {
      extracted.push(DEFAULT_PALETTE[extracted.length]);
    }

    paletteTarget = extracted;

    // Find darkest pixel with some saturation (not pure black)
    let darkestL = 1;
    let darkR = 8, darkG = 2, darkB = 12;
    for (let i = 0; i < data.length; i += 4) {
      const pr = data[i], pg = data[i + 1], pb = data[i + 2];
      const [, ds, dl] = rgbToHsl(pr, pg, pb);
      if (dl < darkestL && dl > 0.02 && ds > 0.05) {
        darkestL = dl;
        darkR = pr; darkG = pg; darkB = pb;
      }
    }
    // Clamp to very dark range so vignette stays subtle
    darkColorTarget = {
      r: Math.min(darkR, 40),
      g: Math.min(darkG, 40),
      b: Math.min(darkB, 40)
    };
  }

  function loadArtworkPalette(src: string) {
    if (!src || src === currentArtwork) return;
    currentArtwork = src;
    const img = new window.Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => extractPalette(img);
    img.onerror = () => { paletteTarget = [...DEFAULT_PALETTE]; darkColorTarget = { r: 8, g: 2, b: 12 }; };
    img.src = src;
  }

  // Smoothly interpolate palette hues toward target each frame
  function lerpPalette() {
    for (let i = 0; i < 4; i++) {
      let diff = paletteTarget[i] - artPalette[i];
      // Shortest path around hue circle
      if (diff > 180) diff -= 360;
      if (diff < -180) diff += 360;
      artPalette[i] = wrapHue(artPalette[i] + diff * 0.02);
    }
    // Lerp dark color
    darkColor.r += (darkColorTarget.r - darkColor.r) * 0.02;
    darkColor.g += (darkColorTarget.g - darkColor.g) * 0.02;
    darkColor.b += (darkColorTarget.b - darkColor.b) * 0.02;
  }

  // Get palette hue with time-based mutation
  function palHue(index: number, timeMs: number, drift: number = 18): number {
    const base = artPalette[index % 4];
    return wrapHue(base + Math.sin(timeMs * 0.0008 + index * 2.1) * drift);
  }

  function clamp01(value: number): number {
    return Math.max(0, Math.min(1, value));
  }

  function wrapHue(value: number): number {
    const hue = value % 360;
    return hue < 0 ? hue + 360 : hue;
  }

  function ensureCloudSeeds() {
    if (cloudSeeds.length === CLOUD_COUNT) return;
    cloudSeeds.length = 0;
    for (let seedIdx = 0; seedIdx < CLOUD_COUNT; seedIdx++) {
      cloudSeeds.push({
        angle: Math.random() * Math.PI * 2,
        offset: Math.random(),
        speed: 0.16 + Math.random() * 0.55,
        scale: 0.72 + Math.random() * 0.58,
        hue: Math.random() * 360,
        tilt: (Math.random() - 0.5) * Math.PI * 0.46,
      });
    }
  }

  function createWisp(intensity: number = 0.5): TunnelWisp {
    // Pick from album-derived palette with random spread
    const palIdx = Math.floor(Math.random() * 4);
    const baseHue = artPalette[palIdx];
    return {
      depth: Math.random() * 0.22,
      angle: Math.random() * Math.PI * 2,
      speed: 0.002 + Math.random() * 0.004 + intensity * 0.008,
      width: 2.8 + Math.random() * 4.5 + intensity * 5.0,
      alpha: 0.25 + Math.random() * 0.3 + intensity * 0.35,
      hue: wrapHue(baseHue + (Math.random() - 0.5) * 30),
      spin: (Math.random() - 0.5) * 0.012,
      curve: 0.05 + Math.random() * 0.14 + intensity * 0.09,
      phase: Math.random() * Math.PI * 2,
    };
  }

  function spawnWisps(count: number, intensity: number) {
    for (let idx = 0; idx < count; idx++) {
      wisps.push(createWisp(intensity));
    }
    while (wisps.length > MAX_WISPS) {
      wisps.shift();
    }
  }

  function primeWisps() {
    if (wisps.length >= TARGET_WISPS) return;
    spawnWisps(TARGET_WISPS - wisps.length, 0.28);
  }

  function getBassEnergy(): number {
    let sum = 0;
    for (let bandIdx = 0; bandIdx < 4; bandIdx++) sum += smoothedData[bandIdx];
    return sum / 4;
  }

  function getMidEnergy(): number {
    let sum = 0;
    for (let bandIdx = 4; bandIdx < 10; bandIdx++) sum += smoothedData[bandIdx];
    return sum / 6;
  }

  function getHighEnergy(): number {
    let sum = 0;
    for (let bandIdx = 10; bandIdx < NUM_BARS; bandIdx++) sum += smoothedData[bandIdx];
    return sum / 6;
  }

  function drawFeedbackWarp(
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

    const swirl = timeMs * 0.0009 + phase * 0.15;
    const driftRadius = minDim * (0.009 + bass * 0.045 + high * 0.03);
    const driftX = Math.cos(swirl * 1.42) * driftRadius;
    const driftY = Math.sin(swirl * 1.16) * driftRadius;
    const shear = Math.sin(timeMs * 0.001 + phase * 2.4) * 0.016;

    ctx.save();
    ctx.translate(centerX, centerY);
    ctx.rotate(0.0009 + (high - bass) * 0.018 + Math.sin(timeMs * 0.0014) * 0.006);
    ctx.transform(1, shear, -shear * 0.8, 1, 0, 0);
    const zoom = 1.008 + bass * 0.07 + high * 0.04;
    ctx.scale(zoom, zoom);
    ctx.translate(-centerX + driftX, -centerY + driftY);
    ctx.globalAlpha = 0.8 + bass * 0.12;
    ctx.drawImage(canvasRef, 0, 0, drawWidth, drawHeight);
    ctx.restore();

    const chromaOffset = minDim * (0.002 + high * 0.012 + bass * 0.008);

    ctx.save();
    ctx.globalCompositeOperation = 'screen';
    const chromaHueShift = Math.round(artPalette[0] - artPalette[1]) % 60;
    ctx.filter = `hue-rotate(${chromaHueShift}deg) saturate(${160 + bass * 130}%)`;
    ctx.globalAlpha = 0.14 + bass * 0.14;
    ctx.drawImage(canvasRef, chromaOffset, -chromaOffset * 0.7, drawWidth, drawHeight);
    ctx.restore();

    ctx.save();
    ctx.globalCompositeOperation = 'screen';
    ctx.filter = `hue-rotate(${-chromaHueShift}deg) saturate(${150 + high * 150}%)`;
    ctx.globalAlpha = 0.12 + high * 0.16;
    ctx.drawImage(canvasRef, -chromaOffset * 0.8, chromaOffset * 0.7, drawWidth, drawHeight);
    ctx.restore();

    ctx.filter = 'none';
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
  }

  function drawWatercolorClouds(
    centerX: number,
    centerY: number,
    minDim: number,
    timeMs: number,
    bass: number,
    mid: number,
    high: number
  ) {
    if (!ctx) return;
    ensureCloudSeeds();

    const redFlood = ctx.createRadialGradient(
      centerX,
      centerY,
      minDim * 0.05,
      centerX,
      centerY,
      minDim * 1.08
    );
    redFlood.addColorStop(0, `hsla(${palHue(0, timeMs, 22)}, 96%, 44%, ${0.18 + bass * 0.16})`);
    redFlood.addColorStop(0.45, `hsla(${palHue(1, timeMs, 18)}, 96%, 36%, ${0.16 + mid * 0.12})`);
    redFlood.addColorStop(1, 'rgba(24, 0, 8, 0)');
    ctx.globalCompositeOperation = 'screen';
    ctx.fillStyle = redFlood;
    ctx.fillRect(0, 0, minDim * 3, minDim * 3);

    for (let seedIdx = 0; seedIdx < cloudSeeds.length; seedIdx++) {
      const seed = cloudSeeds[seedIdx];
      const travel = (phase * (0.06 + seed.speed * 0.12) + seed.offset) % 1;
      const radial = minDim * (0.05 + travel * (0.36 + seed.scale * 0.16));
      const angle =
        seed.angle +
        phase * (0.44 + seed.speed * 0.34) +
        Math.sin(timeMs * 0.0007 + seed.offset * 8) * 0.52;

      const centerJitter = minDim * (0.012 + high * 0.032);
      const cx = centerX + Math.cos(angle) * radial + Math.cos(timeMs * 0.0013 + seedIdx) * centerJitter;
      const cy = centerY + Math.sin(angle) * radial + Math.sin(timeMs * 0.0011 + seedIdx * 1.4) * centerJitter;

      const radiusX = minDim * (0.17 + seed.scale * 0.26 + bass * 0.1 + travel * 0.12);
      const radiusY = minDim * (0.13 + seed.scale * 0.22 + high * 0.08 + travel * 0.1);
      const warmHue = wrapHue(palHue(0, timeMs, 24) + Math.sin(timeMs * 0.0013 + seed.offset * 7) * 18 + bass * 28);
      const coolHue = wrapHue(palHue(1, timeMs, 20) + Math.cos(timeMs * 0.0018 + seed.offset * 10) * 20 + high * 16);
      const mutantHue = wrapHue(palHue(2, timeMs, 16) + Math.sin(timeMs * 0.001 + seed.offset * 5) * 22);
      const hue = seedIdx % 5 === 0 ? coolHue : seedIdx % 7 === 0 ? mutantHue : warmHue;
      const saturation = 70 + high * 20;
      const lightness = 38 + travel * 18 + mid * 8;
      const cloudEnergy = clamp01((bass + mid + high) * 2.5);
      const alpha = (0.08 + (1 - travel) * (0.16 + bass * 0.18)) * (0.1 + cloudEnergy * 0.9);

      const cloudGradient = ctx.createRadialGradient(cx, cy, 0, cx, cy, radiusX * 1.25);
      cloudGradient.addColorStop(0, `hsla(${hue}, ${saturation}%, ${lightness + 8}%, ${alpha})`);
      cloudGradient.addColorStop(0.55, `hsla(${wrapHue(hue + 22)}, ${Math.max(24, saturation - 20)}%, ${lightness}%, ${alpha * 0.72})`);
      cloudGradient.addColorStop(1, 'rgba(0, 0, 0, 0)');

      ctx.save();
      ctx.translate(cx, cy);
      ctx.rotate(seed.tilt + Math.sin(timeMs * 0.0012 + seed.offset * 5) * 0.35);
      ctx.scale(1, radiusY / radiusX);
      ctx.fillStyle = cloudGradient;
      ctx.beginPath();
      ctx.arc(0, 0, radiusX, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
  }

  function drawCurvedTunnelWisps(
    centerX: number,
    centerY: number,
    minDim: number,
    timeMs: number,
    bass: number,
    mid: number,
    high: number,
    totalEnergy: number
  ) {
    if (!ctx) return;
    primeWisps();

    // Fade wisps nearly out when silent
    const energyGate = clamp01(totalEnergy * 3.0);
    ctx.globalCompositeOperation = 'lighter';
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    for (let wispIdx = wisps.length - 1; wispIdx >= 0; wispIdx--) {
      const wisp = wisps[wispIdx];
      wisp.depth += wisp.speed * (0.5 + bass * 2.2);
      wisp.angle += wisp.spin + Math.sin(timeMs * 0.0013 + wisp.phase) * 0.0018;
      wisp.alpha *= 0.993;

      if (wisp.depth > 1.15 || wisp.alpha < 0.025) {
        wisps[wispIdx] = createWisp(0.34 + high * 0.3);
        continue;
      }

      const depthCurve = Math.pow(wisp.depth, 0.9);
      const startRadius = minDim * (0.052 + depthCurve * 0.06);
      const midRadius = minDim * (0.16 + depthCurve * 0.3);
      const endRadius = minDim * (0.28 + depthCurve * 0.58);

      const dirX = Math.cos(wisp.angle);
      const dirY = Math.sin(wisp.angle);
      const perpX = -dirY;
      const perpY = dirX;

      const bend =
        Math.sin(timeMs * 0.003 + wisp.phase * 1.4 + wisp.depth * 8) *
        minDim *
        (wisp.curve + mid * 0.12 + high * 0.08);

      const sx = centerX + dirX * startRadius + perpX * bend * 0.18;
      const sy = centerY + dirY * startRadius + perpY * bend * 0.18;
      const mx = centerX + dirX * midRadius + perpX * bend;
      const my = centerY + dirY * midRadius + perpY * bend;
      const ex = centerX + dirX * endRadius + perpX * bend * 0.45;
      const ey = centerY + dirY * endRadius + perpY * bend * 0.45;

      const hue = wrapHue(wisp.hue + timeMs * 0.028 + Math.sin(timeMs * 0.0017 + wisp.phase) * 26);
      const saturation = 74 + high * 18;
      const lightness = 52 + bass * 20 + (1 - depthCurve) * 8;
      const alpha = clamp01(wisp.alpha * (0.45 + bass * 0.6 + high * 0.15) * (0.05 + energyGate * 0.95));

      const strokeGradient = ctx.createLinearGradient(sx, sy, ex, ey);
      strokeGradient.addColorStop(0, `hsla(${wrapHue(hue - 20)}, ${Math.max(30, saturation - 18)}%, ${lightness - 8}%, 0)`);
      strokeGradient.addColorStop(0.45, `hsla(${hue}, ${saturation}%, ${lightness}%, ${alpha})`);
      strokeGradient.addColorStop(1, `hsla(${wrapHue(hue + 18)}, ${saturation}%, ${lightness + 8}%, 0)`);

      ctx.strokeStyle = strokeGradient;
      const outerWidth = 2.4 + wisp.width + bass * 3.0 + high * 1.6;
      ctx.lineWidth = outerWidth;
      ctx.beginPath();
      ctx.moveTo(sx, sy);
      ctx.quadraticCurveTo(mx, my, ex, ey);
      ctx.stroke();

      ctx.strokeStyle = `hsla(${wrapHue(hue + 20)}, 100%, ${72 + high * 18}%, ${alpha * 0.75})`;
      ctx.lineWidth = Math.max(1.2, outerWidth * 0.24);
      ctx.beginPath();
      ctx.moveTo(sx, sy);
      ctx.quadraticCurveTo(mx, my, ex, ey);
      ctx.stroke();
    }
  }

  function drawWarpedTunnelRings(
    centerX: number,
    centerY: number,
    minDim: number,
    timeMs: number,
    bass: number,
    mid: number,
    high: number
  ) {
    if (!ctx) return;

    const ringEnergy = clamp01((bass + mid + high) * 2.5);
    const ringCount = 12;
    const tunnelSpin = timeMs * 0.00042 + phase * 0.03;
    ctx.globalCompositeOperation = 'screen';

    for (let ringIdx = 0; ringIdx < ringCount; ringIdx++) {
      const flow = (phase * 0.24 + ringIdx / ringCount + timeMs * 0.0001) % 1;
      const fade = Math.pow(1 - flow, 1.08);
      const radius = minDim * (0.08 + flow * (0.62 + bass * 0.22));
      const wobble = Math.sin(timeMs * 0.0022 + ringIdx * 1.2) * minDim * (0.015 + high * 0.04);

      const offsetX = Math.cos(tunnelSpin + ringIdx * 0.26) * wobble;
      const offsetY = Math.sin(tunnelSpin * 1.2 + ringIdx * 0.31) * wobble * (0.7 + mid * 0.5);
      const rx = radius * (1.08 + high * 0.3);
      const ry = radius * (0.65 + bass * 0.4);
      const rotation = tunnelSpin + ringIdx * 0.065 + Math.sin(timeMs * 0.0013 + ringIdx) * 0.18;

      const hue = ringIdx % 4 === 0
        ? wrapHue(palHue(1, timeMs, 18) + Math.cos(timeMs * 0.0018 + ringIdx) * 18 + high * 18)
        : ringIdx % 3 === 0
          ? wrapHue(palHue(2, timeMs, 14) + Math.sin(timeMs * 0.0012 + ringIdx * 0.9) * 16 + mid * 20)
          : wrapHue(palHue(0, timeMs, 14) + Math.sin(timeMs * 0.0015 + ringIdx * 0.7) * 14 + bass * 22);
      const alpha = (0.08 + fade * (0.24 + bass * 0.35)) * (0.05 + ringEnergy * 0.95);
      const width = 1.6 + fade * (2.6 + bass * 3.5);

      ctx.beginPath();
      ctx.ellipse(centerX + offsetX, centerY + offsetY, rx, ry, rotation, 0, Math.PI * 2);
      ctx.strokeStyle = `hsla(${hue}, 94%, ${44 + fade * 24}%, ${alpha})`;
      ctx.lineWidth = width;
      ctx.stroke();

      ctx.beginPath();
      ctx.ellipse(centerX + offsetX, centerY + offsetY, rx, ry, rotation, 0, Math.PI * 2);
      ctx.strokeStyle = `hsla(${wrapHue(hue + 22)}, 100%, ${66 + fade * 16}%, ${alpha * 0.58})`;
      ctx.lineWidth = Math.max(0.8, width * 0.33);
      ctx.stroke();
    }
  }

  function drawRefractiveVeil(drawWidth: number, drawHeight: number, timeMs: number, bass: number, high: number) {
    if (!ctx) return;

    const veilA = ctx.createLinearGradient(0, 0, drawWidth, drawHeight);
    veilA.addColorStop(0, `hsla(${palHue(0, timeMs, 28)}, 100%, 46%, ${0.08 + bass * 0.16})`);
    veilA.addColorStop(0.52, 'rgba(255, 48, 108, 0)');
    veilA.addColorStop(1, `hsla(${palHue(1, timeMs, 22)}, 100%, 50%, ${0.06 + high * 0.11})`);
    ctx.globalCompositeOperation = 'overlay';
    ctx.fillStyle = veilA;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    const veilB = ctx.createLinearGradient(drawWidth, 0, 0, drawHeight);
    veilB.addColorStop(0, `hsla(${palHue(2, timeMs, 18)}, 90%, 48%, ${0.04 + high * 0.08})`);
    veilB.addColorStop(0.5, 'rgba(28, 8, 24, 0)');
    veilB.addColorStop(1, `hsla(${palHue(3, timeMs, 15)}, 92%, 44%, ${0.06 + bass * 0.12})`);
    ctx.globalCompositeOperation = 'soft-light';
    ctx.fillStyle = veilB;
    ctx.fillRect(0, 0, drawWidth, drawHeight);
  }

  function drawAbyss(centerX: number, centerY: number, minDim: number, bass: number, totalEnergy: number) {
    if (!ctx) return;
    // Shrink black hole when silent, grow with bass
    const energyScale = clamp01(totalEnergy * 2.5);
    const abyssRadius = minDim * (0.04 + energyScale * 0.18 + bass * 0.08);
    const abyssAlpha = 0.3 + energyScale * 0.68;
    const abyss = ctx.createRadialGradient(centerX, centerY, minDim * 0.01, centerX, centerY, abyssRadius);
    abyss.addColorStop(0, `rgba(0, 0, 0, ${abyssAlpha})`);
    abyss.addColorStop(0.45, `rgba(8, 0, 22, ${(0.3 + energyScale * 0.46) + bass * 0.1})`);
    abyss.addColorStop(1, 'rgba(0, 0, 0, 0)');
    ctx.globalCompositeOperation = 'source-over';
    ctx.fillStyle = abyss;
    ctx.beginPath();
    ctx.arc(centerX, centerY, abyssRadius, 0, Math.PI * 2);
    ctx.fill();
  }

  function drawCornerVignettes(
    drawWidth: number,
    drawHeight: number,
    timeMs: number,
    bass: number
  ) {
    if (!ctx) return;

    const dr = Math.round(darkColor.r);
    const dg = Math.round(darkColor.g);
    const db = Math.round(darkColor.b);
    const maxDim = Math.max(drawWidth, drawHeight);
    // Base radius breathes gently with bass
    const baseRadius = maxDim * (0.38 + bass * 0.06);

    // Four corners with slight independent drift
    const corners = [
      { x: 0, y: 0 },                          // top-left
      { x: drawWidth, y: 0 },                   // top-right
      { x: 0, y: drawHeight },                  // bottom-left
      { x: drawWidth, y: drawHeight },           // bottom-right
    ];

    ctx.save();
    ctx.globalCompositeOperation = 'source-over';

    for (let ci = 0; ci < corners.length; ci++) {
      const corner = corners[ci];
      // Subtle per-corner drift so they don't feel static
      const drift = bass * maxDim * 0.012;
      const ox = Math.sin(timeMs * 0.0006 + ci * 1.57) * drift;
      const oy = Math.cos(timeMs * 0.0005 + ci * 2.1) * drift;
      const cx = corner.x + ox;
      const cy = corner.y + oy;

      // Radius pulses slightly with bass per corner (phase-shifted)
      const pulsePhase = timeMs * 0.0008 + ci * 1.2;
      const radiusPulse = 1 + Math.sin(pulsePhase) * 0.04 * (1 + bass * 2);
      const radius = baseRadius * radiusPulse;

      const grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, radius);
      // Subtle alpha — slightly stronger at the bottom corners for text readability
      const alphaBase = ci >= 2 ? 0.45 : 0.3;
      const alphaOuter = alphaBase + bass * 0.08;
      grad.addColorStop(0, `rgba(${dr}, ${dg}, ${db}, ${alphaOuter})`);
      grad.addColorStop(0.5, `rgba(${dr}, ${dg}, ${db}, ${alphaOuter * 0.45})`);
      grad.addColorStop(1, `rgba(${dr}, ${dg}, ${db}, 0)`);

      ctx.fillStyle = grad;
      ctx.fillRect(0, 0, drawWidth, drawHeight);
    }

    ctx.restore();
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

    const bass = getBassEnergy();
    const mid = getMidEnergy();
    const high = getHighEnergy();
    phase += 0.004 + bass * 0.022 + high * 0.01;
    lerpPalette();
    if (beatCooldown > 0) beatCooldown -= 1;

    drawFeedbackWarp(centerX, centerY, drawWidth, drawHeight, minDim, timestamp, bass, high);

    ctx.fillStyle = `hsla(${wrapHue(artPalette[0] - 10)}, 60%, 6%, ${0.09 + high * 0.03})`;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    const totalEnergy = (bass + mid + high) / 3;
    drawWatercolorClouds(centerX, centerY, minDim, timestamp, bass, mid, high);
    drawWarpedTunnelRings(centerX, centerY, minDim, timestamp, bass, mid, high);
    drawCurvedTunnelWisps(centerX, centerY, minDim, timestamp, bass, mid, high, totalEnergy);
    drawRefractiveVeil(drawWidth, drawHeight, timestamp, bass, high);
    drawAbyss(centerX, centerY, minDim, bass, totalEnergy);
    drawCornerVignettes(drawWidth, drawHeight, timestamp, bass);

    ctx.globalCompositeOperation = 'source-over';
    animationFrame = requestAnimationFrame(render);
  }

  async function init() {
    if (!canvasRef || isInitialized) return;

    ctx = canvasRef.getContext('2d');
    if (!ctx) return;
    isInitialized = true;
    ensureCloudSeeds();
    primeWisps();

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
    } catch (err) {
      console.error('[CometFlow] Failed to enable backend:', err);
    }

    unlisten = await listen<number[]>('viz:data', (event) => {
      const payload = event.payload;
      if (!Array.isArray(payload)) return;
      const bytes = new Uint8Array(payload);
      const floats = new Float32Array(bytes.buffer);
      if (floats.length !== NUM_BARS) return;

      for (let bandIdx = 0; bandIdx < NUM_BARS; bandIdx++) {
        smoothedData[bandIdx] = smoothedData[bandIdx] * SMOOTHING + floats[bandIdx] * (1 - SMOOTHING);
      }

      const bass = getBassEnergy();
      const high = getHighEnergy();
      const bassDelta = bass - previousBass;
      const highDelta = high - previousHigh;
      previousBass = bass;
      previousHigh = high;

      if (beatCooldown <= 0 && (bassDelta > 0.03 || highDelta > 0.03 || bass > 0.45)) {
        const intensity = clamp01(bass * 1.0 + high * 0.6 + Math.max(bassDelta, 0) * 4.5);
        spawnWisps(2 + Math.floor(intensity * 4), intensity);
        beatCooldown = 2;
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
    cloudSeeds.length = 0;
    wisps.length = 0;
    phase = 0;
    lastRenderTime = 0;
    previousBass = 0;
    previousHigh = 0;
    beatCooldown = 0;

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: false });
    } catch (err) {
      console.error('[CometFlow] Failed to disable backend:', err);
    }

    isInitialized = false;
  }

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

  $effect(() => {
    if (artwork) {
      loadArtworkPalette(artwork);
    }
  });
</script>

<div class="comet-flow-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="comet-flow-canvas"></canvas>

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
  .comet-flow-panel {
    position: absolute;
    inset: 0;
    opacity: 0;
    transition: opacity 240ms ease;
    z-index: 5;
    background: #000;
  }

  .comet-flow-panel.visible {
    opacity: 1;
  }

  .comet-flow-canvas {
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
    text-shadow: 0 1px 6px rgba(0, 0, 0, 0.4);
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
    color: var(--alpha-60, rgba(255, 255, 255, 0.6));
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
</style>
