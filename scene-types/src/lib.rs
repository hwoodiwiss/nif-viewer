//! Renderer-neutral CPU scene descriptors.
//!
//! # Conventions
//!
//! * Right-handed, Y-up world space. Front faces wind counter-clockwise.
//! * Matrices are column-major `[[f32; 4]; 4]`: `m[c]` is column `c`, so the
//!   translation lives in `m[3]`.
//! * UV origin is the top-left of the image; V increases downwards.
//! * Normal maps are tangent-space, +Y "OpenGL" convention, encoded `n * 0.5 + 0.5`.
//! * Tangents store handedness in `w` (`±1`): `bitangent = cross(normal, tangent.xyz) * w`.
//! * Colour factors and vertex colours are linear. Colour textures declare
//!   their colour space through [`ColorSpace`]; data textures (normal, ORM)
//!   must be [`ColorSpace::Linear`].
//! * Normals are transformed by the inverse-transpose of an instance transform.
//!   Singular or non-finite transforms are rejected by [`SceneData::validate`].
//!
//! Descriptors reference each other by index into the owning [`SceneData`].

mod camera;
mod prepare;
mod validate;

pub use camera::{Camera, DirectionalLight, ViewParams};
pub use prepare::{compute_normals, generate_tangents, prepare_mesh, PreparedMesh};
pub use validate::SceneError;

pub type Mat4 = [[f32; 4]; 4];

pub const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// Indexed triangle-list geometry. Optional attributes must match
/// `positions.len()` when present.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshData {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Missing normals are generated (area-weighted smooth) by [`prepare_mesh`].
    pub normals: Option<Vec<[f32; 3]>>,
    /// Missing UVs are treated as `[0, 0]`.
    pub uv0: Option<Vec<[f32; 2]>>,
    /// Linear RGBA vertex colours.
    pub colors: Option<Vec<[f32; 4]>>,
    /// Tangent xyz plus handedness in w. Missing tangents are generated from UVs.
    pub tangents: Option<Vec<[f32; 4]>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    Srgb,
    Linear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    /// Tightly packed 8-bit RGBA, the portable baseline.
    Rgba8,
}

impl ImageFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            ImageFormat::Rgba8 => 4,
        }
    }
}

/// A single-mip 2D image.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageData {
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub color_space: ColorSpace,
    pub pixels: Vec<u8>,
}

impl ImageData {
    pub fn rgba8(
        label: impl Into<String>,
        width: u32,
        height: u32,
        color_space: ColorSpace,
        pixels: Vec<u8>,
    ) -> Self {
        Self {
            label: label.into(),
            width,
            height,
            format: ImageFormat::Rgba8,
            color_space,
            pixels,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressMode {
    ClampToEdge,
    Repeat,
    MirrorRepeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterMode {
    Nearest,
    Linear,
}

/// Sampling state, independent of image identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SamplerDesc {
    pub address_u: AddressMode,
    pub address_v: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
}

impl Default for SamplerDesc {
    fn default() -> Self {
        Self {
            address_u: AddressMode::ClampToEdge,
            address_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Nearest,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureRef {
    /// Index into [`SceneData::images`].
    pub image: usize,
    pub sampler: SamplerDesc,
}

impl TextureRef {
    pub fn new(image: usize) -> Self {
        Self {
            image,
            sampler: SamplerDesc::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlphaMode {
    Opaque,
    Mask { cutoff: f32 },
    Blend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShadingModel {
    /// Cook-Torrance GGX with a packed ORM texture (R=AO, G=roughness, B=metalness).
    MetallicRoughness,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialDesc {
    pub name: String,
    pub shading: ShadingModel,
    /// Linear RGBA multiplier on the base colour texture.
    pub base_color_factor: [f32; 4],
    pub base_color_texture: Option<TextureRef>,
    pub normal_texture: Option<TextureRef>,
    /// Packed occlusion/roughness/metallic, linear.
    pub orm_texture: Option<TextureRef>,
    pub alpha_mode: AlphaMode,
    pub double_sided: bool,
    /// Multiply base colour by the mesh's vertex colours.
    pub use_vertex_colors: bool,
}

impl Default for MaterialDesc {
    fn default() -> Self {
        Self {
            name: String::new(),
            shading: ShadingModel::MetallicRoughness,
            base_color_factor: [1.0; 4],
            base_color_texture: None,
            normal_texture: None,
            orm_texture: None,
            alpha_mode: AlphaMode::Opaque,
            double_sided: true,
            use_vertex_colors: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceDesc {
    /// Index into [`SceneData::meshes`].
    pub mesh: usize,
    /// Index into [`SceneData::materials`].
    pub material: usize,
    pub transform: Mat4,
    pub visible: bool,
    /// Caller-supplied identifier, carried through for picking/diagnostics.
    pub object_id: u32,
}

impl InstanceDesc {
    pub fn new(mesh: usize, material: usize) -> Self {
        Self {
            mesh,
            material,
            transform: IDENTITY,
            visible: true,
            object_id: 0,
        }
    }
}

/// A complete, self-contained scene.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneData {
    pub meshes: Vec<MeshData>,
    pub images: Vec<ImageData>,
    pub materials: Vec<MaterialDesc>,
    pub instances: Vec<InstanceDesc>,
}

/// Axis-aligned bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn center(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// Radius of the sphere enclosing the box, at least `1e-3`.
    pub fn radius(&self) -> f32 {
        let c = self.center();
        let h = [self.max[0] - c[0], self.max[1] - c[1], self.max[2] - c[2]];
        (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt().max(1e-3)
    }

    fn include(&mut self, p: [f32; 3]) {
        for ((min, max), v) in self.min.iter_mut().zip(self.max.iter_mut()).zip(p) {
            *min = min.min(v);
            *max = max.max(v);
        }
    }
}

impl SceneData {
    /// World-space bounds of visible instances, or `None` for an empty scene.
    pub fn bounds(&self) -> Option<Bounds> {
        let mut bounds: Option<Bounds> = None;
        for instance in self.instances.iter().filter(|i| i.visible) {
            let Some(mesh) = self.meshes.get(instance.mesh) else {
                continue;
            };
            for &p in &mesh.positions {
                let w = transform_point(&instance.transform, p);
                match &mut bounds {
                    Some(b) => b.include(w),
                    None => bounds = Some(Bounds { min: w, max: w }),
                }
            }
        }
        bounds
    }
}

pub fn transform_point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

/// Inverse-transpose of the upper 3x3 (column-major) and the sign of its
/// determinant, or `None` when the matrix is singular or non-finite.
pub fn normal_matrix(m: &Mat4) -> Option<([[f32; 3]; 3], f32)> {
    use cgmath::{Matrix, Matrix3, SquareMatrix};
    let upper = Matrix3::new(
        m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2],
    );
    let det = upper.determinant();
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let inv_t = upper.invert()?.transpose();
    Some((inv_t.into(), det.signum()))
}
