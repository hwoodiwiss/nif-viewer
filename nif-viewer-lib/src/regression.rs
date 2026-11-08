//! Stable test seam for the renderer regression suite (`tests/regression.rs`).
//!
//! Not a supported API: it exists so the same assertions can run against the
//! renderer before and after internal restructuring.

use std::collections::HashMap;

use scene_renderer::{ContextDesc, GpuContext, Renderer, RendererDesc, RendererError, ViewDesc};
use scene_types::{Camera, DirectionalLight, ViewParams};

/// CPU conversion output for one mesh, in renderer (Y-up) space.
#[derive(Debug, Clone)]
pub struct ConvertedMesh {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material: usize,
}

#[derive(Debug, Clone)]
pub struct ConvertedModel {
    pub meshes: Vec<ConvertedMesh>,
    /// `(mat_path, diffuse, normal)` per deduplicated material.
    pub materials: Vec<(Option<String>, Option<String>, Option<String>)>,
    pub bounds_center: [f32; 3],
    pub bounds_radius: f32,
}

/// Parse, resolve external geometry and convert a NIF without the GPU.
pub fn convert_nif(nif: &[u8], files: &HashMap<String, Vec<u8>>) -> anyhow::Result<ConvertedModel> {
    let built = nif_scene::nif_scene::parse_and_build(nif, files)?;
    let materials = built
        .materials
        .iter()
        .map(|m| (m.mat_path.clone(), m.diffuse.clone(), m.normal.clone()))
        .collect();
    let converted = nif_scene::nif_scene::scene_from_built(built, files);
    let scene = &converted.scene;
    let bounds = scene.bounds().expect("non-empty scene has bounds");
    Ok(ConvertedModel {
        meshes: scene
            .instances
            .iter()
            .map(|instance| {
                let m = &scene.meshes[instance.mesh];
                ConvertedMesh {
                    name: m.name.clone(),
                    positions: m.positions.clone(),
                    normals: m.normals.clone().unwrap_or_default(),
                    uvs: m.uv0.clone().unwrap_or_default(),
                    indices: m.indices.clone(),
                    material: instance.material,
                }
            })
            .collect(),
        materials,
        bounds_center: bounds.center(),
        bounds_radius: bounds.radius(),
    })
}

/// Fixed light used by regression renders (no auto-orbit).
#[derive(Debug, Clone, Copy)]
pub struct RegressionLight {
    pub direction: [f32; 3],
    pub colour: [f32; 3],
    pub intensity: f32,
    pub ambient: f32,
}

impl Default for RegressionLight {
    fn default() -> Self {
        let d = DirectionalLight::default();
        Self {
            direction: d.direction,
            colour: d.colour,
            intensity: d.intensity,
            ambient: d.ambient,
        }
    }
}

pub type RenderedImage = scene_renderer::CapturedImage;

/// Render the NIF as the viewer frames it (render scale 2, sRGB output).
/// Returns `None` when no wgpu adapter is available.
pub fn render_nif_offscreen(
    nif: &[u8],
    files: &HashMap<String, Vec<u8>>,
    width: u32,
    height: u32,
    light: RegressionLight,
) -> anyhow::Result<Option<RenderedImage>> {
    futures::executor::block_on(async {
        let ctx = match GpuContext::new_offscreen(&ContextDesc::default()).await {
            Ok(ctx) => ctx,
            Err(RendererError::NoAdapter(_)) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let shaders =
            scene_viewer::assets::load_shader_dir(&scene_renderer::packaged_shader_dir())?;
        let mut renderer = Renderer::new(
            &ctx,
            &shaders,
            RendererDesc::new(scene_renderer::wgpu::TextureFormat::Rgba8UnormSrgb),
        )?;
        let converted = nif_scene::nif_to_scene(nif, files)?;
        let bounds = converted
            .scene
            .bounds()
            .expect("non-empty scene has bounds");
        let (scene, _) = renderer.upload_scene(&converted.scene)?;
        let view = renderer.create_view(ViewDesc {
            width,
            height,
            render_scale: 2.0,
        })?;
        let params = ViewParams {
            camera: Camera::framing(bounds.center(), bounds.radius()),
            light: DirectionalLight {
                direction: light.direction,
                colour: light.colour,
                intensity: light.intensity,
                ambient: light.ambient,
            },
            clear_color: [0.0, 0.0, 0.0, 1.0],
        };
        Ok(Some(
            renderer
                .render_to_image(&view, Some(scene), &params)
                .await?,
        ))
    })
}
