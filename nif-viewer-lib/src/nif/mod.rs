//! Bethesda NIF (20.2.0.7) and Starfield `.mesh` parsers.
//!
//! Pure Rust, no platform-specific code — compiles for wasm32.
//! See `docs/nif-format-spec.md` for the format reference.

pub mod mat;
pub mod mesh;
mod parser;
pub mod reader;
mod structure;

pub use mat::{extract_mat_texture_set, extract_mat_textures, MatTextureSet};
pub use mesh::MeshData;
pub use reader::NifError;
pub use structure::nif_structure;

/// A parsed NIF scene: flattened mesh instances with world transforms.
pub struct NifScene {
    pub meshes: Vec<NifMeshInstance>,
}

pub struct NifMeshInstance {
    pub name: String,
    /// Column-major world transform.
    pub transform: [[f32; 4]; 4],
    pub geometry: Geometry,
    pub material: MaterialInfo,
}

pub enum Geometry {
    Embedded(MeshData),
    External {
        /// Normalized: `geometries/<path>.mesh`, lowercase, forward slashes.
        path: String,
        indices_count: u32,
        num_verts: u32,
    },
}

#[derive(Default)]
pub struct MaterialInfo {
    /// Starfield `.mat` path (from shader property Name), normalized
    /// lowercase, forward slashes.
    pub mat_path: Option<String>,
    /// SSE/FO4 texture set slot 0.
    pub diffuse: Option<String>,
    /// Slot 1.
    pub normal: Option<String>,
    pub alpha_blend: bool,
    pub alpha_test: bool,
    pub alpha_threshold: u8,
}

/// Parse a NIF file into a flattened scene.
pub fn parse_nif(bytes: &[u8]) -> Result<NifScene, NifError> {
    parser::parse_nif(bytes)
}

/// Parse a standalone Starfield `.mesh` file.
pub fn parse_mesh(bytes: &[u8]) -> Result<MeshData, NifError> {
    mesh::parse_mesh(bytes)
}

/// Replace `Geometry::External` with `Geometry::Embedded` for every mesh
/// where `lookup` returns file bytes for the normalized path. Unresolved or
/// unparseable externals are left in place (a warning is logged); this
/// function only errors on internal invariant failures (currently never).
pub fn resolve_external(
    scene: &mut NifScene,
    lookup: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<(), NifError> {
    for instance in &mut scene.meshes {
        let path = match &instance.geometry {
            Geometry::External { path, .. } => path.clone(),
            Geometry::Embedded(_) => continue,
        };
        let bytes = match lookup(&path) {
            Some(b) => b,
            None => {
                log::debug!("external mesh not found: {}", path);
                continue;
            }
        };
        match mesh::parse_mesh(&bytes) {
            Ok(data) => instance.geometry = Geometry::Embedded(data),
            Err(e) => log::warn!("failed to parse external mesh {}: {}", path, e),
        }
    }
    Ok(())
}
