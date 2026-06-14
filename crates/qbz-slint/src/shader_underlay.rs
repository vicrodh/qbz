//! WGPU UNDERLAY SPIKE — GPU fragment-shader background for the ImmersiveView.
//!
//! Validates the renderer-swap path (`renderer-femtovg` GL -> `renderer-femtovg-wgpu`)
//! by rendering a WGSL fragment shader into a wgpu texture and handing it back to
//! Slint as an `Image` (the texture-into-scene shape from Slint's upstream
//! `wgpu_texture` example). The texture is bound to an `Image` placed at the
//! bottom of `ImmersiveView`'s z-stack (see ui/immersive/ImmersiveView.slint).
//!
//! Lifecycle:
//!   * `setup()` is called once by the rendering notifier in main.rs at
//!     `RenderingState::RenderingSetup`, with Slint's OWN wgpu Device/Queue —
//!     mandatory so `Image::try_from` operates on the same device Slint renders
//!     with. It builds the pipeline + uniform buffer + bind group (persistent).
//!   * `render_frame()` is called from the 30 fps drain in visualizer.rs while a
//!     shader scene is active. It renders one frame into a FRESH texture (the
//!     documented upstream pattern — avoids read-while-write aliasing) and
//!     returns an `Image`. The caller sets it on `ImmersiveState.shader-texture`.
//!   * `teardown()` clears the state at `RenderingState::RenderingTeardown`.
//!
//! All three run on the UI thread (notifier + Timer share it), so the state lives
//! in a `thread_local`. This file is downstream of the read-only visualizer feed
//! and touches NONE of the protected audio backend.

use std::cell::RefCell;

use slint::wgpu_28::wgpu;
use slint::Image;

/// Offscreen render target size. The `Image` is shown with `image-fit: fill`, so
/// the immersive viewport stretches this to fit. A plasma is organic/soft, so a
/// fixed 720p target is plenty sharp for the spike (HiDPI-correct sizing is a
/// Phase-1 concern, not a spike gate).
const TEX_W: u32 = 1280;
const TEX_H: u32 = 720;
const TEX_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Mirrors the WGSL `Uniforms` struct (ui/shaders/plasma.wgsl). `#[repr(C)]` +
/// explicit padding keeps it vec4-aligned (32 bytes) for std140 uniform layout.
#[repr(C)]
#[derive(Clone, Copy)]
struct Uniforms {
    time: f32,
    energy0: f32,
    transient: f32,
    _pad0: f32,
    res_x: f32,
    res_y: f32,
    _pad1: f32,
    _pad2: f32,
}

/// View `Uniforms` as raw bytes for `Queue::write_buffer`. Sound: `Uniforms` is
/// `#[repr(C)]`, all-`f32`, no padding holes with undefined values we read back —
/// every byte is part of a defined `f32` field.
fn uniforms_bytes(u: &Uniforms) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            (u as *const Uniforms) as *const u8,
            std::mem::size_of::<Uniforms>(),
        )
    }
}

struct RenderState {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    start: std::time::Instant,
}

thread_local! {
    static STATE: RefCell<Option<RenderState>> = const { RefCell::new(None) };
}

/// Build the persistent pipeline from Slint's device/queue. Called once at
/// `RenderingSetup`. Idempotent-ish: a second call rebuilds (cheap; only happens
/// if the rendering surface is torn down and re-created).
pub fn setup(device: wgpu::Device, queue: wgpu::Queue) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("qbz-plasma-shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../ui/shaders/plasma.wgsl").into()),
    });

    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("qbz-shader-bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });

    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("qbz-shader-uniforms"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("qbz-shader-bg"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("qbz-shader-pl"),
        bind_group_layouts: &[&bgl],
        // wgpu 28.x: replaces `push_constant_ranges`. We use none.
        immediate_size: 0,
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("qbz-shader-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: TEX_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });

    STATE.with(|s| {
        *s.borrow_mut() = Some(RenderState {
            device,
            queue,
            pipeline,
            uniform_buf,
            bind_group,
            start: std::time::Instant::now(),
        });
    });
    log::info!("[shader] wgpu underlay pipeline ready ({TEX_W}x{TEX_H} {TEX_FORMAT:?})");
}

/// Drop the pipeline at surface teardown.
pub fn teardown() {
    STATE.with(|s| *s.borrow_mut() = None);
    log::info!("[shader] wgpu underlay torn down");
}

/// Render one frame into a fresh texture and return it as a Slint `Image`.
/// Returns `None` before `setup()` has run. Driven at 30 fps from visualizer.rs.
pub fn render_frame(energy0: f32, transient: f32) -> Option<Image> {
    STATE.with(|s| {
        let borrow = s.borrow();
        let st = borrow.as_ref()?;

        let uniforms = Uniforms {
            time: st.start.elapsed().as_secs_f32(),
            energy0,
            transient,
            _pad0: 0.0,
            res_x: TEX_W as f32,
            res_y: TEX_H as f32,
            _pad1: 0.0,
            _pad2: 0.0,
        };
        st.queue
            .write_buffer(&st.uniform_buf, 0, uniforms_bytes(&uniforms));

        // Fresh target each frame. Image::try_from REQUIRES Rgba8Unorm/Srgb +
        // TEXTURE_BINDING | RENDER_ATTACHMENT (Slint graphics/wgpu_28.rs).
        let texture = st.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("qbz-shader-target"),
            size: wgpu::Extent3d {
                width: TEX_W,
                height: TEX_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEX_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = st
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("qbz-shader-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("qbz-shader-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                // wgpu 28.x render passes also carry the multiview layer mask;
                // we don't use multiview (single 2D target), so None.
                multiview_mask: None,
            });
            pass.set_pipeline(&st.pipeline);
            pass.set_bind_group(0, &st.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        st.queue.submit(Some(encoder.finish()));

        match Image::try_from(texture) {
            Ok(img) => Some(img),
            Err(e) => {
                log::warn!("[shader] Image::try_from failed: {e:?}");
                None
            }
        }
    })
}
