/**
 * WebGL2 Utilities
 *
 * Low-level utilities for WebGL2 context management, shader compilation,
 * and resource creation. All functions handle errors gracefully.
 */

/**
 * Create a WebGL2 rendering context with optimal settings.
 * Returns null if WebGL2 is unavailable.
 */
export function createWebGL2Context(
  canvas: HTMLCanvasElement
): WebGL2RenderingContext | null {
  const contextOptions: WebGLContextAttributes = {
    alpha: true,                    // Support transparency
    antialias: false,               // Not needed for fullscreen quad
    depth: false,                   // No 3D depth testing
    stencil: false,                 // No stencil buffer
    premultipliedAlpha: true,       // Standard alpha blending
    preserveDrawingBuffer: false,   // Better performance
    powerPreference: 'low-power',   // Save battery on laptops
    failIfMajorPerformanceCaveat: false, // Allow software rendering
  };

  try {
    const gl = canvas.getContext('webgl2', contextOptions);
    if (!gl) {
      console.warn('[WebGL] WebGL2 context creation failed');
      return null;
    }
    return gl;
  } catch (e) {
    console.warn('[WebGL] WebGL2 context creation threw:', e);
    return null;
  }
}

/**
 * Compile a shader from source.
 * Returns null on compilation failure.
 */
export function compileShader(
  gl: WebGL2RenderingContext,
  type: number,
  source: string
): WebGLShader | null {
  const shader = gl.createShader(type);
  if (!shader) {
    console.error('[WebGL] Failed to create shader');
    return null;
  }

  gl.shaderSource(shader, source);
  gl.compileShader(shader);

  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const info = gl.getShaderInfoLog(shader);
    console.error('[WebGL] Shader compilation failed:', info);
    gl.deleteShader(shader);
    return null;
  }

  return shader;
}

/**
 * Create and link a shader program from vertex and fragment sources.
 * Returns null on failure.
 */
export function createShaderProgram(
  gl: WebGL2RenderingContext,
  vertexSource: string,
  fragmentSource: string
): WebGLProgram | null {
  const vertexShader = compileShader(gl, gl.VERTEX_SHADER, vertexSource);
  if (!vertexShader) return null;

  const fragmentShader = compileShader(gl, gl.FRAGMENT_SHADER, fragmentSource);
  if (!fragmentShader) {
    gl.deleteShader(vertexShader);
    return null;
  }

  const program = gl.createProgram();
  if (!program) {
    console.error('[WebGL] Failed to create program');
    gl.deleteShader(vertexShader);
    gl.deleteShader(fragmentShader);
    return null;
  }

  gl.attachShader(program, vertexShader);
  gl.attachShader(program, fragmentShader);
  gl.linkProgram(program);

  // Shaders can be deleted after linking
  gl.deleteShader(vertexShader);
  gl.deleteShader(fragmentShader);

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const info = gl.getProgramInfoLog(program);
    console.error('[WebGL] Program linking failed:', info);
    gl.deleteProgram(program);
    return null;
  }

  return program;
}

/**
 * Create a fullscreen quad geometry.
 * Returns vertex buffer and vertex array object.
 */
export function createFullscreenQuad(gl: WebGL2RenderingContext): {
  vao: WebGLVertexArrayObject;
  vertexBuffer: WebGLBuffer;
} | null {
  // Two triangles covering the entire clip space
  const vertices = new Float32Array([
    -1, -1,  // Bottom-left
     1, -1,  // Bottom-right
    -1,  1,  // Top-left
     1,  1,  // Top-right
  ]);

  const vao = gl.createVertexArray();
  if (!vao) {
    console.error('[WebGL] Failed to create VAO');
    return null;
  }

  const vertexBuffer = gl.createBuffer();
  if (!vertexBuffer) {
    console.error('[WebGL] Failed to create vertex buffer');
    gl.deleteVertexArray(vao);
    return null;
  }

  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, vertices, gl.STATIC_DRAW);

  // Position attribute at location 0
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

  gl.bindVertexArray(null);
  gl.bindBuffer(gl.ARRAY_BUFFER, null);

  return { vao, vertexBuffer };
}

/**
 * Create a texture from an HTMLImageElement or ImageBitmap.
 * Returns null on failure.
 */
export function createTextureFromImage(
  gl: WebGL2RenderingContext,
  image: HTMLImageElement | ImageBitmap
): WebGLTexture | null {
  const texture = gl.createTexture();
  if (!texture) {
    console.error('[WebGL] Failed to create texture');
    return null;
  }

  gl.bindTexture(gl.TEXTURE_2D, texture);

  // Mirrored repeat: when ambient shader UVs exceed [0,1] range,
  // the texture mirrors back smoothly instead of stretching edge pixels
  // (CLAMP_TO_EDGE caused visible dark rectangles on simple/dark artwork)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.MIRRORED_REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.MIRRORED_REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);

  // Upload image data
  gl.texImage2D(
    gl.TEXTURE_2D,
    0,              // Mip level
    gl.RGBA,        // Internal format
    gl.RGBA,        // Source format
    gl.UNSIGNED_BYTE,
    image
  );

  gl.bindTexture(gl.TEXTURE_2D, null);

  return texture;
}

/**
 * Create a 1x1 placeholder texture (dark gray).
 * Useful as fallback while loading real texture.
 */
export function createPlaceholderTexture(
  gl: WebGL2RenderingContext
): WebGLTexture | null {
  const texture = gl.createTexture();
  if (!texture) return null;

  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.texImage2D(
    gl.TEXTURE_2D,
    0,
    gl.RGBA,
    1, 1, 0,
    gl.RGBA,
    gl.UNSIGNED_BYTE,
    new Uint8Array([20, 20, 22, 255]) // Dark background color
  );
  gl.bindTexture(gl.TEXTURE_2D, null);

  return texture;
}

/**
 * Resize canvas to match display size.
 * Returns true if size changed.
 */
export function resizeCanvasToDisplaySize(canvas: HTMLCanvasElement): boolean {
  const dpr = window.devicePixelRatio || 1;
  const displayWidth = Math.floor(canvas.clientWidth * dpr);
  const displayHeight = Math.floor(canvas.clientHeight * dpr);

  if (canvas.width !== displayWidth || canvas.height !== displayHeight) {
    canvas.width = displayWidth;
    canvas.height = displayHeight;
    return true;
  }
  return false;
}

/**
 * Clean up WebGL resources.
 */
export function cleanupWebGL(
  gl: WebGL2RenderingContext,
  resources: {
    program?: WebGLProgram | null;
    vao?: WebGLVertexArrayObject | null;
    vertexBuffer?: WebGLBuffer | null;
    textures?: (WebGLTexture | null)[];
  }
): void {
  if (resources.program) gl.deleteProgram(resources.program);
  if (resources.vao) gl.deleteVertexArray(resources.vao);
  if (resources.vertexBuffer) gl.deleteBuffer(resources.vertexBuffer);
  if (resources.textures) {
    for (const tex of resources.textures) {
      if (tex) gl.deleteTexture(tex);
    }
  }
}
