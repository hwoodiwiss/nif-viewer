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
mod load_session;
mod model;
pub mod nif;
mod nif_model;
mod pipeline;
mod state;
mod texture;
mod uniform;
mod vertex;

struct App {
    window: Option<Arc<Window>>,
    state: Option<State<'static>>,
    #[cfg(target_arch = "wasm32")]
    canvas: Option<web_sys::HtmlCanvasElement>,
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
            #[cfg(target_arch = "wasm32")]
            canvas: None,
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
        #[allow(unused_mut)]
        let mut window_attrs = Window::default_attributes()
            .with_title("NIF Viewer")
            .with_inner_size(window_size);

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            if self.canvas.is_some() {
                window_attrs = window_attrs.with_canvas(self.canvas.clone());
            }
        }

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
        if self.canvas.is_none() {
            // No pre-existing canvas supplied: create + append one (run_wasm path).
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

            if let Some(nif_path) = nif_path_from_env_or_args() {
                match load_nif_with_dependencies(&nif_path) {
                    Ok((nif_bytes, files)) => {
                        if let Err(e) = self
                            .state
                            .as_mut()
                            .unwrap()
                            .load_nif_model(&nif_bytes, &files)
                        {
                            log::error!("failed to load NIF model {}: {e:#}", nif_path);
                        } else {
                            log::info!("loaded NIF model: {}", nif_path);
                        }
                    }
                    Err(e) => log::error!("failed to read NIF {}: {e:#}", nif_path),
                }
            }
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
            // Canvas layout size arrives via a Resized event that may have
            // fired while State::new was still pending (and was dropped).
            // Apply the current size now that we can.
            if let (Some(state), Some(window)) = (self.state.as_mut(), self.window.as_ref()) {
                let size = window.inner_size();
                if size.width > 0 && size.height > 0 {
                    state.resize(size);
                }
            }
        }

        let (Some(state), Some(window)) = (self.state.as_mut(), self.window.as_ref()) else {
            return;
        };

        // Drain any NIF load queued from JS via `load_nif`.
        #[cfg(target_arch = "wasm32")]
        if let Some((name, nif_bytes, files)) =
            WASM_PENDING_LOAD.with(|cell| cell.borrow_mut().take())
        {
            match state.load_nif_model(&nif_bytes, &files) {
                Ok(()) => log::info!("loaded NIF model: {name}"),
                Err(e) => log::error!("failed to load NIF model {name}: {e:#}"),
            }
        }

        // Drain pending settings queued from JS.
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(speed) = WASM_PENDING_CAMERA_SPEED.with(|cell| cell.borrow_mut().take()) {
                state.set_camera_speed(speed);
            }
            if let Some(settings) = WASM_PENDING_LIGHT.with(|cell| cell.borrow_mut().take()) {
                state.set_light(settings);
            }
        }

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

/// NIF load requested from JS: (name-for-logging, nif bytes, dependency files).
#[cfg(target_arch = "wasm32")]
type PendingLoad = (String, Vec<u8>, std::collections::HashMap<String, Vec<u8>>);

#[cfg(target_arch = "wasm32")]
thread_local! {
    static WASM_STATE_CELL: RefCell<Option<Rc<RefCell<Option<State<'static>>>>>> =
        const { RefCell::new(None) };

    /// Pending NIF load, waiting for the event loop to pick it up.
    static WASM_PENDING_LOAD: RefCell<Option<PendingLoad>> = const { RefCell::new(None) };

    /// Active streaming load session (see `load_session_begin`).
    static WASM_LOAD_SESSION: RefCell<Option<load_session::LoadSession>> =
        const { RefCell::new(None) };

    /// Camera speed set from JS, waiting for the event loop to pick it up.
    static WASM_PENDING_CAMERA_SPEED: RefCell<Option<f32>> = const { RefCell::new(None) };

    /// Light settings set from JS, waiting for the event loop to pick them up.
    static WASM_PENDING_LIGHT: RefCell<Option<light::LightSettings>> =
        const { RefCell::new(None) };
}

/// Set the camera navigation speed (applied on the next event-loop tick).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_camera_speed(speed: f32) {
    WASM_PENDING_CAMERA_SPEED.with(|cell| {
        *cell.borrow_mut() = Some(speed);
    });
}

/// Configure the directional light (applied on the next event-loop tick).
/// `r,g,b` are 0-1 floats, `x,y,z` the light travel direction (normalized
/// internally). `auto_orbit` slowly rotates the direction about the Y axis.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn set_light(
    r: f32,
    g: f32,
    b: f32,
    x: f32,
    y: f32,
    z: f32,
    intensity: f32,
    ambient: f32,
    auto_orbit: bool,
) {
    WASM_PENDING_LIGHT.with(|cell| {
        *cell.borrow_mut() = Some(light::LightSettings {
            direction: light::normalize([x, y, z]),
            colour: [r, g, b],
            intensity,
            ambient,
            auto_orbit,
        });
    });
}

/// The NIF to load at startup: first CLI arg, falling back to `NIF_PATH`.
#[cfg(not(target_arch = "wasm32"))]
fn nif_path_from_env_or_args() -> Option<String> {
    std::env::args()
        .nth(1)
        .filter(|a| a.to_lowercase().ends_with(".nif"))
        .or_else(|| std::env::var("NIF_PATH").ok())
}

/// Map from normalized resource path to file bytes.
#[cfg(not(target_arch = "wasm32"))]
type ResourceFiles = std::collections::HashMap<String, Vec<u8>>;

/// Read a .nif from disk and resolve its dependencies (external `.mesh`
/// geometry, `.mat` materials and `.dds` textures) by walking up from the
/// nif's location to find a data root containing `geometries` / `textures` /
/// `materials` folders.
#[cfg(not(target_arch = "wasm32"))]
fn load_nif_with_dependencies(nif_path: &str) -> anyhow::Result<(Vec<u8>, ResourceFiles)> {
    use anyhow::Context;
    use std::collections::HashMap;
    use std::path::Path;

    let nif_path = Path::new(nif_path);
    let nif_bytes =
        std::fs::read(nif_path).with_context(|| format!("reading {}", nif_path.display()))?;

    let scene = nif::parse_nif(&nif_bytes).map_err(|e| anyhow::anyhow!("{e}"))?;

    // Walk up from the nif to find the data root.
    let mut root = None;
    let mut dir = nif_path.parent();
    while let Some(d) = dir {
        if ["geometries", "textures", "materials"]
            .iter()
            .any(|sub| d.join(sub).is_dir())
        {
            root = Some(d.to_path_buf());
            break;
        }
        dir = d.parent();
    }
    let Some(root) = root else {
        log::warn!(
            "no data root (geometries/textures/materials) found above {}",
            nif_path.display()
        );
        return Ok((nif_bytes, HashMap::new()));
    };
    log::info!("using data root: {}", root.display());

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    // Lazily built stem → color-dds-path index of <root>/textures.
    let mut stem_index: Option<HashMap<String, std::path::PathBuf>> = None;
    let read_into = |files: &mut HashMap<String, Vec<u8>>, rel: &str| -> bool {
        if files.contains_key(rel) {
            return true;
        }
        match std::fs::read(root.join(rel)) {
            Ok(bytes) => {
                files.insert(rel.to_string(), bytes);
                true
            }
            Err(_) => {
                log::warn!("dependency not found on disk: {}", rel);
                false
            }
        }
    };

    for instance in &scene.meshes {
        if let nif::Geometry::External { path, .. } = &instance.geometry {
            read_into(&mut files, path);
        }

        let mat = &instance.material;
        let mut dds_paths: Vec<String> = Vec::new();
        if let Some(mat_path) = &mat.mat_path {
            if read_into(&mut files, mat_path) {
                let set = nif::extract_mat_texture_set(&files[mat_path]);
                dds_paths.extend(set.albedo);
                dds_paths.extend(set.normal);
                dds_paths.extend(set.rough);
                dds_paths.extend(set.metal);
                dds_paths.extend(set.ao);
            } else {
                // No loose .mat (vanilla keeps them in a binary cdb): try
                // heuristic mirrored texture paths, then a stem search.
                resolve_heuristic_textures(&root, mat_path, &mut files, &mut stem_index);
            }
        }
        dds_paths.extend(mat.diffuse.clone());
        dds_paths.extend(mat.normal.clone());
        for dds in dds_paths {
            let norm = dds.to_lowercase().replace('\\', "/");
            read_into(&mut files, &norm);
        }
    }

    Ok((nif_bytes, files))
}

/// Heuristic texture discovery for a missing `.mat`: try the exact mirrored
/// `textures/.../<stem>_{color,normal}.dds` paths on disk; if absent, search
/// a (lazily built) stem index of `<root>/textures` for the best fuzzy match
/// (exact stem, then prefix either direction, then `_`-boundary suffix — the
/// dumps often drop a model prefix, e.g. `ar99_receiver` → `receiver`). Found
/// files are inserted under the hint path keys so the render-time fallback in
/// `build_nif_material` picks them up.
#[cfg(not(target_arch = "wasm32"))]
fn resolve_heuristic_textures(
    root: &std::path::Path,
    mat_path: &str,
    files: &mut ResourceFiles,
    stem_index: &mut Option<std::collections::HashMap<String, std::path::PathBuf>>,
) {
    let hints = crate::nif_model::material_texture_hints(mat_path);
    if files.contains_key(&hints.color) || files.contains_key(&hints.normal) {
        return;
    }

    // Given the on-disk _color.dds path, also pick up sibling _normal/_rough/
    // _metal/_ao maps and register them under the corresponding hint paths.
    let insert_set = |color_disk: &std::path::Path, files: &mut ResourceFiles| -> bool {
        let Ok(bytes) = std::fs::read(color_disk) else {
            return false;
        };
        files.insert(hints.color.clone(), bytes);
        let color_str = color_disk.to_string_lossy().to_lowercase();
        for (suffix, key) in [
            ("_normal.dds", &hints.normal),
            ("_rough.dds", &hints.rough),
            ("_metal.dds", &hints.metal),
            ("_ao.dds", &hints.ao),
        ] {
            let sibling = std::path::PathBuf::from(color_str.replace("_color.dds", suffix));
            if let Ok(bytes) = std::fs::read(&sibling) {
                files.insert(key.clone(), bytes);
            }
        }
        true
    };

    // Exact mirrored paths first.
    if insert_set(&root.join(&hints.color), files) {
        log::info!(
            "heuristic textures (exact) for {}: {}",
            mat_path,
            hints.color
        );
        return;
    }

    // Fall back to a fuzzy stem match against an index of all *_color.dds.
    let index = stem_index.get_or_insert_with(|| build_color_stem_index(root));
    let mut best: Option<(u64, &String, &std::path::PathBuf)> = None;
    for (cand, path) in index.iter() {
        let Some(score) = stem_match_score(&hints.stem, cand) else {
            continue;
        };
        if best.is_none_or(|(s, _, _)| score > s) {
            best = Some((score, cand, path));
        }
    }
    if let Some((_, cand, path)) = best {
        if insert_set(&path.clone(), files) {
            log::info!(
                "heuristic textures (stem '{}' ~ '{}') for {}: {}",
                hints.stem,
                cand,
                mat_path,
                path.display()
            );
            return;
        }
    }
    log::warn!("no heuristic textures found for {}", mat_path);
}

/// Score a candidate texture stem against a material stem. Higher is better;
/// `None` means no match. Tiers: exact (3), prefix either direction (2,
/// tie-break on common prefix length), `_`-boundary suffix (1, tie-break on
/// candidate length).
#[cfg(not(target_arch = "wasm32"))]
fn stem_match_score(hint_stem: &str, candidate: &str) -> Option<u64> {
    const TIER: u64 = 1 << 32;
    if candidate.is_empty() {
        return None;
    }
    if candidate == hint_stem {
        return Some(3 * TIER);
    }
    // Prefix either direction; require a meaningful shared prefix so tiny
    // candidates (e.g. a bare "_color.dds") never win.
    if (candidate.starts_with(hint_stem) || hint_stem.starts_with(candidate))
        && hint_stem.len().min(candidate.len()) >= 4
    {
        let common = hint_stem.len().min(candidate.len()) as u64;
        return Some(2 * TIER + common);
    }
    if hint_stem.ends_with(&format!("_{candidate}")) {
        return Some(TIER + candidate.len() as u64);
    }
    None
}

/// Index `<root>/textures` recursively: `_color.dds` filename stem → path.
/// Capped at 30000 files to bound the walk on huge dumps.
#[cfg(not(target_arch = "wasm32"))]
fn build_color_stem_index(
    root: &std::path::Path,
) -> std::collections::HashMap<String, std::path::PathBuf> {
    let mut index = std::collections::HashMap::new();
    let mut seen = 0usize;
    let mut stack = vec![root.join("textures")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            seen += 1;
            if seen > 30000 {
                log::warn!("texture stem index capped at 30000 files; matches may be incomplete");
                return index;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if let Some(stem) = name.strip_suffix("_color.dds") {
                index.entry(stem.to_string()).or_insert(path);
            }
        }
    }
    log::info!("texture stem index: {} color textures", index.len());
    index
}

/// Set up panic hook + console logging (idempotent).
#[cfg(target_arch = "wasm32")]
fn init_wasm_logging() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        let _ = console_log::init_with_level(log::Level::Info);
    });
}

/// Start the winit event loop on wasm, optionally attaching to an existing canvas.
/// Uses `spawn_app` so control returns to JS without the exception trick.
#[cfg(target_arch = "wasm32")]
fn start_web(canvas: Option<web_sys::HtmlCanvasElement>) {
    use winit::platform::web::EventLoopExtWebSys;

    init_wasm_logging();

    WASM_LIVE_CANVAS.with(|cell| {
        *cell.borrow_mut() = canvas.clone();
    });

    let evt_loop = EventLoop::new().expect("Failed to create event loop!");
    let mut app = App::new();
    app.canvas = canvas;
    evt_loop.spawn_app(app);
}

// The canvas owned by the (single, page-lifetime) winit event loop. winit
// only allows one event loop per page, so subsequent `attach` calls re-home
// this canvas into the new DOM location instead of starting over.
#[cfg(target_arch = "wasm32")]
thread_local! {
    static WASM_LIVE_CANVAS: RefCell<Option<web_sys::HtmlCanvasElement>> =
        const { RefCell::new(None) };
}

/// Start the viewer, creating a new canvas appended to `<body>`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_wasm() {
    start_web(None);
}

/// Start the viewer attached to the existing `<canvas>` with the given element id.
/// Safe to call again after DOM re-renders (e.g. SPA/Blazor navigation): the
/// live rendering canvas is swapped into the new element's place.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn attach(canvas_id: &str) -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;

    init_wasm_logging();

    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(canvas_id))
        .ok_or_else(|| JsValue::from_str(&format!("no element with id '{canvas_id}'")))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str(&format!("element '{canvas_id}' is not a <canvas>")))?;

    // Re-attach path: the event loop already exists and owns a live canvas.
    // Replace the freshly rendered placeholder canvas with it (unless it IS
    // the same element, e.g. a plain double call).
    let live = WASM_LIVE_CANVAS.with(|cell| cell.borrow().clone());
    if let Some(live) = live {
        if !live.is_same_node(Some(&canvas)) {
            // Carry over identity/styling so page CSS keeps applying.
            live.set_id(&canvas.id());
            if let Some(style) = canvas.get_attribute("style") {
                let _ = live.set_attribute("style", &style);
            }
            let _ = live.set_attribute("class", &canvas.class_name());
            canvas
                .replace_with_with_node_1(&live)
                .map_err(|e| JsValue::from_str(&format!("failed to re-attach canvas: {e:?}")))?;
        }
        return Ok(());
    }

    // winit does not resize user-supplied canvases; give the backing store a
    // real size from the CSS layout (or a sane default before layout) so the
    // initial surface/depth textures aren't created 0x0.
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    let (mut width, mut height) = (
        (canvas.client_width() as f64 * dpr) as u32,
        (canvas.client_height() as f64 * dpr) as u32,
    );
    if width == 0 || height == 0 {
        width = 1280;
        height = 720;
    }
    canvas.set_width(width);
    canvas.set_height(height);

    start_web(Some(canvas));
    Ok(())
}

/// Queue a NIF model for loading. `files` must be a plain JS object mapping
/// relative paths / filenames (lowercase, forward slashes) to `Uint8Array`s.
/// The load is applied on the next event-loop tick once the renderer is ready.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn load_nif(name: &str, nif_bytes: &[u8], files: JsValue) -> Result<(), JsValue> {
    use std::collections::HashMap;
    use wasm_bindgen::JsCast;

    init_wasm_logging();

    let mut file_map: HashMap<String, Vec<u8>> = HashMap::new();
    if !files.is_undefined() && !files.is_null() {
        let obj: js_sys::Object = files
            .dyn_into()
            .map_err(|_| JsValue::from_str("files must be a plain object"))?;
        for entry in js_sys::Object::entries(&obj).iter() {
            let entry: js_sys::Array = entry.unchecked_into();
            let key = entry
                .get(0)
                .as_string()
                .ok_or_else(|| JsValue::from_str("file key must be a string"))?;
            let value: js_sys::Uint8Array = entry
                .get(1)
                .dyn_into()
                .map_err(|_| JsValue::from_str(&format!("file '{key}' must be a Uint8Array")))?;
            file_map.insert(key, value.to_vec());
        }
    }

    log::info!(
        "queued NIF load: {name} ({} bytes, {} dependency file(s))",
        nif_bytes.len(),
        file_map.len()
    );
    WASM_PENDING_LOAD.with(|cell| {
        *cell.borrow_mut() = Some((name.to_string(), nif_bytes.to_vec(), file_map));
    });
    Ok(())
}

/// Parse the raw block structure of a NIF (no rendering, no session) and
/// return it as a JS object — see `nif::nif_structure` for the schema.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn parse_nif_structure(nif_bytes: &[u8]) -> Result<JsValue, JsValue> {
    init_wasm_logging();
    let value = nif::nif_structure(nif_bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
    js_sys::JSON::parse(&value.to_string())
}

/// Convert a list of paths into a JS string array.
#[cfg(target_arch = "wasm32")]
fn paths_to_js(paths: &[String]) -> JsValue {
    paths
        .iter()
        .map(|p| JsValue::from_str(p))
        .collect::<js_sys::Array>()
        .into()
}

/// Begin a streaming load session: parse the NIF immediately and return a JS
/// string array of required dependency paths (external `.mesh` geometry,
/// `.mat` materials, direct `.dds` textures), normalized lowercase forward
/// slashes. Replaces any previous session. Stream files with
/// `load_session_file_begin` / `_chunk` / `_end`, poll `load_session_pending`
/// (completed `.mat` files reveal further `.dds` requirements), then call
/// `load_session_finish`. Missing dependencies never error — rendering falls
/// back to the default material.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn load_session_begin(name: &str, nif_bytes: &[u8]) -> Result<JsValue, JsValue> {
    init_wasm_logging();
    let (session, required) = load_session::LoadSession::begin(name, nif_bytes)
        .map_err(|e| JsValue::from_str(&e))?;
    log::info!(
        "load session begun: {name} ({} bytes, {} required dependency path(s))",
        nif_bytes.len(),
        required.len()
    );
    WASM_LOAD_SESSION.with(|cell| {
        *cell.borrow_mut() = Some(session);
    });
    Ok(paths_to_js(&required))
}

#[cfg(target_arch = "wasm32")]
fn with_session<R>(
    f: impl FnOnce(&mut load_session::LoadSession) -> Result<R, String>,
) -> Result<R, JsValue> {
    WASM_LOAD_SESSION.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsValue::from_str("no active load session"))?;
        f(session).map_err(|e| JsValue::from_str(&e))
    })
}

/// Begin streaming a dependency file into the active session, pre-allocating
/// `size` bytes. `path` is normalized (lowercase, forward slashes).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn load_session_file_begin(path: &str, size: u32) -> Result<(), JsValue> {
    with_session(|s| {
        s.file_begin(path, size as usize);
        Ok(())
    })
}

/// Append a chunk to a file begun with `load_session_file_begin`. The chunk
/// crosses the JS/wasm boundary once; no full-file copy is held in JS.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn load_session_file_chunk(path: &str, chunk: &[u8]) -> Result<(), JsValue> {
    with_session(|s| s.file_chunk(path, chunk))
}

/// Mark a streamed file complete. Completed `.mat` files are scanned for
/// `.dds` references, which show up in subsequent `load_session_pending` calls.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn load_session_file_end(path: &str) -> Result<(), JsValue> {
    with_session(|s| s.file_end(path))
}

/// Return a JS string array of paths currently required but not yet provided.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn load_session_pending() -> Result<JsValue, JsValue> {
    with_session(|s| Ok(paths_to_js(&s.pending())))
}

/// Heuristic texture hints for required `.mat` files not yet provided, as a
/// JSON array of `{ mat, color, normal, stem }`. The color/normal paths mirror
/// the `.mat` path into `textures/` — speculative, so they never appear in
/// `load_session_pending`, but files streamed under those paths are accepted
/// and used as fallback textures at render time.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn load_session_texture_hints() -> Result<JsValue, JsValue> {
    with_session(|s| {
        let hints: Vec<serde_json::Value> = s
            .texture_hints()
            .into_iter()
            .map(|h| {
                serde_json::json!({
                    "mat": h.mat_path,
                    "color": h.color,
                    "normal": h.normal,
                    "rough": h.rough,
                    "metal": h.metal,
                    "ao": h.ao,
                    "stem": h.stem,
                })
            })
            .collect();
        Ok(serde_json::Value::Array(hints).to_string())
    })
    .and_then(|json| js_sys::JSON::parse(&json))
}

/// Finish the session: queue the NIF plus all streamed files for loading on
/// the next event-loop tick, then clear the session. Returns a JS string
/// array of dependency paths that were required but never provided, so
/// callers can locate them (e.g. in a data folder) and retry.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn load_session_finish() -> Result<JsValue, JsValue> {
    let session = WASM_LOAD_SESSION
        .with(|cell| cell.borrow_mut().take())
        .ok_or_else(|| JsValue::from_str("no active load session"))?;
    let missing = session.pending();
    let (name, nif_bytes, files) = session.finish();
    log::info!(
        "load session finished: {name} ({} dependency file(s), {} missing)",
        files.len(),
        missing.len()
    );
    WASM_PENDING_LOAD.with(|cell| {
        *cell.borrow_mut() = Some((name, nif_bytes, files));
    });
    Ok(paths_to_js(&missing))
}

pub fn run() {
    env_logger::init();

    let evt_loop = EventLoop::new().expect("Failed to create event loop!");
    let mut app = App::new();
    evt_loop
        .run_app(&mut app)
        .expect("Failed to run event loop!");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod stem_tests {
    use super::stem_match_score;

    #[test]
    fn stem_scoring_prefers_exact_then_prefix_then_suffix() {
        let exact = stem_match_score("ar99_mag", "ar99_mag").unwrap();
        let prefix = stem_match_score("ar99_mag", "ar99_magsmall").unwrap();
        let suffix = stem_match_score("ar99_receiver", "receiver").unwrap();
        assert!(exact > prefix && prefix > suffix);

        // Empty / tiny candidates never match via the prefix rule.
        assert!(stem_match_score("ar99_receiver", "").is_none());
        assert!(stem_match_score("ar99_receiver", "ar9").is_none());
        assert!(stem_match_score("scope", "barrel").is_none());
    }
}
