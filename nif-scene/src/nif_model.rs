//! CPU-side conversion of a parsed [`NifScene`] into renderer-neutral meshes.
//!
//! Pure functions with no GPU dependency so the conversion can be unit tested.

use std::collections::HashMap;

use scene_types::MeshData;

use crate::nif::{Geometry, MaterialInfo, NifScene};

/// A converted mesh with world transform and Z-up → Y-up rotation baked in.
pub struct BuiltMesh {
    pub mesh: MeshData,
    /// Index into [`BuiltModel::materials`].
    pub material: usize,
}

/// CPU-side model: meshes plus deduplicated NIF material descriptors.
pub struct BuiltModel {
    pub meshes: Vec<BuiltMesh>,
    pub materials: Vec<MaterialInfo>,
}

/// Key used to deduplicate materials across mesh instances. Includes every
/// field that affects rendering.
fn material_key(m: &MaterialInfo) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        m.mat_path.as_deref().unwrap_or(""),
        m.diffuse.as_deref().unwrap_or(""),
        m.normal.as_deref().unwrap_or(""),
        m.alpha_blend,
        m.alpha_test,
        m.alpha_threshold
    )
}

/// Rotate a Z-up vector into the renderer's Y-up space (-90 degrees about X).
fn z_up_to_y_up(v: [f32; 3]) -> [f32; 3] {
    [v[0], v[2], -v[1]]
}

fn transform_point(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    scene_types::transform_point(m, p)
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

/// Convert a parsed NIF scene into renderer-space meshes.
///
/// Positions are transformed by each instance's world transform, normals and
/// tangents by its linear part, and everything is rotated from Z-up into
/// Y-up. The result must be drawn with identity instance transforms. Mesh
/// instances whose geometry is still [`Geometry::External`] (unresolved) are
/// skipped with a warning. UVs, vertex colours and tangent handedness are
/// preserved; missing normals are generated from the transformed positions.
pub fn build_model(scene: &NifScene) -> BuiltModel {
    let mut meshes = Vec::new();
    let mut materials: Vec<MaterialInfo> = Vec::new();
    let mut material_index: HashMap<String, usize> = HashMap::new();
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
        let n = data.positions.len();
        let has_normals = data.normals.len() == n;
        let has_tangents = has_normals && data.tangents.len() == n;

        let positions: Vec<[f32; 3]> = data
            .positions
            .iter()
            .map(|&p| z_up_to_y_up(transform_point(m, p)))
            .collect();

        let normals = if has_normals {
            data.normals
                .iter()
                .map(|&v| normalize(z_up_to_y_up(transform_dir(m, v))))
                .collect()
        } else {
            scene_types::compute_normals(&positions, &data.indices)
        };
        let tangents = has_tangents.then(|| {
            data.tangents
                .iter()
                .map(|t| {
                    let d = normalize(z_up_to_y_up(transform_dir(m, [t[0], t[1], t[2]])));
                    [d[0], d[1], d[2], if t[3] < 0.0 { -1.0 } else { 1.0 }]
                })
                .collect()
        });
        let uv0 = (0..n)
            .map(|i| data.uvs.get(i).copied().unwrap_or([0.0, 0.0]))
            .collect();

        let key = material_key(&instance.material);
        let material = *material_index.entry(key).or_insert_with(|| {
            materials.push(instance.material.clone());
            materials.len() - 1
        });

        meshes.push(BuiltMesh {
            mesh: MeshData {
                name: instance.name.clone(),
                positions,
                indices: data.indices.clone(),
                normals: Some(normals),
                uv0: Some(uv0),
                colors: (data.colors.len() == n).then(|| data.colors.clone()),
                tangents,
            },
            material,
        });
    }

    if materials.is_empty() {
        materials.push(MaterialInfo::default());
    }

    BuiltModel { meshes, materials }
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
    use std::path::Path;

    fn load_scene(root: &Path, nif_rel: &str) -> nif::NifScene {
        let bytes = std::fs::read(root.join(nif_rel)).unwrap();
        let mut scene = nif::parse_nif(&bytes).unwrap();
        nif::resolve_external(&mut scene, &mut |path| std::fs::read(root.join(path)).ok()).unwrap();
        scene
    }

    fn assert_built_model_sane(built: &BuiltModel) {
        assert!(!built.meshes.is_empty(), "expected at least one mesh");
        let scene = scene_types::SceneData {
            meshes: built.meshes.iter().map(|m| m.mesh.clone()).collect(),
            materials: vec![Default::default()],
            instances: (0..built.meshes.len())
                .map(|i| scene_types::InstanceDesc::new(i, 0))
                .collect(),
            ..Default::default()
        };
        scene
            .validate()
            .expect("built meshes must be valid renderer input");
    }

    #[test]
    #[ignore = "requires NIF_STARFIELD_DATA_ROOT (extracted Starfield data folder)"]
    fn builds_marker_north() {
        let root = crate::starfield_data_root();
        let scene = load_scene(&root, "meshes/marker_north.nif");
        let built = build_model(&scene);
        assert_built_model_sane(&built);
    }

    #[test]
    #[ignore = "requires NIF_STARFIELD_DATA_ROOT (extracted Starfield data folder)"]
    fn builds_furniture_with_materials() {
        let root = crate::starfield_data_root();
        let scene = load_scene(&root, "meshes/furniture/sit_chairleatherlowback01.nif");
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
    #[ignore = "requires NIF_STARFIELD_DATA_ROOT (extracted Starfield data folder)"]
    fn decodes_sample_dds() {
        let root = crate::starfield_data_root();
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
                    match crate::textures::decode_dds_rgba8(&bytes, is_normal) {
                        Ok(img) => {
                            assert_eq!(img.pixels.len(), (img.width * img.height * 4) as usize);
                            assert!(img.width > 0 && img.height > 0);
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
