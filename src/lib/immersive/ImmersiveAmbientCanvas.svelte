<script lang="ts">
  /**
   * Immersive Ambient Canvas
   *
   * WebGL2-based canvas for rendering the immersive background.
   * Phase 3: Ambient motion shader with UV drift, zoom, and color breathing.
   *
   * Features:
   * - WebGL2 context with graceful fallback
   * - Context loss/restore handling
   * - Automatic resize handling
   * - Performance metrics reporting
   * - Async texture loading with cancellation
   * - Ambient motion shader (GPU-only, no CPU per-frame work)
   * - Visibility-aware animation (pauses when tab hidden)
   * - Configurable motion intensity
   */

  import { onMount, onDestroy } from 'svelte';
  import {
    createWebGL2Context,
    createShaderProgram,
    createFullscreenQuad,
    createPlaceholderTexture,
    resizeCanvasToDisplaySize,
    cleanupWebGL,
  } from './utils/webgl-utils';
  import { loadBlurredTexture, cancelAllLoads } from './utils/texture-loader';
  import { SHADERS } from './shaders';
  import {
    updateMetrics,
    updateTextureCount,
    handleContextLost,
    handleContextRestored,
    getConfig,
  } from './ImmersiveRenderer';

  // Props
  interface Props {
    /** URL of the artwork to display */
    artworkUrl?: string;
    /** Enable ambient motion (default: true) */
    enableMotion?: boolean;
    /** Motion intensity override (0-1, default: from config) */
    intensity?: number;
    /** Called when WebGL initialization fails */
    onFallback?: () => void;
  }

  let {
    artworkUrl,
    enableMotion = true,
    intensity,
    onFallback,
  }: Props = $props();

  // Canvas reference
  let canvasRef: HTMLCanvasElement | undefined = $state();

  // WebGL state (not reactive - managed imperatively)
  let gl: WebGL2RenderingContext | null = null;
  let program: WebGLProgram | null = null;
  let vao: WebGLVertexArrayObject | null = null;
  let vertexBuffer: WebGLBuffer | null = null;
  let currentTexture: WebGLTexture | null = null;
  let placeholderTexture: WebGLTexture | null = null;

  // Uniform locations
  let u_texture: WebGLUniformLocation | null = null;
  let u_time: WebGLUniformLocation | null = null;
  let u_intensity: WebGLUniformLocation | null = null;

  // Animation state
  let animationFrameId: number | null = null;
  let isInitialized = false;
  let isAnimating = false;
  let lastArtworkUrl = '';
  let textureLoadId = 0;
  let startTime = 0;
  let lastFrameTime = 0;
  let isVisible = true;

  // Frame throttling for power efficiency
  const TARGET_FPS = 30;
  const FRAME_INTERVAL = 1000 / TARGET_FPS;

  /**
   * Initialize WebGL resources.
   */
  function initWebGL(): boolean {
    if (!canvasRef) return false;

    // Create context
    gl = createWebGL2Context(canvasRef);
    if (!gl) {
      console.warn('[ImmersiveCanvas] WebGL2 not available');
      onFallback?.();
      return false;
    }

    // Create shader program (use ambient shader for motion)
    program = createShaderProgram(
      gl,
      SHADERS.ambient.vertex,
      SHADERS.ambient.fragment
    );
    if (!program) {
      console.error('[ImmersiveCanvas] Failed to create shader program');
      onFallback?.();
      return false;
    }

    // Get uniform locations
    u_texture = gl.getUniformLocation(program, 'u_texture');
    u_time = gl.getUniformLocation(program, 'u_time');
    u_intensity = gl.getUniformLocation(program, 'u_intensity');

    // Create fullscreen quad
    const quad = createFullscreenQuad(gl);
    if (!quad) {
      console.error('[ImmersiveCanvas] Failed to create quad geometry');
      onFallback?.();
      return false;
    }
    vao = quad.vao;
    vertexBuffer = quad.vertexBuffer;

    // Create placeholder texture
    placeholderTexture = createPlaceholderTexture(gl);

    // Set up context loss handlers
    canvasRef.addEventListener('webglcontextlost', onContextLost);
    canvasRef.addEventListener('webglcontextrestored', onContextRestored);

    console.log('[ImmersiveCanvas] WebGL2 initialized with ambient shader');
    isInitialized = true;
    startTime = performance.now();
    return true;
  }

  /**
   * Clean up WebGL resources.
   */
  function destroyWebGL(): void {
    cancelAllLoads();
    stopAnimation();

    if (canvasRef) {
      canvasRef.removeEventListener('webglcontextlost', onContextLost);
      canvasRef.removeEventListener('webglcontextrestored', onContextRestored);
    }

    if (gl) {
      cleanupWebGL(gl, {
        program,
        vao,
        vertexBuffer,
        textures: [currentTexture, placeholderTexture],
      });
    }

    gl = null;
    program = null;
    vao = null;
    vertexBuffer = null;
    currentTexture = null;
    placeholderTexture = null;
    u_texture = null;
    u_time = null;
    u_intensity = null;
    isInitialized = false;
  }

  /**
   * Handle WebGL context loss.
   */
  function onContextLost(event: Event): void {
    event.preventDefault();
    console.warn('[ImmersiveCanvas] WebGL context lost');
    stopAnimation();
    handleContextLost();
  }

  /**
   * Handle WebGL context restoration.
   */
  function onContextRestored(): void {
    console.log('[ImmersiveCanvas] WebGL context restored');
    if (initWebGL()) {
      handleContextRestored();
      if (lastArtworkUrl) {
        loadTexture(lastArtworkUrl);
      }
      startAnimation();
    }
  }

  /**
   * Handle visibility change (pause when tab hidden).
   */
  function onVisibilityChange(): void {
    isVisible = document.visibilityState === 'visible';
    if (isVisible && enableMotion && !isAnimating) {
      startAnimation();
    } else if (!isVisible) {
      stopAnimation();
    }
  }

  /**
   * Load a pre-blurred texture from artwork URL.
   */
  async function loadTexture(url: string): Promise<void> {
    if (!gl || !isInitialized) return;

    const loadId = ++textureLoadId;
    const requestId = `artwork-${loadId}`;

    try {
      const result = await loadBlurredTexture(gl, url, requestId);

      if (loadId !== textureLoadId || !result) {
        return;
      }

      if (currentTexture) {
        gl.deleteTexture(currentTexture);
      }
      currentTexture = result.texture;
      lastArtworkUrl = url;

      updateTextureCount(1, result.width * result.height * 4);

      // If not animating, render single frame
      if (!isAnimating) {
        render();
      }

      console.log(`[ImmersiveCanvas] Texture loaded: ${result.width}x${result.height}`);
    } catch (e) {
      if (e instanceof DOMException && e.name === 'AbortError') {
        return;
      }
      console.warn('[ImmersiveCanvas] Failed to load texture:', e);
    }
  }

  /**
   * Start the animation loop.
   */
  function startAnimation(): void {
    if (isAnimating || !isInitialized) return;
    isAnimating = true;
    lastFrameTime = performance.now();
    animationFrameId = requestAnimationFrame(animationLoop);
    console.log('[ImmersiveCanvas] Animation started');
  }

  /**
   * Stop the animation loop.
   */
  function stopAnimation(): void {
    if (animationFrameId !== null) {
      cancelAnimationFrame(animationFrameId);
      animationFrameId = null;
    }
    isAnimating = false;
  }

  /**
   * Animation loop with frame throttling.
   */
  function animationLoop(timestamp: number): void {
    if (!isAnimating) return;

    // Frame throttling for power efficiency
    const elapsed = timestamp - lastFrameTime;
    if (elapsed >= FRAME_INTERVAL) {
      lastFrameTime = timestamp - (elapsed % FRAME_INTERVAL);
      render();
    }

    animationFrameId = requestAnimationFrame(animationLoop);
  }

  /**
   * Render a single frame.
   */
  function render(): void {
    if (!gl || !program || !vao || !isInitialized) return;

    const frameStart = performance.now();

    // Handle resize
    if (canvasRef) {
      if (resizeCanvasToDisplaySize(canvasRef)) {
        gl.viewport(0, 0, canvasRef.width, canvasRef.height);
      }
    }

    // Clear
    gl.clearColor(0.039, 0.039, 0.043, 1.0);
    gl.clear(gl.COLOR_BUFFER_BIT);

    // Use program
    gl.useProgram(program);

    // Set uniforms
    const currentTime = (performance.now() - startTime) / 1000; // seconds
    const motionIntensity = enableMotion
      ? (intensity ?? getConfig().ambientIntensity)
      : 0;

    gl.uniform1f(u_time, currentTime);
    gl.uniform1f(u_intensity, motionIntensity);

    // Bind texture
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, currentTexture || placeholderTexture);
    gl.uniform1i(u_texture, 0);

    // Draw
    gl.bindVertexArray(vao);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    gl.bindVertexArray(null);

    // Report metrics
    const frameTime = performance.now() - frameStart;
    updateMetrics(frameTime);
  }

  // React to artwork changes
  $effect(() => {
    if (artworkUrl && artworkUrl !== lastArtworkUrl && isInitialized) {
      loadTexture(artworkUrl);
    }
  });

  // React to motion enable/disable
  $effect(() => {
    if (isInitialized) {
      if (enableMotion && isVisible && !isAnimating) {
        startAnimation();
      } else if (!enableMotion && isAnimating) {
        stopAnimation();
        render(); // Render one static frame
      }
    }
  });

  onMount(() => {
    if (initWebGL()) {
      // Set up visibility listener
      document.addEventListener('visibilitychange', onVisibilityChange);

      // Load initial artwork if provided
      if (artworkUrl) {
        loadTexture(artworkUrl);
      }

      // Start animation if motion enabled
      if (enableMotion) {
        startAnimation();
      } else {
        render();
      }
    }
  });

  onDestroy(() => {
    document.removeEventListener('visibilitychange', onVisibilityChange);
    destroyWebGL();
  });
</script>

<canvas
  bind:this={canvasRef}
  class="immersive-canvas"
  aria-hidden="true"
></canvas>

<style>
  .immersive-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
</style>
