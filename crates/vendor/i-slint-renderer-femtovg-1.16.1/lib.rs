// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

#![doc = include_str!("README.md")]
#![doc(html_logo_url = "https://slint.dev/logo/slint-logo-square-light.svg")]
#![cfg_attr(slint_nightly_test, feature(non_exhaustive_omitted_patterns_lint))]
#![cfg_attr(slint_nightly_test, warn(non_exhaustive_omitted_patterns))]

use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::pin::Pin;
use std::rc::{Rc, Weak};

use i_slint_core::Brush;
use i_slint_core::api::{RenderingNotifier, RenderingState, SetRenderingNotifierError};
use i_slint_core::graphics::SharedPixelBuffer;
use i_slint_core::graphics::{BorderRadius, Rgba8Pixel};
use i_slint_core::graphics::{euclid, rendering_metrics_collector::RenderingMetricsCollector};
use i_slint_core::item_rendering::ItemRenderer;
use i_slint_core::item_tree::ItemTreeWeak;
use i_slint_core::items::{ItemRc, TextWrap};
use i_slint_core::lengths::{
    LogicalLength, LogicalPoint, LogicalRect, LogicalSize, PhysicalPx, ScaleFactor,
};
use i_slint_core::partial_renderer::PartialRenderingState;
use i_slint_core::platform::PlatformError;
use i_slint_core::renderer::RendererSealed;
use i_slint_core::textlayout::sharedparley;
use i_slint_core::window::{WindowAdapter, WindowInner};
use images::TextureImporter;

type PhysicalLength = euclid::Length<f32, PhysicalPx>;
type PhysicalRect = euclid::Rect<f32, PhysicalPx>;
type PhysicalSize = euclid::Size2D<f32, PhysicalPx>;
type PhysicalPoint = euclid::Point2D<f32, PhysicalPx>;
type PhysicalBorderRadius = BorderRadius<f32, PhysicalPx>;

use self::itemrenderer::CanvasRc;

mod font_cache;
mod images;
mod itemrenderer;
#[cfg(feature = "opengl")]
pub mod opengl;
#[cfg(feature = "wgpu-28")]
pub mod wgpu;
#[cfg(feature = "wgpu-28")]
pub use wgpu::FemtoVGWGPURenderer;

pub trait WindowSurface<R: femtovg::Renderer> {
    fn render_output(&self) -> impl Into<R::RenderOutput>;
}

/// QBZ vendor patch (issue #540): sentinel error a graphics backend returns
/// from `begin_surface_rendering` when the current frame should be silently
/// skipped (e.g. a transient wgpu surface-acquire timeout while the
/// compositor is parked by VRR). The renderer treats it as "nothing to
/// render" instead of a fatal platform error.
#[derive(Debug)]
pub struct FrameSkipped;

impl core::fmt::Display for FrameSkipped {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("frame skipped: surface temporarily unavailable")
    }
}

impl std::error::Error for FrameSkipped {}

pub trait GraphicsBackend {
    type Renderer: femtovg::Renderer + TextureImporter;
    type WindowSurface: WindowSurface<Self::Renderer>;
    const NAME: &'static str;
    fn new_suspended() -> Self;
    fn clear_graphics_context(&self);
    fn begin_surface_rendering(
        &self,
    ) -> Result<Self::WindowSurface, Box<dyn std::error::Error + Send + Sync>>;
    fn submit_commands(&self, commands: <Self::Renderer as femtovg::Renderer>::CommandBuffer);
    fn present_surface(
        &self,
        surface: Self::WindowSurface,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn with_graphics_api<R>(
        &self,
        callback: impl FnOnce(Option<i_slint_core::api::GraphicsAPI<'_>>) -> R,
    ) -> Result<R, i_slint_core::platform::PlatformError>;
    /// This function is called by the renderers when the surface needs to be resized, typically
    /// in response to the windowing system notifying of a change in the window system.
    /// For most implementations this is a no-op, with the exception for wayland for example.
    fn resize(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// QBZ vendor patch (issue #617): partial-rendering mode, env-gated via
/// `SLINT_FEMTOVG_PARTIAL_RENDERING` (mirrors Skia's `SLINT_SKIA_PARTIAL_RENDERING`).
/// Unset / "" / "0" / "off" / "no" = disabled (default; today's full-window
/// repaint straight into the swapchain).
///   - "frozen-blit": stage-2 mode — render the scene into a persistent
///     offscreen texture and blit it whole to the swapchain every frame, dirty
///     tracking OFF. Measures the irreducible acquire+submit+blit+present
///     floor of the partial-rendering design.
///   - any other value ("1", "visualize", "log", …): full partial rendering
///     (stage 3: dirty-region tracking — only the dirty bounding rect of the
///     persistent texture is repainted). "visualize" outlines the dirty rect
///     on the swapchain; "log" prints the repainted percentage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PartialRenderingMode {
    FrozenBlit,
    Enabled { visualize: bool, log: bool },
}

fn partial_rendering_mode() -> Option<PartialRenderingMode> {
    static MODE: std::sync::OnceLock<Option<PartialRenderingMode>> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| {
        let value = std::env::var("SLINT_FEMTOVG_PARTIAL_RENDERING").ok()?.to_ascii_lowercase();
        match value.as_str() {
            "" | "0" | "off" | "no" => None,
            "frozen-blit" => Some(PartialRenderingMode::FrozenBlit),
            "visualize" => Some(PartialRenderingMode::Enabled { visualize: true, log: false }),
            "log" => Some(PartialRenderingMode::Enabled { visualize: false, log: true }),
            _ => Some(PartialRenderingMode::Enabled { visualize: false, log: false }),
        }
    })
}

/// QBZ vendor patch (issue #617): the persistent offscreen scene texture the
/// partial-rendering path renders into. femtovg image passes open with
/// `LoadOp::Load`, so its contents survive across frames — the buffer-age-∞
/// backing store the wgpu swapchain cannot provide.
struct PersistentScene<R: femtovg::Renderer + TextureImporter> {
    texture: Rc<images::Texture<R>>,
    width: u32,
    height: u32,
}

/// Use the FemtoVG renderer when implementing a custom Slint platform where you deliver events to
/// Slint and want the scene to be rendered using OpenGL. The rendering is done using the [FemtoVG](https://github.com/femtovg/femtovg)
/// library.
pub struct FemtoVGRenderer<B: GraphicsBackend> {
    maybe_window_adapter: RefCell<Option<Weak<dyn WindowAdapter>>>,
    rendering_notifier: RefCell<Option<Box<dyn RenderingNotifier>>>,
    canvas: RefCell<Option<CanvasRc<B::Renderer>>>,
    /// #617 stage 2: persistent scene texture for the partial-rendering path;
    /// `None` when the env gate is off. Belongs to the current canvas — dropped
    /// on `reset_canvas` / `clear_graphics_context`.
    persistent_scene: RefCell<Option<PersistentScene<B::Renderer>>>,
    /// #617 stage 3: cross-frame dirty-region bookkeeping (core machinery).
    /// `Some` only when the env gate selects full partial rendering —
    /// "frozen-blit" has no state (it deliberately never tracks dirtiness).
    partial_rendering_state: Option<PartialRenderingState>,
    graphics_cache: itemrenderer::ItemGraphicsCache<B::Renderer>,
    texture_cache: RefCell<images::TextureCache<B::Renderer>>,
    text_layout_cache: sharedparley::TextLayoutCache,
    rendering_metrics_collector: RefCell<Option<Rc<RenderingMetricsCollector>>>,
    rendering_first_time: Cell<bool>,
    // Last field, so that it's dropped last and for example the OpenGL context exists and is current when destroying the FemtoVG canvas
    graphics_backend: B,
}

impl<B: GraphicsBackend> FemtoVGRenderer<B> {
    #[cfg(feature = "wgpu-28")]
    pub(crate) fn new_internal(graphics_backend: B) -> Self {
        Self {
            maybe_window_adapter: Default::default(),
            rendering_notifier: Default::default(),
            canvas: RefCell::new(None),
            persistent_scene: RefCell::new(None),
            partial_rendering_state: match partial_rendering_mode() {
                Some(PartialRenderingMode::Enabled { .. }) => {
                    Some(PartialRenderingState::default())
                }
                _ => None,
            },
            graphics_cache: Default::default(),
            texture_cache: Default::default(),
            text_layout_cache: Default::default(),
            rendering_metrics_collector: Default::default(),
            rendering_first_time: Cell::new(true),
            graphics_backend,
        }
    }

    /// Render the scene using OpenGL.
    pub fn render(&self) -> Result<(), i_slint_core::platform::PlatformError> {
        self.internal_render_with_post_callback(
            0.,
            (0., 0.),
            self.window_adapter()?.window().size(),
            None,
        )
    }

    fn internal_render_with_post_callback(
        &self,
        rotation_angle_degrees: f32,
        translation: (f32, f32),
        surface_size: i_slint_core::api::PhysicalSize,
        post_render_cb: Option<&dyn Fn(&mut dyn ItemRenderer)>,
    ) -> Result<(), i_slint_core::platform::PlatformError> {
        // QBZ vendor patch (issue #540): a FrameSkipped from the backend is
        // "nothing to render this frame", not a fatal error — the next
        // redraw request will try again once the compositor resumes.
        let surface = match self.graphics_backend.begin_surface_rendering() {
            Err(e) if e.is::<FrameSkipped>() => return Ok(()),
            other => other?,
        };

        if self.rendering_first_time.take() {
            *self.rendering_metrics_collector.borrow_mut() = RenderingMetricsCollector::new(
                &format!("FemtoVG renderer with {} backend", B::NAME),
            );

            if let Some(callback) = self.rendering_notifier.borrow_mut().as_mut() {
                self.with_graphics_api(|api| {
                    callback.notify(RenderingState::RenderingSetup, &api)
                })?;
            }
        }

        let window_adapter = self.window_adapter()?;
        let window = window_adapter.window();
        let window_size = window.size();

        let Some((width, height)): Option<(NonZeroU32, NonZeroU32)> =
            window_size.width.try_into().ok().zip(window_size.height.try_into().ok())
        else {
            // Nothing to render
            return Ok(());
        };

        if self.canvas.borrow().is_none() {
            // Nothing to render
            return Ok(());
        }

        let window_inner = WindowInner::from_pub(window);
        let scale = window_inner.scale_factor().ceil();

        window_inner
            .draw_contents(|components| -> Result<(), PlatformError> {
                // self.canvas is checked for being Some(...) at the beginning of this function
                let canvas = self.canvas.borrow().as_ref().unwrap().clone();

                // QBZ vendor patch (issue #617, stage 2): when the partial-rendering
                // env gate is on and this is an untransformed frame, render the scene
                // into a persistent offscreen texture and blit it whole to the
                // swapchain (the "frozen-blit" floor: dirty tracking comes in stage
                // 3). Transformed frames (rotation/translation — unused by desktop
                // QBZ) keep the direct-to-swapchain path.
                let mode = partial_rendering_mode();
                let scene_texture = match mode {
                    Some(_) if rotation_angle_degrees == 0. && translation == (0., 0.) => {
                        let mut persistent = self.persistent_scene.borrow_mut();
                        let size_matches = matches!(
                            persistent.as_ref(),
                            Some(p) if p.width == surface_size.width
                                && p.height == surface_size.height
                        );
                        if !size_matches {
                            // First frame / resize / DPR or UI-scale change: (re)create.
                            *persistent = images::Texture::new_empty_on_gpu(
                                &canvas,
                                surface_size.width,
                                surface_size.height,
                            )
                            .map(|texture| PersistentScene {
                                texture,
                                width: surface_size.width,
                                height: surface_size.height,
                            });
                            // A fresh texture holds no previous frame: the next partial
                            // frame must repaint everything once.
                            if let Some(state) = self.partial_rendering_state.as_ref() {
                                state.force_screen_refresh();
                            }
                            static LOGGED: std::sync::atomic::AtomicBool =
                                std::sync::atomic::AtomicBool::new(false);
                            if persistent.is_some()
                                && !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed)
                            {
                                i_slint_core::debug_log!(
                                    "qbz(femtovg): SLINT_FEMTOVG_PARTIAL_RENDERING active \
                                     ({mode:?}) — persistent scene texture + blit"
                                );
                            }
                        }
                        persistent.as_ref().map(|p| p.texture.clone())
                    }
                    _ => {
                        // Direct-to-swapchain frame: it bypasses the persistent
                        // texture, so if dirty tracking is enabled its contents and
                        // the partial caches are stale — repaint fully next time the
                        // partial path runs.
                        if let Some(state) = self.partial_rendering_state.as_ref() {
                            state.force_screen_refresh();
                        }
                        None
                    }
                };

                // #617 stage 3: dirty-region tracking is live for this frame only
                // when the persistent texture exists and the mode enables it
                // ("frozen-blit" deliberately never tracks).
                let partial_active =
                    scene_texture.is_some() && self.partial_rendering_state.is_some();
                let logical_window_size = i_slint_core::lengths::logical_size_from_api(
                    window.size().to_logical(window_inner.scale_factor()),
                );
                let scale_factor = ScaleFactor::new(window_inner.scale_factor());

                let window_background_brush =
                    window_inner.window_item().map(|w| w.as_pin_ref().background());

                {
                    let mut femtovg_canvas = canvas.borrow_mut();
                    // We pass an integer that is greater than or equal to the scale factor as
                    // dpi / device pixel ratio as the anti-alias of femtovg needs that to draw text clearly.
                    // We need to care about that `ceil()` when calculating metrics.
                    femtovg_canvas.set_size(surface_size.width, surface_size.height, scale);

                    if let Some(scene) = scene_texture.as_ref() {
                        // #617 stage 2: the whole scene (clear included) goes into
                        // the persistent texture instead of the swapchain. MUST come
                        // after set_size: set_size queues a SetRenderTarget(Screen)
                        // command (see the femtovg vendor patch keeping the canvas
                        // field in sync).
                        femtovg_canvas.set_render_target(scene.as_render_target());
                    }

                    // Clear with window background if it is a solid color otherwise it will drawn as gradient.
                    // #617 stage 3: in the partial path this becomes one clear_rect
                    // per dirty rect once the dirty region is known (see below).
                    if !partial_active
                        && let Some(Brush::SolidColor(clear_color)) = window_background_brush
                    {
                        femtovg_canvas.clear_rect(
                            0,
                            0,
                            surface_size.width,
                            surface_size.height,
                            self::itemrenderer::to_femtovg_color(&clear_color),
                        );
                    }
                }

                {
                    let mut femtovg_canvas = canvas.borrow_mut();
                    femtovg_canvas.reset();
                    femtovg_canvas.rotate(rotation_angle_degrees.to_radians());
                    femtovg_canvas.translate(translation.0, translation.1);
                }

                if let Some(notifier_fn) = self.rendering_notifier.borrow_mut().as_mut() {
                    let mut femtovg_canvas = canvas.borrow_mut();
                    // For the BeforeRendering rendering notifier callback it's important that this happens *after* clearing
                    // the back buffer, in order to allow the callback to provide its own rendering of the background.
                    // femtovg's clear_rect() will merely schedule a clear call, so flush right away to make it immediate.

                    let commands = femtovg_canvas.flush_to_output(surface.render_output());
                    self.graphics_backend.submit_commands(commands);

                    femtovg_canvas.set_size(width.get(), height.get(), scale);
                    drop(femtovg_canvas);

                    self.with_graphics_api(|api| {
                        notifier_fn.notify(RenderingState::BeforeRendering, &api)
                    })?;
                }

                if let Some(scene) = scene_texture.as_ref() {
                    // #617: the notifier block above queues SetRenderTarget(Screen)
                    // through set_size — re-assert the scene target for everything
                    // drawn from here on (swallowed as a no-op when no notifier ran).
                    // Design note: a BeforeRendering notifier that actually DREW
                    // would be overwritten by the final Copy blit; QBZ's notifier is
                    // setup-only (device/queue capture), so this is safe here.
                    canvas.borrow_mut().set_render_target(scene.as_render_target());
                }

                self.graphics_cache.clear_cache_if_scale_factor_changed(window);
                self.text_layout_cache.clear_cache_if_scale_factor_changed(window);

                let mut item_renderer = self::itemrenderer::GLItemRenderer::new(
                    &canvas,
                    &self.graphics_cache,
                    &self.texture_cache,
                    &self.text_layout_cache,
                    window,
                    width.get(),
                    height.get(),
                    // #617 stage 2: layers must restore to the target the scene
                    // is actually rendering into (see GLItemRenderer::new).
                    scene_texture
                        .as_ref()
                        .map(|scene| scene.as_render_target())
                        .unwrap_or(femtovg::RenderTarget::Screen),
                );

                // #617 visualize: set by the partial branch so the dirty-rect
                // outline can be stroked on the swapchain after the blit (drawn
                // into the persistent texture it would linger and pile up).
                let mut visualize_dirty_rect: Option<PhysicalRect> = None;

                // #617: both branches yield the concrete item renderer so the
                // blit/flush/drop below is identical for every path.
                let item_renderer = if partial_active {
                    // #617 stage 3: wrap the item renderer in the core's dirty-region
                    // proxy and repaint only the dirty region of the persistent scene
                    // texture (ReusedBuffer semantics — the texture holds the
                    // previous frame, so `None` buffer damage is always valid).
                    let partial_state = self.partial_rendering_state.as_ref().unwrap();
                    let mut partial_renderer =
                        partial_state.create_partial_renderer(item_renderer);
                    let _frame_dirty = partial_state.apply_dirty_region(
                        &mut partial_renderer,
                        components,
                        logical_window_size,
                        None,
                    );

                    // Collapse the (≤3-rect) dirty region to its single bounding
                    // rect so that filter_item, the canvas scissor and the scoped
                    // clears all agree on ONE rect: BoxShadow/Transform/Opacity/
                    // Layer/Clip items bypass filter_item and re-render every frame
                    // (core item_rendering.rs:209-217) — with a multi-rect region
                    // they would re-blend over uncleared pixels in the gaps between
                    // rects, progressively smearing until an unrelated full refresh.
                    partial_renderer.dirty_region =
                        i_slint_core::partial_renderer::DirtyRegion::from(
                            partial_renderer.dirty_region.bounding_rect(),
                        );

                    // Canvas-level scissor on the main pass only (the canvas was
                    // reset above, so this lands in physical pixels). Layer passes
                    // reset their own canvas state and keep rendering full-texture —
                    // exactly what their persistent textures require. NEVER a
                    // wgpu-pass-level scissor: it would clip those layers'
                    // full-texture clears and smear stale texels.
                    let dirty_bounds = partial_renderer.dirty_region.bounding_rect();
                    let phys_dirty = (dirty_bounds * scale_factor).round_out();
                    if matches!(
                        mode,
                        Some(PartialRenderingMode::Enabled { visualize: true, .. })
                    ) {
                        visualize_dirty_rect = Some(phys_dirty);
                    }
                    {
                        let mut femtovg_canvas = canvas.borrow_mut();
                        // An empty dirty region scissors everything away: the proxy
                        // already filters every item out, this just also culls the
                        // always-re-emitted box-shadow draws GPU-side.
                        femtovg_canvas.scissor(
                            phys_dirty.origin.x,
                            phys_dirty.origin.y,
                            phys_dirty.size.width,
                            phys_dirty.size.height,
                        );
                        // clear_rect ignores the canvas scissor — one call per dirty
                        // rect is exactly the scoped clear (a full-window clear would
                        // destroy the reused contents of the persistent texture).
                        if let Some(Brush::SolidColor(clear_color)) = window_background_brush {
                            let clear_color = self::itemrenderer::to_femtovg_color(&clear_color);
                            let surface_rect = PhysicalRect::new(
                                PhysicalPoint::default(),
                                PhysicalSize::new(
                                    surface_size.width as f32,
                                    surface_size.height as f32,
                                ),
                            );
                            for rect in partial_renderer.dirty_region.iter() {
                                let rect = (rect.to_rect() * scale_factor)
                                    .round_out()
                                    .intersection(&surface_rect);
                                if let Some(rect) = rect {
                                    femtovg_canvas.clear_rect(
                                        rect.origin.x as u32,
                                        rect.origin.y as u32,
                                        rect.size.width as u32,
                                        rect.size.height as u32,
                                        clear_color,
                                    );
                                }
                            }
                        }
                    }

                    if matches!(mode, Some(PartialRenderingMode::Enabled { log: true, .. })) {
                        let area_to_repaint: f32 =
                            partial_renderer.dirty_region.iter().map(|b| b.area()).sum();
                        i_slint_core::debug_log!(
                            "qbz(femtovg) partial: repainting {:.2}%",
                            area_to_repaint * 100. / logical_window_size.area()
                        );
                    }

                    if let Some(window_item_rc) = window_inner.window_item_rc() {
                        let window_item =
                            window_item_rc.downcast::<i_slint_core::items::WindowItem>().unwrap();
                        if let Brush::SolidColor(..) = window_item.as_pin_ref().background() {
                            // per-dirty-rect clear_rects were issued above
                        } else {
                            // Draws the window background as gradient (tracked, via the proxy)
                            partial_renderer.draw_rectangle(
                                window_item.as_pin_ref(),
                                &window_item_rc,
                                logical_window_size,
                                &window_item.as_pin_ref().cached_rendering_data,
                            );
                        }
                    }

                    for (component, origin) in components {
                        if let Some(component) = ItemTreeWeak::upgrade(component) {
                            i_slint_core::item_rendering::render_component_items(
                                &component,
                                &mut partial_renderer,
                                *origin,
                                &self.window_adapter()?,
                            );
                        }
                    }

                    if let Some(cb) = post_render_cb.as_ref() {
                        cb(&mut partial_renderer)
                    }

                    let mut item_renderer = partial_renderer.into_inner();

                    if let Some(collector) = &self.rendering_metrics_collector.borrow().as_ref()
                    {
                        let metrics = item_renderer.metrics();
                        collector.measure_frame_rendered(&mut item_renderer, metrics);
                        if collector.refresh_mode()
                            == i_slint_core::graphics::rendering_metrics_collector::RefreshMode::FullSpeed
                        {
                            partial_state.force_screen_refresh();
                        }
                    }
                    item_renderer
                } else {
                    if let Some(window_item_rc) = window_inner.window_item_rc() {
                        let window_item =
                            window_item_rc.downcast::<i_slint_core::items::WindowItem>().unwrap();
                        if let Brush::SolidColor(..) = window_item.as_pin_ref().background() {
                            // clear_rect is called earlier
                        } else {
                            // Draws the window background as gradient
                            item_renderer.draw_rectangle(
                                window_item.as_pin_ref(),
                                &window_item_rc,
                                i_slint_core::lengths::logical_size_from_api(
                                    window.size().to_logical(window_inner.scale_factor()),
                                ),
                                &window_item.as_pin_ref().cached_rendering_data,
                            );
                        }
                    }

                    for (component, origin) in components {
                        if let Some(component) = ItemTreeWeak::upgrade(component) {
                            i_slint_core::item_rendering::render_component_items(
                                &component,
                                &mut item_renderer,
                                *origin,
                                &self.window_adapter()?,
                            );
                        }
                    }

                    if let Some(cb) = post_render_cb.as_ref() {
                        cb(&mut item_renderer)
                    }

                    if let Some(collector) = &self.rendering_metrics_collector.borrow().as_ref()
                    {
                        let metrics = item_renderer.metrics();
                        collector.measure_frame_rendered(&mut item_renderer, metrics);
                    }
                    item_renderer
                };

                if let Some(scene) = scene_texture.as_ref() {
                    // #617 stage 2: blit the persistent scene texture onto the
                    // swapchain. `Copy` ignores the destination — no prior
                    // full-window clear needed, and the transparent
                    // (pre-multiplied) miniplayer swapchain comes out exact,
                    // where a source-over blit of alpha<1 pixels would smear.
                    // Paint first: as_paint() borrows the canvas.
                    let scene_paint = scene.as_paint().with_anti_alias(false);
                    let mut blit_path = femtovg::Path::new();
                    blit_path.rect(
                        0.,
                        0.,
                        surface_size.width as f32,
                        surface_size.height as f32,
                    );
                    let mut femtovg_canvas = canvas.borrow_mut();
                    femtovg_canvas.set_render_target(femtovg::RenderTarget::Screen);
                    femtovg_canvas.save_with(|canvas| {
                        canvas.reset();
                        canvas.global_composite_operation(femtovg::CompositeOperation::Copy);
                        canvas.fill_path(&blit_path, &scene_paint);
                        if let Some(dirty_rect) = visualize_dirty_rect {
                            // #617 visualize: outline the dirty bounding rect on the
                            // swapchain — self-erasing next frame; drawn into the
                            // persistent scene texture it would linger and pile up.
                            canvas.global_composite_operation(
                                femtovg::CompositeOperation::SourceOver,
                            );
                            let mut outline = femtovg::Path::new();
                            outline.rect(
                                dirty_rect.origin.x,
                                dirty_rect.origin.y,
                                dirty_rect.size.width,
                                dirty_rect.size.height,
                            );
                            let mut paint = femtovg::Paint::color(femtovg::Color::rgbaf(
                                1., 0., 0., 0.5,
                            ));
                            paint.set_line_width(2.);
                            canvas.stroke_path(&outline, &paint);
                        }
                    });
                }

                let commands = canvas.borrow_mut().flush_to_output(surface.render_output());
                self.graphics_backend.submit_commands(commands);

                // Delete any images and layer images (and their FBOs) before making the context not current anymore, to
                // avoid GPU memory leaks.
                self.texture_cache.borrow_mut().drain();
                drop(item_renderer);
                Ok(())
            })
            .unwrap_or(Ok(()))?;

        if let Some(callback) = self.rendering_notifier.borrow_mut().as_mut() {
            self.with_graphics_api(|api| callback.notify(RenderingState::AfterRendering, &api))?;
        }

        self.graphics_backend.present_surface(surface)?;
        Ok(())
    }

    fn with_graphics_api(
        &self,
        callback: impl FnOnce(i_slint_core::api::GraphicsAPI<'_>),
    ) -> Result<(), PlatformError> {
        self.graphics_backend.with_graphics_api(|api| callback(api.unwrap()))
    }

    fn window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        self.maybe_window_adapter.borrow().as_ref().and_then(|w| w.upgrade()).ok_or_else(|| {
            "Renderer must be associated with component before use".to_string().into()
        })
    }

    #[cfg(any(feature = "wgpu-28", feature = "opengl"))]
    pub(crate) fn reset_canvas(&self, canvas: CanvasRc<B::Renderer>) {
        // #617: the persistent scene texture belongs to the old canvas's image
        // store — drop it first (its own Rc keeps that canvas alive for the
        // delete_image in Texture::drop).
        self.persistent_scene.borrow_mut().take();
        // Every cached texture (cached layers, images) also belongs to the OLD
        // canvas's image store: after a surface re-creation (Wayland hide/show,
        // miniplayer cycle) their image ids are dangling on the new canvas —
        // a cache-rendering-hint layer then blits a missing image and the whole
        // subtree goes invisible while layout/hit-testing keeps working. Clear
        // both caches so layers re-render and images re-upload on the new
        // canvas (one-time cost per re-creation).
        let _ = self.graphics_backend.with_graphics_api(|_| {
            self.graphics_cache.clear_all();
            self.texture_cache.borrow_mut().clear();
        });
        // #617 stage 4: new canvas/context — the partial caches are stale.
        if let Some(state) = self.partial_rendering_state.as_ref() {
            state.clear_cache();
        }
        *self.canvas.borrow_mut() = canvas.into();
        self.rendering_first_time.set(true);
    }
}

#[doc(hidden)]
impl<B: GraphicsBackend> RendererSealed for FemtoVGRenderer<B> {
    fn text_size(
        &self,
        text_item: Pin<&dyn i_slint_core::item_rendering::RenderString>,
        item_rc: &ItemRc,
        max_width: Option<LogicalLength>,
        text_wrap: TextWrap,
    ) -> LogicalSize {
        sharedparley::text_size(
            self,
            text_item,
            item_rc,
            max_width,
            text_wrap,
            Some(&self.text_layout_cache),
        )
        .unwrap_or_default()
    }

    fn char_size(
        &self,
        text_item: Pin<&dyn i_slint_core::item_rendering::HasFont>,
        item_rc: &i_slint_core::item_tree::ItemRc,
        ch: char,
    ) -> LogicalSize {
        self.slint_context()
            .and_then(|ctx| {
                let mut font_ctx = ctx.font_context().borrow_mut();
                sharedparley::char_size(&mut font_ctx, text_item, item_rc, ch)
            })
            .unwrap_or_default()
    }

    fn font_metrics(
        &self,
        font_request: i_slint_core::graphics::FontRequest,
    ) -> i_slint_core::items::FontMetrics {
        self.slint_context()
            .map(|ctx| {
                let mut font_ctx = ctx.font_context().borrow_mut();
                sharedparley::font_metrics(&mut font_ctx, font_request)
            })
            .unwrap_or_default()
    }

    fn text_input_byte_offset_for_position(
        &self,
        text_input: Pin<&i_slint_core::items::TextInput>,
        item_rc: &i_slint_core::item_tree::ItemRc,
        pos: LogicalPoint,
    ) -> usize {
        sharedparley::text_input_byte_offset_for_position(self, text_input, item_rc, pos)
    }

    fn text_input_cursor_rect_for_byte_offset(
        &self,
        text_input: Pin<&i_slint_core::items::TextInput>,
        item_rc: &i_slint_core::item_tree::ItemRc,
        byte_offset: usize,
    ) -> LogicalRect {
        sharedparley::text_input_cursor_rect_for_byte_offset(self, text_input, item_rc, byte_offset)
    }

    fn register_font_from_memory(
        &self,
        data: &'static [u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ctx = self.slint_context().ok_or("slint platform not initialized")?;
        ctx.font_context().borrow_mut().collection.register_fonts(data.to_vec().into(), None);
        Ok(())
    }

    fn register_font_from_path(
        &self,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let requested_path = path.canonicalize().unwrap_or_else(|_| path.into());
        let contents = std::fs::read(requested_path)?;
        let ctx = self.slint_context().ok_or("slint platform not initialized")?;
        ctx.font_context().borrow_mut().collection.register_fonts(contents.into(), None);
        Ok(())
    }

    fn default_font_size(&self) -> LogicalLength {
        sharedparley::DEFAULT_FONT_SIZE
    }

    fn set_rendering_notifier(
        &self,
        callback: Box<dyn i_slint_core::api::RenderingNotifier>,
    ) -> Result<(), i_slint_core::api::SetRenderingNotifierError> {
        let mut notifier = self.rendering_notifier.borrow_mut();
        if notifier.replace(callback).is_some() {
            Err(SetRenderingNotifierError::AlreadySet)
        } else {
            Ok(())
        }
    }

    fn free_graphics_resources(
        &self,
        component: i_slint_core::item_tree::ItemTreeRef,
        items: &mut dyn Iterator<Item = Pin<i_slint_core::items::ItemRef<'_>>>,
    ) -> Result<(), i_slint_core::platform::PlatformError> {
        self.text_layout_cache.component_destroyed(component);
        if !self.graphics_cache.is_empty() {
            self.graphics_backend.with_graphics_api(|_| {
                self.graphics_cache.component_destroyed(component);
            })?;
        }
        // #617 stage 4: drop the destroyed items' partial-cache entries (and let
        // it force a full refresh — their on-screen regions are unknowable now).
        if let Some(state) = self.partial_rendering_state.as_ref() {
            state.free_graphics_resources(items);
        }
        Ok(())
    }

    // #617 stage 4: forward explicit dirty regions (e.g. popup-window close) to
    // the partial state; the core default is a no-op, which would lose them.
    fn mark_dirty_region(&self, region: i_slint_core::partial_renderer::DirtyRegion) {
        if let Some(state) = self.partial_rendering_state.as_ref() {
            state.mark_dirty_region(region);
        }
    }

    fn set_window_adapter(&self, window_adapter: &Rc<dyn WindowAdapter>) {
        *self.maybe_window_adapter.borrow_mut() = Some(Rc::downgrade(window_adapter));
        self.text_layout_cache.clear_all();
        // #617 stage 4: cached geometries/trackers belong to the previous window.
        if let Some(state) = self.partial_rendering_state.as_ref() {
            state.clear_cache();
        }
        self.graphics_backend
            .with_graphics_api(|_| {
                self.graphics_cache.clear_all();
                self.texture_cache.borrow_mut().clear();
            })
            .ok();
    }

    fn window_adapter(&self) -> Option<Rc<dyn WindowAdapter>> {
        self.maybe_window_adapter
            .borrow()
            .as_ref()
            .and_then(|window_adapter| window_adapter.upgrade())
    }

    fn resize(&self, size: i_slint_core::api::PhysicalSize) -> Result<(), PlatformError> {
        if let Some((width, height)) = size.width.try_into().ok().zip(size.height.try_into().ok()) {
            self.graphics_backend.resize(width, height)?;
        };
        Ok(())
    }

    /// Returns an image buffer of what was rendered last by reading the previous front buffer (using glReadPixels).
    fn take_snapshot(&self) -> Result<SharedPixelBuffer<Rgba8Pixel>, PlatformError> {
        self.graphics_backend.with_graphics_api(|_| {
            let Some(canvas) = self.canvas.borrow().as_ref().cloned() else {
                return Err("FemtoVG renderer cannot take screenshot without a window".into());
            };
            let screenshot = canvas
                .borrow_mut()
                .screenshot()
                .map_err(|e| format!("FemtoVG error reading current back buffer: {e}"))?;

            use rgb::ComponentBytes;
            Ok(SharedPixelBuffer::clone_from_slice(
                screenshot.buf().as_bytes(),
                screenshot.width() as u32,
                screenshot.height() as u32,
            ))
        })?
    }

    fn supports_transformations(&self) -> bool {
        true
    }
}

impl<B: GraphicsBackend> Drop for FemtoVGRenderer<B> {
    fn drop(&mut self) {
        self.clear_graphics_context().ok();
    }
}

/// The purpose of this trait is to add internal API that's accessed from the winit/linuxkms backends, but not
/// public (as the trait isn't re-exported).
#[doc(hidden)]
pub trait FemtoVGRendererExt {
    fn new_suspended() -> Self;
    fn clear_graphics_context(&self) -> Result<(), i_slint_core::platform::PlatformError>;
    fn render_transformed_with_post_callback(
        &self,
        rotation_angle_degrees: f32,
        translation: (f32, f32),
        surface_size: i_slint_core::api::PhysicalSize,
        post_render_cb: Option<&dyn Fn(&mut dyn ItemRenderer)>,
    ) -> Result<(), i_slint_core::platform::PlatformError>;
}

/// The purpose of this trait is to add internal API specific to the OpenGL renderer that's accessed from the winit
/// backend. In this case, the ability to resume a suspended OpenGL renderer by providing a new context.
#[doc(hidden)]
#[cfg(feature = "opengl")]
pub trait FemtoVGOpenGLRendererExt {
    fn set_opengl_context(
        &self,
        #[cfg(not(target_arch = "wasm32"))] opengl_context: impl opengl::OpenGLInterface + 'static,
        #[cfg(target_arch = "wasm32")] html_canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), i_slint_core::platform::PlatformError>;
}

#[doc(hidden)]
impl<B: GraphicsBackend> FemtoVGRendererExt for FemtoVGRenderer<B> {
    /// Creates a new renderer in suspended state without OpenGL. Any attempts at rendering, etc. will produce an error,
    /// until [`Self::set_opengl_context()`] was called successfully.
    fn new_suspended() -> Self {
        Self {
            maybe_window_adapter: Default::default(),
            rendering_notifier: Default::default(),
            canvas: RefCell::new(None),
            persistent_scene: RefCell::new(None),
            partial_rendering_state: match partial_rendering_mode() {
                Some(PartialRenderingMode::Enabled { .. }) => {
                    Some(PartialRenderingState::default())
                }
                _ => None,
            },
            graphics_cache: Default::default(),
            texture_cache: Default::default(),
            text_layout_cache: Default::default(),
            rendering_metrics_collector: Default::default(),
            rendering_first_time: Cell::new(true),
            graphics_backend: B::new_suspended(),
        }
    }

    fn clear_graphics_context(&self) -> Result<(), i_slint_core::platform::PlatformError> {
        // Ensure the context is current before the renderer is destroyed
        self.graphics_backend.with_graphics_api(|api| {
            // If we've rendered a frame before, then we need to invoke the RenderingTearDown notifier.
            if !self.rendering_first_time.get()
                && api.is_some()
                && let Some(callback) = self.rendering_notifier.borrow_mut().as_mut()
            {
                self.with_graphics_api(|api| {
                    callback.notify(RenderingState::RenderingTeardown, &api)
                })
                .ok();
            }

            self.graphics_cache.clear_all();
            self.texture_cache.borrow_mut().clear();
        })?;

        self.text_layout_cache.clear_all();

        // #617: drop the persistent scene texture before the canvas below (its
        // Drop deletes the image from the canvas it was created on).
        self.persistent_scene.borrow_mut().take();

        if let Some(canvas) = self.canvas.borrow_mut().take()
            && Rc::strong_count(&canvas) != 1
        {
            i_slint_core::debug_log!(
                "internal warning: there are canvas references left when destroying the window. OpenGL resources will be leaked."
            )
        }

        self.graphics_backend.clear_graphics_context();

        Ok(())
    }

    fn render_transformed_with_post_callback(
        &self,
        rotation_angle_degrees: f32,
        translation: (f32, f32),
        surface_size: i_slint_core::api::PhysicalSize,
        post_render_cb: Option<&dyn Fn(&mut dyn ItemRenderer)>,
    ) -> Result<(), i_slint_core::platform::PlatformError> {
        self.internal_render_with_post_callback(
            rotation_angle_degrees,
            translation,
            surface_size,
            post_render_cb,
        )
    }
}

#[cfg(feature = "opengl")]
impl FemtoVGOpenGLRendererExt for FemtoVGRenderer<opengl::OpenGLBackend> {
    fn set_opengl_context(
        &self,
        #[cfg(not(target_arch = "wasm32"))] opengl_context: impl opengl::OpenGLInterface + 'static,
        #[cfg(target_arch = "wasm32")] html_canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), i_slint_core::platform::PlatformError> {
        self.graphics_backend.set_opengl_context(
            self,
            #[cfg(not(target_arch = "wasm32"))]
            opengl_context,
            #[cfg(target_arch = "wasm32")]
            html_canvas,
        )
    }
}

#[cfg(feature = "opengl")]
pub type FemtoVGOpenGLRenderer = FemtoVGRenderer<opengl::OpenGLBackend>;
