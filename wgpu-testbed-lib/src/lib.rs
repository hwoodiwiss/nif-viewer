#![allow(dead_code)]

use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
use crate::state::MappedTextureView;
use crate::state::State;

mod camera;
mod file_reader;
mod instance;
mod light;
mod model;
mod pipeline;
mod state;
mod texture;
mod uniform;
mod vertex;

struct App {
    window: Option<Arc<Window>>,
    state: Option<State<'static>>,
    #[cfg(not(target_arch = "wasm32"))]
    texture_copy_sender: Option<mpsc::Sender<MappedTextureView>>,
    #[cfg(not(target_arch = "wasm32"))]
    screenshot_writer: Option<std::thread::JoinHandle<()>>,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            state: None,
            #[cfg(not(target_arch = "wasm32"))]
            texture_copy_sender: None,
            #[cfg(not(target_arch = "wasm32"))]
            screenshot_writer: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window_size = PhysicalSize::new(1920, 1080);
        let window_attrs = Window::default_attributes()
            .with_title("WGPU Rendering")
            .with_inner_size(window_size);

        let window = Arc::new(
            event_loop
                .create_window(window_attrs)
                .expect("Failed to create window!"),
        );

        #[cfg(not(target_arch = "wasm32"))]
        {
            let (texture_copy_sender, texture_copy_receiver) = mpsc::channel();
            self.texture_copy_sender = Some(texture_copy_sender);

            self.screenshot_writer = Some(std::thread::spawn({
                let receiver = texture_copy_receiver;
                move || {
                    let mut counter = 0u32;
                    while let Ok(mapped_view) = receiver.recv() {
                        // Strip row padding — each row in the buffer may have trailing bytes
                        // to satisfy wgpu's COPY_BYTES_PER_ROW_ALIGNMENT (256-byte) requirement.
                        let mut pixels: Vec<u8> = Vec::with_capacity(
                            (mapped_view.unpadded_bytes_per_row * mapped_view.height) as usize,
                        );
                        for row in 0..mapped_view.height {
                            let start = (row * mapped_view.padded_bytes_per_row) as usize;
                            let end = start + mapped_view.unpadded_bytes_per_row as usize;
                            pixels.extend_from_slice(&mapped_view.data[start..end]);
                        }

                        // Convert BGRA → RGBA if the surface format is BGRA (common on DX12/Windows).
                        match mapped_view.format {
                            wgpu::TextureFormat::Bgra8Unorm
                            | wgpu::TextureFormat::Bgra8UnormSrgb => {
                                for chunk in pixels.chunks_mut(4) {
                                    chunk.swap(0, 2);
                                }
                            }
                            _ => {}
                        }

                        let filename =
                            format!("screenshot_{:04}_{}.png", counter, mapped_view.name);
                        counter += 1;

                        match image::RgbaImage::from_raw(
                            mapped_view.width,
                            mapped_view.height,
                            pixels,
                        ) {
                            Some(img) => match img.save(&filename) {
                                Ok(_) => println!("Saved {}", filename),
                                Err(e) => eprintln!("Failed to save {}: {}", filename, e),
                            },
                            None => eprintln!(
                                "Buffer size mismatch for {} ({}x{})",
                                filename, mapped_view.width, mapped_view.height
                            ),
                        }
                    }
                }
            }));
        }

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowExtWebSys;

            let canvas = window.canvas().expect("Could not get canvas reference");

            let web_window = web_sys::window().expect("Could not get window reference");
            let document = web_window
                .document()
                .expect("Could not get document reference");
            let body = document.body().expect("Could not get body reference");

            body.append_child(&canvas)
                .expect("Append canvas to HTML body");

            canvas
                .set_attribute("style", "width: 100%; aspect-ratio: 16/9;")
                .expect("Set canvas style");
        }

        self.window = Some(window.clone());

        #[cfg(not(target_arch = "wasm32"))]
        {
            let state = futures::executor::block_on(State::new(
                window,
                self.texture_copy_sender.as_ref().unwrap().clone(),
            ));
            self.state = Some(state);
        }

        #[cfg(target_arch = "wasm32")]
        {
            // On wasm32, async tasks can't block. Spawn the init and store
            // state once ready; events are silently skipped until state is Some.
            let state_cell: Rc<RefCell<Option<State<'static>>>> = Rc::new(RefCell::new(None));
            // SAFETY: we hold a reference to self.state_cell for the lifetime of App,
            // which outlives the spawn. We use a shared Rc so the closure can store it.
            let state_cell_clone = state_cell.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let state = State::new(window).await;
                *state_cell_clone.borrow_mut() = Some(state);
            });
            // Replace self.state with the cell's contents once filled; for wasm
            // we use a side-channel via the Rc. Store the Rc in a thread-local
            // so window_event can poll it.
            WASM_STATE_CELL.with(|cell| {
                *cell.borrow_mut() = Some(state_cell);
            });
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // On wasm32, poll the side-channel to see if State is ready yet.
        #[cfg(target_arch = "wasm32")]
        if self.state.is_none() {
            WASM_STATE_CELL.with(|cell| {
                if let Some(rc) = cell.borrow().as_ref() {
                    if rc.borrow().is_some() {
                        self.state = rc.borrow_mut().take();
                    }
                }
            });
        }

        let (Some(state), Some(window)) = (self.state.as_mut(), self.window.as_ref()) else {
            return;
        };

        if state.input(&event) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(new_size) => {
                #[cfg(target_arch = "wasm32")]
                {
                    use winit::platform::web::WindowExtWebSys;

                    if new_size.width > 0 && new_size.height > 0 {
                        let canvas = window.canvas().expect("Could not get canvas reference");
                        canvas
                            .set_attribute("style", "width: 100%; aspect-ratio: auto;")
                            .expect("Set canvas style");
                    }
                }
                state.resize(new_size);
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                state.resize(window.inner_size());
            }
            WindowEvent::KeyboardInput {
                event: ref key_event,
                ..
            } if key_event.state == ElementState::Pressed => {
                if let Key::Named(key) = key_event.logical_key {
                    if key == NamedKey::Escape {
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                state.update();
                state.render();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static WASM_STATE_CELL: RefCell<Option<Rc<RefCell<Option<State<'static>>>>>> =
        RefCell::new(None);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_wasm() {
    use std::panic;

    panic::set_hook(Box::new(console_error_panic_hook::hook));
    run();
}

pub fn run() {
    env_logger::init();

    let evt_loop = EventLoop::new().expect("Failed to create event loop!");
    let mut app = App::new();
    evt_loop
        .run_app(&mut app)
        .expect("Failed to run event loop!");
}
