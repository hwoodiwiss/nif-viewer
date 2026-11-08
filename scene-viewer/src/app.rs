//! Optional winit runner. Hosts with their own event loop can drive
//! [`Viewer`] directly instead.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use scene_renderer::ShaderSet;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::handle::{ViewerHandle, ViewerState};
use crate::viewer::{Viewer, ViewerConfig};

type FrameHook = Box<dyn FnMut(&ViewerHandle)>;

/// Window and runner options.
pub struct AppConfig {
    pub title: String,
    pub initial_size: (u32, u32),
    /// Exit the event loop on Escape (native only).
    pub exit_on_escape: bool,
    /// Render into this canvas instead of a winit-created one (web only).
    #[cfg(target_arch = "wasm32")]
    pub canvas: Option<web_sys::HtmlCanvasElement>,
    /// Inline style applied to the canvas after its first non-zero resize (web only).
    #[cfg(target_arch = "wasm32")]
    pub resized_canvas_style: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            title: "Scene Viewer".into(),
            initial_size: (1280, 720),
            exit_on_escape: true,
            #[cfg(target_arch = "wasm32")]
            canvas: None,
            #[cfg(target_arch = "wasm32")]
            resized_canvas_style: None,
        }
    }
}

/// A single-window winit application hosting one [`Viewer`].
pub struct ViewerApp {
    app: AppConfig,
    config: ViewerConfig,
    shaders: ShaderSet,
    handle: ViewerHandle,
    window: Option<Arc<Window>>,
    viewer: Rc<RefCell<Option<Viewer>>>,
    sized: bool,
    on_frame: Option<FrameHook>,
}

impl ViewerApp {
    pub fn new(
        app: AppConfig,
        config: ViewerConfig,
        shaders: ShaderSet,
        handle: ViewerHandle,
    ) -> Self {
        Self {
            app,
            config,
            shaders,
            handle,
            window: None,
            viewer: Rc::new(RefCell::new(None)),
            sized: false,
            on_frame: None,
        }
    }

    /// Called after every redraw, e.g. to drain [`ViewerHandle::drain_events`].
    pub fn on_frame(mut self, hook: impl FnMut(&ViewerHandle) + 'static) -> Self {
        self.on_frame = Some(Box::new(hook));
        self
    }

    /// Run on the current thread until the window closes (native).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run(mut self) -> Result<(), winit::error::EventLoopError> {
        let event_loop = EventLoop::new()?;
        event_loop.run_app(&mut self)
    }

    /// Start the browser event loop and return immediately (web).
    #[cfg(target_arch = "wasm32")]
    pub fn spawn(self) -> Result<(), winit::error::EventLoopError> {
        use winit::platform::web::EventLoopExtWebSys;
        EventLoop::new()?.spawn_app(self);
        Ok(())
    }
}

impl ApplicationHandler for ViewerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title(self.app.title.clone())
            .with_inner_size(PhysicalSize::new(
                self.app.initial_size.0,
                self.app.initial_size.1,
            ));
        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            if let Some(canvas) = self.app.canvas.clone() {
                attrs = attrs.with_canvas(Some(canvas));
            }
        }
        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                self.handle.set_state(ViewerState::Failed(e.to_string()));
                return;
            }
        };
        self.window = Some(window.clone());

        let shaders = self.shaders.clone();
        let config = self.config.clone();
        let handle = self.handle.clone();
        let cell = self.viewer.clone();
        let create = async move {
            match Viewer::new(window, &shaders, config, handle.clone()).await {
                Ok(viewer) => {
                    *cell.borrow_mut() = Some(viewer);
                    handle.set_state(ViewerState::Ready);
                }
                Err(e) => {
                    log::error!("viewer initialisation failed: {}", e);
                    handle.set_state(ViewerState::Failed(e.to_string()));
                }
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        futures::executor::block_on(create);
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(create);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let mut slot = self.viewer.borrow_mut();
        let Some(viewer) = slot.as_mut() else {
            return;
        };
        if !self.sized {
            // Resizes delivered while the viewer was initialising were dropped.
            self.sized = true;
            let size = viewer.window().inner_size();
            viewer.resize(size.width, size.height);
        }
        #[cfg(target_arch = "wasm32")]
        if let (WindowEvent::Resized(size), Some(style)) = (&event, &self.app.resized_canvas_style)
        {
            use winit::platform::web::WindowExtWebSys;
            if size.width > 0 && size.height > 0 {
                if let Some(canvas) = viewer.window().canvas() {
                    let _ = canvas.set_attribute("style", style);
                }
            }
        }
        if viewer.handle_window_event(&event) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.handle.set_state(ViewerState::Disposed);
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event: ref key, .. }
                if self.app.exit_on_escape
                    && key.state == ElementState::Pressed
                    && key.logical_key == Key::Named(NamedKey::Escape) =>
            {
                self.handle.set_state(ViewerState::Disposed);
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                viewer.redraw();
                drop(slot);
                if let Some(hook) = self.on_frame.as_mut() {
                    hook(&self.handle);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
