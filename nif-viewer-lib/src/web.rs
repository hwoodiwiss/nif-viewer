//! wasm-bindgen exports. A single viewer per page; all state lives in this
//! facade's [`ViewerHandle`] rather than in renderer globals.

use std::cell::RefCell;
use std::collections::HashMap;

use scene_viewer::{AppConfig, LightSettings, ViewerApp, ViewerEvent, ViewerHandle};
use wasm_bindgen::prelude::*;

use nif_scene::load_session;

/// Latest load request as reported by [`viewer_status`].
#[derive(Default, Clone)]
struct LoadStatus {
    ticket: u64,
    name: String,
    /// "queued" | "uploaded" | "rendered" | "failed" | "superseded"
    state: &'static str,
    error: Option<String>,
    diagnostics: Vec<String>,
}

thread_local! {
    /// Created on first use so commands issued before `attach` are queued.
    static HANDLE: ViewerHandle = ViewerHandle::new();
    /// The canvas owned by the (single, page-lifetime) winit event loop.
    static LIVE_CANVAS: RefCell<Option<web_sys::HtmlCanvasElement>> = const { RefCell::new(None) };
    static LOAD_SESSION: RefCell<Option<load_session::LoadSession>> = const { RefCell::new(None) };
    static LOAD_STATUS: RefCell<LoadStatus> = RefCell::new(LoadStatus::default());
}

fn handle() -> ViewerHandle {
    HANDLE.with(|h| h.clone())
}

/// Set up panic hook + console logging (idempotent).
fn init_wasm_logging() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));
        let _ = console_log::init_with_level(log::Level::Info);
    });
}

fn record_events(handle: &ViewerHandle) {
    for event in handle.drain_events() {
        match event {
            ViewerEvent::SceneUploaded { ticket, report } => update_status(ticket, |s| {
                s.state = "uploaded";
                s.diagnostics.extend(report.diagnostics);
            }),
            ViewerEvent::SceneFailed { ticket, error } => update_status(ticket, |s| {
                s.state = "failed";
                s.error = Some(error);
            }),
            ViewerEvent::SceneSuperseded { ticket } => {
                update_status(ticket, |s| s.state = "superseded")
            }
            ViewerEvent::FirstFrameSubmitted { ticket, .. } => {
                update_status(ticket, |s| s.state = "rendered")
            }
            ViewerEvent::CaptureReady(image) => download_png(&image),
            ViewerEvent::CaptureFailed(e) => log::error!("capture failed: {}", e),
            ViewerEvent::StateChanged(state) => log::info!("viewer state: {}", state.name()),
        }
    }
}

fn update_status(ticket: u64, f: impl FnOnce(&mut LoadStatus)) {
    LOAD_STATUS.with(|s| {
        let mut s = s.borrow_mut();
        if s.ticket == ticket {
            f(&mut s);
        }
    });
}

fn queue_load(name: &str, nif_bytes: &[u8], files: &HashMap<String, Vec<u8>>) {
    let handle = handle();
    let (ticket, status) = match nif_scene::nif_to_scene(nif_bytes, files) {
        Ok(converted) => {
            let ticket = handle.replace_scene(converted.scene, true);
            (
                ticket,
                LoadStatus {
                    ticket,
                    name: name.to_string(),
                    state: "queued",
                    error: None,
                    diagnostics: converted.diagnostics,
                },
            )
        }
        Err(e) => {
            log::error!("failed to load NIF model {}: {:#}", name, e);
            (
                0,
                LoadStatus {
                    ticket: 0,
                    name: name.to_string(),
                    state: "failed",
                    error: Some(format!("{:#}", e)),
                    diagnostics: Vec::new(),
                },
            )
        }
    };
    log::info!("queued NIF load {}: ticket {}", name, ticket);
    LOAD_STATUS.with(|s| *s.borrow_mut() = status);
}

fn download_png(image: &scene_renderer::CapturedImage) {
    use image::ImageEncoder;
    let mut png_bytes = Vec::new();
    if let Err(e) = image::codecs::png::PngEncoder::new(&mut png_bytes).write_image(
        &image.rgba,
        image.width,
        image.height,
        image::ExtendedColorType::Rgba8,
    ) {
        log::error!("screenshot PNG encode failed: {}", e);
        return;
    }
    if let Err(e) = trigger_browser_download(&png_bytes, &format!("{}.png", image.label)) {
        log::error!("screenshot download failed: {:?}", e);
    }
}

fn trigger_browser_download(data: &[u8], filename: &str) -> Result<(), JsValue> {
    use js_sys::{Array, Uint8Array};
    use wasm_bindgen::JsCast;
    use web_sys::{Blob, BlobPropertyBag, HtmlAnchorElement, Url};

    let array = Array::new();
    array.push(&Uint8Array::from(data).buffer());
    let options = BlobPropertyBag::new();
    options.set_type("image/png");
    let blob = Blob::new_with_u8_array_sequence_and_options(&array, &options)?;
    let url = Url::create_object_url_with_blob(&blob)?;
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let anchor: HtmlAnchorElement = document.create_element("a")?.dyn_into()?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();
    Url::revoke_object_url(&url)?;
    Ok(())
}

async fn start(
    canvas: web_sys::HtmlCanvasElement,
    shader_base_url: Option<String>,
) -> Result<(), JsValue> {
    let base = shader_base_url.unwrap_or_else(|| "shaders/".to_string());
    let base = if base.ends_with('/') {
        base
    } else {
        format!("{}/", base)
    };
    let handle = handle();
    let shaders = scene_viewer::assets::fetch_shader_set(&base)
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let app = ViewerApp::new(
        AppConfig {
            title: "NIF Viewer".into(),
            initial_size: (1920, 1080),
            exit_on_escape: false,
            canvas: Some(canvas),
            resized_canvas_style: Some("width: 100%; aspect-ratio: auto;".into()),
        },
        crate::viewer_config(LightSettings::default()),
        shaders,
        handle.clone(),
    )
    .on_frame(record_events);
    app.spawn().map_err(|e| JsValue::from_str(&e.to_string()))?;
    handle.wait_ready().await.map_err(|e| JsValue::from_str(&e))
}

/// Start the viewer, creating a new canvas appended to `<body>`.
#[wasm_bindgen]
pub fn run_wasm() -> Result<(), JsValue> {
    init_wasm_logging();
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas: web_sys::HtmlCanvasElement =
        wasm_bindgen::JsCast::dyn_into(document.create_element("canvas")?)
            .map_err(|_| JsValue::from_str("not a canvas"))?;
    canvas.set_width(1920);
    canvas.set_height(1080);
    canvas.set_attribute("style", "width: 100%; aspect-ratio: 16/9;")?;
    document
        .body()
        .ok_or_else(|| JsValue::from_str("no body"))?
        .append_child(&canvas)?;
    LIVE_CANVAS.with(|c| *c.borrow_mut() = Some(canvas.clone()));
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = start(canvas, None).await {
            log::error!("viewer start failed: {:?}", e);
        }
    });
    Ok(())
}

/// Attach the viewer to the `<canvas>` with the given id. Resolves once the
/// GPU device, shaders and pipelines are ready, or rejects with the failure.
///
/// `shader_base_url` locates `manifest.json` and the WGSL files (default
/// `"shaders/"`, relative to the page). Safe to call again after DOM
/// re-renders (SPA/Blazor navigation): the live canvas is swapped into the
/// new element's place.
#[wasm_bindgen]
pub async fn attach(canvas_id: String, shader_base_url: Option<String>) -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;

    init_wasm_logging();
    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(&canvas_id))
        .ok_or_else(|| JsValue::from_str(&format!("no element with id '{}'", canvas_id)))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str(&format!("element '{}' is not a <canvas>", canvas_id)))?;

    // Re-attach path: the event loop already exists and owns a live canvas.
    if let Some(live) = LIVE_CANVAS.with(|cell| cell.borrow().clone()) {
        if !live.is_same_node(Some(&canvas)) {
            // Carry over identity/styling so page CSS keeps applying.
            live.set_id(&canvas.id());
            if let Some(style) = canvas.get_attribute("style") {
                let _ = live.set_attribute("style", &style);
            }
            let _ = live.set_attribute("class", &canvas.class_name());
            canvas
                .replace_with_with_node_1(&live)
                .map_err(|e| JsValue::from_str(&format!("failed to re-attach canvas: {:?}", e)))?;
        }
        return handle()
            .wait_ready()
            .await
            .map_err(|e| JsValue::from_str(&e));
    }

    // winit does not resize user-supplied canvases; give the backing store a
    // real size from the CSS layout (or a default before layout).
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
    LIVE_CANVAS.with(|cell| *cell.borrow_mut() = Some(canvas.clone()));
    start(canvas, shader_base_url).await
}

/// Viewer lifecycle and latest load status as a JS object:
/// `{ state, load: { ticket, name, state, error, diagnostics } }`.
/// Viewer `state` is "initializing" | "ready" | "suspended" | "device-lost" |
/// "failed" | "disposed"; load `state` is "queued" | "uploaded" | "rendered"
/// | "failed" | "superseded" (empty before any load).
#[wasm_bindgen]
pub fn viewer_status() -> Result<JsValue, JsValue> {
    let handle = handle();
    record_events(&handle);
    let load = LOAD_STATUS.with(|s| s.borrow().clone());
    let state = handle.state();
    let error = match &state {
        scene_viewer::ViewerState::Failed(e) | scene_viewer::ViewerState::DeviceLost(e) => {
            Some(e.clone())
        }
        _ => None,
    };
    let json = serde_json::json!({
        "state": state.name(),
        "error": error,
        "load": {
            "ticket": load.ticket,
            "name": load.name,
            "state": load.state,
            "error": load.error,
            "diagnostics": load.diagnostics,
        }
    });
    js_sys::JSON::parse(&json.to_string())
}

/// Set the camera navigation speed (applied on the next frame).
#[wasm_bindgen]
pub fn set_camera_speed(speed: f32) {
    handle().set_camera_speed(speed);
}

/// Configure the directional light (applied on the next frame). `r,g,b` are
/// 0-1 floats, `x,y,z` the light travel direction (normalized internally).
/// `auto_orbit` slowly rotates the direction about the Y axis.
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
    let mut light = scene_types::DirectionalLight {
        direction: [x, y, z],
        colour: [r, g, b],
        intensity,
        ambient,
    };
    light.direction = light.normalized_direction();
    handle().set_light(LightSettings { light, auto_orbit });
}

/// Queue a NIF model for display. `files` must be a plain JS object mapping
/// relative paths / filenames (lowercase, forward slashes) to `Uint8Array`s.
/// Conversion happens now; upload happens on the next frame once the viewer
/// is ready. Track progress with `viewer_status`.
#[wasm_bindgen]
pub fn load_nif(name: &str, nif_bytes: &[u8], files: JsValue) -> Result<(), JsValue> {
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
                .map_err(|_| JsValue::from_str(&format!("file '{}' must be a Uint8Array", key)))?;
            file_map.insert(key, value.to_vec());
        }
    }
    queue_load(name, nif_bytes, &file_map);
    Ok(())
}

/// Parse the raw block structure of a NIF (no rendering, no session) and
/// return it as a JS object — see `nif::nif_structure` for the schema.
#[wasm_bindgen]
pub fn parse_nif_structure(nif_bytes: &[u8]) -> Result<JsValue, JsValue> {
    init_wasm_logging();
    let value =
        nif_scene::nif::nif_structure(nif_bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
    js_sys::JSON::parse(&value.to_string())
}

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
#[wasm_bindgen]
pub fn load_session_begin(name: &str, nif_bytes: &[u8]) -> Result<JsValue, JsValue> {
    init_wasm_logging();
    let (session, required) =
        load_session::LoadSession::begin(name, nif_bytes).map_err(|e| JsValue::from_str(&e))?;
    log::info!(
        "load session begun: {} ({} bytes, {} required dependency path(s))",
        name,
        nif_bytes.len(),
        required.len()
    );
    LOAD_SESSION.with(|cell| *cell.borrow_mut() = Some(session));
    Ok(paths_to_js(&required))
}

fn with_session<R>(
    f: impl FnOnce(&mut load_session::LoadSession) -> Result<R, String>,
) -> Result<R, JsValue> {
    LOAD_SESSION.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let session = borrow
            .as_mut()
            .ok_or_else(|| JsValue::from_str("no active load session"))?;
        f(session).map_err(|e| JsValue::from_str(&e))
    })
}

/// Begin streaming a dependency file into the active session, pre-allocating
/// `size` bytes. `path` is normalized (lowercase, forward slashes).
#[wasm_bindgen]
pub fn load_session_file_begin(path: &str, size: u32) -> Result<(), JsValue> {
    with_session(|s| {
        s.file_begin(path, size as usize);
        Ok(())
    })
}

/// Append a chunk to a file begun with `load_session_file_begin`.
#[wasm_bindgen]
pub fn load_session_file_chunk(path: &str, chunk: &[u8]) -> Result<(), JsValue> {
    with_session(|s| s.file_chunk(path, chunk))
}

/// Mark a streamed file complete. Completed `.mat` files are scanned for
/// `.dds` references, which show up in subsequent `load_session_pending` calls.
#[wasm_bindgen]
pub fn load_session_file_end(path: &str) -> Result<(), JsValue> {
    with_session(|s| s.file_end(path))
}

/// Return a JS string array of paths currently required but not yet provided.
#[wasm_bindgen]
pub fn load_session_pending() -> Result<JsValue, JsValue> {
    with_session(|s| Ok(paths_to_js(&s.pending())))
}

/// Heuristic texture hints for required `.mat` files not yet provided, as a
/// JSON array of `{ mat, color, normal, rough, metal, ao, stem }`.
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

/// Finish the session: queue the NIF plus all streamed files for display on
/// the next frame, then clear the session. Returns a JS string array of
/// dependency paths that were required but never provided. Upload and first
/// frame are reported later through `viewer_status`.
#[wasm_bindgen]
pub fn load_session_finish() -> Result<JsValue, JsValue> {
    let session = LOAD_SESSION
        .with(|cell| cell.borrow_mut().take())
        .ok_or_else(|| JsValue::from_str("no active load session"))?;
    let missing = session.pending();
    let (name, nif_bytes, files) = session.finish();
    log::info!(
        "load session finished: {} ({} dependency file(s), {} missing)",
        name,
        files.len(),
        missing.len()
    );
    queue_load(&name, &nif_bytes, &files);
    Ok(paths_to_js(&missing))
}
