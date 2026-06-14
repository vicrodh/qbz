// WGPU UNDERLAY SPIKE — plasma fragment shader.
//
// One fullscreen-triangle pass. The fragment stage is a classic sin-field
// plasma, audio-reactive via the sub-bass energy (u.energy0) and a decayed
// transient (u.transient). This shader's only job is to prove the GPU
// fragment-shader path works end to end (renderer swap -> wgpu texture ->
// Slint Image), NOT to be a final look. The Neon/Audio-Cube catalog replaces
// it once the spike is validated.
//
// CPU side: src/shader_underlay.rs. Uniform layout must stay in lockstep with
// the `Uniforms` #[repr(C)] struct there (vec4-aligned, 32 bytes).

struct Uniforms {
    time: f32,
    energy0: f32,
    transient: f32,
    _pad0: f32,
    resolution: vec2<f32>,
    _pad1: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // Oversized triangle covering the whole clip space (the classic
    // fullscreen-triangle trick — no vertex buffer needed).
    var verts = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    let p = verts[vid];
    var out: VsOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.uv = p * 0.5 + vec2<f32>(0.5, 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = u.time;
    let bass = clamp(u.energy0, 0.0, 1.0);
    let tr = clamp(u.transient, 0.0, 1.0);

    // Plasma field: bass widens the spatial frequency so the pattern "breathes".
    let scale = 3.0 + bass * 6.0;
    let p = in.uv * scale;
    var v = 0.0;
    v += sin(p.x * 3.0 + t);
    v += sin((p.y * 3.0 + t) * 0.7);
    v += sin((p.x + p.y) * 2.0 + t * 1.3);
    let cx = p.x + 0.5 * sin(t * 0.4);
    let cy = p.y + 0.5 * cos(t * 0.6);
    v += sin(sqrt(cx * cx + cy * cy) * 6.0 + t);
    v = v * 0.25;

    let pi = 3.14159265;
    let r = sin(v * pi + 0.0) * 0.5 + 0.5;
    let g = sin(v * pi + 2.094) * 0.5 + 0.5;
    let b = sin(v * pi + 4.188) * 0.5 + 0.5;
    var col = vec3<f32>(r, g, b);

    // Bass lifts overall brightness; a transient adds a brief white flash.
    col = col * (0.55 + bass * 0.85) + vec3<f32>(tr * 0.6);
    col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(col, 1.0);
}
