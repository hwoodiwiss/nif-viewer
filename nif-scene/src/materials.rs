//! The NIF viewer's `bethesda-pbr` material type.
//!
//! The WGSL lives in this crate's `shaders/` directory (with a
//! `manifest.json`) and is staged and loaded by hosts like any other shader
//! asset; this module only describes the type's bindings.

use scene_types::{ColorSpace, MaterialTypeDesc, TextureSlot};

/// Material type key used by [`crate::nif_scene`] conversions.
pub const BETHESDA_PBR: &str = "bethesda-pbr";

/// Texture slot order for [`BETHESDA_PBR`] materials.
pub const SLOT_DIFFUSE: usize = 0;
pub const SLOT_NORMAL: usize = 1;
pub const SLOT_ORM: usize = 2;

/// Default ORM texel: AO=255 (no occlusion), roughness≈0.78, metalness=0.
pub use crate::textures::DEFAULT_ORM;

/// Directory of this crate's material shaders in its source package, for
/// development and asset staging. Distributed hosts load a staged copy.
pub fn packaged_material_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders")
}

/// Describe the `bethesda-pbr` type for `source` (its WGSL, loaded by the host).
pub fn bethesda_pbr_type(source: impl Into<String>) -> MaterialTypeDesc {
    MaterialTypeDesc {
        key: BETHESDA_PBR.to_string(),
        source: source.into(),
        textures: vec![
            TextureSlot {
                name: "diffuse".into(),
                color_space: ColorSpace::Srgb,
                fallback: [255, 255, 255, 255],
            },
            TextureSlot {
                name: "normal".into(),
                color_space: ColorSpace::Linear,
                fallback: [128, 128, 255, 255],
            },
            TextureSlot {
                name: "orm".into(),
                color_space: ColorSpace::Linear,
                fallback: DEFAULT_ORM,
            },
        ],
        uniform_size: 32,
        default_uniforms: uniforms([1.0; 4], false),
    }
}

/// Encode the `bethesda-pbr` uniform block: base colour factor and flags
/// (`x` = multiply by vertex colour).
pub fn uniforms(base_color_factor: [f32; 4], use_vertex_colors: bool) -> Vec<u8> {
    let flags = [if use_vertex_colors { 1.0f32 } else { 0.0 }, 0.0, 0.0, 0.0];
    base_color_factor
        .iter()
        .chain(flags.iter())
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_declaration_is_valid() {
        let ty = bethesda_pbr_type("");
        ty.validate().unwrap();
        assert_eq!(ty.textures[SLOT_ORM].color_space, ColorSpace::Linear);
        assert_eq!(&uniforms([1.0; 4], true)[16..20], &1.0f32.to_le_bytes());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn packaged_manifest_lists_the_type() {
        let manifest =
            std::fs::read_to_string(packaged_material_dir().join("manifest.json")).unwrap();
        assert!(manifest.contains(&format!("\"{}\"", BETHESDA_PBR)));
        assert!(packaged_material_dir().join("bethesda_pbr.wgsl").is_file());
    }
}
