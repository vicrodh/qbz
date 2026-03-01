#version 300 es
/**
 * Ambient Immersion Shader
 *
 * Single-layer approach with slow drift, rotation, and breathing.
 * Produces smooth organic motion without multi-layer blending artifacts.
 * The pre-blurred texture does all the heavy lifting — the shader just
 * adds gentle life to it.
 */

precision mediump float;

in vec2 v_texCoord;
out vec4 fragColor;

uniform sampler2D u_texture;
uniform float u_time;
uniform float u_intensity;

void main() {
    vec2 uv = v_texCoord;
    float time = u_time * 0.8;
    float intensity = u_intensity;

    // Slow organic drift (Lissajous-like path, never repeats quickly)
    vec2 drift = vec2(
        sin(time * 0.07) * 0.03 + sin(time * 0.03) * 0.02,
        cos(time * 0.05) * 0.025 + cos(time * 0.02) * 0.015
    ) * intensity;

    // Gentle scale breathing around center
    float scaleBreath = 1.0 + sin(time * 0.09) * 0.02 * intensity;
    vec2 center = vec2(0.5);
    vec2 scaledUV = center + (uv - center) * scaleBreath;

    // Slow rotation
    float angle = sin(time * 0.04) * 0.015 * intensity;
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

    // Radial distance from center (0 at center, ~1 at corners)
    float radial = length(uv - center) * 1.414;

    // Subtle pulsing vignette
    float vignetteStrength = 0.25 + sin(time * 0.12) * 0.05 * intensity;
    float vignette = 1.0 - radial * vignetteStrength;
    color *= vignette;

    // Vertical gradient tint (cool top, warm bottom)
    vec3 tintTop = vec3(0.96, 0.97, 1.0);
    vec3 tintBottom = vec3(1.0, 0.97, 0.94);
    color *= mix(tintBottom, tintTop, uv.y);

    // Gentle brightness breathing
    float breath = 1.0 + sin(time * 0.15) * 0.06 * intensity;
    color *= breath;

    fragColor = vec4(color, 1.0);
}
