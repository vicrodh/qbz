#version 300 es
/**
 * Heavy Immersion Ambient Shader
 *
 * Creates a spatially enveloping ambient field from a pre-blurred texture.
 * NOT a zoomed/cropped image - a layered depth illusion.
 *
 * Three mandatory mechanisms:
 * 1. Multi-scale texture sampling (near/mid/far layers)
 * 2. Scale-dependent UV drift (parallax-like depth)
 * 3. Spatial weighting (center vs edge differentiation)
 */

precision mediump float;

in vec2 v_texCoord;
out vec4 fragColor;

uniform sampler2D u_texture;
uniform float u_time;
uniform float u_intensity;

// Sample texture with UV offset and scale (zoom)
vec3 sampleLayer(vec2 uv, vec2 offset, float scale) {
    vec2 center = vec2(0.5);
    // Apply scale (zoom) around center
    vec2 scaledUV = center + (uv - center) * scale;
    // Apply offset
    scaledUV += offset;
    // Clamp to valid range
    scaledUV = clamp(scaledUV, 0.0, 1.0);
    return texture(u_texture, scaledUV).rgb;
}

// Radial distance from center (0 at center, 1 at corners)
float radialDist(vec2 uv) {
    vec2 centered = uv - vec2(0.5);
    return length(centered) * 1.414; // normalize so corners = 1
}

void main() {
    vec2 uv = v_texCoord;
    float time = u_time * 0.5; // Slow base time
    float intensity = u_intensity;

    // ===========================================
    // 1. MULTI-SCALE TEXTURE SAMPLING
    // ===========================================
    // Three depth layers: far (background), mid, near (foreground)
    // Different zoom levels create scale separation

    // Far layer: zoomed OUT (sees more of texture, feels distant)
    float farScale = 0.7;
    vec2 farOffset = vec2(
        sin(time * 0.03) * 0.08,
        cos(time * 0.025) * 0.06
    ) * intensity;
    vec3 farLayer = sampleLayer(uv, farOffset, farScale);

    // Mid layer: slight zoom, different drift direction
    float midScale = 0.9;
    vec2 midOffset = vec2(
        cos(time * 0.05) * 0.04,
        sin(time * 0.04) * 0.05
    ) * intensity;
    vec3 midLayer = sampleLayer(uv, midOffset, midScale);

    // Near layer: zoomed IN (feels close, foreground)
    float nearScale = 1.3;
    vec2 nearOffset = vec2(
        sin(time * 0.07 + 1.5) * 0.03,
        cos(time * 0.06 + 0.8) * 0.025
    ) * intensity;
    vec3 nearLayer = sampleLayer(uv, nearOffset, nearScale);

    // ===========================================
    // 2. SCALE-DEPENDENT UV DRIFT (handled above)
    // Each layer has different drift speeds and phases
    // Far: slowest (0.03, 0.025)
    // Mid: medium (0.05, 0.04)
    // Near: fastest (0.07, 0.06)
    // ===========================================

    // ===========================================
    // 3. SPATIAL WEIGHTING
    // ===========================================
    // Center-vs-edge differentiation for depth illusion
    // Near layer dominant at center, far layer at edges

    float radial = radialDist(uv);

    // Weight curves (cubic for smooth falloff)
    float nearWeight = 1.0 - smoothstep(0.0, 0.6, radial);  // Strong at center
    float farWeight = smoothstep(0.2, 0.9, radial);          // Strong at edges
    float midWeight = 1.0 - abs(radial - 0.5) * 1.5;         // Strong in middle ring
    midWeight = max(midWeight, 0.3);

    // Normalize weights
    float totalWeight = nearWeight + midWeight + farWeight;
    nearWeight /= totalWeight;
    midWeight /= totalWeight;
    farWeight /= totalWeight;

    // Blend layers based on spatial position
    vec3 color = farLayer * farWeight + midLayer * midWeight + nearLayer * nearWeight;

    // ===========================================
    // ADDITIONAL DEPTH ENHANCEMENT
    // ===========================================

    // Subtle vignette (darken edges for depth)
    float vignette = 1.0 - radial * 0.35;
    color *= vignette;

    // Gentle luminance preservation (30% blend)
    float luma = dot(color, vec3(0.2126, 0.7152, 0.0722));
    vec3 lumaColor = color * (luma / max(length(color), 0.001));
    color = mix(color, lumaColor, 0.3);

    // Gradient tint (cool top, warm bottom) - adds vertical depth
    vec3 tintTop = vec3(0.95, 0.97, 1.0);
    vec3 tintBottom = vec3(1.0, 0.96, 0.92);
    color *= mix(tintBottom, tintTop, v_texCoord.y);

    // Subtle brightness breathing (very slow)
    float breath = 1.0 + sin(time * 0.15) * 0.04 * intensity;
    color *= breath;

    fragColor = vec4(color, 1.0);
}
