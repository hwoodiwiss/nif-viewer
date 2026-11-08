//! Stock material types shipped with the renderer.

use scene_types::{
    AlphaMode, ColorSpace, MaterialDesc, MaterialTypeDesc, TextureRef, TextureSlot, BASIC_MATERIAL,
};

/// Type description for the stock `basic` material: diffuse (sRGB) and
/// normal-map (linear) slots, Lambert lighting, and a 32-byte uniform block
/// of `base_color_factor` and flags (`x` = use vertex colours).
pub fn basic_material_type(source: impl Into<String>) -> MaterialTypeDesc {
    MaterialTypeDesc {
        key: BASIC_MATERIAL.to_string(),
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
        ],
        uniform_size: 32,
        default_uniforms: factor_and_flags([1.0; 4], false),
    }
}

/// Encode a `vec4 base_color_factor; vec4 flags` uniform block, the layout
/// used by the stock material (and suitable for similar material types).
pub fn factor_and_flags(base_color_factor: [f32; 4], use_vertex_colors: bool) -> Vec<u8> {
    let flags = [if use_vertex_colors { 1.0f32 } else { 0.0 }, 0.0, 0.0, 0.0];
    base_color_factor
        .iter()
        .chain(flags.iter())
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

/// Parameters for a stock `basic` material instance.
#[derive(Debug, Clone, PartialEq)]
pub struct BasicMaterial {
    pub name: String,
    /// Linear RGBA multiplier on the diffuse texture.
    pub base_color_factor: [f32; 4],
    pub diffuse: Option<TextureRef>,
    pub normal: Option<TextureRef>,
    pub use_vertex_colors: bool,
    pub alpha_mode: AlphaMode,
    pub double_sided: bool,
}

impl Default for BasicMaterial {
    fn default() -> Self {
        Self {
            name: String::new(),
            base_color_factor: [1.0; 4],
            diffuse: None,
            normal: None,
            use_vertex_colors: false,
            alpha_mode: AlphaMode::Opaque,
            double_sided: true,
        }
    }
}

impl From<BasicMaterial> for MaterialDesc {
    fn from(m: BasicMaterial) -> Self {
        MaterialDesc {
            name: m.name,
            material_type: BASIC_MATERIAL.to_string(),
            textures: vec![m.diffuse, m.normal],
            uniforms: factor_and_flags(m.base_color_factor, m.use_vertex_colors),
            alpha_mode: m.alpha_mode,
            double_sided: m.double_sided,
        }
    }
}
