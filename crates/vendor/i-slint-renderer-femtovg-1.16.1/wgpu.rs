// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use std::{cell::RefCell, pin::Pin, rc::Rc};

use i_slint_core::platform::PlatformError;
use i_slint_core::renderer::RendererSealed;
use i_slint_core::{api::PhysicalSize as PhysicalWindowSize, graphics::RequestedGraphicsAPI};

use crate::{FemtoVGRenderer, GraphicsBackend, WindowSurface};

use wgpu_28 as wgpu;

/// QBZ vendor patch: per-window swapchain alpha opt-in. When true, a
/// `WGPUBackend` constructed via `new_suspended` captures a preference for a
/// non-opaque composite alpha mode (needed for the borderless transparent
/// miniplayer); when false (default) its surfaces stay Opaque/Auto so the
/// compositor does not alpha-blend the whole main window every frame. The app
/// sets this from its winit window-attributes hook, which runs on the event
/// loop thread right before the window ADAPTER — and therefore this backend —
/// is created. The value is captured PER BACKEND at construction rather than
/// read in `set_surface`: `set_surface` re-runs at every winit re-realization
/// (Wayland destroys the window on hide, and even the first realization is
/// deferred to a later event-loop iteration), long after the hook fired, so
/// reading the global there would leak the last-created window's preference
/// into whichever window is (re)realized next.
static SURFACE_PREFERS_TRANSPARENT: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// QBZ vendor patch: mark whether `WGPUBackend`s constructed from now on
/// prefer a transparent (non-opaque) composite alpha mode for their surfaces.
pub fn set_surface_prefers_transparent(transparent: bool) {
    SURFACE_PREFERS_TRANSPARENT.store(transparent, core::sync::atomic::Ordering::Relaxed);
}

fn preferred_alpha_mode(
    modes: &[wgpu::CompositeAlphaMode],
    transparent: bool,
) -> wgpu::CompositeAlphaMode {
    if transparent {
        [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .into_iter()
        .find(|mode| modes.contains(mode))
        .unwrap_or(wgpu::CompositeAlphaMode::Auto)
    } else if modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
        wgpu::CompositeAlphaMode::Opaque
    } else {
        // Auto is always valid (wgpu resolves it to Opaque or Inherit) and is
        // upstream Slint's behavior, so this is the graceful fallback.
        wgpu::CompositeAlphaMode::Auto
    }
}

pub struct WGPUBackend {
    instance: RefCell<Option<wgpu::Instance>>,
    device: RefCell<Option<wgpu::Device>>,
    queue: RefCell<Option<wgpu::Queue>>,
    surface_config: RefCell<Option<wgpu::SurfaceConfiguration>>,
    surface: RefCell<Option<wgpu::Surface<'static>>>,
    /// QBZ vendor patch: captured from `SURFACE_PREFERS_TRANSPARENT` at
    /// construction — this backend's window keeps the same alpha preference
    /// across every surface re-creation.
    prefers_transparent: bool,
    /// QBZ vendor patch (issue #540): when the compositor stops delivering
    /// frames (Hyprland VFR with a static screen / hidden workspace), every
    /// `get_current_texture()` BLOCKS the UI thread until it times out.
    /// After a timeout we skip frames without touching the surface until
    /// this deadline, doubling the cooldown up to ~2 s while the compositor
    /// stays parked. Reset on the first successful acquire.
    acquire_skip_until: std::cell::Cell<Option<std::time::Instant>>,
    acquire_backoff_ms: std::cell::Cell<u64>,
}

pub struct WGPUWindowSurface {
    surface_texture: wgpu::SurfaceTexture,
}

impl WindowSurface<femtovg::renderer::WGPURenderer> for WGPUWindowSurface {
    fn render_output(&self) -> impl Into<femtovg::renderer::WGPURenderOutput> {
        &self.surface_texture.texture
    }
}

impl GraphicsBackend for WGPUBackend {
    type Renderer = femtovg::renderer::WGPURenderer;
    type WindowSurface = WGPUWindowSurface;
    const NAME: &'static str = "WGPU";

    fn new_suspended() -> Self {
        Self {
            instance: Default::default(),
            device: Default::default(),
            queue: Default::default(),
            surface_config: Default::default(),
            surface: Default::default(),
            prefers_transparent: SURFACE_PREFERS_TRANSPARENT
                .load(core::sync::atomic::Ordering::Relaxed),
            acquire_skip_until: Default::default(),
            acquire_backoff_ms: Default::default(),
        }
    }

    fn clear_graphics_context(&self) {
        self.surface_config.borrow_mut().take();
        self.surface.borrow_mut().take();
        self.queue.borrow_mut().take();
        self.device.borrow_mut().take();
    }

    fn begin_surface_rendering(
        &self,
    ) -> Result<Self::WindowSurface, Box<dyn std::error::Error + Send + Sync>> {
        // QBZ vendor patch (issue #540): during a timeout cooldown, skip the
        // frame WITHOUT touching the surface — each blocked acquire wedges
        // the UI thread for seconds while the compositor is parked.
        if let Some(until) = self.acquire_skip_until.get() {
            if std::time::Instant::now() < until {
                return Err(Box::new(crate::FrameSkipped));
            }
        }
        let surface = self.surface.borrow();
        let surface = surface.as_ref().unwrap();
        let frame = match surface.get_current_texture() {
            Ok(texture) => texture,
            // QBZ vendor patch (issue #540): a surface-acquire timeout is
            // TRANSIENT (Hyprland VFR parks the display when nothing moves;
            // no frame callback will arrive until it resumes). Upstream
            // retried once and propagated the second failure, which killed
            // the event loop and the whole app. Skip the frame instead and
            // back off exponentially (250 ms → 2 s) so repeated redraw
            // requests don't keep blocking the UI thread. First successful
            // acquire resets the cooldown.
            Err(wgpu::SurfaceError::Timeout) => {
                let backoff = (self.acquire_backoff_ms.get().max(125) * 2).min(2000);
                self.acquire_backoff_ms.set(backoff);
                self.acquire_skip_until.set(Some(
                    std::time::Instant::now() + std::time::Duration::from_millis(backoff),
                ));
                static WARNED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    eprintln!(
                        "qbz(femtovg-wgpu): surface acquire timed out — compositor is not \
                         delivering frames (VRR/hidden surface?); skipping frames until it resumes"
                    );
                }
                return Err(Box::new(crate::FrameSkipped));
            }
            // Outdated or lost: reconfigure from the current config and retry
            // ONCE.
            //
            // QBZ vendor patch (issue #558): if the retry is STILL Outdated the
            // reconfigure hasn't caught up with the surface yet — this happens
            // at startup under a non-default UI scale, where SLINT_SCALE_FACTOR
            // changes the physical surface size and the first frames race the
            // winit resize/scale-factor event that reconfigures the swapchain.
            // Upstream `?`-propagated that second failure, which killed the
            // event loop with "The underlying surface has changed, and
            // therefore the swap chain must be updated" and left the app unable
            // to start at ANY interface size other than default (the crash the
            // reporter hit even after the degradation-ladder fix). Treat a
            // persistent Outdated like the #540 timeout instead: skip this
            // frame and back off briefly, never a fatal error. The next
            // resize/redraw reconfigures with the correct dimensions and the
            // following acquire succeeds.
            Err(_) => {
                {
                    let mut device = self.device.borrow_mut();
                    let device = device.as_mut().unwrap();
                    surface.configure(device, self.surface_config.borrow().as_ref().unwrap());
                }
                match surface.get_current_texture() {
                    Ok(texture) => texture,
                    Err(_) => {
                        let backoff = (self.acquire_backoff_ms.get().max(30) * 2).min(1000);
                        self.acquire_backoff_ms.set(backoff);
                        self.acquire_skip_until.set(Some(
                            std::time::Instant::now()
                                + std::time::Duration::from_millis(backoff),
                        ));
                        static WARNED: std::sync::atomic::AtomicBool =
                            std::sync::atomic::AtomicBool::new(false);
                        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            eprintln!(
                                "qbz(femtovg-wgpu): surface still outdated after reconfigure \
                                 (UI-scale/resize race?); skipping frames until it settles"
                            );
                        }
                        return Err(Box::new(crate::FrameSkipped));
                    }
                }
            }
        };
        self.acquire_backoff_ms.set(0);
        self.acquire_skip_until.set(None);
        Ok(WGPUWindowSurface { surface_texture: frame })
    }

    fn submit_commands(&self, commands: <Self::Renderer as femtovg::Renderer>::CommandBuffer) {
        self.queue.borrow().as_ref().unwrap().submit(commands);
    }

    fn present_surface(
        &self,
        surface: Self::WindowSurface,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        surface.surface_texture.present();
        Ok(())
    }

    #[cfg(feature = "unstable-wgpu-28")]
    fn with_graphics_api<R>(
        &self,
        callback: impl FnOnce(Option<i_slint_core::api::GraphicsAPI<'_>>) -> R,
    ) -> Result<R, i_slint_core::platform::PlatformError> {
        let instance = self.instance.borrow().clone();
        let device = self.device.borrow().clone();
        let queue = self.queue.borrow().clone();
        if let (Some(instance), Some(device), Some(queue)) = (instance, device, queue) {
            Ok(callback(Some(i_slint_core::graphics::create_graphics_api_wgpu_28(
                instance, device, queue,
            ))))
        } else {
            Ok(callback(None))
        }
    }

    #[cfg(not(feature = "unstable-wgpu-28"))]
    fn with_graphics_api<R>(
        &self,
        callback: impl FnOnce(Option<i_slint_core::api::GraphicsAPI<'_>>) -> R,
    ) -> Result<R, i_slint_core::platform::PlatformError> {
        Ok(callback(None))
    }

    fn resize(
        &self,
        width: std::num::NonZeroU32,
        height: std::num::NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Try to get hold of the wgpu types, but if we receive the resize event while suspended, ignore it.
        let mut surface_config = self.surface_config.borrow_mut();
        let Some(surface_config) = surface_config.as_mut() else { return Ok(()) };
        let mut device = self.device.borrow_mut();
        let Some(device) = device.as_mut() else { return Ok(()) };
        let mut surface = self.surface.borrow_mut();
        let Some(surface) = surface.as_mut() else { return Ok(()) };

        // Prefer FIFO modes over possible Mailbox setting for frame pacing and better energy efficiency.
        surface_config.present_mode = wgpu::PresentMode::AutoVsync;
        surface_config.width = width.get();
        surface_config.height = height.get();

        surface.configure(device, surface_config);
        Ok(())
    }
}

impl FemtoVGRenderer<WGPUBackend> {
    pub fn set_surface(
        &self,
        surface_target: impl Into<i_slint_core::graphics::wgpu_28::SurfaceTarget>,
        size: PhysicalWindowSize,
        requested_graphics_api: Option<RequestedGraphicsAPI>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let (instance, adapter, device, queue, surface) =
            i_slint_core::graphics::wgpu_28::init_instance_adapter_device_queue_surface(
                surface_target,
                requested_graphics_api,
                /* rendering artifacts :( */
                wgpu::Backends::GL,
            )?;

        let mut surface_config =
            surface.get_default_config(&adapter, size.width, size.height).unwrap();

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities
            .formats
            .iter()
            .find(|f| {
                matches!(f, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm)
            })
            .copied()
            .unwrap_or_else(|| swapchain_capabilities.formats[0]);
        surface_config.format = swapchain_format;
        let wants_transparent = self.graphics_backend.prefers_transparent;
        surface_config.alpha_mode =
            preferred_alpha_mode(&swapchain_capabilities.alpha_modes, wants_transparent);
        i_slint_core::debug_log!(
            "[qbz-slint] femtovg-wgpu surface alpha modes: {:?}; transparent={}; selected: {:?}",
            swapchain_capabilities.alpha_modes,
            wants_transparent,
            surface_config.alpha_mode
        );
        surface.configure(&device, &surface_config);

        *self.graphics_backend.instance.borrow_mut() = Some(instance.clone());
        *self.graphics_backend.device.borrow_mut() = Some(device.clone());
        *self.graphics_backend.queue.borrow_mut() = Some(queue.clone());
        *self.graphics_backend.surface_config.borrow_mut() = Some(surface_config);
        *self.graphics_backend.surface.borrow_mut() = Some(surface);

        let wgpu_renderer = femtovg::renderer::WGPURenderer::new(device, queue);
        let femtovg_canvas = femtovg::Canvas::new_with_text_context(
            wgpu_renderer,
            crate::font_cache::FONT_CACHE.with(|cache| cache.borrow().text_context.clone()),
        )
        .unwrap();

        let canvas = Rc::new(RefCell::new(femtovg_canvas));
        self.reset_canvas(canvas);
        Ok(())
    }
}

struct TextureWindowSurface {
    render_output: femtovg::renderer::WGPURenderOutput,
}

impl WindowSurface<femtovg::renderer::WGPURenderer> for TextureWindowSurface {
    fn render_output(&self) -> impl Into<femtovg::renderer::WGPURenderOutput> {
        self.render_output.clone()
    }
}

struct WgpuTextureBackend {
    instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    render_output: RefCell<Option<femtovg::renderer::WGPURenderOutput>>,
}

impl GraphicsBackend for WgpuTextureBackend {
    type Renderer = femtovg::renderer::WGPURenderer;
    type WindowSurface = TextureWindowSurface;
    const NAME: &'static str = "WGPU Texture";

    fn new_suspended() -> Self {
        panic!("Suspended backend not supported for WgpuTextureBackend (requires device/queue)");
    }

    fn clear_graphics_context(&self) {
        // Nothing to clear here, we don't own the device/queue/texture
    }

    fn begin_surface_rendering(
        &self,
    ) -> Result<Self::WindowSurface, Box<dyn std::error::Error + Send + Sync>> {
        let render_output =
            self.render_output.borrow().clone().ok_or("No texture set for rendering")?;
        Ok(TextureWindowSurface { render_output })
    }

    fn submit_commands(&self, commands: <Self::Renderer as femtovg::Renderer>::CommandBuffer) {
        self.queue.submit(commands);
    }

    fn present_surface(
        &self,
        _surface: Self::WindowSurface,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // No presentation needed - the caller owns the texture and handles presenting it
        Ok(())
    }

    #[cfg(feature = "unstable-wgpu-28")]
    fn with_graphics_api<R>(
        &self,
        callback: impl FnOnce(Option<i_slint_core::api::GraphicsAPI<'_>>) -> R,
    ) -> Result<R, i_slint_core::platform::PlatformError> {
        Ok(callback(Some(i_slint_core::graphics::create_graphics_api_wgpu_28(
            self.instance.clone(),
            self.device.clone(),
            self.queue.clone(),
        ))))
    }

    #[cfg(not(feature = "unstable-wgpu-28"))]
    fn with_graphics_api<R>(
        &self,
        callback: impl FnOnce(Option<i_slint_core::api::GraphicsAPI<'_>>) -> R,
    ) -> Result<R, i_slint_core::platform::PlatformError> {
        Ok(callback(None))
    }

    fn resize(
        &self,
        _width: std::num::NonZeroU32,
        _height: std::num::NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // No resize needed - texture size is determined by the texture passed to render_to_texture
        Ok(())
    }
}

/// Use the FemtoVG renderer with WGPU when implementing a custom Slint platform where you want the scene to be rendered
/// into a WGPU texture. The rendering is done using the [FemtoVG](https://github.com/femtovg/femtovg) library.
pub struct FemtoVGWGPURenderer(FemtoVGRenderer<WgpuTextureBackend>);

impl FemtoVGWGPURenderer {
    /// Creates a new FemtoVGWGPURenderer.
    ///
    /// The `instance`, `device` and `queue` are the WGPU resources used for rendering.
    /// These are also provided to [`Window::set_rendering_notifier()`](i_slint_core::api::Window::set_rendering_notifier) callbacks via [`GraphicsAPI::WGPU28`](i_slint_core::api::GraphicsAPI::WGPU28).
    pub fn new(
        instance: wgpu::Instance,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Result<Self, PlatformError> {
        let backend = WgpuTextureBackend {
            instance,
            device: device.clone(),
            queue: queue.clone(),
            render_output: RefCell::new(None),
        };
        let renderer = FemtoVGRenderer::new_internal(backend);

        let wgpu_renderer = femtovg::renderer::WGPURenderer::new(device, queue);
        let femtovg_canvas = femtovg::Canvas::new_with_text_context(
            wgpu_renderer,
            crate::font_cache::FONT_CACHE.with(|cache| cache.borrow().text_context.clone()),
        )
        .map_err(|e| format!("Failed to create femtovg canvas: {:?}", e))?;

        let canvas = Rc::new(RefCell::new(femtovg_canvas));
        renderer.reset_canvas(canvas);
        Ok(Self(renderer))
    }

    /// Render the scene to the given texture view, with the specified size and format.
    pub fn render_to_texture_view(
        &self,
        texture_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<(), PlatformError> {
        *self.0.graphics_backend.render_output.borrow_mut() =
            Some(femtovg::renderer::WGPURenderOutput {
                view: texture_view.clone(),
                width,
                height,
                format,
            });
        let result = self.0.render();
        *self.0.graphics_backend.render_output.borrow_mut() = None;
        result
    }

    /// Render the scene to the given texture.
    /// This is a convenience method that creates a texture view for the entire texture and calls `render_to_texture_view`.
    pub fn render_to_texture(&self, texture: &wgpu::Texture) -> Result<(), PlatformError> {
        let size = texture.size();
        self.render_to_texture_view(
            &texture.create_view(&wgpu::TextureViewDescriptor::default()),
            size.width,
            size.height,
            texture.format(),
        )
    }
}

#[doc(hidden)]
impl RendererSealed for FemtoVGWGPURenderer {
    fn text_size(
        &self,
        text_item: Pin<&dyn i_slint_core::item_rendering::RenderString>,
        item_rc: &i_slint_core::items::ItemRc,
        max_width: Option<i_slint_core::lengths::LogicalLength>,
        text_wrap: i_slint_core::items::TextWrap,
    ) -> i_slint_core::lengths::LogicalSize {
        self.0.text_size(text_item, item_rc, max_width, text_wrap)
    }

    fn char_size(
        &self,
        text_item: Pin<&dyn i_slint_core::item_rendering::HasFont>,
        item_rc: &i_slint_core::items::ItemRc,
        ch: char,
    ) -> i_slint_core::lengths::LogicalSize {
        self.0.char_size(text_item, item_rc, ch)
    }

    fn font_metrics(
        &self,
        font_request: i_slint_core::graphics::FontRequest,
    ) -> i_slint_core::items::FontMetrics {
        self.0.font_metrics(font_request)
    }

    fn text_input_byte_offset_for_position(
        &self,
        text_input: Pin<&i_slint_core::items::TextInput>,
        item_rc: &i_slint_core::items::ItemRc,
        pos: i_slint_core::lengths::LogicalPoint,
    ) -> usize {
        self.0.text_input_byte_offset_for_position(text_input, item_rc, pos)
    }

    fn text_input_cursor_rect_for_byte_offset(
        &self,
        text_input: Pin<&i_slint_core::items::TextInput>,
        item_rc: &i_slint_core::items::ItemRc,
        byte_offset: usize,
    ) -> i_slint_core::lengths::LogicalRect {
        self.0.text_input_cursor_rect_for_byte_offset(text_input, item_rc, byte_offset)
    }

    fn register_font_from_memory(
        &self,
        data: &'static [u8],
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.0.register_font_from_memory(data)
    }

    fn register_font_from_path(
        &self,
        path: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.0.register_font_from_path(path)
    }

    fn default_font_size(&self) -> i_slint_core::lengths::LogicalLength {
        self.0.default_font_size()
    }

    fn set_rendering_notifier(
        &self,
        callback: Box<dyn i_slint_core::api::RenderingNotifier>,
    ) -> Result<(), i_slint_core::api::SetRenderingNotifierError> {
        self.0.set_rendering_notifier(callback)
    }

    fn free_graphics_resources(
        &self,
        component: i_slint_core::item_tree::ItemTreeRef,
        items: &mut dyn Iterator<Item = Pin<i_slint_core::items::ItemRef<'_>>>,
    ) -> Result<(), PlatformError> {
        self.0.free_graphics_resources(component, items)
    }

    fn set_window_adapter(&self, window_adapter: &Rc<dyn i_slint_core::window::WindowAdapter>) {
        self.0.set_window_adapter(window_adapter)
    }

    fn window_adapter(&self) -> Option<Rc<dyn i_slint_core::window::WindowAdapter>> {
        RendererSealed::window_adapter(&self.0)
    }

    fn resize(&self, size: i_slint_core::api::PhysicalSize) -> Result<(), PlatformError> {
        self.0.resize(size)
    }

    fn take_snapshot(
        &self,
    ) -> Result<
        i_slint_core::graphics::SharedPixelBuffer<i_slint_core::graphics::Rgba8Pixel>,
        PlatformError,
    > {
        self.0.take_snapshot()
    }

    fn supports_transformations(&self) -> bool {
        self.0.supports_transformations()
    }
}
