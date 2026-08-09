//! Native integration tests against local Starfield sample assets.
//! Tests pass trivially when the sample directory doesn't exist.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use nif_viewer_lib::nif::{parse_mesh, parse_nif, resolve_external, Geometry};

const RES_ROOT: &str = r"C:\Users\secro\Documents\StarfieldResources";

fn meshes_dir() -> PathBuf {
    Path::new(RES_ROOT).join("meshes")
}

fn geometries_dir() -> PathBuf {
    Path::new(RES_ROOT).join("geometries")
}

fn collect_nifs(dir: &Path, out: &mut Vec<PathBuf>, limit: usize) {
    if out.len() >= limit {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut dirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else if path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("nif"))
            .unwrap_or(false)
        {
            out.push(path);
            if out.len() >= limit {
                return;
            }
        }
    }
    for d in dirs {
        collect_nifs(&d, out, limit);
        if out.len() >= limit {
            return;
        }
    }
}

fn check_mesh_consistency(name: &str, data: &nif_viewer_lib::nif::MeshData) {
    let nv = data.positions.len();
    assert!(nv > 0, "{}: no positions", name);
    if !data.normals.is_empty() {
        assert_eq!(data.normals.len(), nv, "{}: normals/positions mismatch", name);
    }
    if !data.uvs.is_empty() {
        assert_eq!(data.uvs.len(), nv, "{}: uvs/positions mismatch", name);
    }
    if let Some(&max) = data.indices.iter().max() {
        assert!(
            (max as usize) < nv,
            "{}: max index {} >= vertex count {}",
            name,
            max,
            nv
        );
    }
}

#[test]
fn parse_marker_north() {
    let path = meshes_dir().join("marker_north.nif");
    if !path.exists() {
        eprintln!("sample assets not present; skipping");
        return;
    }
    let bytes = fs::read(&path).expect("read marker_north.nif");
    let scene = parse_nif(&bytes).expect("parse marker_north.nif");
    assert!(!scene.meshes.is_empty(), "marker_north.nif has no meshes");
}

#[test]
fn parse_nif_corpus() {
    let root = meshes_dir();
    if !root.exists() {
        eprintln!("sample assets not present; skipping");
        return;
    }

    // Mix directories per instructions.
    let mut files = Vec::new();
    for sub in ["items", "furniture", "setdressing", "ships"] {
        let limit = files.len() + 40;
        collect_nifs(&root.join(sub), &mut files, limit);
    }
    collect_nifs(&root, &mut files, 200);
    files.truncate(200);
    assert!(!files.is_empty(), "no .nif files found under {:?}", root);

    let geo_root = geometries_dir();
    let mut ok = 0usize;
    let mut failures: Vec<(PathBuf, String)> = Vec::new();
    let mut resolved = 0usize;
    let mut unresolved = 0usize;
    let mut mesh_counts: BTreeMap<&'static str, usize> = BTreeMap::new();

    for file in &files {
        let bytes = match fs::read(file) {
            Ok(b) => b,
            Err(e) => {
                failures.push((file.clone(), e.to_string()));
                continue;
            }
        };
        match parse_nif(&bytes) {
            Ok(mut scene) => {
                ok += 1;
                let mut lookup = |p: &str| -> Option<Vec<u8>> {
                    // p is "geometries/<hash>.mesh"; strip the prefix.
                    let rel = p.strip_prefix("geometries/").unwrap_or(p);
                    fs::read(geo_root.join(rel)).ok()
                };
                resolve_external(&mut scene, &mut lookup).unwrap();
                for inst in &scene.meshes {
                    match &inst.geometry {
                        Geometry::Embedded(data) => {
                            *mesh_counts.entry("embedded").or_default() += 1;
                            resolved += 1;
                            check_mesh_consistency(&format!("{:?}/{}", file, inst.name), data);
                        }
                        Geometry::External { .. } => {
                            *mesh_counts.entry("external-unresolved").or_default() += 1;
                            unresolved += 1;
                        }
                    }
                }
            }
            Err(e) => failures.push((file.clone(), e.to_string())),
        }
    }

    println!("--- NIF corpus summary ---");
    println!("parsed OK: {} / {}", ok, files.len());
    println!("mesh instances: {:?}", mesh_counts);
    println!("resolved geometry: {}, unresolved: {}", resolved, unresolved);
    for (f, e) in &failures {
        println!("FAIL {:?}: {}", f, e);
    }

    let success_rate = ok as f64 / files.len() as f64;
    assert!(
        success_rate >= 0.8,
        "parse success rate {:.1}% < 80%",
        success_rate * 100.0
    );
}

#[test]
fn parse_standalone_mesh_files() {
    let geo_root = geometries_dir();
    if !geo_root.exists() {
        eprintln!("sample assets not present; skipping");
        return;
    }
    let mut count = 0usize;
    let mut stack = vec![geo_root];
    'outer: while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("mesh"))
                .unwrap_or(false)
            {
                let bytes = fs::read(&path).expect("read .mesh");
                let data = parse_mesh(&bytes)
                    .unwrap_or_else(|e| panic!("parse {:?}: {}", path, e));
                check_mesh_consistency(&format!("{:?}", path), &data);
                count += 1;
                if count >= 10 {
                    break 'outer;
                }
            }
        }
    }
    println!("parsed {} standalone .mesh files", count);
    assert!(count > 0, "no .mesh files found");
}
