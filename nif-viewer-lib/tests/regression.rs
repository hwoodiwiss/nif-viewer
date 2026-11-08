//! Renderer regression suite: generated fixtures only.
//!
//! Exercises NIF conversion and an offscreen render through the
//! `nif_viewer_lib::regression` seam so identical expectations hold before and
//! after renderer restructuring. GPU tests skip when no adapter is available
//! unless `NIF_VIEWER_REQUIRE_GPU=1` is set.

use std::collections::HashMap;

use nif_viewer_lib::regression::{
    convert_nif, render_nif_offscreen, RegressionLight, RenderedImage,
};

fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_le_bytes());
}
fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_le_bytes());
}
fn f32le(out: &mut Vec<u8>, value: f32) {
    out.extend(value.to_le_bytes());
}
fn string(out: &mut Vec<u8>, value: &str) {
    u32le(out, value.len() as u32);
    out.extend(value.as_bytes());
}

/// A 20.2.0.7 (FO3/NV) NiTriStrips quad: unit square in NIF XY (Z-up) with
/// +Z normals, translated by (10,20,30) and scaled by 2.
fn quad_nif() -> Vec<u8> {
    let mut shape = Vec::new();
    u32le(&mut shape, 0);
    u32le(&mut shape, 0);
    u32le(&mut shape, u32::MAX);
    u32le(&mut shape, 14);
    for v in [10.0, 20.0, 30.0] {
        f32le(&mut shape, v);
    }
    for v in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] {
        f32le(&mut shape, v);
    }
    f32le(&mut shape, 2.0);
    u32le(&mut shape, 1);
    u32le(&mut shape, 2);
    u32le(&mut shape, u32::MAX);
    u32le(&mut shape, 1);
    u32le(&mut shape, u32::MAX);
    u32le(&mut shape, 0);
    u32le(&mut shape, u32::MAX);
    shape.push(0);

    let mut data = Vec::new();
    u32le(&mut data, 0);
    u16le(&mut data, 4);
    data.extend([0, 0, 1]);
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ] {
        for v in p {
            f32le(&mut data, v);
        }
    }
    u16le(&mut data, 1);
    data.push(1);
    for _ in 0..4 {
        for v in [0.0, 0.0, 1.0] {
            f32le(&mut data, v);
        }
    }
    data.extend([0; 16]);
    data.push(1);
    for _ in 0..16 {
        f32le(&mut data, 1.0);
    }
    for uv in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
        for v in uv {
            f32le(&mut data, v);
        }
    }
    u16le(&mut data, 0);
    u32le(&mut data, u32::MAX);
    u16le(&mut data, 2);
    u16le(&mut data, 1);
    u16le(&mut data, 4);
    data.push(1);
    for i in [0u16, 1, 2, 3] {
        u16le(&mut data, i);
    }

    let mut shader = Vec::new();
    u32le(&mut shader, u32::MAX);
    u32le(&mut shader, 0);
    u32le(&mut shader, u32::MAX);
    shader.extend([0; 22]);
    u32le(&mut shader, 3);
    let mut textures = Vec::new();
    u32le(&mut textures, 2);
    string(&mut textures, "textures\\test.dds");
    string(&mut textures, "textures\\test_n.dds");

    let blocks = [shape, data, shader, textures];
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    u32le(&mut out, 0x14020007);
    out.push(1);
    u32le(&mut out, 11);
    u32le(&mut out, 4);
    u32le(&mut out, 34);
    out.extend([1, 0, 1, 0, 1, 0]);
    u16le(&mut out, 4);
    for name in [
        "NiTriStrips",
        "NiTriStripsData",
        "BSShaderPPLightingProperty",
        "BSShaderTextureSet",
    ] {
        string(&mut out, name);
    }
    for i in 0..4 {
        u16le(&mut out, i);
    }
    for block in &blocks {
        u32le(&mut out, block.len() as u32);
    }
    u32le(&mut out, 1);
    u32le(&mut out, 4);
    string(&mut out, "quad");
    u32le(&mut out, 0);
    for block in blocks {
        out.extend(block);
    }
    u32le(&mut out, 1);
    u32le(&mut out, 0);
    out
}

/// Uncompressed 32-bit RGBA DDS (legacy header, A8B8G8R8 masks).
fn solid_dds(rgba: [u8; 4], size: u32) -> Vec<u8> {
    let mut out = b"DDS ".to_vec();
    u32le(&mut out, 124);
    u32le(&mut out, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000);
    u32le(&mut out, size);
    u32le(&mut out, size);
    u32le(&mut out, size * 4);
    u32le(&mut out, 0);
    u32le(&mut out, 1);
    for _ in 0..11 {
        u32le(&mut out, 0);
    }
    u32le(&mut out, 32);
    u32le(&mut out, 0x40 | 0x1);
    u32le(&mut out, 0);
    u32le(&mut out, 32);
    for mask in [0xff, 0xff00, 0xff_0000, 0xff00_0000] {
        u32le(&mut out, mask);
    }
    u32le(&mut out, 0x1000);
    for _ in 0..4 {
        u32le(&mut out, 0);
    }
    for _ in 0..size * size {
        out.extend(rgba);
    }
    out
}

fn red_files() -> HashMap<String, Vec<u8>> {
    HashMap::from([(
        "textures/test.dds".to_string(),
        solid_dds([200, 40, 40, 255], 4),
    )])
}

fn assert_close(a: [f32; 3], b: [f32; 3]) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < 1e-5, "{:?} != {:?}", a, b);
    }
}

#[test]
fn conversion_applies_world_transform_then_z_up_to_y_up_once() {
    let model = convert_nif(&quad_nif(), &HashMap::new()).unwrap();
    assert_eq!(model.meshes.len(), 1);
    let mesh = &model.meshes[0];
    assert_eq!(mesh.name, "quad");
    assert_eq!(mesh.indices, [0, 1, 2, 2, 1, 3]);
    // NIF (x,y,z) -> world (2x+10, 2y+20, 30) -> Y-up (wx, wz, -wy).
    let expected = [
        [10.0, 30.0, -20.0],
        [12.0, 30.0, -20.0],
        [10.0, 30.0, -22.0],
        [12.0, 30.0, -22.0],
    ];
    for (p, e) in mesh.positions.iter().zip(expected) {
        assert_close(*p, e);
    }
    for n in &mesh.normals {
        assert_close(*n, [0.0, 1.0, 0.0]);
    }
    assert_eq!(mesh.uvs, [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]);
    assert_eq!(
        model.materials,
        [(
            None,
            Some("textures/test.dds".to_string()),
            Some("textures/test_n.dds".to_string())
        )]
    );
    assert_eq!(mesh.material, 0);
    assert_close(model.bounds_center, [11.0, 30.0, -21.0]);
    assert!((model.bounds_radius - 2f32.sqrt()).abs() < 1e-5);
}

#[test]
fn conversion_rejects_scene_without_renderable_meshes() {
    assert!(convert_nif(b"not a nif", &HashMap::new()).is_err());
}

fn render(files: &HashMap<String, Vec<u8>>, light: RegressionLight) -> Option<RenderedImage> {
    let image = render_nif_offscreen(&quad_nif(), files, 64, 64, light).unwrap();
    if image.is_none() {
        assert!(
            std::env::var_os("NIF_VIEWER_REQUIRE_GPU").is_none(),
            "no wgpu adapter available but NIF_VIEWER_REQUIRE_GPU is set"
        );
        eprintln!("no wgpu adapter; skipping GPU regression check");
    }
    image
}

fn assert_pixel(image: &RenderedImage, x: u32, y: u32, expected: [u8; 4], tolerance: u8) {
    let actual = image.pixel(x, y);
    for c in 0..4 {
        assert!(
            actual[c].abs_diff(expected[c]) <= tolerance,
            "pixel ({},{}) = {:?}, expected {:?} ±{}",
            x,
            y,
            actual,
            expected,
            tolerance
        );
    }
}

/// Baselines recorded from the pre-refactor renderer (commit-time `main`).
const LIT_CENTRE: [u8; 4] = [198, 73, 73, 255];
const FALLBACK_CENTRE: [u8; 4] = [217, 217, 217, 255];
const BACKLIT_CENTRE: [u8; 4] = [100, 16, 16, 255];
const TOLERANCE: u8 = 3;
const COVERAGE: usize = 658;
const COVERAGE_BBOX: (u32, u32, u32, u32) = (4, 58, 26, 40);

#[test]
fn renders_textured_quad_matching_baseline() {
    let Some(image) = render(&red_files(), RegressionLight::default()) else {
        return;
    };
    eprintln!("lit centre {:?}", image.pixel(32, 32));
    assert_pixel(&image, 32, 32, LIT_CENTRE, TOLERANCE);
    let [r, g, b, _] = image.pixel(32, 32);
    assert!(r > g + 40 && r > b + 40, "texture colour not visible");
    // Corners are outside the framed quad: cleared background.
    for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63)] {
        assert_pixel(&image, x, y, [0, 0, 0, 255], 0);
    }
}

#[test]
fn missing_textures_render_white_fallback_material() {
    let Some(image) = render(&HashMap::new(), RegressionLight::default()) else {
        return;
    };
    eprintln!("fallback centre {:?}", image.pixel(32, 32));
    assert_pixel(&image, 32, 32, FALLBACK_CENTRE, TOLERANCE);
    let [r, g, b, _] = image.pixel(32, 32);
    assert!(r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2 && r > 20);
}

#[test]
fn quad_coverage_matches_baseline_framing() {
    let Some(image) = render(&red_files(), RegressionLight::default()) else {
        return;
    };
    let covered: Vec<(u32, u32)> = (0..64)
        .flat_map(|y| (0..64).map(move |x| (x, y)))
        .filter(|&(x, y)| image.pixel(x, y)[0] > 0)
        .collect();
    let min_x = covered.iter().map(|p| p.0).min().unwrap();
    let max_x = covered.iter().map(|p| p.0).max().unwrap();
    let min_y = covered.iter().map(|p| p.1).min().unwrap();
    let max_y = covered.iter().map(|p| p.1).max().unwrap();
    eprintln!(
        "coverage {} bbox {:?}",
        covered.len(),
        (min_x, max_x, min_y, max_y)
    );
    assert!(covered.len().abs_diff(COVERAGE) <= COVERAGE / 50);
    let bbox = (min_x, max_x, min_y, max_y);
    for (a, b) in [
        (bbox.0, COVERAGE_BBOX.0),
        (bbox.1, COVERAGE_BBOX.1),
        (bbox.2, COVERAGE_BBOX.2),
        (bbox.3, COVERAGE_BBOX.3),
    ] {
        assert!(a.abs_diff(b) <= 1, "bbox {:?} != {:?}", bbox, COVERAGE_BBOX);
    }
}

#[test]
fn light_from_behind_leaves_ambient_only() {
    let backlit = RegressionLight {
        direction: [0.0, 1.0, 0.0],
        ..RegressionLight::default()
    };
    let (Some(lit), Some(dark)) = (
        render(&red_files(), RegressionLight::default()),
        render(&red_files(), backlit),
    ) else {
        return;
    };
    eprintln!("backlit centre {:?}", dark.pixel(32, 32));
    assert_pixel(&dark, 32, 32, BACKLIT_CENTRE, TOLERANCE);
    assert!(dark.pixel(32, 32)[0] + 30 < lit.pixel(32, 32)[0]);
}
