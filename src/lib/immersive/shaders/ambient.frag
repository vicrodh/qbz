#version 300 es
/**
 * Ambient Motion Fragment Shader
 *
 * Adds GPU-driven motion and blur simulation to the background:
 * - Multi-sample blur (samples multiple nearby points)
 * - UV drift (slow wandering)
 * - Zoom oscillation (breathing effect)
 * - Color breathing (subtle brightness pulsing)
 * - Noise-based distortion to break up pixelation
 */

precision mediump float;

in vec2 v_texCoord;
out vec4 fragColor;

uniform sampler2D u_texture;
uniform float u_time;
uniform float u_intensity;

// Motion parameters
const float DRIFT_SPEED_X = 0.12;
const float DRIFT_SPEED_Y = 0.09;
const float DRIFT_AMOUNT = 0.06;

const float ZOOM_SPEED = 0.08;
const float ZOOM_AMOUNT = 0.04;

const float BREATH_SPEED = 0.15;
const float BREATH_AMOUNT = 0.08;

// Blur simulation parameters
const float BLUR_RADIUS = 0.015;  // UV space blur radius
const int BLUR_SAMPLES = 12;      // Number of samples for blur

// Simple pseudo-random function
float random(vec2 st) {
    return fract(sin(dot(st.xy, vec2(12.9898, 78.233))) * 43758.5453123);
}

// Smooth noise function
float noise(vec2 st) {
    vec2 i = floor(st);
    vec2 f = fract(st);

    float a = random(i);
    float b = random(i + vec2(1.0, 0.0));
    float c = random(i + vec2(0.0, 1.0));
    float d = random(i + vec2(1.0, 1.0));

    vec2 u = f * f * (3.0 - 2.0 * f);

    return mix(a, b, u.x) + (c - a) * u.y * (1.0 - u.x) + (d - b) * u.x * u.y;
}

void main() {
    vec2 uv = v_texCoord;
    vec2 center = vec2(0.5);

    // UV drift
    float driftX = sin(u_time * DRIFT_SPEED_X) * DRIFT_AMOUNT * u_intensity;
    float driftY = cos(u_time * DRIFT_SPEED_Y * 1.3) * DRIFT_AMOUNT * u_intensity;
    uv += vec2(driftX, driftY);

    // Zoom oscillation
    float zoom = 1.0 + sin(u_time * ZOOM_SPEED) * ZOOM_AMOUNT * u_intensity;
    uv = center + (uv - center) / zoom;

    // Add subtle noise-based distortion to break up pixelation
    float noiseScale = 8.0;
    float distortAmount = 0.008;
    float n1 = noise(uv * noiseScale + u_time * 0.1);
    float n2 = noise(uv * noiseScale + 100.0 + u_time * 0.1);
    uv += vec2(n1 - 0.5, n2 - 0.5) * distortAmount;

    // Multi-sample blur to smooth out the texture
    vec4 color = vec4(0.0);
    float totalWeight = 0.0;

    for (int i = 0; i < BLUR_SAMPLES; i++) {
        float angle = float(i) * 6.28318530718 / float(BLUR_SAMPLES);
        float r = BLUR_RADIUS * (0.5 + 0.5 * random(uv + float(i)));
        vec2 offset = vec2(cos(angle), sin(angle)) * r;

        vec2 sampleUV = clamp(uv + offset, 0.0, 1.0);
        float weight = 1.0 - length(offset) / BLUR_RADIUS;

        color += texture(u_texture, sampleUV) * weight;
        totalWeight += weight;
    }

    // Add center sample with higher weight
    color += texture(u_texture, clamp(uv, 0.0, 1.0)) * 2.0;
    totalWeight += 2.0;

    color /= totalWeight;

    // Color breathing
    float breath = 1.0 + sin(u_time * BREATH_SPEED) * BREATH_AMOUNT * u_intensity;
    color.rgb *= breath;

    fragColor = color;
}
