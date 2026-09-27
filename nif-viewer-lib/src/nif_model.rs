//! CPU-side conversion of a parsed [`NifScene`] into renderer vertex data.
//!
//! Pure functions with no GPU dependency so the pipeline can be unit tested.

use std::collections::HashMap;

use crate::model::ModelVertex;
use crate::nif::{Geometry, MaterialInfo, NifScene};

/// A mesh converted to renderer vertices, ready for buffer upload.
pub struct BuiltMesh {
    pub name: String,
    pub vertices: Vec<ModelVertex>,
    pub indices: Vec<u32>,
    /// Index into [`BuiltModel::materials`].
    pub material: usize,
    /// True when tangents came from the NIF data (no compute pass needed).
    pub has_tangents: bool,
}

/// CPU-side model: meshes plus deduplicated material descriptors.
pub struct BuiltModel {
    pub meshes: Vec<BuiltMesh>,
    pub materials: Vec<MaterialInfo>,
    pub bounding_center: [f32; 3],
    pub bounding_radius: f32,
}

/// Key used to deduplicate materials across mesh instances.
fn material_key(m: &MaterialInfo) -> String {
    format!(
        "{}|{}|{}",
        m.mat_path.as_deref().unwrap_or(""),
        m.diffuse.as_deref().unwrap_or(""),
        m.normal.as_deref().unwrap_or("")
    )
}

/// Rotate a Z-up vector into the renderer's Y-up space (-90 degrees about X).
fn z_up_to_y_up(v: [f32; 3]) -> [f32; 3] {
    [v[0], v[2], -v[1]]
}

fn transform_point(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    // Column-major: column i is m[i].
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

fn transform_dir(m: &[[f32; 4]; 4], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-8 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, 1.0, 0.0]
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Compute smooth (area-weighted averaged) normals from triangle data.
fn compute_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; positions.len()];
    for tri in indices.as_chunks::<3>().0 {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if a >= positions.len() || b >= positions.len() || c >= positions.len() {
            continue;
        }
        let n = cross(
            sub(positions[b], positions[a]),
            sub(positions[c], positions[a]),
        );
        for &i in &[a, b, c] {
            normals[i][0] += n[0];
            normals[i][1] += n[1];
            normals[i][2] += n[2];
        }
    }
    normals.into_iter().map(normalize).collect()
}

/// Convert a parsed NIF scene into renderer-space vertex data.
///
/// Positions are transformed by each instance's world transform, normals and
/// tangents by its rotation part, and everything is rotated from Z-up into
/// Y-up. Mesh instances whose geometry is still [`Geometry::External`]
/// (unresolved) are skipped with a warning.
pub fn build_model(scene: &NifScene) -> BuiltModel {
    let mut meshes = Vec::new();
    let mut materials: Vec<MaterialInfo> = Vec::new();
    let mut material_index: HashMap<String, usize> = HashMap::new();

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];

    for instance in &scene.meshes {
        let data = match &instance.geometry {
            Geometry::Embedded(d) => d,
            Geometry::External { path, .. } => {
                log::warn!(
                    "skipping mesh '{}': unresolved external geometry {}",
                    instance.name,
                    path
                );
                continue;
            }
        };
        if data.positions.is_empty() || data.indices.is_empty() {
            continue;
        }

        let m = &instance.transform;
        let has_normals = data.normals.len() == data.positions.len();
        let has_tangents = has_normals && data.tangents.len() == data.positions.len();

        let world_positions: Vec<[f32; 3]> = data
            .positions
            .iter()
            .map(|&p| z_up_to_y_up(transform_point(m, p)))
            .collect();

        for p in &world_positions {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }

        let normals: Vec<[f32; 3]> = if has_normals {
            data.normals
                .iter()
                .map(|&n| normalize(z_up_to_y_up(transform_dir(m, n))))
                .collect()
        } else {
            compute_normals(&world_positions, &data.indices)
        };

        let vertices: Vec<ModelVertex> = world_positions
            .iter()
            .enumerate()
            .map(|(i, &position)| {
                let tex_coords = data.uvs.get(i).copied().unwrap_or([0.0, 0.0]);
                let normal = normals[i];
                let (tangent, bitangent) = if has_tangents {
                    let t = data.tangents[i];
                    let tangent = normalize(z_up_to_y_up(transform_dir(m, [t[0], t[1], t[2]])));
                    let b = cross(normal, tangent);
                    (tangent, [b[0] * t[3], b[1] * t[3], b[2] * t[3]])
                } else {
                    ([0.0; 3], [0.0; 3])
                };
                ModelVertex {
                    position,
                    tex_coords,
                    normal,
                    tangent,
                    bitangent,
                    padding: [0u32; 2],
                }
            })
            .collect();

        let key = material_key(&instance.material);
        let material = *material_index.entry(key).or_insert_with(|| {
            materials.push(MaterialInfo {
                mat_path: instance.material.mat_path.clone(),
                diffuse: instance.material.diffuse.clone(),
                normal: instance.material.normal.clone(),
                alpha_blend: instance.material.alpha_blend,
                alpha_test: instance.material.alpha_test,
                alpha_threshold: instance.material.alpha_threshold,
            });
            materials.len() - 1
        });

        meshes.push(BuiltMesh {
            name: instance.name.clone(),
            vertices,
            indices: data.indices.clone(),
            material,
            has_tangents,
        });
    }

    if materials.is_empty() {
        materials.push(MaterialInfo::default());
    }

    let (bounding_center, bounding_radius) = if meshes.is_empty() {
        ([0.0; 3], 1.0)
    } else {
        let center = [
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ];
        let half = sub(max, center);
        let radius = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2])
            .sqrt()
            .max(1e-3);
        (center, radius)
    };

    BuiltModel {
        meshes,
        materials,
        bounding_center,
        bounding_radius,
    }
}

/// Normalize a resource path: lowercase, forward slashes, and remove the
/// optional Data-root prefix used by some Creation Engine references.
pub fn normalize_path(path: &str) -> String {
    let mut normalized = path.to_lowercase().replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("data/") {
        normalized = rest.to_string();
    }
    normalized
}

/// Heuristic texture paths derived from a Starfield `.mat` path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureHintPaths {
    pub color: String,
    pub normal: String,
    pub rough: String,
    pub metal: String,
    pub ao: String,
    pub stem: String,
}

/// Derive heuristic texture paths from a Starfield `.mat` path, mirroring
/// `materials/<rel>/<stem>.mat` →
/// `textures/<rel>/<stem>_{color,normal,rough,metal,ao}.dds`.
///
/// Vanilla Starfield keeps materials in a binary cdb, so loose dumps often
/// lack `.mat` files entirely; the texture tree usually mirrors the material
/// tree, making this a useful fallback. All paths are normalized lowercase
/// with forward slashes.
pub fn material_texture_hints(mat_path: &str) -> TextureHintPaths {
    let norm = normalize_path(mat_path);
    // Strip the .mat (or any) extension.
    let no_ext = match norm.rfind('.') {
        Some(dot) if dot > norm.rfind('/').map_or(0, |s| s + 1) => &norm[..dot],
        _ => norm.as_str(),
    };
    // Mirror materials/ → textures/; otherwise keep the path as-is.
    let base = if let Some(rest) = no_ext.strip_prefix("materials/") {
        format!("textures/{rest}")
    } else {
        no_ext.to_string()
    };
    let stem = base.rsplit('/').next().unwrap_or(&base).to_string();
    TextureHintPaths {
        color: format!("{base}_color.dds"),
        normal: format!("{base}_normal.dds"),
        rough: format!("{base}_rough.dds"),
        metal: format!("{base}_metal.dds"),
        ao: format!("{base}_ao.dds"),
        stem,
    }
}

/// Look up `path` in `files`, trying the full normalized path first and then
/// just the filename (so loose file selections work).
pub fn lookup_file<'a>(files: &'a HashMap<String, Vec<u8>>, path: &str) -> Option<&'a Vec<u8>> {
    let norm = normalize_path(path);
    if let Some(bytes) = files.get(&norm) {
        return Some(bytes);
    }
    let filename = norm.rsplit('/').next().unwrap_or(&norm);
    files.get(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_texture_hints_mirrors_materials_tree() {
        let h = material_texture_hints("Materials\\Weapons\\AR99\\AR99_Receiver.mat");
        assert_eq!(h.color, "textures/weapons/ar99/ar99_receiver_color.dds");
        assert_eq!(h.normal, "textures/weapons/ar99/ar99_receiver_normal.dds");
        assert_eq!(h.rough, "textures/weapons/ar99/ar99_receiver_rough.dds");
        assert_eq!(h.metal, "textures/weapons/ar99/ar99_receiver_metal.dds");
        assert_eq!(h.ao, "textures/weapons/ar99/ar99_receiver_ao.dds");
        assert_eq!(h.stem, "ar99_receiver");
    }

    #[test]
    fn material_texture_hints_without_materials_prefix() {
        let h = material_texture_hints("foo/bar/baz.mat");
        assert_eq!(h.color, "foo/bar/baz_color.dds");
        assert_eq!(h.normal, "foo/bar/baz_normal.dds");
        assert_eq!(h.stem, "baz");

        // No extension: still produces sensible hints.
        let h = material_texture_hints("materials/thing");
        assert_eq!(h.color, "textures/thing_color.dds");
        assert_eq!(h.stem, "thing");
    }

    #[test]
    fn lookup_falls_back_to_filename() {
        let mut files = HashMap::new();
        files.insert("hash.mesh".to_string(), vec![1u8]);
        files.insert("textures/foo_color.dds".to_string(), vec![2u8]);

        assert_eq!(
            lookup_file(&files, "geometries/ab/hash.mesh"),
            Some(&vec![1u8])
        );
        assert_eq!(
            lookup_file(&files, "Textures\\Foo_Color.DDS"),
            Some(&vec![2u8])
        );
        assert!(lookup_file(&files, "geometries/ab/missing.mesh").is_none());
    }

    #[test]
    fn normalize_strips_data_prefix() {
        assert_eq!(
            normalize_path("Data\\Textures\\Foo.DDS"),
            "textures/foo.dds"
        );
        assert_eq!(
            normalize_path("data/data/geometries/foo.mesh"),
            "geometries/foo.mesh"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_tests {
    use super::*;
    use crate::nif;
    use std::path::{Path, PathBuf};

    const SAMPLE_ROOT: &str = r"C:\Users\user\Documents\StarfieldResources";

    fn sample_root() -> Option<PathBuf> {
        let p = PathBuf::from(SAMPLE_ROOT);
        p.exists().then_some(p)
    }

    fn load_scene(root: &Path, nif_rel: &str) -> nif::NifScene {
        let bytes = std::fs::read(root.join(nif_rel)).unwrap();
        let mut scene = nif::parse_nif(&bytes).unwrap();
        nif::resolve_external(&mut scene, &mut |path| std::fs::read(root.join(path)).ok()).unwrap();
        scene
    }

    fn assert_built_model_sane(built: &BuiltModel) {
        assert!(!built.meshes.is_empty(), "expected at least one mesh");
        for mesh in &built.meshes {
            assert!(!mesh.vertices.is_empty());
            assert!(!mesh.indices.is_empty());
            assert_eq!(mesh.indices.len() % 3, 0);
            let max_index = *mesh.indices.iter().max().unwrap() as usize;
            assert!(max_index < mesh.vertices.len());
            for v in &mesh.vertices {
                assert!(v.position.iter().all(|f| f.is_finite()));
                assert!(v.normal.iter().all(|f| f.is_finite()));
                assert!(v.tangent.iter().all(|f| f.is_finite()));
                assert!(v.bitangent.iter().all(|f| f.is_finite()));
            }
        }
        assert!(built.bounding_radius.is_finite());
        assert!(built.bounding_radius > 0.0);
    }

    #[test]
    fn builds_marker_north() {
        let Some(root) = sample_root() else {
            eprintln!("sample dir missing, skipping");
            return;
        };
        let scene = load_scene(&root, r"meshes\marker_north.nif");
        let built = build_model(&scene);
        assert_built_model_sane(&built);
    }

    #[test]
    fn builds_furniture_with_materials() {
        let Some(root) = sample_root() else {
            eprintln!("sample dir missing, skipping");
            return;
        };
        let scene = load_scene(&root, r"meshes\furniture\sit_chairleatherlowback01.nif");
        let built = build_model(&scene);
        assert_built_model_sane(&built);

        // Verify .mat / .dds lookup logic against loose files on disk.
        let mut found_mat = 0usize;
        let mut found_dds = 0usize;
        for mat in &built.materials {
            let Some(mat_path) = &mat.mat_path else {
                continue;
            };
            let disk_path = root.join(mat_path);
            if !disk_path.exists() {
                eprintln!("note: .mat not present on disk: {}", mat_path);
                continue;
            }
            found_mat += 1;
            let mat_bytes = std::fs::read(&disk_path).unwrap();
            let (albedo, normal) = nif::extract_mat_textures(&mat_bytes);
            for tex in [albedo, normal].iter().flatten() {
                if root.join(tex).exists() {
                    found_dds += 1;
                } else {
                    eprintln!("note: dds referenced by .mat not on disk: {}", tex);
                }
            }
        }
        eprintln!(
            "materials: {}, .mat found on disk: {}, dds found: {}",
            built.materials.len(),
            found_mat,
            found_dds
        );
        // Missing .mat/.dds is acceptable — the renderer falls back to the
        // default material — but the scene itself must reference materials.
        assert!(!built.materials.is_empty());
    }

    #[test]
    fn decodes_sample_dds() {
        let Some(root) = sample_root() else {
            eprintln!("sample dir missing, skipping");
            return;
        };
        // Find a couple of real DDS files and decode their top mip.
        let mut decoded = 0usize;
        let mut stack = vec![root.join("textures")];
        'outer: while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
                {
                    let bytes = std::fs::read(&path).unwrap();
                    let is_normal = path
                        .to_string_lossy()
                        .to_lowercase()
                        .ends_with("_normal.dds");
                    match crate::texture::decode_dds_rgba8(&bytes, is_normal) {
                        Ok((rgba, w, h)) => {
                            assert_eq!(rgba.len(), (w * h * 4) as usize);
                            assert!(w > 0 && h > 0);
                            decoded += 1;
                        }
                        Err(e) => eprintln!("note: {} not decodable: {}", path.display(), e),
                    }
                    if decoded >= 4 {
                        break 'outer;
                    }
                }
            }
        }
        assert!(decoded > 0, "expected to decode at least one DDS");
    }
}
