use std::sync::mpsc;
use std::sync::Arc;

use scene_renderer::{
    CaptureSource, ContextDesc, GpuContext, RenderView, Renderer, RendererDesc, RendererError,
    SceneId, ShaderSet, ViewDesc,
};
use scene_types::{Camera, MaterialTypeDesc, ViewParams};
use winit::event::{DeviceEvent, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

use crate::handle::{LightSettings, LoadTicket, ViewerEvent, ViewerHandle, ViewerState};
use crate::navigation::{aspect_of, Bounds, CameraController};
use crate::presenter::{Acquire, SurfacePresenter};

/// Options for a [`Viewer`].
#[derive(Debug, Clone)]
pub struct ViewerConfig {
    pub context: ContextDesc,
    /// Internal resolution multiplier.
    pub render_scale: f32,
    pub clear_color: [f64; 4],
    pub camera: Camera,
    pub camera_speed: f32,
    pub light: LightSettings,
    /// Key that requests a capture of the next frame.
    pub capture_key: Option<KeyCode>,
    /// Material types registered after the renderer's stock types, e.g. a
    /// host's own shaders loaded with [`crate::assets`].
    pub material_types: Vec<MaterialTypeDesc>,
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self {
            context: ContextDesc::default(),
            render_scale: 1.0,
            clear_color: [0.0, 0.0, 0.0, 1.0],
            camera: Camera::default(),
            camera_speed: 0.2,
            light: LightSettings::default(),
            capture_key: Some(KeyCode::Backspace),
            material_types: Vec::new(),
        }
    }
}

/// Presentation, rendering and navigation for one window or canvas.
pub struct Viewer {
    window: Arc<Window>,
    ctx: GpuContext,
    presenter: SurfacePresenter,
    renderer: Renderer,
    view: RenderView,
    config: ViewerConfig,
    scene: Option<SceneId>,
    first_frame: Option<LoadTicket>,
    camera: Camera,
    controller: CameraController,
    light: LightSettings,
    handle: ViewerHandle,
    captures_tx: mpsc::Sender<ViewerEvent>,
    captures_rx: mpsc::Receiver<ViewerEvent>,
    pending_captures: std::collections::VecDeque<String>,
    device_lost: Arc<std::sync::Mutex<Option<String>>>,
}

impl Viewer {
    pub async fn new(
        window: Arc<Window>,
        shaders: &ShaderSet,
        config: ViewerConfig,
        handle: ViewerHandle,
    ) -> Result<Self, RendererError> {
        let size = window.inner_size();
        let instance = config.context.create_instance();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| RendererError::Device(e.to_string()))?;
        let ctx = GpuContext::new(instance, Some(&surface), &config.context).await?;
        let device_lost = Arc::new(std::sync::Mutex::new(None));
        {
            let device_lost = device_lost.clone();
            ctx.device.set_device_lost_callback(move |reason, message| {
                if let Ok(mut lost) = device_lost.lock() {
                    *lost = Some(format!("{:?}: {}", reason, message));
                }
            });
        }
        let presenter = SurfacePresenter::new(&ctx, surface, size.width, size.height)?;
        let mut renderer = Renderer::new(&ctx, shaders, RendererDesc::new(presenter.format()))?;
        for material_type in &config.material_types {
            renderer.register_material_type(material_type)?;
        }
        let view = renderer.create_view(ViewDesc {
            width: size.width,
            height: size.height,
            render_scale: config.render_scale,
        })?;
        let (captures_tx, captures_rx) = mpsc::channel();
        let controller = CameraController::new(window.clone(), &config.camera, config.camera_speed);
        Ok(Self {
            window,
            ctx,
            presenter,
            renderer,
            view,
            scene: None,
            first_frame: None,
            camera: config.camera,
            controller,
            light: config.light,
            config,
            handle,
            captures_tx,
            captures_rx,
            pending_captures: Default::default(),
            device_lost,
        })
    }

    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn handle(&self) -> &ViewerHandle {
        &self.handle
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.presenter.resize(&self.ctx, width, height);
        let desc = ViewDesc {
            width,
            height,
            render_scale: self.config.render_scale,
        };
        if let Err(e) = self.renderer.resize_view(&mut self.view, desc) {
            log::error!("resize failed: {}", e);
        }
        self.handle.set_state(if self.presenter.is_suspended() {
            ViewerState::Suspended
        } else {
            ViewerState::Ready
        });
    }

    /// Feed a raw device event (captured mouse motion).
    pub fn handle_device_event(&mut self, event: &DeviceEvent) {
        self.controller.device_event(event);
    }

    /// Release navigation input, e.g. when the application is suspended.
    pub fn deactivate_navigation(&mut self) {
        self.controller.deactivate();
    }

    /// Feed a window event. Returns true when it was consumed.
    pub fn handle_window_event(&mut self, event: &WindowEvent) -> bool {
        if self.controller.process_event(event) {
            return true;
        }
        match event {
            WindowEvent::KeyboardInput { event: key, .. } => {
                if let (Some(capture_key), PhysicalKey::Code(code)) =
                    (self.config.capture_key, key.physical_key)
                {
                    if code == capture_key && key.state.is_pressed() {
                        self.handle.request_capture("frame");
                        return true;
                    }
                }
                false
            }
            WindowEvent::Resized(size) => {
                self.resize(size.width, size.height);
                false
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = self.window.inner_size();
                self.resize(size.width, size.height);
                false
            }
            _ => false,
        }
    }

    fn apply_commands(&mut self) {
        let commands = self.handle.take_commands();
        if let Some(pending) = commands.scene {
            match self.renderer.upload_scene(&pending.scene) {
                Ok((id, report)) => {
                    if let Some(old) = self.scene.replace(id) {
                        self.renderer.release_scene(old);
                    }
                    if pending.frame_camera {
                        if let Some(bounds) = pending.scene.bounds() {
                            let radius = bounds.radius();
                            self.camera = Camera::framing(bounds.center(), radius);
                            if let Some(nav_bounds) = Bounds::new(bounds.center(), radius) {
                                let aspect = aspect_of(&self.window);
                                self.controller.set_bounds(
                                    nav_bounds,
                                    &mut self.camera,
                                    aspect,
                                    true,
                                );
                            }
                        }
                    }
                    self.first_frame = Some(pending.ticket);
                    self.handle.emit(ViewerEvent::SceneUploaded {
                        ticket: pending.ticket,
                        report,
                    });
                }
                Err(e) => {
                    log::error!("scene upload failed: {}", e);
                    self.handle.emit(ViewerEvent::SceneFailed {
                        ticket: pending.ticket,
                        error: e.to_string(),
                    });
                }
            }
        }
        if let Some(camera) = commands.camera {
            self.camera = camera;
            let aspect = aspect_of(&self.window);
            self.controller.sync_pose(&self.camera, aspect);
        }
        if let Some(speed) = commands.camera_speed {
            self.controller.set_speed(speed);
        }
        if let Some(light) = commands.light {
            self.light = light;
        }
        self.pending_captures = commands.captures;
    }

    /// Apply commands, advance navigation/animation, render and present.
    ///
    /// After device loss, rendering stops and the handle reports
    /// [`ViewerState::DeviceLost`]; hosts recover by creating a new viewer and
    /// re-sending their scene.
    pub fn redraw(&mut self) {
        let lost = self.device_lost.lock().ok().and_then(|mut l| l.take());
        if let Some(reason) = lost {
            log::error!("GPU device lost: {}", reason);
            self.handle.set_state(ViewerState::DeviceLost(reason));
        }
        if matches!(self.handle.state(), ViewerState::DeviceLost(_)) {
            return;
        }
        self.apply_commands();
        if self.light.auto_orbit {
            use cgmath::Rotation3;
            let rotation =
                cgmath::Quaternion::from_axis_angle(cgmath::Vector3::unit_y(), cgmath::Deg(0.5));
            let dir: cgmath::Vector3<f32> = self.light.light.direction.into();
            self.light.light.direction = (rotation * dir).into();
        }
        let aspect = aspect_of(&self.window);
        if self.controller.update_camera(&mut self.camera, aspect) {
            self.handle.request_capture("frame");
        }

        let frame = match self.presenter.acquire(&self.ctx) {
            Ok(Acquire::Frame(frame)) => frame,
            Ok(Acquire::Skip) => return,
            Err(e) => {
                log::error!("{}", e);
                return;
            }
        };
        let target = frame.texture.create_view(&Default::default());
        let params = ViewParams {
            camera: self.camera,
            light: self.light.light,
            clear_color: self.config.clear_color,
        };
        let mut encoder = self.ctx.create_encoder("viewer frame");
        let receipt =
            match self
                .renderer
                .encode(&mut encoder, &self.view, self.scene, &params, &target)
            {
                Ok(receipt) => receipt,
                Err(e) => {
                    log::error!("encode failed: {}", e);
                    return;
                }
            };
        let mut captures = Vec::new();
        for label in std::mem::take(&mut self.pending_captures) {
            let scene = self.renderer.encode_capture(
                &mut encoder,
                &receipt,
                CaptureSource::SceneColor(&self.view),
                &format!("{}_scene", label),
            );
            let output = self.renderer.encode_capture(
                &mut encoder,
                &receipt,
                CaptureSource::Texture(&frame.texture),
                &format!("{}_output", label),
            );
            for capture in [scene, output] {
                match capture {
                    Ok(c) => captures.push(c),
                    Err(e) => self.handle.emit(ViewerEvent::CaptureFailed(e.to_string())),
                }
            }
        }
        self.ctx.submit(encoder);
        frame.present();
        for capture in captures {
            let tx = self.captures_tx.clone();
            capture.map(move |result| {
                let _ = tx.send(match result {
                    Ok(image) => ViewerEvent::CaptureReady(image),
                    Err(e) => ViewerEvent::CaptureFailed(e.to_string()),
                });
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.ctx.device.poll(scene_renderer::wgpu::PollType::Poll);
        while let Ok(event) = self.captures_rx.try_recv() {
            self.handle.emit(event);
        }
        if let Some(ticket) = self.first_frame.take() {
            self.handle.emit(ViewerEvent::FirstFrameSubmitted {
                ticket,
                frame: receipt.frame,
            });
        }
    }
}
