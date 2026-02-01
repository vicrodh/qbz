<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';

  interface Props {
    enabled?: boolean;
  }

  let { enabled = true }: Props = $props();

  let canvasRef: HTMLCanvasElement | null = $state(null);
  let gl: WebGL2RenderingContext | null = null;
  let program: WebGLProgram | null = null;
  let frequencyTexture: WebGLTexture | null = null;
  let vao: WebGLVertexArrayObject | null = null;
  let animationFrame: number | null = null;
  let unlisten: UnlistenFn | null = null;

  const NUM_BARS = 64;
  const frequencyData = new Float32Array(NUM_BARS);

  // Vertex shader - instanced bar rendering
  const vertexSource = `#version 300 es
    precision highp float;

    // Per-vertex (quad corners)
    in vec2 a_position;

    // Uniforms
    uniform sampler2D u_frequencies;
    uniform vec2 u_resolution;
    uniform float u_barWidth;
    uniform float u_gap;
    uniform float u_maxHeight;

    out float v_height;
    out float v_barIndex;

    void main() {
      int barIndex = gl_InstanceID;

      // Sample frequency from texture
      float freq = texelFetch(u_frequencies, ivec2(barIndex, 0), 0).r;

      // Bar dimensions
      float totalWidth = u_barWidth + u_gap;
      float totalBarsWidth = float(${NUM_BARS}) * totalWidth - u_gap;
      float offsetX = (u_resolution.x - totalBarsWidth) / 2.0;

      float x = offsetX + float(barIndex) * totalWidth + a_position.x * u_barWidth;
      float y = a_position.y * freq * u_maxHeight;

      // Normalize to clip space
      vec2 clipSpace = (vec2(x, y) / u_resolution) * 2.0 - 1.0;
      gl_Position = vec4(clipSpace * vec2(1, 1), 0, 1);

      v_height = freq;
      v_barIndex = float(barIndex) / float(${NUM_BARS});
    }
  `;

  // Fragment shader - gradient coloring
  const fragmentSource = `#version 300 es
    precision highp float;

    in float v_height;
    in float v_barIndex;

    out vec4 fragColor;

    uniform vec3 u_colorLow;
    uniform vec3 u_colorHigh;

    void main() {
      // Vertical gradient based on height
      vec3 color = mix(u_colorLow, u_colorHigh, v_height);

      // Add subtle glow for peaks
      float glow = smoothstep(0.6, 1.0, v_height) * 0.4;
      color += glow;

      // Fade edges slightly for softer look
      float alpha = 0.85 + 0.15 * v_height;

      fragColor = vec4(color, alpha);
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

  async function initWebGL() {
    if (!canvasRef) return;

    gl = canvasRef.getContext('webgl2', { alpha: true, premultipliedAlpha: false });
    if (!gl) {
      console.warn('WebGL2 not available for visualizer');
      return;
    }

    // Create shaders
    const vs = createShader(gl, gl.VERTEX_SHADER, vertexSource);
    const fs = createShader(gl, gl.FRAGMENT_SHADER, fragmentSource);
    if (!vs || !fs) return;

    program = createProgram(gl, vs, fs);
    if (!program) return;

    // Create VAO with quad vertices (2 triangles)
    vao = gl.createVertexArray();
    gl.bindVertexArray(vao);

    const positions = new Float32Array([
      0, 0,  // Bottom-left
      1, 0,  // Bottom-right
      0, 1,  // Top-left
      0, 1,  // Top-left
      1, 0,  // Bottom-right
      1, 1,  // Top-right
    ]);

    const posBuffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, posBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.STATIC_DRAW);

    const posLoc = gl.getAttribLocation(program, 'a_position');
    gl.enableVertexAttribArray(posLoc);
    gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

    // Create 1D texture for frequency data (R32F format)
    frequencyTexture = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, frequencyTexture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R32F, NUM_BARS, 1, 0, gl.RED, gl.FLOAT, frequencyData);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    // Enable the visualizer in backend
    try {
      await invoke('set_visualizer_enabled', { enabled: true });
      console.log('[Visualizer] Backend enabled');
    } catch (e) {
      console.error('[Visualizer] Failed to enable backend:', e);
    }

    // Listen for frequency data from backend
    unlisten = await listen<number[]>('viz:data', (event) => {
      // Convert bytes to Float32Array
      const bytes = new Uint8Array(event.payload as unknown as ArrayBuffer);
      const floats = new Float32Array(bytes.buffer);

      if (floats.length === NUM_BARS) {
        frequencyData.set(floats);
        updateFrequencyTexture();
      }
    });

    render();
  }

  function updateFrequencyTexture() {
    if (!gl || !frequencyTexture) return;

    gl.bindTexture(gl.TEXTURE_2D, frequencyTexture);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, NUM_BARS, 1, gl.RED, gl.FLOAT, frequencyData);
  }

  function render() {
    if (!gl || !program || !frequencyTexture || !canvasRef || !vao) return;

    // Set canvas size
    const rect = canvasRef.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvasRef.width = rect.width * dpr;
    canvasRef.height = rect.height * dpr;

    gl.viewport(0, 0, canvasRef.width, canvasRef.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);

    gl.useProgram(program);
    gl.bindVertexArray(vao);

    // Set uniforms
    const barWidth = rect.width / NUM_BARS * 0.7;
    const gap = rect.width / NUM_BARS * 0.3;
    const maxHeight = rect.height * 0.8;

    gl.uniform2f(gl.getUniformLocation(program, 'u_resolution'), rect.width, rect.height);
    gl.uniform1f(gl.getUniformLocation(program, 'u_barWidth'), barWidth);
    gl.uniform1f(gl.getUniformLocation(program, 'u_gap'), gap);
    gl.uniform1f(gl.getUniformLocation(program, 'u_maxHeight'), maxHeight);

    // Colors - cyan to magenta gradient
    gl.uniform3f(gl.getUniformLocation(program, 'u_colorLow'), 0.0, 0.8, 0.9);
    gl.uniform3f(gl.getUniformLocation(program, 'u_colorHigh'), 0.9, 0.2, 0.8);

    // Bind frequency texture
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, frequencyTexture);
    gl.uniform1i(gl.getUniformLocation(program, 'u_frequencies'), 0);

    // Enable blending
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);

    // Draw instanced bars
    gl.drawArraysInstanced(gl.TRIANGLES, 0, 6, NUM_BARS);

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

    // Disable visualizer in backend
    try {
      await invoke('set_visualizer_enabled', { enabled: false });
      console.log('[Visualizer] Backend disabled');
    } catch (e) {
      console.error('[Visualizer] Failed to disable backend:', e);
    }

    if (gl) {
      if (frequencyTexture) gl.deleteTexture(frequencyTexture);
      if (program) gl.deleteProgram(program);
      if (vao) gl.deleteVertexArray(vao);
    }
  }

  onMount(() => {
    if (enabled) {
      initWebGL();
    }

    return cleanup;
  });

  // React to enabled prop changes
  $effect(() => {
    if (enabled && !animationFrame) {
      initWebGL();
    } else if (!enabled && animationFrame) {
      cleanup();
    }
  });
</script>

<canvas
  bind:this={canvasRef}
  class="visualizer-canvas"
  class:visible={enabled}
></canvas>

<style>
  .visualizer-canvas {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    height: 200px;
    pointer-events: none;
    opacity: 0;
    transition: opacity 300ms ease;
  }

  .visualizer-canvas.visible {
    opacity: 1;
  }
</style>
