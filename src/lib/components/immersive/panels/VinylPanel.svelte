<script lang="ts">
  import { onMount } from 'svelte';
  import QualityBadge from '$lib/components/QualityBadge.svelte';

  interface Props {
    artwork: string;
    trackTitle: string;
    artist: string;
    album?: string;
    isPlaying?: boolean;
    quality?: string;
    bitDepth?: number;
    samplingRate?: number;
  }

  let {
    artwork,
    trackTitle,
    artist,
    album,
    isPlaying = false,
    quality,
    bitDepth,
    samplingRate
  }: Props = $props();

  // WebGL2 refs
  let canvasRef: HTMLCanvasElement | null = $state(null);
  let gl: WebGL2RenderingContext | null = null;
  let program: WebGLProgram | null = null;
  let vinylTexture: WebGLTexture | null = null;
  let animationFrame: number | null = null;
  let startTime = 0;
  let rotation = 0;

  // Mouse parallax state
  let mouseX = $state(0.5);
  let mouseY = $state(0.5);
  let containerRef: HTMLDivElement | null = $state(null);

  function handleMouseMove(e: MouseEvent) {
    if (!containerRef) return;
    const rect = containerRef.getBoundingClientRect();
    mouseX = (e.clientX - rect.left) / rect.width;
    mouseY = (e.clientY - rect.top) / rect.height;
  }

  function handleMouseLeave() {
    mouseX = 0.5;
    mouseY = 0.5;
  }

  // Parallax calculations (extremely aggressive horizontal)
  const coverOffsetX = $derived((mouseX - 0.5) * 200);
  const coverOffsetY = $derived((mouseY - 0.5) * 40);
  const revealOffset = $derived(isPlaying ? -8 : 0);

  // Vertex shader
  const vertexShaderSource = `#version 300 es
    in vec2 a_position;
    in vec2 a_texCoord;
    out vec2 v_texCoord;

    uniform vec2 u_resolution;
    uniform vec2 u_offset;
    uniform float u_scale;

    void main() {
      vec2 pos = a_position * u_scale + u_offset;
      vec2 clipSpace = (pos / u_resolution) * 2.0 - 1.0;
      gl_Position = vec4(clipSpace * vec2(1, -1), 0, 1);
      v_texCoord = a_texCoord;
    }
  `;

  // Fragment shader for vinyl
  const fragmentShaderSource = `#version 300 es
    precision mediump float;

    in vec2 v_texCoord;
    out vec4 fragColor;

    uniform sampler2D u_texture;
    uniform float u_rotation;
    uniform vec2 u_parallax;

    void main() {
      vec2 center = vec2(0.5);
      vec2 uv = v_texCoord - center;

      // Apply rotation
      float c = cos(u_rotation);
      float s = sin(u_rotation);
      uv = vec2(uv.x * c - uv.y * s, uv.x * s + uv.y * c);

      // Apply aggressive parallax offset (horizontal emphasis)
      uv += vec2(u_parallax.x * 0.15, u_parallax.y * 0.05);

      uv += center;

      // Add padding - scale down UV to add margin around vinyl
      uv = (uv - 0.5) * 1.15 + 0.5;

      // Discard pixels outside texture bounds (creates padding)
      if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        fragColor = vec4(0.0);
        return;
      }

      // Sample texture
      vec4 color = texture(u_texture, uv);

      fragColor = color;
    }
  `;

  function createShader(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader | null {
    const shader = gl.createShader(type);
    if (!shader) return null;
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      console.error('Shader compile error:', gl.getShaderInfoLog(shader));
      gl.deleteShader(shader);
      return null;
    }
    return shader;
  }

  function createProgram(gl: WebGL2RenderingContext, vs: WebGLShader, fs: WebGLShader): WebGLProgram | null {
    const prog = gl.createProgram();
    if (!prog) return null;
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      console.error('Program link error:', gl.getProgramInfoLog(prog));
      gl.deleteProgram(prog);
      return null;
    }
    return prog;
  }

  function loadTexture(gl: WebGL2RenderingContext, url: string): Promise<WebGLTexture | null> {
    return new Promise((resolve) => {
      const texture = gl.createTexture();
      const img = new Image();
      img.crossOrigin = 'anonymous';
      img.onload = () => {
        gl.bindTexture(gl.TEXTURE_2D, texture);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        resolve(texture);
      };
      img.onerror = () => resolve(null);
      img.src = url;
    });
  }

  async function initWebGL() {
    if (!canvasRef) return;

    gl = canvasRef.getContext('webgl2', { alpha: true, premultipliedAlpha: false });
    if (!gl) {
      console.warn('WebGL2 not available for VinylPanel');
      return;
    }

    // Create shaders
    const vs = createShader(gl, gl.VERTEX_SHADER, vertexShaderSource);
    const fs = createShader(gl, gl.FRAGMENT_SHADER, fragmentShaderSource);
    if (!vs || !fs) return;

    program = createProgram(gl, vs, fs);
    if (!program) return;

    // Load vinyl texture (PNG for better WebGL compatibility)
    vinylTexture = await loadTexture(gl, '/qbz-vinyl-v3.png');

    // Setup geometry (full quad)
    const positions = new Float32Array([
      0, 0, 0, 0,
      1, 0, 1, 0,
      0, 1, 0, 1,
      0, 1, 0, 1,
      1, 0, 1, 0,
      1, 1, 1, 1,
    ]);

    const vao = gl.createVertexArray();
    gl.bindVertexArray(vao);

    const buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);

    const posLoc = gl.getAttribLocation(program, 'a_position');
    const texLoc = gl.getAttribLocation(program, 'a_texCoord');

    gl.enableVertexAttribArray(posLoc);
    gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 16, 0);
    gl.enableVertexAttribArray(texLoc);
    gl.vertexAttribPointer(texLoc, 2, gl.FLOAT, false, 16, 8);

    startTime = performance.now();
    render();
  }

  function render() {
    if (!gl || !program || !vinylTexture || !canvasRef) return;

    const now = performance.now();
    const elapsed = (now - startTime) / 1000;

    // Update rotation when playing
    if (isPlaying) {
      rotation += 0.02; // ~0.3 RPM visual effect
    }

    // Set canvas size
    const rect = canvasRef.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvasRef.width = rect.width * dpr;
    canvasRef.height = rect.height * dpr;

    gl.viewport(0, 0, canvasRef.width, canvasRef.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);

    gl.useProgram(program);

    // Calculate vinyl size and position (centered, with padding)
    // Vinyl moves opposite to cover for depth effect
    const size = Math.min(rect.width, rect.height) * 0.85;
    const offsetX = (rect.width - size) / 2 + (mouseX - 0.5) * -120;
    const offsetY = (rect.height - size) / 2 + (mouseY - 0.5) * -25;

    // Set uniforms
    gl.uniform2f(gl.getUniformLocation(program, 'u_resolution'), rect.width, rect.height);
    gl.uniform2f(gl.getUniformLocation(program, 'u_offset'), offsetX, offsetY);
    gl.uniform1f(gl.getUniformLocation(program, 'u_scale'), size);
    gl.uniform1f(gl.getUniformLocation(program, 'u_rotation'), rotation);
    gl.uniform2f(gl.getUniformLocation(program, 'u_parallax'), mouseX - 0.5, mouseY - 0.5);

    // Bind texture
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, vinylTexture);
    gl.uniform1i(gl.getUniformLocation(program, 'u_texture'), 0);

    // Enable blending for transparency
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    // Draw
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    animationFrame = requestAnimationFrame(render);
  }

  function cleanup() {
    if (animationFrame) {
      cancelAnimationFrame(animationFrame);
    }
    if (gl) {
      if (vinylTexture) gl.deleteTexture(vinylTexture);
      if (program) gl.deleteProgram(program);
    }
  }

  onMount(() => {
    initWebGL();
    return cleanup;
  });
</script>

<div
  class="vinyl-panel"
  bind:this={containerRef}
  onmousemove={handleMouseMove}
  onmouseleave={handleMouseLeave}
>
  <div class="vinyl-container">
    <!-- WebGL2 Vinyl Canvas (behind) -->
    <canvas bind:this={canvasRef} class="vinyl-canvas"></canvas>

    <!-- Album Cover (in front, slides to reveal) -->
    <div
      class="album-cover"
      style="transform: translate(calc(-50% + {coverOffsetX}px + {revealOffset}%), calc(-50% + {coverOffsetY}px))"
    >
      <img src={artwork} alt={trackTitle} />
    </div>
  </div>

  <!-- Track Info (below) -->
  <div class="track-info">
    <h1 class="track-title">{trackTitle}</h1>
    <p class="track-artist">{artist}</p>
    {#if album}
      <p class="track-album">{album}</p>
    {/if}
    <div class="quality-badge-wrapper">
      <QualityBadge {quality} {bitDepth} {samplingRate} />
    </div>
  </div>
</div>

<style>
  .vinyl-panel {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding-top: 70px;
    padding-bottom: 120px;
    padding-left: 40px;
    padding-right: 40px;
    z-index: 5;
    gap: 24px;
  }

  .vinyl-container {
    position: relative;
    width: min(500px, 55vh);
    height: min(500px, 55vh);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .vinyl-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  /* Album Cover - in front of vinyl */
  .album-cover {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 70%;
    height: 70%;
    border-radius: 8px;
    overflow: hidden;
    box-shadow:
      0 10px 40px rgba(0, 0, 0, 0.5),
      0 20px 60px rgba(0, 0, 0, 0.3);
    z-index: 10;
    transition: transform 600ms cubic-bezier(0.23, 1, 0.32, 1);
  }

  .album-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Track Info */
  .track-info {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 6px;
    max-width: 500px;
  }

  .track-title {
    font-size: clamp(20px, 3vw, 28px);
    font-weight: 700;
    color: var(--text-primary, white);
    margin: 0;
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.3);
  }

  .track-artist {
    font-size: clamp(14px, 2vw, 18px);
    color: var(--alpha-70, rgba(255, 255, 255, 0.7));
    margin: 0;
  }

  .track-album {
    font-size: clamp(12px, 1.5vw, 14px);
    color: var(--alpha-50, rgba(255, 255, 255, 0.5));
    margin: 0;
    font-style: italic;
  }

  .quality-badge-wrapper {
    margin-top: 12px;
  }

  /* Responsive */
  @media (max-width: 768px) {
    .vinyl-panel {
      padding: 70px 24px 130px;
      gap: 20px;
    }

    .vinyl-container {
      width: min(350px, 50vw);
      height: min(350px, 50vw);
    }
  }

  @media (max-height: 600px) {
    .vinyl-container {
      width: min(280px, 40vh);
      height: min(280px, 40vh);
    }

    .track-info {
      gap: 4px;
    }
  }
</style>
