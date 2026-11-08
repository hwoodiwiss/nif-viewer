//! Viewer entry points: wires `nif-scene` (NIF parsing, dependency loading and
//! conversion) to `scene-viewer` for the native app and the WebAssembly exports.

#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub mod regression;

pub use nif_scene;
/// Compatibility path for the NIF parser, now in the `nif-scene` crate.
pub use nif_scene::nif;
pub use scene_renderer;
pub use scene_types;
pub use scene_viewer;

use scene_renderer::{RendererError, ShaderSet};
use scene_types::MaterialTypeDesc;
use scene_viewer::LightSettings;
#[cfg(not(target_arch = "wasm32"))]
use scene_viewer::{ViewerEvent, ViewerHandle};

/// Relative location of the staged NIF material package (`manifest.json` +
/// WGSL from `nif-scene/shaders`) beside the renderer's `shaders/`.
pub const NIF_MATERIALS_DIR: &str = "materials/nif";

/// Material types the NIF conversion needs, from a loaded NIF material package.
pub fn nif_material_types(materials: &ShaderSet) -> Result<Vec<MaterialTypeDesc>, RendererError> {
    let key = nif_scene::materials::BETHESDA_PBR;
    let source = materials
        .material_source(key)
        .ok_or_else(|| RendererError::Shader(format!("NIF material package lacks '{}'", key)))?;
    Ok(vec![nif_scene::materials::bethesda_pbr_type(source)])
}

/// Renderer settings shared by the native and web hosts.
fn viewer_config(
    light: LightSettings,
    material_types: Vec<MaterialTypeDesc>,
) -> scene_viewer::ViewerConfig {
    scene_viewer::ViewerConfig {
        render_scale: 2.0,
        light,
        material_types,
        ..Default::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn convert_and_queue(
    handle: &ViewerHandle,
    name: &str,
    nif_bytes: &[u8],
    files: &std::collections::HashMap<String, Vec<u8>>,
) -> anyhow::Result<u64> {
    let converted = nif_scene::nif_to_scene(nif_bytes, files)?;
    log::info!(
        "converted NIF {}: {} mesh(es), {} material(s), {} image(s), {} diagnostic(s)",
        name,
        converted.scene.meshes.len(),
        converted.scene.materials.len(),
        converted.scene.images.len(),
        converted.diagnostics.len()
    );
    Ok(handle.replace_scene(converted.scene, true))
}

// ---------------------------------------------------------------------------
// Native host
// ---------------------------------------------------------------------------

/// Run the native viewer. The NIF is the first `.nif` CLI argument or
/// `NIF_PATH`. Renderer shaders come from `NIF_VIEWER_SHADER_DIR`,
/// `<exe dir>/shaders`, `./shaders`, or the renderer package in development
/// builds; the NIF material package likewise from `NIF_VIEWER_MATERIAL_DIR`,
/// `<exe dir>/materials/nif`, `./materials/nif`, or `nif-scene/shaders`.
///
/// Debug environment: `NIF_LIGHT_{COLOR,DIR,INTENSITY,AMBIENT}`,
/// `NIF_CAM_SPEED`, `NIF_CAPTURE=<frame>`. Backspace captures PNG screenshots.
#[cfg(not(target_arch = "wasm32"))]
pub fn run() -> anyhow::Result<()> {
    use anyhow::Context;

    let shader_dir = scene_viewer::assets::find_shader_dir(Some("NIF_VIEWER_SHADER_DIR"))
        .context("no shader directory with manifest.json found (set NIF_VIEWER_SHADER_DIR)")?;
    log::info!("loading shaders from {}", shader_dir.display());
    let shaders = scene_viewer::assets::load_shader_dir(&shader_dir)?;
    let material_dir = scene_viewer::assets::find_asset_dir(
        Some("NIF_VIEWER_MATERIAL_DIR"),
        NIF_MATERIALS_DIR,
        &nif_scene::materials::packaged_material_dir(),
    )
    .context("no NIF material directory with manifest.json found (set NIF_VIEWER_MATERIAL_DIR)")?;
    log::info!("loading NIF materials from {}", material_dir.display());
    let material_types =
        nif_material_types(&scene_viewer::assets::load_shader_dir(&material_dir)?)?;

    let mut light = LightSettings::default();
    apply_light_env(&mut light);
    let handle = ViewerHandle::new();

    let nif_path = nif_path_from_env_or_args();
    if let Some(nif_path) = &nif_path {
        match nif_scene::disk::load_nif_with_dependencies(std::path::Path::new(nif_path)) {
            Ok((nif_bytes, files)) => {
                if let Err(e) = convert_and_queue(&handle, nif_path, &nif_bytes, &files) {
                    log::error!("failed to load NIF model {}: {:#}", nif_path, e);
                }
            }
            Err(e) => log::error!("failed to read NIF {}: {:#}", nif_path, e),
        }
    }
    let model_name = nif_path.unwrap_or_default();

    let cam_speed = std::env::var("NIF_CAM_SPEED")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok());
    let capture_frame = std::env::var("NIF_CAPTURE")
        .ok()
        .and_then(|s| s.parse::<u64>().ok());
    let (png_tx, png_rx) = std::sync::mpsc::channel::<scene_renderer::CapturedImage>();
    let writer = std::thread::spawn(move || {
        for (counter, image) in png_rx.into_iter().enumerate() {
            let filename = format!("screenshot_{:04}_{}.png", counter, image.label);
            match image::RgbaImage::from_raw(image.width, image.height, image.rgba) {
                Some(img) => match img.save(&filename) {
                    Ok(_) => println!("Saved {}", filename),
                    Err(e) => eprintln!("Failed to save {}: {}", filename, e),
                },
                None => eprintln!("Buffer size mismatch for {}", filename),
            }
        }
    });

    let mut frame = 0u64;
    let app = scene_viewer::ViewerApp::new(
        scene_viewer::AppConfig {
            title: "NIF Viewer".into(),
            initial_size: (1920, 1080),
            exit_on_escape: true,
        },
        viewer_config(light, material_types),
        shaders,
        handle,
    )
    .on_frame(move |handle| {
        if Some(frame) == capture_frame {
            handle.request_capture("frame");
        }
        frame += 1;
        for event in handle.drain_events() {
            match event {
                ViewerEvent::SceneUploaded { ticket, report } => {
                    log::info!("loaded NIF model: {}", model_name);
                    log::info!("scene {} uploaded: {:?}", ticket, report.stats);
                    if let Some(speed) = cam_speed {
                        handle.set_camera_speed(speed);
                    }
                }
                ViewerEvent::SceneFailed { error, .. } => {
                    log::error!("failed to load NIF model {}: {}", model_name, error)
                }
                ViewerEvent::CaptureReady(image) => {
                    let _ = png_tx.send(image);
                }
                ViewerEvent::CaptureFailed(e) => log::error!("capture failed: {}", e),
                ViewerEvent::StateChanged(state) => log::info!("viewer state: {}", state.name()),
                _ => {}
            }
        }
    });
    app.run()?;
    drop(writer);
    Ok(())
}

/// Leniently parse "x,y,z" (also accepts spaces/semicolons as separators).
#[cfg(not(target_arch = "wasm32"))]
fn parse_vec3(s: &str) -> Option<[f32; 3]> {
    let parts: Vec<f32> = s
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.trim().parse::<f32>().ok())
        .collect();
    (parts.len() == 3).then(|| [parts[0], parts[1], parts[2]])
}

/// Apply NIF_LIGHT_* env var overrides. Setting NIF_LIGHT_DIR disables auto-orbit.
#[cfg(not(target_arch = "wasm32"))]
fn apply_light_env(settings: &mut LightSettings) {
    if let Some(c) = std::env::var("NIF_LIGHT_COLOR")
        .ok()
        .as_deref()
        .and_then(parse_vec3)
    {
        settings.light.colour = c;
        log::info!("NIF_LIGHT_COLOR applied: {:?}", c);
    }
    if let Some(d) = std::env::var("NIF_LIGHT_DIR")
        .ok()
        .as_deref()
        .and_then(parse_vec3)
    {
        settings.light.direction = d;
        settings.light.direction = settings.light.normalized_direction();
        settings.auto_orbit = false;
        log::info!(
            "NIF_LIGHT_DIR applied: {:?} (auto-orbit off)",
            settings.light.direction
        );
    }
    if let Some(i) = std::env::var("NIF_LIGHT_INTENSITY")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
    {
        settings.light.intensity = i;
        log::info!("NIF_LIGHT_INTENSITY applied: {}", i);
    }
    if let Some(a) = std::env::var("NIF_LIGHT_AMBIENT")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
    {
        settings.light.ambient = a;
        log::info!("NIF_LIGHT_AMBIENT applied: {}", a);
    }
}

/// The NIF to load at startup: first CLI arg, falling back to `NIF_PATH`.
#[cfg(not(target_arch = "wasm32"))]
fn nif_path_from_env_or_args() -> Option<String> {
    std::env::args()
        .nth(1)
        .filter(|a| a.to_lowercase().ends_with(".nif"))
        .or_else(|| std::env::var("NIF_PATH").ok())
}

// ---------------------------------------------------------------------------
// WebAssembly host (single-viewer facade over scene-viewer)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;
