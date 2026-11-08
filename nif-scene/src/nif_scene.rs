//! NIF → [`SceneData`] adapter: resolves NIF material/texture semantics and
//! produces renderer-neutral scene data plus load diagnostics. Materials use
//! the [`materials::BETHESDA_PBR`] type, which hosts must register.

use std::collections::HashMap;

use crate::materials;
use crate::textures::{decode_dds_rgba8, pack_orm, Rgba8Image};
use scene_types::{
    AlphaMode, ColorSpace, ImageData, InstanceDesc, MaterialDesc, SceneData, TextureRef,
};

use crate::nif::{MatTextureSet, MaterialInfo};
use crate::nif_model::{build_model, lookup_file, material_texture_hints, BuiltModel};

/// A converted NIF ready for upload.
pub struct NifSceneData {
    pub scene: SceneData,
    /// Missing or undecodable textures/materials, with NIF resource paths.
    pub diagnostics: Vec<String>,
}

/// Parse, resolve external geometry and convert to a scene. Meshes carry
/// baked world transforms (identity instances). Missing dependencies fall
/// back to renderer defaults and are reported in `diagnostics`.
pub fn nif_to_scene(
    nif_bytes: &[u8],
    files: &HashMap<String, Vec<u8>>,
) -> anyhow::Result<NifSceneData> {
    let built = parse_and_build(nif_bytes, files)?;
    Ok(scene_from_built(built, files))
}

/// First half of [`nif_to_scene`]: parse, resolve external geometry from
/// `files` and convert meshes. Fails when nothing renderable remains.
pub fn parse_and_build(
    nif_bytes: &[u8],
    files: &HashMap<String, Vec<u8>>,
) -> anyhow::Result<BuiltModel> {
    let mut scene = crate::nif::parse_nif(nif_bytes).map_err(|e| anyhow::anyhow!("{}", e))?;
    crate::nif::resolve_external(&mut scene, &mut |path| lookup_file(files, path).cloned())
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let built = build_model(&scene);
    if built.meshes.is_empty() {
        anyhow::bail!("NIF contains no renderable meshes");
    }
    Ok(built)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Role {
    Albedo,
    Normal,
}

struct Builder<'a> {
    files: &'a HashMap<String, Vec<u8>>,
    images: Vec<ImageData>,
    cache: HashMap<(String, Role), Option<usize>>,
    diagnostics: Vec<String>,
}

impl Builder<'_> {
    fn texture(&mut self, path: &Option<String>, role: Role) -> Option<TextureRef> {
        let path = path.as_ref()?;
        let key = (crate::nif_model::normalize_path(path), role);
        if let Some(cached) = self.cache.get(&key) {
            return cached.map(TextureRef::new);
        }
        let image = match lookup_file(self.files, path) {
            None => {
                self.diagnostics
                    .push(format!("texture not found: {}", path));
                None
            }
            Some(bytes) => match decode_dds_rgba8(bytes, role == Role::Normal) {
                Ok(img) => {
                    let space = if role == Role::Normal {
                        ColorSpace::Linear
                    } else {
                        ColorSpace::Srgb
                    };
                    self.images.push(ImageData::rgba8(
                        path.clone(),
                        img.width,
                        img.height,
                        space,
                        img.pixels,
                    ));
                    Some(self.images.len() - 1)
                }
                Err(e) => {
                    self.diagnostics
                        .push(format!("failed to decode {}: {}", path, e));
                    None
                }
            },
        };
        self.cache.insert(key, image);
        image.map(TextureRef::new)
    }

    fn gray(&mut self, path: &Option<String>) -> Option<Rgba8Image> {
        let path = path.as_ref()?;
        let bytes = lookup_file(self.files, path)?;
        match decode_dds_rgba8(bytes, false) {
            Ok(img) => Some(img),
            Err(e) => {
                self.diagnostics
                    .push(format!("failed to decode {}: {}", path, e));
                None
            }
        }
    }

    fn texture_set(&mut self, info: &MaterialInfo) -> MatTextureSet {
        // Starfield .mat or SSE/FO4 texture-set slots.
        let mut set = match &info.mat_path {
            Some(mat_path) => match lookup_file(self.files, mat_path) {
                Some(bytes) => crate::nif::extract_mat_texture_set(bytes),
                None => {
                    self.diagnostics
                        .push(format!("material file not found: {}", mat_path));
                    MatTextureSet::default()
                }
            },
            None => MatTextureSet {
                albedo: info.diffuse.clone(),
                normal: info.normal.clone(),
                ..Default::default()
            },
        };
        // Heuristic fallback: mirrored texture paths derived from the .mat path.
        if let Some(mat_path) = &info.mat_path {
            let hints = material_texture_hints(mat_path);
            for (slot, hint) in [
                (&mut set.albedo, &hints.color),
                (&mut set.normal, &hints.normal),
                (&mut set.rough, &hints.rough),
                (&mut set.metal, &hints.metal),
                (&mut set.ao, &hints.ao),
            ] {
                if slot.is_none() && lookup_file(self.files, hint).is_some() {
                    log::info!("heuristic texture match for {}: {}", mat_path, hint);
                    *slot = Some(hint.clone());
                }
            }
        }
        set
    }

    fn material(&mut self, info: &MaterialInfo) -> MaterialDesc {
        let set = self.texture_set(info);
        let base_color_texture = self.texture(&set.albedo, Role::Albedo);
        let normal_texture = self.texture(&set.normal, Role::Normal);
        let (ao, rough, metal) = (
            self.gray(&set.ao),
            self.gray(&set.rough),
            self.gray(&set.metal),
        );
        let orm_texture = if ao.is_some() || rough.is_some() || metal.is_some() {
            let orm = pack_orm(ao.as_ref(), rough.as_ref(), metal.as_ref());
            self.images.push(ImageData::rgba8(
                "ORM",
                orm.width,
                orm.height,
                ColorSpace::Linear,
                orm.pixels,
            ));
            Some(TextureRef::new(self.images.len() - 1))
        } else {
            None
        };
        let alpha_mode = if info.alpha_blend {
            AlphaMode::Blend
        } else if info.alpha_test {
            AlphaMode::Mask {
                cutoff: info.alpha_threshold as f32 / 255.0,
            }
        } else {
            AlphaMode::Opaque
        };
        MaterialDesc {
            name: info
                .mat_path
                .clone()
                .or_else(|| info.diffuse.clone())
                .unwrap_or_else(|| "NIF Material".to_string()),
            material_type: materials::BETHESDA_PBR.to_string(),
            textures: vec![base_color_texture, normal_texture, orm_texture],
            uniforms: materials::uniforms([1.0; 4], false),
            alpha_mode,
            // NIF two-sidedness flags are not yet interpreted; draw both faces.
            double_sided: true,
        }
    }
}

/// Second half of [`nif_to_scene`]: resolve materials and textures from
/// `files` and assemble the scene.
pub fn scene_from_built(built: BuiltModel, files: &HashMap<String, Vec<u8>>) -> NifSceneData {
    let missing_mats = built
        .materials
        .iter()
        .filter_map(|m| m.mat_path.as_ref())
        .filter(|p| lookup_file(files, p).is_none())
        .collect::<Vec<_>>();
    if !missing_mats.is_empty() {
        let hits = missing_mats
            .iter()
            .filter(|p| {
                let hints = material_texture_hints(p);
                lookup_file(files, &hints.color).is_some()
                    || lookup_file(files, &hints.normal).is_some()
            })
            .count();
        log::info!(
            "{} of {} missing materials resolved via heuristic textures",
            hits,
            missing_mats.len()
        );
    }

    let mut builder = Builder {
        files,
        images: Vec::new(),
        cache: HashMap::new(),
        diagnostics: Vec::new(),
    };
    let materials: Vec<MaterialDesc> = built
        .materials
        .iter()
        .map(|m| builder.material(m))
        .collect();
    let instances = built
        .meshes
        .iter()
        .enumerate()
        .map(|(i, m)| InstanceDesc {
            object_id: i as u32,
            ..InstanceDesc::new(i, m.material)
        })
        .collect();
    for d in &builder.diagnostics {
        log::warn!("{}", d);
    }
    NifSceneData {
        scene: SceneData {
            meshes: built.meshes.into_iter().map(|m| m.mesh).collect(),
            images: builder.images,
            materials,
            instances,
        },
        diagnostics: builder.diagnostics,
    }
}
