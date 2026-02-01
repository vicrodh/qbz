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

// Rotate UV around center by angle (radians)
vec2 rotateUV(vec2 uv, float angle) {
    vec2 center = vec2(0.5);
    vec2 centered = uv - center;
    float c = cos(angle);
    float s = sin(angle);
    vec2 rotated = vec2(
        centered.x * c - centered.y * s,
        centered.x * s + centered.y * c
    );
    return rotated + center;
}

// Sample texture with offset, scale, and rotation
vec3 sampleLayer(vec2 uv, vec2 offset, float scale, float rotation) {
    vec2 center = vec2(0.5);
    // Apply rotation first
    vec2 rotatedUV = rotateUV(uv, rotation);
    // Apply scale (zoom) around center
    vec2 scaledUV = center + (rotatedUV - center) * scale;
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
    float time = u_time;
    float intensity = u_intensity;

    // ===========================================
    // 1. MULTI-SCALE SAMPLING WITH FULL ANIMATION
    // ===========================================
    // Each layer has: drift, scale breathing, rotation
    // All parameters are decorrelated for organic feel

    // FAR LAYER: Background, slowest movement
    float farScaleBase = 0.7;
    float farScaleBreath = farScaleBase + sin(time * 0.1) * 0.05 * intensity;
    float farRotation = sin(time * 0.05) * 0.03 * intensity; // ~1.7° max
    vec2 farOffset = vec2(
        sin(time * 0.08) * 0.20,
        cos(time * 0.06) * 0.15
    ) * intensity;
    vec3 farLayer = sampleLayer(uv, farOffset, farScaleBreath, farRotation);

    // MID LAYER: Middle ground, medium movement
    float midScaleBase = 0.9;
    float midScaleBreath = midScaleBase + sin(time * 0.15 + 1.0) * 0.04 * intensity;
    float midRotation = sin(time * 0.08 + 2.0) * 0.025 * intensity; // ~1.4° max
    vec2 midOffset = vec2(
        cos(time * 0.12 + 2.0) * 0.14,
        sin(time * 0.10 + 1.0) * 0.12
    ) * intensity;
    vec3 midLayer = sampleLayer(uv, midOffset, midScaleBreath, midRotation);

    // NEAR LAYER: Foreground, fastest movement
    float nearScaleBase = 1.3;
    float nearScaleBreath = nearScaleBase + sin(time * 0.2 + 2.5) * 0.06 * intensity;
    float nearRotation = sin(time * 0.12 + 4.0) * 0.02 * intensity; // ~1.1° max
    vec2 nearOffset = vec2(
        sin(time * 0.18 + 3.5) * 0.08,
        cos(time * 0.15 + 2.5) * 0.07
    ) * intensity;
    vec3 nearLayer = sampleLayer(uv, nearOffset, nearScaleBreath, nearRotation);

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

    // Pulsing vignette (darken edges, intensity varies)
    float vignetteStrength = 0.30 + sin(time * 0.18) * 0.08 * intensity;
    float vignette = 1.0 - radial * vignetteStrength;
    color *= vignette;

    // Gentle luminance preservation (30% blend)
    float luma = dot(color, vec3(0.2126, 0.7152, 0.0722));
    vec3 lumaColor = color * (luma / max(length(color), 0.001));
    color = mix(color, lumaColor, 0.3);

    // Gradient tint (cool top, warm bottom) - adds vertical depth
    vec3 tintTop = vec3(0.95, 0.97, 1.0);
    vec3 tintBottom = vec3(1.0, 0.96, 0.92);
    color *= mix(tintBottom, tintTop, v_texCoord.y);

    // Brightness breathing (pronounced pulse)
    float breath = 1.0 + sin(time * 0.3) * 0.12 * intensity;
    color *= breath;

    fragColor = vec4(color, 1.0);
}
