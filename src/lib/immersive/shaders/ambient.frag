#version 300 es
/**
 * Ambient Immersion Shader
 *
 * Single-layer approach with pronounced drift, rotation, scale breathing,
 * and color modulation. No multi-layer blending = no geometric artifacts.
 * Safe because the texture is generated at 8x8 and has no structure.
 */

precision mediump float;

in vec2 v_texCoord;
out vec4 fragColor;

uniform sampler2D u_texture;
uniform float u_time;
uniform float u_intensity;

void main() {
    vec2 uv = v_texCoord;
    float time = u_time;
    float intensity = u_intensity;
    vec2 center = vec2(0.5);

    // === DRIFT: Lissajous-like wandering path ===
    // Multiple frequencies for organic, non-repeating motion
    vec2 drift = vec2(
        sin(time * 0.13) * 0.12 + sin(time * 0.07 + 1.3) * 0.08,
        cos(time * 0.11) * 0.10 + cos(time * 0.05 + 2.1) * 0.07
    ) * intensity;

    // === SCALE: Pronounced breathing (zoom in/out) ===
    float scaleBreath = 1.0
        + sin(time * 0.17) * 0.08 * intensity
        + sin(time * 0.09 + 0.7) * 0.04 * intensity;
    vec2 scaledUV = center + (uv - center) * scaleBreath;

    // === ROTATION: Slow but visible swaying ===
    float angle = (sin(time * 0.08) * 0.06 + sin(time * 0.04 + 1.5) * 0.03) * intensity;
    vec2 centered = scaledUV - center;
    float c = cos(angle);
    float s = sin(angle);
    vec2 rotatedUV = vec2(
        centered.x * c - centered.y * s,
        centered.x * s + centered.y * c
    ) + center;

    // Apply drift
    vec2 finalUV = rotatedUV + drift;

    // Sample texture (MIRRORED_REPEAT handles out-of-range UVs)
    vec3 color = texture(u_texture, finalUV).rgb;

    // === COLOR MODULATION: Warm/cool shifting ===
    // Slowly shifts hue warmth over time
    float warmShift = sin(time * 0.06) * 0.08 * intensity;
    color.r *= 1.0 + warmShift;
    color.b *= 1.0 - warmShift * 0.6;

    // Radial distance from center (0 at center, ~1 at corners)
    float radial = length(uv - center) * 1.414;

    // === VIGNETTE: Pulsing edge darkening ===
    float vignetteStrength = 0.30 + sin(time * 0.14) * 0.10 * intensity;
    float vignette = 1.0 - radial * vignetteStrength;
    color *= vignette;

    // Vertical gradient tint (cool top, warm bottom)
    vec3 tintTop = vec3(0.95, 0.97, 1.0);
    vec3 tintBottom = vec3(1.0, 0.96, 0.93);
    color *= mix(tintBottom, tintTop, uv.y);

    // === BRIGHTNESS BREATHING: Pronounced pulse ===
    float breath = 1.0 + sin(time * 0.20) * 0.12 * intensity;
    color *= breath;

    fragColor = vec4(color, 1.0);
}
