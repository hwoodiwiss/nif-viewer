//! Generated Fallout 3/NV-style NIFs; no proprietary fixture is needed.
use nif_viewer_lib::nif::{parse_nif, Geometry};

fn u16le(out: &mut Vec<u8>, value: u16) { out.extend(value.to_le_bytes()); }
fn u32le(out: &mut Vec<u8>, value: u32) { out.extend(value.to_le_bytes()); }
fn f32le(out: &mut Vec<u8>, value: f32) { out.extend(value.to_le_bytes()); }
fn string(out: &mut Vec<u8>, value: &str) { u32le(out, value.len() as u32); out.extend(value.as_bytes()); }

fn fixture(data_ref: i32, strips: &[&[u16]]) -> Vec<u8> {
    let mut shape = Vec::new();
    u32le(&mut shape, 0); // name index
    u32le(&mut shape, 0); // extra data count
    u32le(&mut shape, u32::MAX); // controller
    u32le(&mut shape, 14); // flags
    for value in [10.0, 20.0, 30.0] { f32le(&mut shape, value); }
    for value in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] { f32le(&mut shape, value); }
    f32le(&mut shape, 2.0); // scale
    u32le(&mut shape, 1); // property count
    u32le(&mut shape, 2); // legacy shader ref
    u32le(&mut shape, u32::MAX); // collision
    u32le(&mut shape, data_ref as u32);
    u32le(&mut shape, u32::MAX); // skin
    u32le(&mut shape, 0); // materials
    u32le(&mut shape, u32::MAX); // active material
    shape.push(0);

    let mut data = Vec::new();
    u32le(&mut data, 0); // group
    u16le(&mut data, 4); // vertices
    data.extend([0, 0, 1]); // keep/compress/has vertices
    for vertex in [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]] {
        for value in vertex { f32le(&mut data, value); }
    }
    u16le(&mut data, 1); // UV present
    data.push(1); // normals
    for _ in 0..4 { for v in [0.0, 0.0, 1.0] { f32le(&mut data, v); } }
    data.extend([0; 16]); // bounds
    data.push(1); // colors
    for _ in 0..16 { f32le(&mut data, 1.0); }
    for uv in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
        for v in uv { f32le(&mut data, v); }
    }
    u16le(&mut data, 0); // consistency
    u32le(&mut data, u32::MAX); // additional data
    u16le(&mut data, 2); // triangles
    u16le(&mut data, strips.len() as u16);
    for strip in strips { u16le(&mut data, strip.len() as u16); }
    data.push(1);
    for strip in strips { for &index in *strip { u16le(&mut data, index); } }

    let mut shader = Vec::new();
    u32le(&mut shader, u32::MAX);
    u32le(&mut shader, 0);
    u32le(&mut shader, u32::MAX);
    shader.extend([0; 22]);
    u32le(&mut shader, 3); // texture set
    let mut textures = Vec::new();
    u32le(&mut textures, 2);
    string(&mut textures, "textures\\test.dds");
    string(&mut textures, "textures\\test_n.dds");

    let blocks = [shape, data, shader, textures];
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    u32le(&mut out, 0x14020007);
    out.push(1); // endian
    u32le(&mut out, 11); // user version
    u32le(&mut out, 4); // blocks
    u32le(&mut out, 34); // Bethesda version
    out.extend([1, 0, 1, 0, 1, 0]); // export strings
    u16le(&mut out, 4);
    for name in ["NiTriStrips", "NiTriStripsData", "BSShaderPPLightingProperty", "BSShaderTextureSet"] { string(&mut out, name); }
    for index in 0..4 { u16le(&mut out, index); }
    for block in &blocks { u32le(&mut out, block.len() as u32); }
    u32le(&mut out, 1); u32le(&mut out, 4); string(&mut out, "quad");
    u32le(&mut out, 0); // groups
    for block in blocks { out.extend(block); }
    u32le(&mut out, 1); u32le(&mut out, 0); // root
    out
}

#[test]
fn parses_strip_attributes_transform_and_legacy_texture_refs() {
    let scene = parse_nif(&fixture(1, &[&[0, 1, 2, 3]])).unwrap();
    assert_eq!(scene.meshes.len(), 1);
    let instance = &scene.meshes[0];
    assert_eq!(instance.name, "quad");
    assert_eq!(instance.transform[3], [10.0, 20.0, 30.0, 1.0]);
    assert_eq!(instance.transform[0][0], 2.0);
    assert_eq!(instance.material.diffuse.as_deref(), Some("textures/test.dds"));
    assert_eq!(instance.material.normal.as_deref(), Some("textures/test_n.dds"));
    let Geometry::Embedded(mesh) = &instance.geometry else { panic!("not embedded") };
    assert_eq!(mesh.indices, [0, 1, 2, 2, 1, 3]);
    assert_eq!(mesh.positions.len(), 4);
    assert_eq!(mesh.normals, [[0.0, 0.0, 1.0]; 4]);
    assert_eq!(mesh.colors, [[1.0; 4]; 4]);
    assert_eq!(mesh.uvs[3], [1.0, 1.0]);
}

#[test]
fn degenerate_connectors_preserve_parity_and_each_strip_restarts() {
    let scene = parse_nif(&fixture(1, &[&[0, 1, 1, 2, 3], &[0, 1, 2]])).unwrap();
    let Geometry::Embedded(mesh) = &scene.meshes[0].geometry else { panic!("not embedded") };
    assert_eq!(mesh.indices, [1, 2, 3, 0, 1, 2]);
}

#[test]
fn invalid_data_refs_and_vertex_indices_do_not_panic_or_reach_renderer() {
    for reference in [-1, 99, 2] {
        assert!(parse_nif(&fixture(reference, &[&[0, 1, 2]])).unwrap().meshes.is_empty());
    }
    assert!(parse_nif(&fixture(1, &[&[0, 1, 99]])).unwrap().meshes.is_empty());
}

#[test]
#[ignore = "set NIF_TEST_FILE to a local NiTriStrips NIF"]
fn local_strip_model_has_renderable_geometry_and_existing_textures() {
    let path = std::path::PathBuf::from(std::env::var("NIF_TEST_FILE").expect("NIF_TEST_FILE"));
    let root = std::path::PathBuf::from(std::env::var("NIF_TEST_DATA_ROOT").expect("NIF_TEST_DATA_ROOT"));
    let scene = parse_nif(&std::fs::read(path).unwrap()).unwrap();
    assert!(!scene.meshes.is_empty());
    for instance in &scene.meshes {
        let Geometry::Embedded(mesh) = &instance.geometry else { panic!("external geometry") };
        assert!(!mesh.indices.is_empty());
        assert!(mesh.indices.iter().all(|&i| (i as usize) < mesh.positions.len()));
        for texture in [&instance.material.diffuse, &instance.material.normal].iter().filter_map(|value| value.as_ref()) {
            assert!(root.join(texture).is_file(), "missing {}", texture);
        }
    }
    println!("Verified {} strip meshes and their texture paths", scene.meshes.len());
}
