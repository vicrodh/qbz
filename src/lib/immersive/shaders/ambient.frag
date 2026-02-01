#version 300 es
/**
 * Ambient Motion Fragment Shader
 *
 * Adds subtle GPU-driven motion to the background:
 * - UV drift (slow wandering)
 * - Zoom oscillation (breathing effect)
 * - Color breathing (subtle brightness pulsing)
 *
 * All effects are controlled by u_intensity (0 = static, 1 = full motion).
 * All computation is GPU-side - no CPU per-frame work.
 */

precision mediump float;

// Input from vertex shader
in vec2 v_texCoord;

// Output
out vec4 fragColor;

// Uniforms
uniform sampler2D u_texture;
uniform float u_time;          // Time in seconds
uniform float u_intensity;     // Motion intensity (0.0 - 1.0)

// Constants for motion parameters
const float DRIFT_SPEED_X = 0.08;
const float DRIFT_SPEED_Y = 0.06;
const float DRIFT_AMOUNT = 0.03;

const float ZOOM_SPEED = 0.05;
const float ZOOM_AMOUNT = 0.02;

const float BREATH_SPEED = 0.12;
const float BREATH_AMOUNT = 0.06;

void main() {
    vec2 uv = v_texCoord;
    vec2 center = vec2(0.5);

    // UV drift - slow wandering motion
    float driftX = sin(u_time * DRIFT_SPEED_X) * DRIFT_AMOUNT * u_intensity;
    float driftY = cos(u_time * DRIFT_SPEED_Y * 1.3) * DRIFT_AMOUNT * u_intensity;
    uv += vec2(driftX, driftY);

    // Zoom oscillation - subtle breathing
    float zoom = 1.0 + sin(u_time * ZOOM_SPEED) * ZOOM_AMOUNT * u_intensity;
    uv = center + (uv - center) / zoom;

    // Clamp UV to avoid edge artifacts
    uv = clamp(uv, 0.0, 1.0);

    // Sample texture
    vec4 color = texture(u_texture, uv);

    // Color breathing - subtle brightness pulsing
    float breath = 1.0 + sin(u_time * BREATH_SPEED) * BREATH_AMOUNT * u_intensity;
    color.rgb *= breath;

    fragColor = color;
}
