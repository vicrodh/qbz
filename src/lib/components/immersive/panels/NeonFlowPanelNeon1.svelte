<script lang="ts">
  import { onMount } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { getPanelFrameInterval } from '$lib/immersive/fpsConfig';

  interface Props {
    enabled?: boolean;
    artwork?: string;
  }

  interface NeonGlyph {
    radius: number;
    angle: number;
    speed: number;
    size: number;
    rotation: number;
    spin: number;
    alpha: number;
    hue: number;
    kind: 'square' | 'diamond';
  }

  interface NeonBurstParticle {
    radius: number;
    angle: number;
    speed: number;
    size: number;
    alpha: number;
    hue: number;
    spin: number;
    rotation: number;
    kind: 'dot' | 'shard';
  }

  interface LaserRingState {
    rotation: number;
    angularVelocity: number;
    direction: 1 | -1;
    nextFlipAtMs: number;
    phaseOffset: number;
  }

  let { enabled = true, artwork = '' }: Props = $props();

  let canvasRef: HTMLCanvasElement | null = $state(null);
  let ctx: CanvasRenderingContext2D | null = null;
  let animationFrame: number | null = null;
  let unlisten: UnlistenFn | null = null;
  let isInitialized = false;

  const NUM_BARS = 16;
  const SMOOTHING = 0.72;
  const FRAME_INTERVAL = getPanelFrameInterval('neon-flow');
  const MODE_SWITCH_MS = 11000;

  // Render intentionally below native resolution to keep this effect lightweight.
  const BASE_RENDER_SCALE = FRAME_INTERVAL <= 20 ? 0.5 : FRAME_INTERVAL <= 34 ? 0.56 : 0.62;
  const smoothedData = new Float32Array(NUM_BARS);
  const glyphs: NeonGlyph[] = [];
  const burstParticles: NeonBurstParticle[] = [];
  const LASER_RING_COUNT = 3;
  const laserRings: LaserRingState[] = [];
  const MAX_GLYPHS = 24;
  const MAX_BURST_PARTICLES = 52;

  let lastRenderTime = 0;
  let phase = 0;
  let beatCooldown = 0;
  let highCooldown = 0;
  let previousBass = 0;
  let previousHigh = 0;
  let lastWidth = 1280;
  let lastHeight = 720;

  let baseHue = $state(300);
  let accentHue = $state(192);

  function createLaserRingState(ringIndex: number): LaserRingState {
    // Angular velocity in radians per millisecond.
    const baseVel = 0.00062 - ringIndex * 0.00016;
    return {
      rotation: Math.random() * Math.PI * 2,
      angularVelocity: Math.max(0.00016, baseVel + Math.random() * 0.0001),
      direction: Math.random() > 0.5 ? 1 : -1,
      nextFlipAtMs: 900 + Math.random() * 2200,
      phaseOffset: ringIndex * 0.73 + Math.random() * 0.28,
    };
  }

  function ensureLaserRings() {
    if (laserRings.length === LASER_RING_COUNT) return;
    laserRings.length = 0;
    for (let ringIndex = 0; ringIndex < LASER_RING_COUNT; ringIndex++) {
      laserRings.push(createLaserRingState(ringIndex));
    }
  }

  function clamp01(value: number): number {
    return Math.max(0, Math.min(1, value));
  }

  function wrapHue(value: number): number {
    const hue = value % 360;
    return hue < 0 ? hue + 360 : hue;
  }

  function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
    const rr = r / 255;
    const gg = g / 255;
    const bb = b / 255;
    const max = Math.max(rr, gg, bb);
    const min = Math.min(rr, gg, bb);
    const delta = max - min;
    let h = 0;
    const l = (max + min) / 2;
    if (delta > 0) {
      if (max === rr) h = ((gg - bb) / delta + (gg < bb ? 6 : 0)) / 6;
      else if (max === gg) h = ((bb - rr) / delta + 2) / 6;
      else h = ((rr - gg) / delta + 4) / 6;
    }
    const s = delta === 0 ? 0 : delta / (1 - Math.abs(2 * l - 1));
    return [h * 360, s, l];
  }

  function extractPalette(imgSrc: string) {
    if (!imgSrc) return;

    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      const sampleCanvas = document.createElement('canvas');
      sampleCanvas.width = 14;
      sampleCanvas.height = 14;
      const sampleCtx = sampleCanvas.getContext('2d');
      if (!sampleCtx) return;

      sampleCtx.drawImage(img, 0, 0, 14, 14);
      const pixels = sampleCtx.getImageData(0, 0, 14, 14).data;
      const candidates: Array<{ hue: number; sat: number; lum: number }> = [];

      for (let pixelIdx = 0; pixelIdx < pixels.length; pixelIdx += 4) {
        const red = pixels[pixelIdx];
        const green = pixels[pixelIdx + 1];
        const blue = pixels[pixelIdx + 2];
        const [hue, sat, lum] = rgbToHsl(red, green, blue);
        if (sat > 0.2 && lum > 0.12 && lum < 0.88) {
          candidates.push({ hue, sat, lum });
        }
      }

      if (candidates.length === 0) return;
      candidates.sort((a, b) => b.sat - a.sat);
      baseHue = Math.round(candidates[0].hue);
      accentHue = Math.round(candidates[Math.max(1, Math.floor(candidates.length * 0.35))].hue);
    };
    img.src = imgSrc;
  }

  $effect(() => {
    if (artwork) extractPalette(artwork);
  });

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

  function getDominantBandAngle(fromBand: number, toBand: number, phaseOffset: number = 0): number {
    const startBand = Math.max(0, Math.min(NUM_BARS - 1, fromBand));
    const endBand = Math.max(startBand, Math.min(NUM_BARS - 1, toBand));
    let dominantBand = startBand;
    let dominantValue = -1;
    for (let bandIdx = startBand; bandIdx <= endBand; bandIdx++) {
      const value = smoothedData[bandIdx];
      if (value > dominantValue) {
        dominantValue = value;
        dominantBand = bandIdx;
      }
    }

    const sectionSize = Math.max(1, endBand - startBand + 1);
    const sectionPosition = (dominantBand - startBand + 0.5) / sectionSize;
    const ringRotation = laserRings.length > 0 ? laserRings[0].rotation : phase * 0.32;
    return ringRotation + phaseOffset + sectionPosition * Math.PI * 2;
  }

  function getLaserBandHue(bandIdx: number, ringIndex: number, spokeIdx: number, timeMs: number): number {
    // Faster + wider hue motion, with band-aware color families.
    const spin = timeMs * 0.095;
    const waveA = Math.sin(timeMs * 0.0028 + ringIndex * 1.4 + spokeIdx * 0.31) * 34;
    const waveB = Math.cos(timeMs * 0.0019 + bandIdx * 0.8) * 22;

    if (bandIdx <= 3) {
      // Bass: warm reds/oranges.
      return wrapHue(10 + bandIdx * 12 + spin * 1.08 + waveA + waveB * 0.4);
    }
    if (bandIdx >= 10) {
      // Highs: cyan/blue range.
      return wrapHue(188 + (bandIdx - 10) * 13 + spin * 1.22 + waveA * 0.75 + waveB * 0.55);
    }
    // Mids: wider bridge through greens/magentas.
    return wrapHue(62 + (bandIdx - 4) * 24 + spin + waveA * 0.9 + waveB);
  }

  function spawnGlyph(intensity: number) {
    const minDim = Math.min(lastWidth, lastHeight);
    glyphs.push({
      radius: minDim * (0.07 + Math.random() * 0.18),
      angle: Math.random() * Math.PI * 2,
      speed: 0.0018 + Math.random() * 0.006 + intensity * 0.008,
      size: minDim * (0.03 + Math.random() * 0.06 + intensity * 0.04),
      rotation: Math.random() * Math.PI * 2,
      spin: (Math.random() - 0.5) * 0.045,
      alpha: 0.2 + intensity * 0.5,
      hue: (baseHue + Math.random() * 140) % 360,
      kind: Math.random() > 0.5 ? 'square' : 'diamond',
    });

    if (glyphs.length > MAX_GLYPHS) glyphs.shift();
  }

  function spawnBurstParticles(
    intensity: number,
    profile: 'bass' | 'high' | 'soft',
    forcedCount?: number,
    baseAngle?: number,
    spread: number = Math.PI * 0.8
  ) {
    const minDim = Math.min(lastWidth, lastHeight);
    const count =
      forcedCount ??
      (profile === 'bass'
        ? 2 + Math.floor(intensity * 4)
        : profile === 'high'
          ? 3 + Math.floor(intensity * 6)
          : 1 + Math.floor(intensity * 2));
    for (let idx = 0; idx < count; idx++) {
      const angle =
        baseAngle === undefined
          ? Math.random() * Math.PI * 2
          : baseAngle + (Math.random() - 0.5) * spread;
      const isBass = profile === 'bass';
      const isHigh = profile === 'high';
      burstParticles.push({
        radius: minDim * (0.008 + Math.random() * 0.012),
        angle,
        speed: isBass
          ? 0.0014 + Math.random() * 0.0024 + intensity * 0.0024
          : isHigh
            ? 0.0022 + Math.random() * 0.0038 + intensity * 0.003
            : 0.0012 + Math.random() * 0.0022 + intensity * 0.0018,
        size: isBass
          ? minDim * (0.004 + Math.random() * 0.012 + intensity * 0.009)
          : isHigh
            ? minDim * (0.0024 + Math.random() * 0.006 + intensity * 0.0045)
            : minDim * (0.003 + Math.random() * 0.006 + intensity * 0.003),
        alpha: isBass
          ? 0.18 + intensity * 0.38 + Math.random() * 0.16
          : isHigh
            ? 0.14 + intensity * 0.28 + Math.random() * 0.14
            : 0.12 + intensity * 0.2 + Math.random() * 0.1,
        hue: isBass
          ? wrapHue(8 + Math.random() * 42)
          : isHigh
            ? wrapHue(188 + Math.random() * 68)
            : wrapHue(baseHue + Math.random() * 150),
        spin: (Math.random() - 0.5) * 0.08,
        rotation: Math.random() * Math.PI * 2,
        kind: isBass ? (Math.random() > 0.5 ? 'dot' : 'shard') : (Math.random() > 0.18 ? 'dot' : 'shard'),
      });
    }
    while (burstParticles.length > MAX_BURST_PARTICLES) {
      burstParticles.shift();
    }
  }

  async function init() {
    if (!canvasRef || isInitialized) return;

    ctx = canvasRef.getContext('2d');
    if (!ctx) return;
    isInitialized = true;
    ensureLaserRings();

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: true });
    } catch (err) {
      console.error('[NeonFlow] Failed to enable backend:', err);
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

      if (beatCooldown > 0) beatCooldown -= 1;
      if (highCooldown > 0) highCooldown -= 1;

      // Kick/bass transient burst.
      if (beatCooldown <= 0 && (bassDelta > 0.065 || bass > 0.66)) {
        const intensity = clamp01(bass * 0.8 + bassDelta * 3.4);
        const bassAngle = getDominantBandAngle(0, 3, 0);
        spawnGlyph(intensity);
        spawnBurstParticles(intensity, 'bass', undefined, bassAngle, Math.PI * 0.42);
        beatCooldown = 4;
      }

      // Hi-hat / high-frequency transient burst.
      if (highCooldown <= 0 && (highDelta > 0.05 || high > 0.62)) {
        const intensity = clamp01(high * 0.9 + highDelta * 4.4);
        const highAngle = getDominantBandAngle(10, 15, Math.PI * 0.18);
        spawnBurstParticles(intensity, 'high', undefined, highAngle, Math.PI * 0.72);
        highCooldown = 3;
      }
    });

    render(0);
  }

  function drawMirroredRibbons(
    centerX: number,
    centerY: number,
    minDim: number,
    mode: number,
    timeMs: number
  ) {
    if (!ctx) return;
    const layerAlpha = mode === 1 ? 0.5 : 0.4;
    for (let rowIdx = 0; rowIdx < 8; rowIdx++) {
      const amp = smoothedData[rowIdx * 2];
      const spread = minDim * (0.06 + amp * 0.35);
      const wave = Math.sin(phase * (1.3 + rowIdx * 0.1) + rowIdx) * minDim * 0.055;
      const yOffset = (rowIdx - 3.5) * minDim * 0.11 + wave;
      const y = centerY + yOffset;
      const hue = (accentHue + rowIdx * 18 + timeMs * 0.02) % 360;
      const width = 0.8 + amp * 2.2;

      ctx.beginPath();
      ctx.moveTo(centerX - spread, y);
      ctx.quadraticCurveTo(centerX, y - minDim * (0.04 + amp * 0.16), centerX + spread, y);
      ctx.strokeStyle = `hsla(${hue}, 96%, ${44 + amp * 24}%, ${layerAlpha})`;
      ctx.lineWidth = width;
      ctx.stroke();

      if (mode !== 2) {
        const mirroredY = centerY - yOffset;
        ctx.beginPath();
        ctx.moveTo(centerX - spread, mirroredY);
        ctx.quadraticCurveTo(centerX, mirroredY + minDim * (0.04 + amp * 0.16), centerX + spread, mirroredY);
        ctx.strokeStyle = `hsla(${(hue + 26) % 360}, 96%, ${44 + amp * 24}%, ${layerAlpha * 0.9})`;
        ctx.lineWidth = width * 0.95;
        ctx.stroke();
      }
    }
  }

  function drawTunnelStreaks(
    centerX: number,
    centerY: number,
    minDim: number,
    mode: number,
    timeMs: number,
    bass: number,
    high: number
  ) {
    if (!ctx) return;
    const stretch = mode === 2 ? 1.2 : 1;
    for (let ringIndex = 0; ringIndex < laserRings.length; ringIndex++) {
      const ring = laserRings[ringIndex];
      const spokeCount = (mode === 0 ? 8 : 6) - ringIndex;
      const ringAlpha = 1 - ringIndex * 0.2;
      const ringScale = 1 - ringIndex * 0.08;

      for (let spokeIdx = 0; spokeIdx < spokeCount; spokeIdx++) {
        const bandIdx = (spokeIdx + ringIndex * 3) % NUM_BARS;
        const ampRaw = smoothedData[bandIdx];
        const amp = Math.pow(Math.max(0, ampRaw), 0.62);
        const angle = ring.rotation + ring.phaseOffset + (spokeIdx / spokeCount) * Math.PI * 2;

        // Three concentric laser rings with independent cadence.
        const muzzle = minDim * (0.014 + ringIndex * 0.018 + bass * 0.012);
        const maxReach = minDim * (0.34 + ringIndex * 0.065 + amp * 0.52 * stretch + high * 0.08);
        const burstSpeed = (0.00022 + bass * 0.00095 + amp * 0.00085) * (1 - ringIndex * 0.14);
        const cycle = (timeMs * burstSpeed + spokeIdx * 0.13 + ring.phaseOffset * 0.4) % 1;
        const front = 0.16 + Math.pow(cycle, 0.92) * 0.84;
        const tail = Math.max(0, front - (0.56 + amp * 0.22 + bass * 0.12 + ringIndex * 0.03));
        const inner = muzzle + maxReach * tail;
        const outer = muzzle + maxReach * Math.min(1, front + 0.14 + amp * 0.08);
        if (outer - inner < minDim * 0.004) continue;

        const x1 = centerX + Math.cos(angle) * inner;
        const y1 = centerY + Math.sin(angle) * inner;
        const x2 = centerX + Math.cos(angle) * outer;
        const y2 = centerY + Math.sin(angle) * outer;

        const hue = getLaserBandHue(bandIdx, ringIndex, spokeIdx, timeMs);
        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.lineTo(x2, y2);
        ctx.strokeStyle = `hsla(${hue}, 100%, ${52 + amp * 24}%, ${(0.18 + amp * 0.32 + bass * 0.12) * ringAlpha})`;
        ctx.lineWidth = (1.5 + amp * 1.8 + bass * 0.55) * ringScale;
        ctx.stroke();

        // Bright core for classic sci-fi laser look.
        const coreHue = wrapHue(hue + 24 + Math.sin(timeMs * 0.0031 + bandIdx) * 16);
        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.lineTo(x2, y2);
        ctx.strokeStyle = `hsla(${coreHue}, 100%, ${84 + amp * 12}%, ${(0.28 + amp * 0.24) * ringAlpha})`;
        ctx.lineWidth = (0.68 + amp * 0.7) * ringScale;
        ctx.stroke();

        // Tip accent reinforces forward projectile feel.
        const tipInner = Math.max(inner, outer - minDim * (0.03 + amp * 0.038));
        const tx1 = centerX + Math.cos(angle) * tipInner;
        const ty1 = centerY + Math.sin(angle) * tipInner;
        const tipHue = wrapHue(hue + 46 + Math.cos(timeMs * 0.0024 + spokeIdx + ringIndex) * 22);
        ctx.beginPath();
        ctx.moveTo(tx1, ty1);
        ctx.lineTo(x2, y2);
        ctx.strokeStyle = `hsla(${tipHue}, 100%, ${76 + amp * 14}%, ${(0.2 + amp * 0.25) * ringAlpha})`;
        ctx.lineWidth = (0.6 + amp * 0.65) * ringScale;
        ctx.stroke();
      }
    }
  }

  function updateLaserRingMotion(timestamp: number, bass: number, high: number, deltaMs: number) {
    ensureLaserRings();
    const dt = Math.max(8, Math.min(48, deltaMs));
    for (let ringIndex = 0; ringIndex < laserRings.length; ringIndex++) {
      const ring = laserRings[ringIndex];

      if (timestamp >= ring.nextFlipAtMs) {
        if (Math.random() < 0.78) {
          ring.direction = ring.direction === 1 ? -1 : 1;
        }
        const baseVelocity = 0.00058 - ringIndex * 0.00015;
        ring.angularVelocity = Math.max(
          0.00014,
          baseVelocity + Math.random() * 0.00012 + bass * 0.00012 + high * 0.00008
        );
        ring.nextFlipAtMs = timestamp + 1000 + Math.random() * (2200 + ringIndex * 700);
      }

      const jitter = Math.sin(timestamp * 0.0014 + ring.phaseOffset * 4.1) * 0.000015 * dt;
      ring.rotation += ring.direction * ring.angularVelocity * dt + jitter;
    }
  }

  function drawGlyphs(centerX: number, centerY: number, minDim: number, timeMs: number) {
    if (!ctx) return;
    for (let glyphIdx = glyphs.length - 1; glyphIdx >= 0; glyphIdx--) {
      const glyph = glyphs[glyphIdx];
      glyph.radius += minDim * (0.0008 + glyph.speed);
      glyph.rotation += glyph.spin;
      glyph.alpha *= 0.985;
      if (glyph.alpha < 0.015 || glyph.radius > minDim * 0.95) {
        glyphs.splice(glyphIdx, 1);
        continue;
      }

      const angle = glyph.angle + phase * 0.32;
      const x = centerX + Math.cos(angle) * glyph.radius;
      const y = centerY + Math.sin(angle) * glyph.radius;
      const hue = (glyph.hue + timeMs * 0.02) % 360;

      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(glyph.rotation);
      ctx.strokeStyle = `hsla(${hue}, 100%, 64%, ${glyph.alpha})`;
      ctx.lineWidth = 1 + glyph.alpha * 1.5;
      if (glyph.kind === 'square') {
        ctx.strokeRect(-glyph.size * 0.5, -glyph.size * 0.5, glyph.size, glyph.size);
      } else {
        ctx.rotate(Math.PI / 4);
        ctx.strokeRect(-glyph.size * 0.45, -glyph.size * 0.45, glyph.size * 0.9, glyph.size * 0.9);
      }
      ctx.restore();
    }
  }

  function drawBurstParticles(centerX: number, centerY: number, minDim: number, timeMs: number) {
    if (!ctx) return;
    for (let particleIdx = burstParticles.length - 1; particleIdx >= 0; particleIdx--) {
      const particle = burstParticles[particleIdx];
      particle.radius += minDim * particle.speed;
      particle.alpha *= 0.965;
      particle.rotation += particle.spin;
      if (particle.alpha < 0.018 || particle.radius > minDim * 0.92) {
        burstParticles.splice(particleIdx, 1);
        continue;
      }

      const drift = Math.sin(phase * 1.8 + particle.angle * 2.1) * minDim * 0.018;
      const px = centerX + Math.cos(particle.angle) * particle.radius + Math.cos(particle.angle + Math.PI / 2) * drift;
      const py = centerY + Math.sin(particle.angle) * particle.radius + Math.sin(particle.angle + Math.PI / 2) * drift;
      const hue = (particle.hue + timeMs * 0.028) % 360;

      if (particle.kind === 'dot') {
        ctx.beginPath();
        ctx.arc(px, py, particle.size * (0.8 + particle.alpha * 0.5), 0, Math.PI * 2);
        ctx.fillStyle = `hsla(${hue}, 100%, ${62 + particle.alpha * 16}%, ${particle.alpha})`;
        ctx.fill();
      } else {
        const shardLen = particle.size * (1.2 + particle.alpha);
        ctx.save();
        ctx.translate(px, py);
        ctx.rotate(particle.rotation);
        ctx.beginPath();
        ctx.moveTo(-shardLen * 0.5, 0);
        ctx.lineTo(shardLen * 0.5, 0);
        ctx.strokeStyle = `hsla(${hue}, 100%, ${68 + particle.alpha * 18}%, ${particle.alpha * 0.95})`;
        ctx.lineWidth = 0.6 + particle.alpha * 0.9;
        ctx.stroke();
        ctx.restore();
      }
    }
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

    lastWidth = drawWidth;
    lastHeight = drawHeight;
    const centerX = drawWidth * 0.5;
    const centerY = drawHeight * 0.5;
    const minDim = Math.min(drawWidth, drawHeight);

    const bass = getBassEnergy();
    const mid = getMidEnergy();
    const high = getHighEnergy();
    const mode = Math.floor(timestamp / MODE_SWITCH_MS) % 3;

    phase += 0.0038 + bass * 0.012;
    updateLaserRingMotion(timestamp, bass, high, delta);

    // Feedback warp pass.
    ctx.save();
    ctx.translate(centerX, centerY);
    ctx.rotate(0.0008 + (high - mid) * 0.01);
    const zoom = 1.006 + bass * 0.028;
    ctx.scale(zoom, zoom);
    ctx.translate(-centerX, -centerY);
    ctx.globalAlpha = 0.88;
    ctx.drawImage(canvasRef, 0, 0, drawWidth, drawHeight);
    ctx.restore();

    // Decay/fade pass.
    ctx.fillStyle = `rgba(0, 0, 0, ${0.07 + (mode === 2 ? 0.02 : 0.01)})`;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    // Ambient color wash.
    const bgGradient = ctx.createRadialGradient(
      centerX,
      centerY,
      minDim * 0.05,
      centerX,
      centerY,
      minDim * 0.85
    );
    bgGradient.addColorStop(0, `hsla(${(baseHue + timestamp * 0.014) % 360}, 92%, 12%, ${0.2 + bass * 0.22})`);
    bgGradient.addColorStop(0.5, `hsla(${(accentHue + timestamp * 0.018) % 360}, 92%, 10%, ${0.08 + mid * 0.15})`);
    bgGradient.addColorStop(1, 'rgba(0, 0, 0, 0)');
    ctx.fillStyle = bgGradient;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    ctx.globalCompositeOperation = 'screen';
    drawTunnelStreaks(centerX, centerY, minDim, mode, timestamp, bass, high);
    drawMirroredRibbons(centerX, centerY, minDim, mode, timestamp);
    drawBurstParticles(centerX, centerY, minDim, timestamp);
    drawGlyphs(centerX, centerY, minDim, timestamp);

    // Core bloom.
    const coreRadius = minDim * (0.05 + bass * 0.08);
    const coreGradient = ctx.createRadialGradient(centerX, centerY, 0, centerX, centerY, coreRadius * 3.6);
    coreGradient.addColorStop(0, `hsla(${(accentHue + timestamp * 0.05) % 360}, 100%, 74%, ${0.46 + bass * 0.34})`);
    coreGradient.addColorStop(0.5, `hsla(${(baseHue + timestamp * 0.03) % 360}, 98%, 54%, ${0.18 + high * 0.22})`);
    coreGradient.addColorStop(1, 'rgba(0, 0, 0, 0)');
    ctx.fillStyle = coreGradient;
    ctx.beginPath();
    ctx.arc(centerX, centerY, coreRadius * 3.6, 0, Math.PI * 2);
    ctx.fill();

    ctx.globalCompositeOperation = 'source-over';
    animationFrame = requestAnimationFrame(render);
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

    glyphs.length = 0;
    burstParticles.length = 0;
    laserRings.length = 0;
    smoothedData.fill(0);
    previousBass = 0;
    previousHigh = 0;
    beatCooldown = 0;
    highCooldown = 0;
    phase = 0;
    lastRenderTime = 0;

    try {
      await invoke('v2_set_visualizer_enabled', { enabled: false });
    } catch (err) {
      console.error('[NeonFlow] Failed to disable backend:', err);
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
</script>

<div class="neon-flow-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="neon-flow-canvas"></canvas>
</div>

<style>
  .neon-flow-panel {
    position: absolute;
    inset: 0;
    opacity: 0;
    transition: opacity 240ms ease;
    z-index: 5;
    background: #000;
  }

  .neon-flow-panel.visible {
    opacity: 1;
  }

  .neon-flow-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
