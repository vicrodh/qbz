<script lang="ts">
  import { onMount } from 'svelte';
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
    const colorFamilies = [2, 8, 14, 20, 190, 206];
    const family = colorFamilies[Math.floor(Math.random() * colorFamilies.length)];
    return {
      depth: Math.random() * 0.22,
      angle: Math.random() * Math.PI * 2,
      speed: 0.0018 + Math.random() * 0.003 + intensity * 0.0045,
      width: 2.4 + Math.random() * 4.2 + intensity * 2.8,
      alpha: 0.2 + Math.random() * 0.28 + intensity * 0.22,
      hue: wrapHue(family + (Math.random() - 0.5) * 16),
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
    const zoom = 1.008 + bass * 0.048 + high * 0.026;
    ctx.scale(zoom, zoom);
    ctx.translate(-centerX + driftX, -centerY + driftY);
    ctx.globalAlpha = 0.8 + bass * 0.12;
    ctx.drawImage(canvasRef, 0, 0, drawWidth, drawHeight);
    ctx.restore();

    const chromaOffset = minDim * (0.002 + high * 0.012 + bass * 0.008);

    ctx.save();
    ctx.globalCompositeOperation = 'screen';
    ctx.filter = `hue-rotate(22deg) saturate(${160 + bass * 130}%)`;
    ctx.globalAlpha = 0.14 + bass * 0.14;
    ctx.drawImage(canvasRef, chromaOffset, -chromaOffset * 0.7, drawWidth, drawHeight);
    ctx.restore();

    ctx.save();
    ctx.globalCompositeOperation = 'screen';
    ctx.filter = `hue-rotate(-26deg) saturate(${150 + high * 150}%)`;
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
    redFlood.addColorStop(0, `hsla(${wrapHue(4 + timeMs * 0.022)}, 96%, 44%, ${0.18 + bass * 0.16})`);
    redFlood.addColorStop(0.45, `hsla(${wrapHue(356 + timeMs * 0.017)}, 96%, 36%, ${0.16 + mid * 0.12})`);
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
      const warmHue = wrapHue(356 + Math.sin(timeMs * 0.0013 + seed.offset * 7) * 18 + bass * 28);
      const coolHue = wrapHue(196 + Math.cos(timeMs * 0.0018 + seed.offset * 10) * 20 + high * 16);
      const hue = seedIdx % 5 === 0 ? coolHue : warmHue;
      const saturation = 70 + high * 20;
      const lightness = 38 + travel * 18 + mid * 8;
      const alpha = 0.08 + (1 - travel) * (0.16 + bass * 0.18);

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
    high: number
  ) {
    if (!ctx) return;
    primeWisps();

    ctx.globalCompositeOperation = 'lighter';
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';

    for (let wispIdx = wisps.length - 1; wispIdx >= 0; wispIdx--) {
      const wisp = wisps[wispIdx];
      wisp.depth += wisp.speed * (0.44 + bass * 1.45);
      wisp.angle += wisp.spin + Math.sin(timeMs * 0.0013 + wisp.phase) * 0.0018;
      wisp.alpha *= 0.995;

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
        (wisp.curve + mid * 0.06 + high * 0.04);

      const sx = centerX + dirX * startRadius + perpX * bend * 0.18;
      const sy = centerY + dirY * startRadius + perpY * bend * 0.18;
      const mx = centerX + dirX * midRadius + perpX * bend;
      const my = centerY + dirY * midRadius + perpY * bend;
      const ex = centerX + dirX * endRadius + perpX * bend * 0.45;
      const ey = centerY + dirY * endRadius + perpY * bend * 0.45;

      const hue = wrapHue(wisp.hue + timeMs * 0.028 + Math.sin(timeMs * 0.0017 + wisp.phase) * 26);
      const saturation = 74 + high * 18;
      const lightness = 52 + bass * 20 + (1 - depthCurve) * 8;
      const alpha = clamp01(wisp.alpha * (0.42 + bass * 0.4));

      const strokeGradient = ctx.createLinearGradient(sx, sy, ex, ey);
      strokeGradient.addColorStop(0, `hsla(${wrapHue(hue - 20)}, ${Math.max(30, saturation - 18)}%, ${lightness - 8}%, 0)`);
      strokeGradient.addColorStop(0.45, `hsla(${hue}, ${saturation}%, ${lightness}%, ${alpha})`);
      strokeGradient.addColorStop(1, `hsla(${wrapHue(hue + 18)}, ${saturation}%, ${lightness + 8}%, 0)`);

      ctx.strokeStyle = strokeGradient;
      const outerWidth = 2.2 + wisp.width + bass * 1.4 + high * 0.9;
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

    const ringCount = 12;
    const tunnelSpin = timeMs * 0.00042 + phase * 0.03;
    ctx.globalCompositeOperation = 'screen';

    for (let ringIdx = 0; ringIdx < ringCount; ringIdx++) {
      const flow = (phase * 0.24 + ringIdx / ringCount + timeMs * 0.0001) % 1;
      const fade = Math.pow(1 - flow, 1.08);
      const radius = minDim * (0.08 + flow * (0.62 + bass * 0.08));
      const wobble = Math.sin(timeMs * 0.0022 + ringIdx * 1.2) * minDim * (0.01 + high * 0.02);

      const offsetX = Math.cos(tunnelSpin + ringIdx * 0.26) * wobble;
      const offsetY = Math.sin(tunnelSpin * 1.2 + ringIdx * 0.31) * wobble * (0.7 + mid * 0.4);
      const rx = radius * (1.08 + high * 0.16);
      const ry = radius * (0.7 + bass * 0.2);
      const rotation = tunnelSpin + ringIdx * 0.065 + Math.sin(timeMs * 0.0013 + ringIdx) * 0.18;

      const hue = ringIdx % 4 === 0
        ? wrapHue(198 + Math.cos(timeMs * 0.0018 + ringIdx) * 18 + high * 18)
        : wrapHue(356 + Math.sin(timeMs * 0.0015 + ringIdx * 0.7) * 14 + bass * 22);
      const alpha = 0.06 + fade * (0.2 + bass * 0.2);
      const width = 1.4 + fade * (2.2 + bass * 2.2);

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
    veilA.addColorStop(0, `hsla(${wrapHue(356 + timeMs * 0.028)}, 100%, 46%, ${0.08 + bass * 0.16})`);
    veilA.addColorStop(0.52, 'rgba(255, 48, 108, 0)');
    veilA.addColorStop(1, `hsla(${wrapHue(204 + timeMs * 0.022)}, 100%, 50%, ${0.06 + high * 0.11})`);
    ctx.globalCompositeOperation = 'overlay';
    ctx.fillStyle = veilA;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    const veilB = ctx.createLinearGradient(drawWidth, 0, 0, drawHeight);
    veilB.addColorStop(0, `hsla(${wrapHue(196 + timeMs * 0.018)}, 90%, 48%, ${0.04 + high * 0.08})`);
    veilB.addColorStop(0.5, 'rgba(28, 8, 24, 0)');
    veilB.addColorStop(1, `hsla(${wrapHue(10 + timeMs * 0.015)}, 92%, 44%, ${0.06 + bass * 0.12})`);
    ctx.globalCompositeOperation = 'soft-light';
    ctx.fillStyle = veilB;
    ctx.fillRect(0, 0, drawWidth, drawHeight);
  }

  function drawAbyss(centerX: number, centerY: number, minDim: number, bass: number) {
    if (!ctx) return;
    const abyssRadius = minDim * (0.22 + bass * 0.08);
    const abyss = ctx.createRadialGradient(centerX, centerY, minDim * 0.02, centerX, centerY, abyssRadius);
    abyss.addColorStop(0, 'rgba(0, 0, 0, 0.98)');
    abyss.addColorStop(0.45, `rgba(8, 0, 22, ${0.76 + bass * 0.1})`);
    abyss.addColorStop(1, 'rgba(0, 0, 0, 0)');
    ctx.globalCompositeOperation = 'source-over';
    ctx.fillStyle = abyss;
    ctx.beginPath();
    ctx.arc(centerX, centerY, abyssRadius, 0, Math.PI * 2);
    ctx.fill();
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
    phase += 0.004 + bass * 0.014 + high * 0.006;
    if (beatCooldown > 0) beatCooldown -= 1;

    drawFeedbackWarp(centerX, centerY, drawWidth, drawHeight, minDim, timestamp, bass, high);

    ctx.fillStyle = `rgba(42, 0, 10, ${0.09 + high * 0.03})`;
    ctx.fillRect(0, 0, drawWidth, drawHeight);

    drawWatercolorClouds(centerX, centerY, minDim, timestamp, bass, mid, high);
    drawWarpedTunnelRings(centerX, centerY, minDim, timestamp, bass, mid, high);
    drawCurvedTunnelWisps(centerX, centerY, minDim, timestamp, bass, mid, high);
    drawRefractiveVeil(drawWidth, drawHeight, timestamp, bass, high);
    drawAbyss(centerX, centerY, minDim, bass);

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

      if (beatCooldown <= 0 && (bassDelta > 0.06 || highDelta > 0.05 || bass > 0.68)) {
        const intensity = clamp01(bass * 0.8 + high * 0.5 + Math.max(bassDelta, 0) * 3.2);
        spawnWisps(1 + Math.floor(intensity * 3), intensity);
        beatCooldown = 4;
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
</script>

<div class="comet-flow-panel" class:visible={enabled}>
  <canvas bind:this={canvasRef} class="comet-flow-canvas"></canvas>

  <div class="bottom-info">
    <div class="track-meta">
      <span class="track-title">{trackTitle}</span>
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
</style>
